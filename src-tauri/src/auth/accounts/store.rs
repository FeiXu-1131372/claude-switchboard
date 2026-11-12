//! On-disk store for managed accounts. Single JSON file at
//! `<app-data-dir>/accounts.json`, protected by a sibling `.accounts.lock`
//! file. Atomic write: temp + rename.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, specta::Type)]
pub enum AddSource {
    OAuth,
    ImportedFromClaudeCode,
    LongLivedToken,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ManagedAccount {
    pub slot: u32,
    pub email: String,
    #[serde(default)]
    pub label: Option<String>,
    pub account_uuid: String,
    pub organization_uuid: Option<String>,
    pub organization_name: Option<String>,
    pub subscription_type: Option<String>,
    pub source: AddSource,
    pub claude_code_oauth_blob: serde_json::Value,
    pub oauth_account_blob: serde_json::Value,
    pub token_expires_at: Option<DateTime<Utc>>,
    pub added_at: DateTime<Utc>,
    pub last_seen_active: Option<DateTime<Utc>>,
}

impl ManagedAccount {
    pub fn display_name(&self) -> String {
        self.label.clone().unwrap_or_else(|| {
            if self.email.is_empty() {
                format!("Token account {}", self.slot)
            } else {
                self.email.clone()
            }
        })
    }

    /// The plan this account is on, as current as the stored blobs are.
    /// `subscription_type` is the label saved when the account was added and
    /// is only the fallback: it goes stale as soon as the plan changes.
    pub fn plan(&self) -> Option<String> {
        super::identity::plan_from_blobs(
            Some(&self.claude_code_oauth_blob),
            Some(&self.oauth_account_blob),
        )
        .or_else(|| self.subscription_type.clone())
    }

    pub fn supports_usage(&self) -> bool {
        self.claude_code_oauth_blob["scopes"]
            .as_array()
            .map(|scopes| scopes.iter().any(|scope| scope == "user:profile"))
            .unwrap_or(self.source != AddSource::LongLivedToken)
    }

    pub fn can_refresh(&self) -> bool {
        self.claude_code_oauth_blob["refreshToken"]
            .as_str()
            .is_some_and(|token| !token.is_empty())
    }

    pub fn matches_credentials(
        &self,
        cc: &serde_json::Value,
        oauth_account: &serde_json::Value,
    ) -> bool {
        if self.source == AddSource::LongLivedToken {
            return self.claude_code_oauth_blob["accessToken"]
                .as_str()
                .filter(|token| !token.is_empty())
                .is_some_and(|token| cc["accessToken"].as_str() == Some(token));
        }
        self.is_identity(
            oauth_account["accountUuid"].as_str().unwrap_or_default(),
            oauth_account["organizationUuid"].as_str(),
        )
    }

    /// The Anthropic account uuid.
    ///
    /// `account_uuid` is this app's id for the account (database rows and
    /// warm-up are keyed on it). Usually it *is* the Anthropic uuid. A Team seat
    /// and a personal login under one email are two organizations of one
    /// Anthropic account, with separate limits, so they share that uuid: the
    /// second and later ones carry their organization in the id
    /// (`<uuid>@<organization uuid>`) to stay distinct. Anything that compares
    /// against what Claude Code, the API or the status line says uses this.
    pub fn anthropic_uuid(&self) -> &str {
        self.account_uuid
            .split_once('@')
            .map_or(self.account_uuid.as_str(), |(uuid, _)| uuid)
    }

    /// The organization this account signs in to, as best it is known: Claude
    /// Code's own copy of the profile is the freshest, then what was saved when
    /// the account was added, then the id's suffix. `None` for an account that
    /// never recorded one (older saves, a token with no profile).
    pub fn org_uuid(&self) -> Option<&str> {
        self.oauth_account_blob["organizationUuid"]
            .as_str()
            .filter(|org| !org.is_empty())
            .or_else(|| self.organization_uuid.as_deref().filter(|org| !org.is_empty()))
            .or_else(|| self.account_uuid.split_once('@').map(|(_, org)| org))
    }

    /// Whether `uuid` and `org`, as found in a live login, a sign-in or a
    /// status-line reading, name this account. The organizations must agree
    /// when both are known; one that is unknown on either side cannot
    /// disagree, so an account saved before organizations were recorded still
    /// matches its own sign-ins.
    pub fn is_identity(&self, uuid: &str, org: Option<&str>) -> bool {
        self.anthropic_uuid() == uuid
            && match (self.org_uuid(), org.filter(|org| !org.is_empty())) {
                (Some(mine), Some(theirs)) => mine == theirs,
                _ => true,
            }
    }

    /// Whether another saved account is the same Anthropic account in another
    /// organization. A source that names only the Anthropic account (the CLI's
    /// usage cache) cannot be attributed to either of the two.
    pub fn shares_anthropic_uuid_with_another(&self, all: &[ManagedAccount]) -> bool {
        all.iter()
            .any(|other| other.slot != self.slot && other.anthropic_uuid() == self.anthropic_uuid())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountsStore {
    pub schema_version: u32,
    pub accounts: BTreeMap<u32, ManagedAccount>,
}

impl AccountsStore {
    pub const CURRENT_SCHEMA_VERSION: u32 = 1;

    pub fn next_slot(&self) -> u32 {
        self.accounts.keys().max().copied().unwrap_or(0) + 1
    }

    pub fn find_by_account_uuid(&self, uuid: &str) -> Option<&ManagedAccount> {
        self.accounts.values().find(|a| a.account_uuid == uuid)
    }

    /// The account a sign-in or live login with this Anthropic uuid and
    /// organization belongs to. An exact organization match wins over one that
    /// is only compatible (an account that never recorded its organization).
    pub fn find_by_identity(&self, uuid: &str, org: Option<&str>) -> Option<&ManagedAccount> {
        let org = org.filter(|org| !org.is_empty());
        self.accounts
            .values()
            .find(|a| org.is_some() && a.org_uuid() == org && a.is_identity(uuid, org))
            .or_else(|| self.accounts.values().find(|a| a.is_identity(uuid, org)))
    }

    /// The id for a *new* account with this Anthropic uuid and organization,
    /// when [`Self::find_by_identity`] found none: the plain uuid, unless
    /// another organization of the same Anthropic account is already saved, in
    /// which case the organization is appended so the two stay distinct.
    pub fn local_id_for(&self, uuid: &str, org: Option<&str>) -> String {
        match org.filter(|org| !org.is_empty()) {
            Some(org) if self.accounts.values().any(|a| a.anthropic_uuid() == uuid) => format!("{uuid}@{org}"),
            _ => uuid.to_string(),
        }
    }
}

fn store_path(dir: &Path) -> PathBuf {
    dir.join("accounts.json")
}

fn lock_path(dir: &Path) -> PathBuf {
    dir.join(".accounts.lock")
}

fn corrupt_path(dir: &Path) -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    dir.join(format!("accounts.json.corrupt-{ts}"))
}

pub struct AccountsLock {
    _file: File,
}

pub fn acquire_lock(dir: &Path) -> Result<AccountsLock> {
    std::fs::create_dir_all(dir).context("create accounts dir")?;
    let f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(lock_path(dir))
        .context("open .accounts.lock")?;
    f.try_lock_exclusive()
        .context("another instance holds .accounts.lock")?;
    Ok(AccountsLock { _file: f })
}

pub fn load(dir: &Path) -> Result<AccountsStore> {
    let p = store_path(dir);
    if !p.exists() {
        return Ok(AccountsStore {
            schema_version: AccountsStore::CURRENT_SCHEMA_VERSION,
            accounts: BTreeMap::new(),
        });
    }
    let text = std::fs::read_to_string(&p).context("read accounts.json")?;
    match serde_json::from_str::<AccountsStore>(&text) {
        Ok(store) => Ok(store),
        Err(e) => {
            tracing::warn!(
                "accounts.json corrupt ({e}); backing up and starting fresh"
            );
            let backup = corrupt_path(dir);
            let _ = std::fs::rename(&p, &backup);
            Ok(AccountsStore {
                schema_version: AccountsStore::CURRENT_SCHEMA_VERSION,
                accounts: BTreeMap::new(),
            })
        }
    }
}

pub fn save(dir: &Path, store: &AccountsStore, _lock: &AccountsLock) -> Result<()> {
    std::fs::create_dir_all(dir).context("create accounts dir")?;
    let p = store_path(dir);
    let tmp = dir.join("accounts.json.tmp");
    let payload = serde_json::to_string_pretty(store)?;
    std::fs::write(&tmp, &payload).context("write temp accounts.json")?;
    restrict_permissions(&tmp)?;
    std::fs::rename(&tmp, &p).context("rename temp accounts.json into place")?;
    Ok(())
}

#[cfg(unix)]
fn restrict_permissions(p: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(p)?.permissions();
    perms.set_mode(0o600);
    std::fs::set_permissions(p, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_permissions(_p: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample_account(slot: u32, uuid: &str) -> ManagedAccount {
        ManagedAccount {
            slot,
            email: format!("user{slot}@x.com"),
            label: None,
            account_uuid: uuid.to_string(),
            organization_uuid: None,
            organization_name: None,
            subscription_type: Some("max".to_string()),
            source: AddSource::ImportedFromClaudeCode,
            claude_code_oauth_blob: serde_json::json!({
                "accessToken": "a",
                "refreshToken": "r",
                "expiresAt": 1840000000000_i64,
                "scopes": ["user:inference"],
                "subscriptionType": "max"
            }),
            oauth_account_blob: serde_json::json!({
                "accountUuid": uuid,
                "emailAddress": format!("user{slot}@x.com"),
                "organizationUuid": null,
                "organizationName": null
            }),
            token_expires_at: Some(Utc::now()),
            added_at: Utc::now(),
            last_seen_active: None,
        }
    }

    #[test]
    fn plan_prefers_the_blobs_and_falls_back_to_the_saved_label() {
        // Signed in on Pro, upgraded later: the label saved at import and
        // the credentials still say Pro, the profile says Max 20x.
        let mut upgraded = sample_account(1, "u1");
        upgraded.subscription_type = Some("pro".to_string());
        upgraded.claude_code_oauth_blob["subscriptionType"] = serde_json::json!("pro");
        upgraded.oauth_account_blob["organizationType"] = serde_json::json!("claude_max");
        upgraded.oauth_account_blob["organizationRateLimitTier"] =
            serde_json::json!("default_claude_max_20x");
        assert_eq!(upgraded.plan().as_deref(), Some("ultra"));

        // Nothing in either blob: the saved label is all there is.
        let mut bare = sample_account(2, "u2");
        bare.claude_code_oauth_blob = serde_json::json!({ "accessToken": "a" });
        assert_eq!(bare.plan().as_deref(), Some("max"));
    }

    #[test]
    fn round_trip_multiple_accounts_preserves_unknown_fields() {
        let dir = tempdir().unwrap();
        let lock = acquire_lock(dir.path()).unwrap();
        let mut store = AccountsStore {
            schema_version: 1,
            accounts: BTreeMap::new(),
        };
        let mut a = sample_account(1, "uuid-a");
        a.claude_code_oauth_blob["futureField"] =
            serde_json::Value::String("preserved".to_string());
        store.accounts.insert(1, a);
        store.accounts.insert(2, sample_account(2, "uuid-b"));

        save(dir.path(), &store, &lock).unwrap();
        drop(lock);

        let loaded = load(dir.path()).unwrap();
        assert_eq!(loaded.accounts.len(), 2);
        assert_eq!(
            loaded.accounts[&1].claude_code_oauth_blob["futureField"],
            "preserved"
        );
        assert_eq!(loaded.find_by_account_uuid("uuid-b").unwrap().slot, 2);
    }

    #[test]
    fn corrupt_file_backs_up_and_returns_empty() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("accounts.json"), "not json{").unwrap();
        let loaded = load(dir.path()).unwrap();
        assert_eq!(loaded.accounts.len(), 0);
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().into_string().unwrap_or_default())
            .collect();
        assert!(
            entries.iter().any(|n| n.starts_with("accounts.json.corrupt-")),
            "expected backup file; got {entries:?}"
        );
    }

    #[test]
    fn next_slot_starts_at_one_and_increments() {
        let mut store = AccountsStore {
            schema_version: 1,
            accounts: BTreeMap::new(),
        };
        assert_eq!(store.next_slot(), 1);
        store.accounts.insert(1, sample_account(1, "u1"));
        store.accounts.insert(3, sample_account(3, "u3"));
        assert_eq!(store.next_slot(), 4);
    }

    #[test]
    fn double_lock_is_rejected() {
        let dir = tempdir().unwrap();
        let _first = acquire_lock(dir.path()).unwrap();
        let second = acquire_lock(dir.path());
        assert!(second.is_err());
    }
}
