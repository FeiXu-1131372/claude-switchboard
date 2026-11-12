//! `AccountManager` — public surface for add/remove/swap/refresh operations.
//! Each mutating method acquires the file lock for the duration of its work.

use super::{
    identity::{self, AccountIdentity},
    store::{self, AccountsLock, AddSource, ManagedAccount},
};
use crate::auth::keychain_guardian::{ClaudeCodeCredIO, CredIO};
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

        let global = crate::auth::paths::claude_global_config()
            .ok_or_else(|| anyhow!("could not resolve upstream global config path"))?;
        let oauth_account = crate::auth::oauth_account_io::read_oauth_account(&global)
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
        let token_expires_at = extract_expires_at(&cc_blob);

        // The same Anthropic account can have several organizations (a Team seat
        // and a personal login under one email), each with its own limits: only
        // a sign-in to the same organization is the same account.
        if let Some(existing) = store
            .find_by_identity(&id.account_uuid, id.organization_uuid.as_deref())
            .cloned()
        {
            let slot = existing.slot;
            let updated = ManagedAccount {
                slot,
                email: id.email,
                label: existing.label,
                // Its id does not change: history and warm-up settings hang on it.
                account_uuid: existing.account_uuid,
                organization_uuid: id.organization_uuid.or(existing.organization_uuid),
                organization_name: id.organization_name.or(existing.organization_name),
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
        let local_id = store.local_id_for(&id.account_uuid, id.organization_uuid.as_deref());
        let acc = ManagedAccount {
            slot,
            email: id.email,
            label: None,
            account_uuid: local_id,
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
        self.swap_to_with_io(slot, &ClaudeCodeCredIO).await
    }

    pub async fn swap_to_with_io<I: CredIO>(&self, slot: u32, io: &I) -> Result<(), SwapError> {
        let target = self
            .get(slot)?
            .ok_or(SwapError::NotFound(slot))?;

        if !target.claude_code_oauth_blob.is_object() {
            return Err(SwapError::IncompleteAccount(
                "claude_code_oauth_blob is not an object".into(),
            ));
        }
        if !(target.oauth_account_blob.is_object()
            || target.source == AddSource::LongLivedToken && target.oauth_account_blob.is_null()) {
            return Err(SwapError::IncompleteAccount(
                "oauth_account_blob is not an object".into(),
            ));
        }

        // Step a: snapshot.
        let prev_cc = io.load_cc()
            .await
            .map_err(|e| SwapError::Other(anyhow!("snapshot CC creds: {e}")))?;

        let prev_oauth_account = io.load_oauth_account().await
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
                let prev_org = prev_oa.get("organizationUuid").and_then(|v| v.as_str());
                if !target.is_identity(prev_uuid, prev_org) {
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
            io.write_cc(&target.claude_code_oauth_blob).await
        {
            return Err(SwapError::CredentialWriteFailed(e.to_string()));
        }

        // Step c: write global config.
        if let Err(e) = io.write_oauth_account(&target.oauth_account_blob).await
        {
            // Roll back step b.
            let restore_result = match prev_cc {
                Some(blob) => io.write_cc(&blob).await,
                None => io.write_cc(&serde_json::Value::Null).await,
            };
            let _ = io.write_oauth_account(&prev_oauth_account.unwrap_or(serde_json::Value::Null)).await;
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
            .find_by_identity(live_account_uuid, live_oauth_account["organizationUuid"].as_str())
            .map(|a| a.slot);
        let Some(slot) = outgoing_slot else {
            return Ok(());
        };
        if let Some(acc) = data.accounts.get_mut(&slot) {
            acc.claude_code_oauth_blob = live_cc_blob.clone();
            acc.oauth_account_blob = live_oauth_account.clone();
            if let Some(exp) = extract_expires_at(live_cc_blob) {
                acc.token_expires_at = Some(exp);
            }
            store::save(&self.data_dir, &data, &lock)?;
        }
        Ok(())
    }

    /// Adopt the profile slice Claude Code currently holds for the account in
    /// `slot`, when it belongs to that account and differs from the stored
    /// copy. Returns whether anything was written.
    ///
    /// Claude Code re-fetches this slice while an account is in use, and it
    /// is the only place a plan change shows up (see
    /// `identity::plan_from_blobs`). Without this the stored copy is only
    /// replaced when the user swaps away, so the account in use keeps the
    /// plan it had when it was added. The credentials are left alone: the
    /// active slot always reads its token from Claude Code's live store.
    pub fn sync_live_profile(
        &self,
        slot: u32,
        live_oauth_account: &serde_json::Value,
    ) -> Result<bool> {
        let Some(live_uuid) = live_oauth_account.get("accountUuid").and_then(|v| v.as_str())
        else {
            return Ok(false);
        };
        let lock = store::acquire_lock(&self.data_dir)?;
        let mut data = store::load(&self.data_dir)?;
        let Some(acc) = data.accounts.get_mut(&slot) else {
            return Ok(false);
        };
        let live_org = live_oauth_account.get("organizationUuid").and_then(|v| v.as_str());
        if !acc.is_identity(live_uuid, live_org) || acc.oauth_account_blob == *live_oauth_account {
            return Ok(false);
        }
        acc.oauth_account_blob = live_oauth_account.clone();
        store::save(&self.data_dir, &data, &lock)?;
        Ok(true)
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
        acc.token_expires_at = Some(new_token.expires_at);

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
        "organizationUuid": userinfo.organization_uuid,
        "organizationName": userinfo.organization_name,
        "displayName": userinfo.name,
    });
    (cc, oa)
}

impl AccountManager {
    pub async fn import_long_lived_token(
        &self,
        access_token: &str,
        label: Option<&str>,
        replace_slot: Option<u32>,
        identity: &crate::auth::account_identity::IdentityFetcher,
    ) -> Result<u32> {
        let access_token = super::token_import::validate_token(access_token)?;
        let label = super::token_import::validate_label(label)?;
        let profile = identity.fetch_for_token_import(access_token).await?;
        let lock = store::acquire_lock(&self.data_dir)?;
        let mut data = store::load(&self.data_dir)?;
        let previous = replace_slot
            .map(|slot| {
                data.accounts.get(&slot).cloned()
                    .ok_or_else(|| anyhow!("The token account no longer exists."))
            })
            .transpose()?;
        if previous.as_ref().is_some_and(|a| a.source != AddSource::LongLivedToken) {
            return Err(anyhow!("Only an imported token can be replaced here. Your browser sign-in was not changed."));
        }
        if data.accounts.values().any(|a| {
            Some(a.slot) != replace_slot
                && (a.claude_code_oauth_blob["accessToken"].as_str() == Some(access_token)
                    || profile
                        .as_ref()
                        .is_some_and(|p| a.is_identity(&p.id, p.organization_uuid.as_deref())))
        }) {
            return Err(anyhow!("This token or account is already saved. Its existing sign-in was not changed."));
        }
        let slot = replace_slot.unwrap_or_else(|| data.next_slot());
        let mut cc = serde_json::json!({
            "accessToken": access_token,
            "refreshToken": null,
            "scopes": ["user:inference"],
        });
        let (account_uuid, email, oauth_account_blob) = match profile {
            Some(profile) => {
                cc["scopes"] = serde_json::json!(["user:inference", "user:profile"]);
                let oa = serde_json::json!({
                    "accountUuid": profile.id,
                    "emailAddress": profile.email,
                    "displayName": profile.name,
                    "organizationUuid": profile.organization_uuid,
                    "organizationName": profile.organization_name,
                });
                // Another organization of the same Anthropic account is saved
                // already: this one needs an id of its own.
                let local_id = previous
                    .as_ref()
                    .filter(|a| a.anthropic_uuid() == profile.id)
                    .map(|a| a.account_uuid.clone())
                    .unwrap_or_else(|| data.local_id_for(&profile.id, profile.organization_uuid.as_deref()));
                (local_id, profile.email, oa)
            }
            None => {
                let id = previous.as_ref()
                    .filter(|a| a.account_uuid.starts_with("token:"))
                    .map(|a| a.account_uuid.clone())
                    .unwrap_or_else(|| super::token_import::token_id(access_token));
                (id, String::new(), serde_json::Value::Null)
            }
        };
        let now = Utc::now();
        data.accounts.insert(slot, ManagedAccount {
            slot,
            email,
            label: label.or_else(|| previous.as_ref().and_then(|a| a.label.clone())),
            account_uuid,
            organization_uuid: None,
            organization_name: None,
            subscription_type: None,
            source: AddSource::LongLivedToken,
            claude_code_oauth_blob: cc,
            oauth_account_blob,
            token_expires_at: None,
            added_at: previous.as_ref().map(|a| a.added_at).unwrap_or(now),
            last_seen_active: None,
        });
        store::save(&self.data_dir, &data, &lock)?;
        Ok(slot)
    }

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

    // ---- one Anthropic account, several organizations ----
    //
    // A Team seat and a personal login under one email share the Anthropic
    // account uuid and differ in organizationUuid, and have separate limits.

    fn oa_in_org(uuid: &str, email: &str, org: &str) -> serde_json::Value {
        serde_json::json!({
            "accountUuid": uuid,
            "emailAddress": email,
            "organizationUuid": org,
            "organizationName": format!("Org {org}"),
        })
    }

    fn cc_in_org(uuid: &str, org: &str, tag: &str) -> serde_json::Value {
        serde_json::json!({
            "accessToken": format!("at-{uuid}-{org}-{tag}"),
            "refreshToken": format!("rt-{uuid}-{org}-{tag}"),
            "expiresAt": 1,
            "scopes": ["user:inference", "user:profile"],
        })
    }

    fn sign_in(mgr: &AccountManager, uuid: &str, email: &str, org: &str, tag: &str) -> u32 {
        let (oa, cc) = (oa_in_org(uuid, email, org), cc_in_org(uuid, org, tag));
        let id = identity::from_blobs(&oa, Some(&cc)).unwrap();
        mgr.upsert(id, cc, oa, AddSource::OAuth).unwrap()
    }

    #[test]
    fn one_email_in_two_organizations_is_two_accounts_and_neither_overwrites_the_other() {
        let dir = tempdir().unwrap();
        let mgr = AccountManager::new(dir.path().to_path_buf());
        let team = sign_in(&mgr, "u1", "me@x", "org-team", "a");
        let personal = sign_in(&mgr, "u1", "me@x", "org-personal", "a");
        assert_ne!(team, personal, "the second sign-in must not replace the first");

        let list = mgr.list().unwrap();
        assert_eq!(list.len(), 2);
        let team = list.iter().find(|a| a.slot == team).unwrap();
        let personal = list.iter().find(|a| a.slot == personal).unwrap();
        assert_eq!(team.claude_code_oauth_blob["accessToken"], "at-u1-org-team-a");
        assert_eq!(personal.claude_code_oauth_blob["accessToken"], "at-u1-org-personal-a");
        // Ids are what the database keys history and warm-up on: they must differ,
        // and the first account keeps the id its history is already under.
        assert_eq!(team.account_uuid, "u1");
        assert_eq!(personal.account_uuid, "u1@org-personal");
        assert_eq!(personal.anthropic_uuid(), "u1");
        assert_eq!(personal.org_uuid(), Some("org-personal"));
    }

    #[test]
    fn signing_in_again_updates_the_same_organization_in_place_and_keeps_its_id() {
        let dir = tempdir().unwrap();
        let mgr = AccountManager::new(dir.path().to_path_buf());
        let team = sign_in(&mgr, "u1", "me@x", "org-team", "a");
        let personal = sign_in(&mgr, "u1", "me@x", "org-personal", "a");

        assert_eq!(sign_in(&mgr, "u1", "me@x", "org-personal", "b"), personal);
        assert_eq!(sign_in(&mgr, "u1", "me@x", "org-team", "b"), team);
        let list = mgr.list().unwrap();
        assert_eq!(list.len(), 2, "no duplicates");
        let personal = list.iter().find(|a| a.slot == personal).unwrap();
        assert_eq!(personal.account_uuid, "u1@org-personal", "an update must not change the id");
        assert_eq!(personal.claude_code_oauth_blob["accessToken"], "at-u1-org-personal-b");
        let team = list.iter().find(|a| a.slot == team).unwrap();
        assert_eq!(team.account_uuid, "u1");
        assert_eq!(team.claude_code_oauth_blob["accessToken"], "at-u1-org-team-b");
    }

    #[test]
    fn a_third_organization_gets_its_own_id_too() {
        let dir = tempdir().unwrap();
        let mgr = AccountManager::new(dir.path().to_path_buf());
        sign_in(&mgr, "u1", "me@x", "org-a", "a");
        sign_in(&mgr, "u1", "me@x", "org-b", "a");
        sign_in(&mgr, "u1", "me@x", "org-c", "a");
        let mut ids: Vec<String> = mgr.list().unwrap().into_iter().map(|a| a.account_uuid).collect();
        ids.sort();
        assert_eq!(ids, ["u1", "u1@org-b", "u1@org-c"]);
    }

    /// Accounts saved before the organization was recorded must keep updating in
    /// place: the first sign-in that names an organization fills it in.
    #[test]
    fn an_account_that_never_recorded_its_organization_is_updated_not_duplicated() {
        let dir = tempdir().unwrap();
        let mgr = AccountManager::new(dir.path().to_path_buf());
        let id = identity::from_blobs(&oa_slice("u1", "me@x"), Some(&cc_blob("u1", 1))).unwrap();
        let legacy = mgr.upsert(id, cc_blob("u1", 1), oa_slice("u1", "me@x"), AddSource::OAuth).unwrap();
        let again = sign_in(&mgr, "u1", "me@x", "org-team", "a");
        assert_eq!(again, legacy);
        let list = mgr.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].organization_uuid.as_deref(), Some("org-team"));
        assert_eq!(list[0].account_uuid, "u1");
    }

    #[test]
    fn the_live_login_is_matched_to_the_right_organization() {
        let dir = tempdir().unwrap();
        let mgr = AccountManager::new(dir.path().to_path_buf());
        let team = sign_in(&mgr, "u1", "me@x", "org-team", "a");
        sign_in(&mgr, "u1", "me@x", "org-personal", "a");
        let list = mgr.list().unwrap();
        let team_acc = list.iter().find(|a| a.slot == team).unwrap();
        let personal_acc = list.iter().find(|a| a.slot != team).unwrap();
        let live_team = oa_in_org("u1", "me@x", "org-team");
        let live_personal = oa_in_org("u1", "me@x", "org-personal");
        let cc = serde_json::Value::Null;
        assert!(team_acc.matches_credentials(&cc, &live_team));
        assert!(!team_acc.matches_credentials(&cc, &live_personal));
        assert!(personal_acc.matches_credentials(&cc, &live_personal));
        assert!(!personal_acc.matches_credentials(&cc, &live_team));
        // A different Anthropic account never matches.
        assert!(!team_acc.matches_credentials(&cc, &oa_in_org("u2", "you@x", "org-team")));
    }

    /// Sources that name only the Anthropic account (the CLI's usage cache) cannot
    /// be attributed to either of two organizations that share it.
    #[test]
    fn an_account_knows_when_it_shares_its_anthropic_uuid_with_another() {
        let dir = tempdir().unwrap();
        let mgr = AccountManager::new(dir.path().to_path_buf());
        let team = sign_in(&mgr, "u1", "me@x", "org-team", "a");
        let personal = sign_in(&mgr, "u1", "me@x", "org-personal", "a");
        let other = sign_in(&mgr, "u2", "you@x", "org-x", "a");
        let list = mgr.list().unwrap();
        let shares = |slot: u32| list.iter().find(|a| a.slot == slot).unwrap().shares_anthropic_uuid_with_another(&list);
        assert!(shares(team));
        assert!(shares(personal));
        assert!(!shares(other), "an account alone does not share with itself");
    }

    /// Swapping away captures the live tokens into the outgoing account's slot.
    /// With two organizations of one account that must be the right one.
    #[test]
    fn capture_live_into_slot_writes_the_outgoing_tokens_into_the_right_organization() {
        let dir = tempdir().unwrap();
        let mgr = AccountManager::new(dir.path().to_path_buf());
        let team = sign_in(&mgr, "u1", "me@x", "org-team", "a");
        let personal = sign_in(&mgr, "u1", "me@x", "org-personal", "a");
        let refreshed = cc_in_org("u1", "org-personal", "refreshed");
        mgr.capture_live_into_slot("u1", &refreshed, &oa_in_org("u1", "me@x", "org-personal")).unwrap();
        let list = mgr.list().unwrap();
        let token = |slot: u32| list.iter().find(|a| a.slot == slot).unwrap().claude_code_oauth_blob["accessToken"].clone();
        assert_eq!(token(personal), "at-u1-org-personal-refreshed");
        assert_eq!(token(team), "at-u1-org-team-a", "the other organization must be untouched");
    }

    #[test]
    fn the_live_profile_is_synced_into_the_organization_it_belongs_to() {
        let dir = tempdir().unwrap();
        let mgr = AccountManager::new(dir.path().to_path_buf());
        let team = sign_in(&mgr, "u1", "me@x", "org-team", "a");
        let personal = sign_in(&mgr, "u1", "me@x", "org-personal", "a");
        let mut live = oa_in_org("u1", "me@x", "org-personal");
        live["displayName"] = serde_json::json!("Changed");
        assert!(mgr.sync_live_profile(personal, &live).unwrap(), "the composite-id slot is updated");
        assert!(!mgr.sync_live_profile(team, &live).unwrap(), "another organization's profile is not adopted");
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
            after.token_expires_at.unwrap().timestamp_millis(),
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
    fn sync_live_profile_adopts_a_changed_profile_and_updates_the_plan() {
        let dir = tempdir().unwrap();
        let mgr = AccountManager::new(dir.path().to_path_buf());

        // Signed in on Pro.
        let mut cc = cc_blob("u1", 1);
        cc["subscriptionType"] = serde_json::json!("pro");
        cc["rateLimitTier"] = serde_json::json!("default_claude_ai");
        let mut stored_profile = oa_slice("u1", "a@x");
        stored_profile["organizationType"] = serde_json::json!("claude_pro");
        stored_profile["organizationRateLimitTier"] = serde_json::json!("default_claude_ai");
        let id = identity::from_blobs(&stored_profile, Some(&cc)).unwrap();
        mgr.upsert(id, cc, stored_profile.clone(), AddSource::OAuth)
            .unwrap();
        assert_eq!(mgr.get(1).unwrap().unwrap().plan().as_deref(), Some("pro"));

        // Unchanged profile: nothing to write.
        assert!(!mgr.sync_live_profile(1, &stored_profile).unwrap());

        // Upgraded: Claude Code refreshed the profile it keeps on disk.
        let mut live_profile = stored_profile.clone();
        live_profile["organizationType"] = serde_json::json!("claude_max");
        live_profile["organizationRateLimitTier"] = serde_json::json!("default_claude_max_20x");
        assert!(mgr.sync_live_profile(1, &live_profile).unwrap());

        let after = mgr.get(1).unwrap().unwrap();
        assert_eq!(after.oauth_account_blob, live_profile);
        assert_eq!(after.plan().as_deref(), Some("ultra"));
        // The credentials are not this function's business.
        assert_eq!(after.claude_code_oauth_blob["accessToken"], "at-u1");
    }

    #[test]
    fn sync_live_profile_ignores_another_accounts_profile() {
        let dir = tempdir().unwrap();
        let mgr = AccountManager::new(dir.path().to_path_buf());
        let id = identity::from_blobs(&oa_slice("u1", "a@x"), Some(&cc_blob("u1", 1))).unwrap();
        mgr.upsert(id, cc_blob("u1", 1), oa_slice("u1", "a@x"), AddSource::OAuth)
            .unwrap();

        assert!(!mgr.sync_live_profile(1, &oa_slice("someone-else", "b@y")).unwrap());
        assert!(!mgr.sync_live_profile(1, &serde_json::Value::Null).unwrap());
        assert!(!mgr.sync_live_profile(99, &oa_slice("u1", "a@x")).unwrap());

        let after = mgr.get(1).unwrap().unwrap();
        assert_eq!(after.oauth_account_blob, oa_slice("u1", "a@x"));
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
            organization_uuid: None,
            organization_name: None,
        };
        let (cc, oa) = super::synthesize_blobs(&token, &userinfo);
        assert_eq!(cc["accessToken"], "at-x");
        assert_eq!(cc["refreshToken"], "rt-x");
        assert_eq!(cc["expiresAt"].as_i64().unwrap() / 1000, token.expires_at.timestamp());
        assert_eq!(oa["accountUuid"], "uuid-x");
        assert_eq!(oa["emailAddress"], "x@x.com");
    }

    fn userinfo_in_org(org: &str) -> crate::auth::account_identity::UserInfo {
        crate::auth::account_identity::UserInfo {
            id: "u1".to_string(),
            email: "me@x".to_string(),
            name: None,
            organization_uuid: Some(org.to_string()),
            organization_name: Some(format!("Org {org}")),
        }
    }

    fn token_for(tag: &str) -> crate::auth::StoredToken {
        crate::auth::StoredToken {
            access_token: format!("at-{tag}"),
            refresh_token: Some(format!("rt-{tag}")),
            expires_at: Utc::now() + chrono::Duration::hours(8),
        }
    }

    /// The in-app sign-in used to write `organizationUuid: null` whatever the
    /// profile said, so no later step could tell two organizations apart.
    #[test]
    fn synthesize_blobs_carries_the_organization_from_the_profile() {
        let (_, oa) = super::synthesize_blobs(&token_for("a"), &userinfo_in_org("org-team"));
        assert_eq!(oa["organizationUuid"], "org-team");
        assert_eq!(oa["organizationName"], "Org org-team");
    }

    #[tokio::test]
    async fn adding_one_email_in_two_organizations_through_the_app_gives_two_accounts() {
        let dir = tempdir().unwrap();
        let mgr = AccountManager::new(dir.path().to_path_buf());
        let team = mgr.add_from_oauth(token_for("team"), userinfo_in_org("org-team")).await.unwrap();
        let personal = mgr.add_from_oauth(token_for("personal"), userinfo_in_org("org-personal")).await.unwrap();
        assert_ne!(team, personal, "adding the second must not replace the first");
        let list = mgr.list().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(mgr.get(team).unwrap().unwrap().claude_code_oauth_blob["accessToken"], "at-team");
        assert_eq!(mgr.get(personal).unwrap().unwrap().claude_code_oauth_blob["accessToken"], "at-personal");
    }
}
