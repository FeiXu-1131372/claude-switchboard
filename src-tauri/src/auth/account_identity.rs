use super::AccountId;
use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::sync::Arc;

pub const USERINFO_URL: &str = "https://api.anthropic.com/api/oauth/userinfo";
const ANTHROPIC_BETA: &str = "oauth-2025-04-20";

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct UserInfo {
    #[serde(rename = "sub")]
    pub id: String,
    pub email: String,
    pub name: Option<String>,
}

pub struct IdentityFetcher {
    endpoint: String,
    client: Arc<reqwest::Client>,
}

impl IdentityFetcher {
    pub fn new(client: Arc<reqwest::Client>) -> Self {
        Self {
            endpoint: USERINFO_URL.to_string(),
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
        tracing::debug!(target: "switchboard.http", "GET {} (userinfo) starting", self.endpoint);
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
                "GET /userinfo → {status}; body: {text}"
            );
            return Err(anyhow!("userinfo failed: {status}"));
        }
        let info: UserInfo = resp.json().await?;
        tracing::info!(
            target: "switchboard.http",
            status = status.as_u16(),
            elapsed_ms,
            "GET /userinfo → 200 (sub={}, email={})",
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
