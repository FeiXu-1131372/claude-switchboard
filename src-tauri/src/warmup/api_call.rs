//! The actual HTTP POST to /v1/messages. Per spec §6 wire-level shape.

use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::config::{WARMUP_HTTP_TIMEOUT_SECS, WARMUP_MODEL};

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";

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
    http.post(ENDPOINT)
        .bearer_auth(oauth_token)
        .header("anthropic-version", "2023-06-01")
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
}
