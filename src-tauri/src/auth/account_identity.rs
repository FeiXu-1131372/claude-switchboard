use super::AccountId;
use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::sync::Arc;

// `/api/oauth/userinfo` (the OIDC convention) returns 404 on
// api.anthropic.com. Claude Code calls `/api/oauth/profile` instead — see
// claude-code's services/oauth/getOauthProfile.ts. The profile response is
// nested `{ account: { uuid, email, display_name }, organization: {...} }`
// rather than flat OIDC claims, which is why ProfileResponse is separate
// from the public UserInfo struct.
pub const PROFILE_URL: &str = "https://api.anthropic.com/api/oauth/profile";
const ANTHROPIC_BETA: &str = "oauth-2025-04-20";

#[derive(Debug, Clone, PartialEq)]
pub struct UserInfo {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
}

#[derive(Deserialize)]
struct ProfileResponse {
    account: ProfileAccount,
}

#[derive(Deserialize)]
struct ProfileAccount {
    uuid: String,
    email: String,
    display_name: Option<String>,
}

pub struct IdentityFetcher {
    endpoint: String,
    client: Arc<reqwest::Client>,
}

impl IdentityFetcher {
    pub fn new(client: Arc<reqwest::Client>) -> Self {
        Self {
            endpoint: PROFILE_URL.to_string(),
            client,
        }
    }

    /// Test-only constructor: builds a fresh client pointed at a mock endpoint.
    pub fn with_endpoint(endpoint: String) -> Self {
        Self {
            endpoint,
            client: Arc::new(reqwest::Client::new()),
        }
    }

    pub fn client_arc(&self) -> Arc<reqwest::Client> {
        self.client.clone()
    }

    pub async fn fetch(&self, access_token: &str) -> Result<UserInfo> {
        tracing::debug!(target: "switchboard.http", "GET {} (profile) starting", self.endpoint);
        let start = std::time::Instant::now();
        let resp = self
            .client
            .get(&self.endpoint)
            .bearer_auth(access_token)
            .header("anthropic-beta", ANTHROPIC_BETA)
            .send()
            .await?;
        let status = resp.status();
        let elapsed_ms = start.elapsed().as_millis() as u64;
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            tracing::warn!(
                target: "switchboard.http",
                status = status.as_u16(),
                elapsed_ms,
                "GET /profile → {status}; body: {text}"
            );
            return Err(anyhow!("profile failed: {status}"));
        }
        let profile: ProfileResponse = resp.json().await?;
        let info = UserInfo {
            id: profile.account.uuid,
            email: profile.account.email,
            name: profile.account.display_name,
        };
        tracing::info!(
            target: "switchboard.http",
            status = status.as_u16(),
            elapsed_ms,
            "GET /profile → 200 (sub={}, email={})",
            info.id,
            info.email
        );
        Ok(info)
    }
}

impl From<&UserInfo> for AccountId {
    fn from(u: &UserInfo) -> Self {
        AccountId(u.id.clone())
    }
}
