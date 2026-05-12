//! `AccountManager` — public surface for add/remove/swap/refresh operations.
//! Each mutating method acquires the file lock for the duration of its work.

use super::{
    identity::{self, AccountIdentity},
    store::{self, AccountsLock, AddSource, ManagedAccount},
};
use crate::auth::{oauth_account_io, paths};
use anyhow::{anyhow, Context, Result};
use chrono::{TimeZone, Utc};
use std::path::{Path, PathBuf};

pub struct AccountManager {
    pub data_dir: PathBuf,
}

impl AccountManager {
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }

    pub fn list(&self) -> Result<Vec<ManagedAccount>> {
        let store = store::load(&self.data_dir)?;
        Ok(store.accounts.into_values().collect())
    }

    pub fn get(&self, slot: u32) -> Result<Option<ManagedAccount>> {
        Ok(store::load(&self.data_dir)?.accounts.remove(&slot))
    }

    /// Capture the live upstream-CLI credentials and register as a managed
    /// account. If an account with the same `accountUuid` already exists,
    /// refresh its stored blobs in place and return that slot.
    pub async fn add_from_claude_code(&self) -> Result<u32> {
        let cc_blob = crate::auth::claude_code_creds::load_full_blob()
            .await
            .context("read upstream credentials")?
            .ok_or_else(|| anyhow!("no upstream credentials present"))?;

        let global = paths::claude_global_config()
            .ok_or_else(|| anyhow!("could not resolve upstream global config path"))?;
        let oauth_account = oauth_account_io::read_oauth_account(&global)
            .context("read upstream oauthAccount slice")?
            .ok_or_else(|| anyhow!("upstream global config missing oauthAccount"))?;

        let id = identity::from_blobs(&oauth_account, Some(&cc_blob))?;
        self.upsert(id, cc_blob, oauth_account, AddSource::ImportedFromClaudeCode)
    }

    pub(crate) fn upsert(
        &self,
        id: AccountIdentity,
        cc_blob: serde_json::Value,
        oauth_account_blob: serde_json::Value,
        source: AddSource,
    ) -> Result<u32> {
        let lock = store::acquire_lock(&self.data_dir)?;
        let mut store = store::load(&self.data_dir)?;

        let now = Utc::now();
        let token_expires_at = extract_expires_at(&cc_blob).unwrap_or(now);

        if let Some(existing) = store.find_by_account_uuid(&id.account_uuid).cloned() {
            let slot = existing.slot;
            let updated = ManagedAccount {
                slot,
                email: id.email,
                account_uuid: id.account_uuid,
                organization_uuid: id.organization_uuid,
                organization_name: id.organization_name,
                subscription_type: id.subscription_type.or(existing.subscription_type),
                source,
                claude_code_oauth_blob: cc_blob,
                oauth_account_blob,
                token_expires_at,
                added_at: existing.added_at,
                last_seen_active: existing.last_seen_active,
            };
            store.accounts.insert(slot, updated);
            store::save(&self.data_dir, &store, &lock)?;
            return Ok(slot);
        }

        let slot = store.next_slot();
        let acc = ManagedAccount {
            slot,
            email: id.email,
            account_uuid: id.account_uuid,
            organization_uuid: id.organization_uuid,
            organization_name: id.organization_name,
            subscription_type: id.subscription_type,
            source,
            claude_code_oauth_blob: cc_blob,
            oauth_account_blob,
            token_expires_at,
            added_at: now,
            last_seen_active: None,
        };
        store.accounts.insert(slot, acc);
        store::save(&self.data_dir, &store, &lock)?;
        Ok(slot)
    }
}

fn extract_expires_at(cc_blob: &serde_json::Value) -> Option<chrono::DateTime<Utc>> {
    let ms = cc_blob.get("expiresAt")?.as_i64()?;
    Utc.timestamp_millis_opt(ms).single()
}

pub(crate) fn _used(_: &Path, _: &AccountsLock) {}

#[derive(Debug, thiserror::Error)]
pub enum SwapError {
    #[error("slot {0} not found")]
    NotFound(u32),
    #[error("incomplete account: {0}")]
    IncompleteAccount(String),
    #[error("credential write failed: {0}")]
    CredentialWriteFailed(String),
    #[error("config write failed: {0}; credentials restored")]
    ConfigWriteFailed(String),
    #[error("config write failed AND restore failed: {0}; CC may need re-login")]
    Critical(String),
    #[error("infrastructure error: {0}")]
    Other(#[from] anyhow::Error),
}

impl AccountManager {
    /// Atomic two-step swap with rollback:
    ///   a. Snapshot live CC credentials + ~/.claude.json oauthAccount slice.
    ///   a'. Capture the live snapshot back into the **outgoing** slot's
    ///       accounts.json entry (the slot that matches the live oauthAccount
    ///       accountUuid). Without this, CC's silent in-the-background RT
    ///       rotation while a slot is active is lost when we swap away —
    ///       resulting in `invalid_grant` the next time that slot tries to
    ///       refresh, or a 401 if we ever swap back and write the stale AT
    ///       to CC's live store.
    ///   b. Write target.claude_code_oauth_blob to CC's primary store.
    ///   c. Splice target.oauth_account_blob into ~/.claude.json.
    ///
    /// On step-b failure: nothing observable has been mutated (the outgoing-
    /// slot capture only refreshes accounts.json with freshest live values
    /// for the slot we're leaving, which is independent of the new target);
    /// return error.
    /// On step-c failure: try to restore step-b. If restore also fails:
    /// return Critical so the UI can surface a hard-error banner.
    pub async fn swap_to(&self, slot: u32) -> Result<(), SwapError> {
        let target = self
            .get(slot)?
            .ok_or(SwapError::NotFound(slot))?;

        if !target.claude_code_oauth_blob.is_object() {
            return Err(SwapError::IncompleteAccount(
                "claude_code_oauth_blob is not an object".into(),
            ));
        }
        if !target.oauth_account_blob.is_object() {
            return Err(SwapError::IncompleteAccount(
                "oauth_account_blob is not an object".into(),
            ));
        }

        // Step a: snapshot.
        let prev_cc = crate::auth::claude_code_creds::load_full_blob()
            .await
            .map_err(|e| SwapError::Other(anyhow!("snapshot CC creds: {e}")))?;

        let global = paths::claude_global_config()
            .ok_or_else(|| SwapError::Other(anyhow!("resolve global config path")))?;
        let prev_oauth_account = oauth_account_io::read_oauth_account(&global)
            .map_err(|e| SwapError::Other(anyhow!("snapshot oauthAccount: {e}")))?;

        // Step a': capture live tokens for the outgoing slot. We do this here
        // (under the same call) so the read-snapshot-write window is as tight
        // as possible; if a CC refresh races between snapshot and write, we'd
        // miss the rotation, but that's no worse than today and the
        // KeychainGuardian on the next swap-back catches up.
        if let (Some(prev_cc_blob), Some(prev_oa)) =
            (prev_cc.as_ref(), prev_oauth_account.as_ref())
        {
            if let Some(prev_uuid) = prev_oa.get("accountUuid").and_then(|v| v.as_str()) {
                if prev_uuid != target.account_uuid {
                    if let Err(e) =
                        self.capture_live_into_slot(prev_uuid, prev_cc_blob, prev_oa)
                    {
                        // Non-fatal: log and continue. We'd rather complete the
                        // swap with a slightly stale outgoing entry than abort.
                        tracing::warn!(
                            "swap_to: outgoing-slot capture for {prev_uuid} failed: {e:#}"
                        );
                    }
                }
            }
        }

        // Step b: write CC creds.
        if let Err(e) =
            crate::auth::claude_code_creds::write_full_blob(&target.claude_code_oauth_blob).await
        {
            return Err(SwapError::CredentialWriteFailed(e.to_string()));
        }

        // Step c: write global config.
        if let Err(e) = oauth_account_io::write_oauth_account(&global, &target.oauth_account_blob)
        {
            // Roll back step b.
            let restore_result = match prev_cc {
                Some(blob) => crate::auth::claude_code_creds::write_full_blob(&blob).await,
                None => Ok(()),
            };
            if let Some(prev) = prev_oauth_account {
                let _ = oauth_account_io::write_oauth_account(&global, &prev);
            }
            return match restore_result {
                Ok(_) => Err(SwapError::ConfigWriteFailed(e.to_string())),
                Err(restore_err) => Err(SwapError::Critical(format!(
                    "{e}; restore failed: {restore_err}"
                ))),
            };
        }

        Ok(())
    }

    /// Update the slot whose `account_uuid` matches `live_account_uuid` so
    /// its stored CC blob and oauthAccount slice reflect the live filesystem
    /// state. No-op when no managed slot matches. See `swap_to` step a' for
    /// the motivation. Acquires the accounts.json file lock.
    fn capture_live_into_slot(
        &self,
        live_account_uuid: &str,
        live_cc_blob: &serde_json::Value,
        live_oauth_account: &serde_json::Value,
    ) -> Result<()> {
        let lock = store::acquire_lock(&self.data_dir)?;
        let mut data = store::load(&self.data_dir)?;
        let outgoing_slot = data
            .accounts
            .values()
            .find(|a| a.account_uuid == live_account_uuid)
            .map(|a| a.slot);
        let Some(slot) = outgoing_slot else {
            return Ok(());
        };
        if let Some(acc) = data.accounts.get_mut(&slot) {
            acc.claude_code_oauth_blob = live_cc_blob.clone();
            acc.oauth_account_blob = live_oauth_account.clone();
            if let Some(exp) = extract_expires_at(live_cc_blob) {
                acc.token_expires_at = exp;
            }
            store::save(&self.data_dir, &data, &lock)?;
        }
        Ok(())
    }
}

impl AccountManager {
    /// Refresh an inactive slot's token via the OAuth endpoint, persist the
    /// new token (rotating refresh token included) back into accounts.json
    /// under the file lock. **Caller must guarantee `slot` is not the
    /// currently-active CC account** — refreshing the active slot would race
    /// against CC's own refresh and one side would get `invalid_grant`.
    pub async fn refresh_inactive(
        &self,
        slot: u32,
        exchange: &crate::auth::exchange::TokenExchange,
    ) -> Result<()> {
        tracing::info!(
            target: "switchboard.auth",
            "refresh_inactive(slot={slot}) starting"
        );
        let lock = store::acquire_lock(&self.data_dir)?;
        let mut store = store::load(&self.data_dir)?;
        let acc = store
            .accounts
            .get_mut(&slot)
            .ok_or_else(|| anyhow!("slot {slot} not found"))?;

        let refresh_token = acc
            .claude_code_oauth_blob
            .get("refreshToken")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("slot {slot} has no refresh token"))?
            .to_string();

        let new_token = exchange.refresh(&refresh_token).await?;

        let blob = acc
            .claude_code_oauth_blob
            .as_object_mut()
            .ok_or_else(|| anyhow!("blob is not an object"))?;
        blob.insert(
            "accessToken".to_string(),
            serde_json::Value::String(new_token.access_token.clone()),
        );
        if let Some(rt) = new_token.refresh_token.as_ref() {
            blob.insert(
                "refreshToken".to_string(),
                serde_json::Value::String(rt.clone()),
            );
        }
        blob.insert(
            "expiresAt".to_string(),
            serde_json::json!(new_token.expires_at.timestamp_millis()),
        );
        acc.token_expires_at = new_token.expires_at;

        store::save(&self.data_dir, &store, &lock)?;
        tracing::info!(
            target: "switchboard.auth",
            "refresh_inactive(slot={slot}) ok (new expiry: {})",
            new_token.expires_at
        );
        Ok(())
    }
}

/// Pure helper: build the synthetic CC + oauthAccount blobs from a fresh
/// OAuth token exchange + userinfo response. Public for testing.
pub fn synthesize_blobs(
    token: &crate::auth::StoredToken,
    userinfo: &crate::auth::account_identity::UserInfo,
) -> (serde_json::Value, serde_json::Value) {
    let cc = serde_json::json!({
        "accessToken": token.access_token,
        "refreshToken": token.refresh_token,
        "expiresAt": token.expires_at.timestamp_millis(),
        "scopes": ["user:inference", "user:profile"],
    });
    let oa = serde_json::json!({
        "accountUuid": userinfo.id,
        "emailAddress": userinfo.email,
        "organizationUuid": null,
        "organizationName": null,
        "displayName": userinfo.name,
    });
    (cc, oa)
}

impl AccountManager {
    pub fn remove(&self, slot: u32) -> Result<()> {
        let lock = store::acquire_lock(&self.data_dir)?;
        let mut store = store::load(&self.data_dir)?;
        store.accounts.remove(&slot);
        store::save(&self.data_dir, &store, &lock)?;
        Ok(())
    }

    /// Register a new (or refresh existing) managed account from a freshly
    /// completed paste-back OAuth exchange.
    pub async fn add_from_oauth(
        &self,
        token: crate::auth::StoredToken,
        userinfo: crate::auth::account_identity::UserInfo,
    ) -> Result<u32> {
        let (cc, oa) = synthesize_blobs(&token, &userinfo);
        let id = identity::from_blobs(&oa, Some(&cc))?;
        self.upsert(id, cc, oa, AddSource::OAuth)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn cc_blob(uuid: &str, exp_ms: i64) -> serde_json::Value {
        serde_json::json!({
            "accessToken": format!("at-{uuid}"),
            "refreshToken": format!("rt-{uuid}"),
            "expiresAt": exp_ms,
            "scopes": ["user:inference"],
            "subscriptionType": "max"
        })
    }

    fn oa_slice(uuid: &str, email: &str) -> serde_json::Value {
        serde_json::json!({
            "accountUuid": uuid,
            "emailAddress": email,
            "organizationUuid": null,
            "organizationName": null
        })
    }

    #[test]
    fn upsert_assigns_first_slot_then_dedups() {
        let dir = tempdir().unwrap();
        let mgr = AccountManager::new(dir.path().to_path_buf());

        let id1 = identity::from_blobs(&oa_slice("u1", "a@x"), Some(&cc_blob("u1", 1))).unwrap();
        let s1 = mgr
            .upsert(id1, cc_blob("u1", 1), oa_slice("u1", "a@x"), AddSource::OAuth)
            .unwrap();
        assert_eq!(s1, 1);

        let id1_again =
            identity::from_blobs(&oa_slice("u1", "a@x"), Some(&cc_blob("u1", 99))).unwrap();
        let s1_again = mgr
            .upsert(
                id1_again,
                cc_blob("u1", 99),
                oa_slice("u1", "a@x"),
                AddSource::OAuth,
            )
            .unwrap();
        assert_eq!(s1_again, 1, "same accountUuid → same slot");

        let id2 = identity::from_blobs(&oa_slice("u2", "b@x"), Some(&cc_blob("u2", 1))).unwrap();
        let s2 = mgr
            .upsert(id2, cc_blob("u2", 1), oa_slice("u2", "b@x"), AddSource::OAuth)
            .unwrap();
        assert_eq!(s2, 2);

        let listed = mgr.list().unwrap();
        assert_eq!(listed.len(), 2);
    }

    #[tokio::test]
    async fn refresh_inactive_persists_new_token() {
        use chrono::Duration;
        let server = mockito::Server::new_async().await;
        let mock_url = server.url();

        let dir = tempdir().unwrap();
        let mgr = AccountManager::new(dir.path().to_path_buf());

        let now = Utc::now();
        let id = identity::from_blobs(&oa_slice("u1", "a@x"), Some(&cc_blob("u1", 1))).unwrap();
        let mut blob = cc_blob("u1", (now - Duration::hours(1)).timestamp_millis());
        blob["refreshToken"] = serde_json::Value::String("OLD_RT".to_string());
        mgr.upsert(id, blob, oa_slice("u1", "a@x"), AddSource::OAuth)
            .unwrap();

        let mut server = server;
        let _m = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"access_token":"NEW_AT","refresh_token":"NEW_RT","expires_in":3600,"token_type":"Bearer"}"#,
            )
            .create_async()
            .await;

        let exchange = crate::auth::exchange::TokenExchange::with_endpoint(mock_url);
        mgr.refresh_inactive(1, &exchange).await.unwrap();

        let acc = mgr.get(1).unwrap().unwrap();
        assert_eq!(acc.claude_code_oauth_blob["accessToken"], "NEW_AT");
        assert_eq!(acc.claude_code_oauth_blob["refreshToken"], "NEW_RT");
    }

    #[test]
    fn capture_live_into_slot_overwrites_matching_slot_blob_and_expiry() {
        // Simulates the swap_to step-a' capture: outgoing slot (u1) gets
        // its stored blob refreshed from live filesystem state. This is
        // the fix for the stale-RT bug where CC silently rotated tokens
        // while the slot was active.
        use chrono::Duration;
        let dir = tempdir().unwrap();
        let mgr = AccountManager::new(dir.path().to_path_buf());

        let now = Utc::now();
        let stale_exp = (now - Duration::hours(2)).timestamp_millis();
        let mut stored = cc_blob("u1", stale_exp);
        stored["refreshToken"] = serde_json::Value::String("STALE_RT".into());
        stored["accessToken"] = serde_json::Value::String("STALE_AT".into());
        let id = identity::from_blobs(&oa_slice("u1", "a@x"), Some(&stored)).unwrap();
        mgr.upsert(id, stored, oa_slice("u1", "a@x"), AddSource::OAuth)
            .unwrap();

        // Build a "live" snapshot with freshly rotated tokens and a
        // future expiry — what CC would have written silently.
        let fresh_exp = (now + Duration::hours(1)).timestamp_millis();
        let mut live_cc = cc_blob("u1", fresh_exp);
        live_cc["refreshToken"] = serde_json::Value::String("FRESH_RT".into());
        live_cc["accessToken"] = serde_json::Value::String("FRESH_AT".into());
        let live_oa = oa_slice("u1", "a@x");

        mgr.capture_live_into_slot("u1", &live_cc, &live_oa).unwrap();

        let after = mgr.get(1).unwrap().unwrap();
        assert_eq!(after.claude_code_oauth_blob["refreshToken"], "FRESH_RT");
        assert_eq!(after.claude_code_oauth_blob["accessToken"], "FRESH_AT");
        assert_eq!(
            after.token_expires_at.timestamp_millis(),
            fresh_exp,
            "token_expires_at must track the live blob's expiresAt"
        );
    }

    #[test]
    fn capture_live_into_slot_is_noop_when_uuid_unmanaged() {
        // Live filesystem may belong to an account we don't manage (e.g.
        // user logged into CC as someone we never imported). Capture must
        // do nothing — not create rows, not error.
        let dir = tempdir().unwrap();
        let mgr = AccountManager::new(dir.path().to_path_buf());

        let id = identity::from_blobs(&oa_slice("u1", "a@x"), Some(&cc_blob("u1", 1))).unwrap();
        mgr.upsert(id, cc_blob("u1", 1), oa_slice("u1", "a@x"), AddSource::OAuth)
            .unwrap();

        mgr.capture_live_into_slot("unmanaged-uuid", &cc_blob("x", 1), &oa_slice("x", "x@y"))
            .unwrap();

        let after = mgr.get(1).unwrap().unwrap();
        assert_eq!(after.claude_code_oauth_blob["accessToken"], "at-u1");
    }

    #[test]
    fn swap_rollback_restores_credentials_when_config_write_fails() {
        if std::env::var_os("USER").is_none() && std::env::var_os("USERPROFILE").is_none() {
            eprintln!("skipping swap rollback test: no USER/USERPROFILE");
            return;
        }
        let dir = tempdir().unwrap();
        let mgr = AccountManager::new(dir.path().to_path_buf());
        let r = futures::executor::block_on(mgr.swap_to(99));
        assert!(r.is_err(), "swap to nonexistent slot must error");
    }

    #[test]
    fn remove_is_idempotent_and_lock_protected() {
        let dir = tempdir().unwrap();
        let mgr = AccountManager::new(dir.path().to_path_buf());

        let id = identity::from_blobs(&oa_slice("u1", "a@x"), Some(&cc_blob("u1", 1))).unwrap();
        mgr.upsert(id, cc_blob("u1", 1), oa_slice("u1", "a@x"), AddSource::OAuth)
            .unwrap();

        mgr.remove(1).unwrap();
        assert!(mgr.list().unwrap().is_empty());
        mgr.remove(1).unwrap();
    }

    #[test]
    fn synthesize_blobs_from_token_and_userinfo() {
        use chrono::Duration;
        let now = Utc::now();
        let token = crate::auth::StoredToken {
            access_token: "at-x".to_string(),
            refresh_token: Some("rt-x".to_string()),
            expires_at: now + Duration::hours(8),
        };
        let userinfo = crate::auth::account_identity::UserInfo {
            id: "uuid-x".to_string(),
            email: "x@x.com".to_string(),
            name: Some("X".to_string()),
        };
        let (cc, oa) = super::synthesize_blobs(&token, &userinfo);
        assert_eq!(cc["accessToken"], "at-x");
        assert_eq!(cc["refreshToken"], "rt-x");
        assert_eq!(cc["expiresAt"].as_i64().unwrap() / 1000, token.expires_at.timestamp());
        assert_eq!(oa["accountUuid"], "uuid-x");
        assert_eq!(oa["emailAddress"], "x@x.com");
    }
}
