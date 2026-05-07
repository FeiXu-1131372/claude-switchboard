//! Background task that holds a swap target's credentials in the platform
//! credential store for 60 seconds post-swap, defeating the narrow race
//! where a still-running Claude Code process completes an in-flight OAuth
//! refresh after our swap and writes the previous account's rotated tokens
//! back. CC's own keychain cache TTL (30s, src/utils/secureStorage/
//! macOsKeychainHelpers.ts:69) does the natural hot-reload; we just protect
//! the entry through the danger window.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

#[async_trait]
pub trait CredIO: Send + Sync + 'static {
    async fn load(&self) -> Result<Option<Value>>;
    async fn write(&self, blob: &Value) -> Result<()>;
}

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const GUARD_DURATION: Duration = Duration::from_secs(60);

pub struct KeychainGuardian {
    cancel: Arc<Notify>,
}

impl KeychainGuardian {
    pub fn arm<I: CredIO>(target_blob: Value, io: Arc<I>) -> Self {
        let cancel = Arc::new(Notify::new());
        let cancel_for_task = cancel.clone();
        tokio::spawn(async move {
            run_guardian(target_blob, io, cancel_for_task).await;
        });
        Self { cancel }
    }

    pub fn cancel(self) {
        self.cancel.notify_one();
    }
}

async fn run_guardian<I: CredIO>(
    target_blob: Value,
    io: Arc<I>,
    cancel: Arc<Notify>,
) {
    let target_refresh = target_blob
        .get("refreshToken")
        .and_then(|v| v.as_str())
        .map(str::to_string);
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

        match io.load().await {
            Ok(Some(current)) => {
                let current_refresh = current
                    .get("refreshToken")
                    .and_then(|v| v.as_str());
                if current_refresh != target_refresh.as_deref() {
                    if let Err(e) = io.write(&target_blob).await {
                        tracing::warn!("keychain_guardian: re-apply write failed: {e:#}");
                    } else {
                        tracing::info!("keychain_guardian: re-applied target after clobber");
                    }
                }
            }
            Ok(None) => {
                if let Err(e) = io.write(&target_blob).await {
                    tracing::warn!("keychain_guardian: re-apply (was empty) failed: {e:#}");
                }
            }
            Err(e) => {
                tracing::warn!("keychain_guardian: load failed: {e:#}");
            }
        }
    }
}

pub struct ClaudeCodeCredIO;

#[async_trait]
impl CredIO for ClaudeCodeCredIO {
    async fn load(&self) -> Result<Option<Value>> {
        crate::auth::claude_code_creds::load_full_blob().await
    }
    async fn write(&self, blob: &Value) -> Result<()> {
        crate::auth::claude_code_creds::write_full_blob(blob).await
    }
}

impl KeychainGuardian {
    pub fn arm_with_claude_code_creds(target_blob: Value) -> Self {
        Self::arm(target_blob, Arc::new(ClaudeCodeCredIO))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::time::Duration;

    struct MockIO {
        current: Mutex<Option<Value>>,
        writes: AtomicUsize,
    }

    impl MockIO {
        fn new(initial: Value) -> Arc<Self> {
            Arc::new(Self {
                current: Mutex::new(Some(initial)),
                writes: AtomicUsize::new(0),
            })
        }
    }

    #[async_trait]
    impl CredIO for MockIO {
        async fn load(&self) -> Result<Option<Value>> {
            Ok(self.current.lock().unwrap().clone())
        }
        async fn write(&self, blob: &Value) -> Result<()> {
            *self.current.lock().unwrap() = Some(blob.clone());
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn blob(refresh: &str) -> Value {
        serde_json::json!({ "refreshToken": refresh, "accessToken": "at" })
    }

    #[tokio::test]
    async fn arm_returns_a_guardian_handle() {
        let io = MockIO::new(blob("rt-b"));
        let g = KeychainGuardian::arm(blob("rt-b"), io.clone());
        // Smoke: cancel must consume self without panicking.
        g.cancel();
        assert_eq!(io.writes.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn rewrites_target_when_keychain_drifts() {
        // Keychain initially has B (the swap target). After 1s a "rogue"
        // refresh writes A back. Guardian should detect and re-apply B.
        let io = MockIO::new(blob("rt-b"));
        let g = KeychainGuardian::arm(blob("rt-b"), io.clone());

        // Simulate clobber at t+1s.
        tokio::time::sleep(Duration::from_secs(1)).await;
        *io.current.lock().unwrap() = Some(blob("rt-a"));

        // Advance to t+5s — guardian polls every 2s, so it must have
        // written at least once by now.
        tokio::time::sleep(Duration::from_secs(4)).await;

        let cur = io.current.lock().unwrap().clone().unwrap();
        assert_eq!(
            cur.get("refreshToken").and_then(|v| v.as_str()),
            Some("rt-b"),
            "guardian must re-apply target after drift"
        );
        assert!(
            io.writes.load(Ordering::SeqCst) >= 1,
            "guardian must have issued at least one write"
        );
        g.cancel();
    }

    #[tokio::test(start_paused = true)]
    async fn stops_writing_after_deadline() {
        let io = MockIO::new(blob("rt-b"));
        let _g = KeychainGuardian::arm(blob("rt-b"), io.clone());

        // Drift in at t+5s, well within the guard window.
        tokio::time::sleep(Duration::from_secs(5)).await;
        *io.current.lock().unwrap() = Some(blob("rt-a"));
        tokio::time::sleep(Duration::from_secs(4)).await; // expect re-apply
        let writes_during_guard = io.writes.load(Ordering::SeqCst);
        assert!(writes_during_guard >= 1, "should re-apply during guard window");

        // Past deadline (t+9s + 60s + slack), drift in again — must NOT re-apply.
        tokio::time::sleep(Duration::from_secs(70)).await;
        *io.current.lock().unwrap() = Some(blob("rt-c"));
        tokio::time::sleep(Duration::from_secs(10)).await;
        let writes_after_deadline = io.writes.load(Ordering::SeqCst);
        assert_eq!(
            writes_after_deadline, writes_during_guard,
            "guardian must not write after deadline"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_stops_writes_immediately() {
        let io = MockIO::new(blob("rt-b"));
        let g = KeychainGuardian::arm(blob("rt-b"), io.clone());

        // Cancel at t+1s, before the first poll fires.
        tokio::time::sleep(Duration::from_secs(1)).await;
        g.cancel();

        // Drift in after cancel — guardian must not react.
        tokio::time::sleep(Duration::from_secs(2)).await;
        *io.current.lock().unwrap() = Some(blob("rt-a"));
        tokio::time::sleep(Duration::from_secs(10)).await;

        assert_eq!(
            io.writes.load(Ordering::SeqCst),
            0,
            "no writes should happen after cancel"
        );
    }
}
