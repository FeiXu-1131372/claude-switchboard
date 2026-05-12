//! Background task that holds a swap target's identity in the global config
//! for 60 seconds post-swap. The original design fought *any* drift in the
//! `.credentials.json` `refreshToken`, but that misfired in the common case
//! where the now-active CC process rotates its own tokens: comparing
//! `refreshToken` alone, the guardian would clobber CC's freshly-issued
//! tokens with our stored (now-expired) ones, killing both AT and RT for
//! the active account and bricking the swap.
//!
//! The redesign keys off `oauthAccount.accountUuid` in `~/.claude.json`.
//! Same account, different tokens (CC's own refresh) → leave alone.
//! Different account in oauthAccount (an external swap, or a rogue CC
//! process writing a different account back) → revert both `.credentials.json`
//! and `~/.claude.json` to the target. CC's `checkAndRefreshOAuthTokenIfNeeded`
//! does not touch `oauthAccount` during refresh, so this signal is a clean
//! "the active account changed" indicator.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

#[async_trait]
pub trait CredIO: Send + Sync + 'static {
    async fn load_cc(&self) -> Result<Option<Value>>;
    async fn write_cc(&self, blob: &Value) -> Result<()>;
    async fn load_oauth_account(&self) -> Result<Option<Value>>;
    async fn write_oauth_account(&self, blob: &Value) -> Result<()>;
}

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const GUARD_DURATION: Duration = Duration::from_secs(60);

pub struct KeychainGuardian {
    cancel: Arc<Notify>,
}

impl KeychainGuardian {
    pub fn arm<I: CredIO>(
        target_cc_blob: Value,
        target_oauth_account: Value,
        target_account_uuid: String,
        io: Arc<I>,
    ) -> Self {
        let cancel = Arc::new(Notify::new());
        let cancel_for_task = cancel.clone();
        tokio::spawn(async move {
            run_guardian(
                target_cc_blob,
                target_oauth_account,
                target_account_uuid,
                io,
                cancel_for_task,
            )
            .await;
        });
        Self { cancel }
    }

    pub fn cancel(self) {
        self.cancel.notify_one();
    }
}

async fn run_guardian<I: CredIO>(
    target_cc_blob: Value,
    target_oauth_account: Value,
    target_account_uuid: String,
    io: Arc<I>,
    cancel: Arc<Notify>,
) {
    let deadline = tokio::time::Instant::now() + GUARD_DURATION;

    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            break;
        }
        let sleep_until = (now + POLL_INTERVAL).min(deadline);
        tokio::select! {
            _ = tokio::time::sleep_until(sleep_until) => {}
            _ = cancel.notified() => return,
        }

        // Account-aware drift detection: read the global config's
        // oauthAccount.accountUuid. If it still matches the target, any
        // change in .credentials.json is the same account's own token
        // rotation (CC refreshes its own AT/RT routinely) — do NOT
        // clobber. If it differs, the active account was swapped out
        // from under us; revert both files.
        let live_uuid = match io.load_oauth_account().await {
            Ok(Some(oa)) => oa
                .get("accountUuid")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            Ok(None) => None,
            Err(e) => {
                tracing::warn!("keychain_guardian: load_oauth_account failed: {e:#}");
                continue;
            }
        };

        let drifted = match live_uuid.as_deref() {
            Some(uuid) => uuid != target_account_uuid,
            // oauthAccount slice absent — treat as drift; we expect our
            // own swap to have written it. If it's gone, something
            // upstream cleared it and we should restore.
            None => true,
        };

        if !drifted {
            continue;
        }

        if let Err(e) = io.write_cc(&target_cc_blob).await {
            tracing::warn!("keychain_guardian: re-apply cred write failed: {e:#}");
        } else {
            tracing::info!(
                "keychain_guardian: re-applied target {target_account_uuid} after drift to {live_uuid:?}"
            );
        }
        if let Err(e) = io.write_oauth_account(&target_oauth_account).await {
            tracing::warn!("keychain_guardian: re-apply oauthAccount write failed: {e:#}");
        }
    }
}

pub struct ClaudeCodeCredIO;

#[async_trait]
impl CredIO for ClaudeCodeCredIO {
    async fn load_cc(&self) -> Result<Option<Value>> {
        crate::auth::claude_code_creds::load_full_blob().await
    }
    async fn write_cc(&self, blob: &Value) -> Result<()> {
        crate::auth::claude_code_creds::write_full_blob(blob).await
    }
    async fn load_oauth_account(&self) -> Result<Option<Value>> {
        let p = crate::auth::paths::claude_global_config()
            .ok_or_else(|| anyhow!("resolve global config path"))?;
        crate::auth::oauth_account_io::read_oauth_account(&p)
    }
    async fn write_oauth_account(&self, blob: &Value) -> Result<()> {
        let p = crate::auth::paths::claude_global_config()
            .ok_or_else(|| anyhow!("resolve global config path"))?;
        crate::auth::oauth_account_io::write_oauth_account(&p, blob)
    }
}

impl KeychainGuardian {
    pub fn arm_with_claude_code_creds(
        target_cc_blob: Value,
        target_oauth_account: Value,
        target_account_uuid: String,
    ) -> Self {
        Self::arm(
            target_cc_blob,
            target_oauth_account,
            target_account_uuid,
            Arc::new(ClaudeCodeCredIO),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    struct MockIO {
        cc: Mutex<Option<Value>>,
        oa: Mutex<Option<Value>>,
        cc_writes: AtomicUsize,
        oa_writes: AtomicUsize,
    }

    impl MockIO {
        fn new(initial_cc: Value, initial_oa: Value) -> Arc<Self> {
            Arc::new(Self {
                cc: Mutex::new(Some(initial_cc)),
                oa: Mutex::new(Some(initial_oa)),
                cc_writes: AtomicUsize::new(0),
                oa_writes: AtomicUsize::new(0),
            })
        }
    }

    #[async_trait]
    impl CredIO for MockIO {
        async fn load_cc(&self) -> Result<Option<Value>> {
            Ok(self.cc.lock().unwrap().clone())
        }
        async fn write_cc(&self, blob: &Value) -> Result<()> {
            *self.cc.lock().unwrap() = Some(blob.clone());
            self.cc_writes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn load_oauth_account(&self) -> Result<Option<Value>> {
            Ok(self.oa.lock().unwrap().clone())
        }
        async fn write_oauth_account(&self, blob: &Value) -> Result<()> {
            *self.oa.lock().unwrap() = Some(blob.clone());
            self.oa_writes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn cc(refresh: &str) -> Value {
        serde_json::json!({ "refreshToken": refresh, "accessToken": "at" })
    }

    fn oa(uuid: &str) -> Value {
        serde_json::json!({ "accountUuid": uuid, "emailAddress": "u@x" })
    }

    #[tokio::test]
    async fn arm_returns_a_guardian_handle() {
        let io = MockIO::new(cc("rt-b"), oa("uuid-b"));
        let g = KeychainGuardian::arm(
            cc("rt-b"),
            oa("uuid-b"),
            "uuid-b".to_string(),
            io.clone(),
        );
        g.cancel();
        assert_eq!(io.cc_writes.load(Ordering::SeqCst), 0);
        assert_eq!(io.oa_writes.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn same_account_rt_rotation_is_left_alone() {
        // Live oauthAccount still points to target; .credentials.json has
        // a different refreshToken (CC's own routine refresh). Guardian
        // must NOT clobber — that would destroy CC's freshly-issued tokens.
        let io = MockIO::new(cc("rt-b"), oa("uuid-b"));
        let g = KeychainGuardian::arm(
            cc("rt-b"),
            oa("uuid-b"),
            "uuid-b".to_string(),
            io.clone(),
        );

        tokio::time::sleep(Duration::from_secs(1)).await;
        // Simulate CC rotating its own refresh token for the same account.
        *io.cc.lock().unwrap() = Some(cc("rt-b-rotated"));
        tokio::time::sleep(Duration::from_secs(6)).await;

        assert_eq!(
            io.cc_writes.load(Ordering::SeqCst),
            0,
            "guardian must NOT overwrite same-account rotation"
        );
        let current_rt = io.cc.lock().unwrap().clone().unwrap();
        assert_eq!(current_rt["refreshToken"], "rt-b-rotated");
        g.cancel();
    }

    #[tokio::test(start_paused = true)]
    async fn account_drift_triggers_revert_of_both_files() {
        // oauthAccount in .claude.json has switched to a different account
        // (external swap or rogue CC writing a different account's data
        // back). Guardian must restore both files to the target.
        let io = MockIO::new(cc("rt-a"), oa("uuid-a"));
        let g = KeychainGuardian::arm(
            cc("rt-b"),
            oa("uuid-b"),
            "uuid-b".to_string(),
            io.clone(),
        );

        // Within the guard window, the live state differs from target.
        tokio::time::sleep(Duration::from_secs(5)).await;

        assert!(
            io.cc_writes.load(Ordering::SeqCst) >= 1,
            "expected at least one cc revert write, got {}",
            io.cc_writes.load(Ordering::SeqCst)
        );
        assert!(
            io.oa_writes.load(Ordering::SeqCst) >= 1,
            "expected at least one oauthAccount revert write"
        );
        let cur_oa = io.oa.lock().unwrap().clone().unwrap();
        assert_eq!(cur_oa["accountUuid"], "uuid-b");
        g.cancel();
    }

    #[tokio::test(start_paused = true)]
    async fn stops_writing_after_deadline() {
        let io = MockIO::new(cc("rt-a"), oa("uuid-a"));
        let _g = KeychainGuardian::arm(
            cc("rt-b"),
            oa("uuid-b"),
            "uuid-b".to_string(),
            io.clone(),
        );

        // Allow several reverts within the guard window.
        tokio::time::sleep(Duration::from_secs(10)).await;
        let writes_during_guard = io.cc_writes.load(Ordering::SeqCst);
        assert!(writes_during_guard >= 1);

        // Past deadline, even more drift must NOT trigger writes.
        tokio::time::sleep(Duration::from_secs(70)).await;
        *io.oa.lock().unwrap() = Some(oa("uuid-c"));
        *io.cc.lock().unwrap() = Some(cc("rt-c"));
        tokio::time::sleep(Duration::from_secs(10)).await;

        assert_eq!(
            io.cc_writes.load(Ordering::SeqCst),
            writes_during_guard,
            "guardian must not write after deadline"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_stops_writes_immediately() {
        let io = MockIO::new(cc("rt-a"), oa("uuid-a"));
        let g = KeychainGuardian::arm(
            cc("rt-b"),
            oa("uuid-b"),
            "uuid-b".to_string(),
            io.clone(),
        );

        // Cancel before the first poll fires.
        tokio::time::sleep(Duration::from_millis(500)).await;
        g.cancel();

        // Further drift after cancel — guardian must not react.
        tokio::time::sleep(Duration::from_secs(2)).await;
        *io.oa.lock().unwrap() = Some(oa("uuid-c"));
        tokio::time::sleep(Duration::from_secs(10)).await;

        assert_eq!(
            io.cc_writes.load(Ordering::SeqCst),
            0,
            "no writes should happen after cancel"
        );
    }
}
