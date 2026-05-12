//! The actual HTTP POST to /v1/messages. Per spec §6 wire-level shape.

use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::branding;

use super::config::{WARMUP_HTTP_TIMEOUT_SECS, WARMUP_MODEL};

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";

/// Required header when using an OAuth bearer token (rather than an x-api-key)
/// against the public Anthropic endpoints. Same value the userinfo and usage
/// callers send — without it /v1/messages returns 401 for OAuth-auth'd
/// requests.
const ANTHROPIC_BETA: &str = "oauth-2025-04-20";

#[derive(Debug, Clone, Serialize)]
pub struct WarmupRequest {
    pub model: &'static str,
    pub max_tokens: u32,
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub role: &'static str,
    pub content: &'static str,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // fields exist for completeness; we only branch on status code
pub struct WarmupResponse {
    pub id: Option<String>,
    pub r#type: Option<String>,
    pub model: Option<String>,
}

pub fn build_payload() -> WarmupRequest {
    WarmupRequest {
        model: WARMUP_MODEL,
        max_tokens: 1,
        messages: vec![Message {
            role: "user",
            content: "hi",
        }],
    }
}

/// Issue the warm-up call. `oauth_token` is the slot's bearer token.
/// Returns the raw `reqwest::Response` so the caller can inspect status —
/// `errors::classify` does the routing.
pub async fn issue(
    http: &reqwest::Client,
    oauth_token: &str,
) -> Result<reqwest::Response, reqwest::Error> {
    issue_to(http, oauth_token, ENDPOINT).await
}

/// Test-friendly variant: POSTs to an arbitrary URL with the same headers
/// and payload. Production code path goes through `issue`.
pub async fn issue_to(
    http: &reqwest::Client,
    oauth_token: &str,
    endpoint: &str,
) -> Result<reqwest::Response, reqwest::Error> {
    http.post(endpoint)
        .bearer_auth(oauth_token)
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", ANTHROPIC_BETA)
        .header(
            "User-Agent",
            format!(
                "{}/{}",
                branding::USER_AGENT_PREFIX,
                env!("CARGO_PKG_VERSION")
            ),
        )
        .json(&build_payload())
        .timeout(Duration::from_secs(WARMUP_HTTP_TIMEOUT_SECS))
        .send()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_uses_haiku_at_one_max_token() {
        let p = build_payload();
        assert!(p.model.starts_with("claude-haiku"));
        assert_eq!(p.max_tokens, 1);
        assert_eq!(p.messages.len(), 1);
        assert_eq!(p.messages[0].role, "user");
        assert_eq!(p.messages[0].content, "hi");
    }

    #[test]
    fn payload_serializes_to_expected_json_shape() {
        let p = build_payload();
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["max_tokens"], 1);
        assert_eq!(json["messages"][0]["role"], "user");
        assert_eq!(json["messages"][0]["content"], "hi");
        assert!(json["model"].as_str().unwrap().starts_with("claude-haiku"));
    }

    #[tokio::test]
    async fn issue_sends_oauth_beta_and_bearer_and_user_agent_headers() {
        // Regression guard: without anthropic-beta the upstream returns 401
        // for OAuth-authenticated /v1/messages, which is what caused
        // warm-up to silently never succeed. Lock in all three
        // OAuth-required headers + the bearer.
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("POST", "/")
            .match_header("authorization", "Bearer test-token")
            .match_header("anthropic-version", "2023-06-01")
            .match_header("anthropic-beta", "oauth-2025-04-20")
            .match_header(
                "user-agent",
                mockito::Matcher::Regex("^claude-switchboard/".to_string()),
            )
            .with_status(200)
            .with_body("{}")
            .create_async()
            .await;

        let client = reqwest::Client::new();
        let resp = issue_to(&client, "test-token", &server.url()).await.unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        m.assert_async().await;
    }
}
