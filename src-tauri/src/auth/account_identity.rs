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
    /// The organization this sign-in belongs to. One Anthropic account can sit
    /// in several (a Team seat and a personal login under one email), each with
    /// its own limits, so this is half of what makes an account *this* account.
    pub organization_uuid: Option<String>,
    pub organization_name: Option<String>,
}

#[derive(Deserialize)]
struct ProfileResponse {
    account: ProfileAccount,
    #[serde(default)]
    organization: Option<ProfileOrganization>,
}

#[derive(Deserialize)]
struct ProfileOrganization {
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

impl ProfileResponse {
    fn into_user_info(self) -> UserInfo {
        let organization = self.organization;
        let non_empty = |text: Option<String>| text.filter(|t| !t.trim().is_empty());
        UserInfo {
            id: self.account.uuid,
            email: self.account.email,
            name: self.account.display_name,
            organization_uuid: non_empty(organization.as_ref().and_then(|o| o.uuid.clone())),
            organization_name: non_empty(organization.and_then(|o| o.name)),
        }
    }
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

    pub async fn fetch_for_token_import(&self, access_token: &str) -> Result<Option<UserInfo>> {
        let response = self.client
            .get(&self.endpoint)
            .bearer_auth(access_token)
            .header("anthropic-beta", ANTHROPIC_BETA)
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
            .map_err(|_| anyhow!("Could not check the token with Anthropic. Check your connection and try again."))?;
        match response.status() {
            reqwest::StatusCode::FORBIDDEN => Ok(None),
            reqwest::StatusCode::UNAUTHORIZED => {
                Err(anyhow!("Anthropic rejected this token. It may be expired or revoked."))
            }
            status if status.is_success() => {
                let profile: ProfileResponse = response.json().await
                    .map_err(|_| anyhow!("Anthropic returned an unreadable account profile."))?;
                if profile.account.uuid.trim().is_empty() || profile.account.email.trim().is_empty() {
                    return Err(anyhow!("Anthropic returned an incomplete account profile."));
                }
                Ok(Some(profile.into_user_info()))
            }
            status => Err(anyhow!("Could not check the token with Anthropic (HTTP {}). Try again later.", status.as_u16())),
        }
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
        let info = profile.into_user_info();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn body(organization: Option<serde_json::Value>) -> String {
        let mut v = serde_json::json!({
            "account": { "uuid": "u1", "email": "me@x", "display_name": "Me" }
        });
        if let Some(org) = organization {
            v["organization"] = org;
        }
        v.to_string()
    }

    async fn serve(status: usize, text: String) -> (mockito::ServerGuard, IdentityFetcher) {
        let mut server = mockito::Server::new_async().await;
        server.mock("GET", "/profile").with_status(status).with_body(text).create_async().await;
        let fetcher = IdentityFetcher::with_endpoint(format!("{}/profile", server.url()));
        (server, fetcher)
    }

    /// A Team seat and a personal login under one email are told apart only by
    /// the organization, so the profile's organization must survive parsing.
    #[tokio::test]
    async fn the_profile_carries_the_organization() {
        let org = serde_json::json!({ "uuid": "org-1", "name": "Catchsoftware" });
        let (_server, fetcher) = serve(200, body(Some(org))).await;
        let info = fetcher.fetch("tok").await.unwrap();
        assert_eq!(info.id, "u1");
        assert_eq!(info.organization_uuid.as_deref(), Some("org-1"));
        assert_eq!(info.organization_name.as_deref(), Some("Catchsoftware"));
    }

    #[tokio::test]
    async fn a_profile_without_an_organization_still_parses() {
        let (_server, fetcher) = serve(200, body(None)).await;
        let info = fetcher.fetch("tok").await.unwrap();
        assert_eq!((info.organization_uuid, info.organization_name), (None, None));
        // An organization object with nothing usable in it is no organization.
        let (_server, fetcher) = serve(200, body(Some(serde_json::json!({ "uuid": "", "name": null })))).await;
        assert_eq!(fetcher.fetch("tok").await.unwrap().organization_uuid, None);
    }

    #[tokio::test]
    async fn the_token_import_profile_carries_the_organization_too() {
        let org = serde_json::json!({ "uuid": "org-2", "name": "Personal" });
        let (_server, fetcher) = serve(200, body(Some(org))).await;
        let info = fetcher.fetch_for_token_import("tok").await.unwrap().unwrap();
        assert_eq!(info.organization_uuid.as_deref(), Some("org-2"));
    }
}
