use super::{
    account_identity::IdentityFetcher,
    exchange::TokenExchange,
    oauth_paste_back::PkcePair,
};
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

pub struct AuthOrchestrator {
    pub fallback_dir: PathBuf,
    pub exchange: TokenExchange,
    pub identity: IdentityFetcher,
    #[allow(dead_code)]
    pub pending_oauth: RwLock<Option<(PkcePair, Instant)>>,
}

impl AuthOrchestrator {
    pub fn new(fallback_dir: PathBuf, client: Arc<reqwest::Client>) -> Self {
        Self {
            fallback_dir,
            exchange: TokenExchange::new(client.clone()),
            identity: IdentityFetcher::new(client),
            pending_oauth: RwLock::new(None),
        }
    }

    pub fn with_collaborators(
        fallback_dir: PathBuf,
        exchange: TokenExchange,
        identity: IdentityFetcher,
    ) -> Self {
        Self {
            fallback_dir,
            exchange,
            identity,
            pending_oauth: RwLock::new(None),
        }
    }

    pub fn identity_arc(&self) -> std::sync::Arc<crate::auth::account_identity::IdentityFetcher> {
        std::sync::Arc::new(crate::auth::account_identity::IdentityFetcher::new(
            self.identity.client_arc(),
        ))
    }
}

#[derive(Debug, Clone)]
pub struct LiveClaudeCode {
    pub claude_code_oauth_blob: serde_json::Value,
    pub oauth_account_blob: serde_json::Value,
    pub account_uuid: String,
    pub email: String,
}

impl LiveClaudeCode {
    pub fn from_blobs(
        claude_code_oauth_blob: serde_json::Value,
        oauth_account_blob: Option<serde_json::Value>,
    ) -> anyhow::Result<Self> {
        let token = claude_code_oauth_blob["accessToken"].as_str()
            .filter(|token| !token.is_empty())
            .ok_or_else(|| anyhow::anyhow!("live CC credentials have no access token"))?;
        let oauth_account_blob = oauth_account_blob.unwrap_or(serde_json::Value::Null);
        let account_uuid = oauth_account_blob["accountUuid"].as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| crate::auth::accounts::token_import::token_id(token));
        let email = oauth_account_blob["emailAddress"].as_str()
            .unwrap_or("Token account").to_string();
        Ok(Self { claude_code_oauth_blob, oauth_account_blob, account_uuid, email })
    }
}

impl AuthOrchestrator {
    /// Read the live credentials even when an inference-only token has no profile.
    pub async fn read_live_claude_code(&self) -> anyhow::Result<Option<LiveClaudeCode>> {
        let cc_blob = match crate::auth::claude_code_creds::load_full_blob().await? {
            Some(b) => b,
            None => return Ok(None),
        };
        let global = match crate::auth::paths::claude_global_config() {
            Some(p) => p,
            None => return Ok(None),
        };
        let oauth_account = crate::auth::oauth_account_io::read_oauth_account(&global)?;
        Ok(Some(LiveClaudeCode::from_blobs(cc_blob, oauth_account)?))
    }

    /// Returns a usable access token for `slot`.
    /// - Active slot: read straight from CC's live store; never refresh.
    /// - Inactive slot: refresh if expiring within 2 minutes, persist back.
    pub async fn token_for_slot(
        &self,
        slot: u32,
        active_slot: Option<u32>,
        accounts: &crate::auth::accounts::AccountManager,
    ) -> anyhow::Result<String> {
        if Some(slot) == active_slot {
            let live = self
                .read_live_claude_code()
                .await?
                .ok_or_else(|| anyhow::anyhow!("active slot {slot} but no live CC creds"))?;
            return live
                .claude_code_oauth_blob
                .get("accessToken")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .ok_or_else(|| anyhow::anyhow!("live CC blob missing accessToken"));
        }

        let acc = accounts
            .get(slot)?
            .ok_or_else(|| anyhow::anyhow!("slot {slot} not in store"))?;

        let now = chrono::Utc::now();
        if !acc.can_refresh() && acc.token_expires_at.is_some_and(|expiry| expiry <= now) {
            anyhow::bail!("This token has expired. Import a new long-lived token.");
        }
        let needs_refresh = acc.can_refresh()
            && acc.token_expires_at.is_some_and(|expiry| expiry <= now + chrono::Duration::minutes(2));
        if needs_refresh {
            accounts.refresh_inactive(slot, &self.exchange).await?;
            let acc = accounts
                .get(slot)?
                .ok_or_else(|| anyhow::anyhow!("slot {slot} disappeared after refresh"))?;
            return acc
                .claude_code_oauth_blob
                .get("accessToken")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .ok_or_else(|| anyhow::anyhow!("post-refresh blob missing accessToken"));
        }

        acc.claude_code_oauth_blob
            .get("accessToken")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("slot {slot} blob missing accessToken"))
    }
}
