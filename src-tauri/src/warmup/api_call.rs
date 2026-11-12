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
    tracing::debug!(target: "switchboard.warmup", "POST {} starting (model={WARMUP_MODEL})", endpoint);
    let start = std::time::Instant::now();
    let resp = http
        .post(endpoint)
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
        .await?;
    let status = resp.status();
    tracing::info!(
        target: "switchboard.warmup",
        status = status.as_u16(),
        elapsed_ms = start.elapsed().as_millis() as u64,
        "POST /v1/messages (warm-up) → {}",
        status
    );
    Ok(resp)
}

/// How much of a refusal's body goes in the log.
const BODY_LOG_LIMIT: usize = 600;

/// How long to wait for a refusal's body before logging without it.
const BODY_READ_TIMEOUT: Duration = Duration::from_secs(5);

/// One log line for a response that was not a success: its status, the few
/// headers that explain a refusal (`request-id`, `retry-after`,
/// `x-should-retry`, and every `anthropic-ratelimit-*`), and the start of the
/// body.
///
/// An allowlist, never a denylist: the log is a local file a person may attach
/// to a bug report, and nothing that authenticates a request (or a person)
/// may reach it.
pub(crate) fn describe_refusal(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: &[u8],
) -> String {
    let mut kept: Vec<String> = headers
        .iter()
        .filter(|(name, _)| {
            let name = name.as_str();
            matches!(name, "request-id" | "retry-after" | "x-should-retry")
                || name.starts_with("anthropic-ratelimit-")
        })
        .filter_map(|(name, value)| value.to_str().ok().map(|v| format!("{}={v}", name.as_str())))
        .collect();
    kept.sort();

    let text = String::from_utf8_lossy(body);
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let body_part = if compact.is_empty() {
        "body: (empty)".to_string()
    } else {
        let shown: String = compact.chars().take(BODY_LOG_LIMIT).collect();
        let cut = if compact.chars().count() > BODY_LOG_LIMIT { "…" } else { "" };
        format!("body: {shown}{cut}")
    };

    let mut parts = vec![format!("HTTP {}", status.as_u16())];
    parts.extend(kept);
    parts.push(body_part);
    parts.join("; ")
}

/// Read a refused response and describe it. Consumes the response: only for a
/// status that is about to be classified and dropped.
pub(crate) async fn describe_response(resp: reqwest::Response) -> String {
    let status = resp.status();
    let headers = resp.headers().clone();
    let body = match tokio::time::timeout(BODY_READ_TIMEOUT, resp.bytes()).await {
        Ok(Ok(bytes)) => bytes,
        _ => Default::default(),
    };
    describe_refusal(status, &headers, &body)
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

    fn headers(pairs: &[(&str, &str)]) -> reqwest::header::HeaderMap {
        let mut map = reqwest::header::HeaderMap::new();
        for (k, v) in pairs {
            map.insert(
                reqwest::header::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                reqwest::header::HeaderValue::from_str(v).unwrap(),
            );
        }
        map
    }

    #[test]
    fn a_refusal_is_described_with_its_status_rate_limit_headers_and_body() {
        let line = describe_refusal(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            &headers(&[
                ("request-id", "req_011CXYZ"),
                ("retry-after", "1800"),
                ("anthropic-ratelimit-unified-status", "rejected"),
                ("anthropic-ratelimit-unified-5h-reset", "1794455732"),
                ("content-type", "application/json"),
            ]),
            br#"{"type":"error","error":{"type":"rate_limit_error","message":"Number of request tokens has exceeded your per-minute rate limit"}}"#,
        );
        assert!(line.starts_with("HTTP 429"), "{line}");
        for want in [
            "request-id=req_011CXYZ",
            "retry-after=1800",
            "anthropic-ratelimit-unified-status=rejected",
            "anthropic-ratelimit-unified-5h-reset=1794455732",
            "rate_limit_error",
            "per-minute rate limit",
        ] {
            assert!(line.contains(want), "missing {want:?} in {line}");
        }
        assert!(!line.contains("content-type"), "only an allowlist of headers is kept: {line}");
    }

    /// The log is a plain local file a user may attach to a bug report: nothing
    /// that authenticates a request or a person may reach it.
    #[test]
    fn a_refusal_never_logs_credentials_or_cookies() {
        let line = describe_refusal(
            reqwest::StatusCode::FORBIDDEN,
            &headers(&[
                ("authorization", "Bearer sk-ant-oat01-SECRET"),
                ("x-api-key", "sk-ant-api03-SECRET"),
                ("set-cookie", "session=SECRET"),
                ("request-id", "req_1"),
            ]),
            b"{}",
        );
        assert!(!line.contains("SECRET"), "{line}");
        assert!(line.contains("request-id=req_1"));
    }

    #[test]
    fn a_long_body_is_cut_on_a_character_boundary_and_whitespace_is_collapsed() {
        let body = format!("{{\n  \"message\": \"{}\"\n}}", "é".repeat(2000));
        let line = describe_refusal(reqwest::StatusCode::BAD_REQUEST, &headers(&[]), body.as_bytes());
        assert!(line.ends_with('…'), "a cut body says so: {}", &line[line.len().saturating_sub(20)..]);
        assert!(line.chars().count() < 800, "{} chars", line.chars().count());
        assert!(!line.contains('\n'), "one line in the log");
        assert!(line.contains("{ \"message\": \"é"), "{line}");
    }

    #[test]
    fn an_empty_or_binary_body_still_gives_a_line() {
        let empty = describe_refusal(reqwest::StatusCode::SERVICE_UNAVAILABLE, &headers(&[]), b"");
        assert_eq!(empty, "HTTP 503; body: (empty)");
        let binary = describe_refusal(reqwest::StatusCode::BAD_GATEWAY, &headers(&[]), &[0xff, 0xfe, b'o', b'k']);
        assert!(binary.starts_with("HTTP 502; body: "), "{binary}");
    }

    /// End to end through the real HTTP client: a 429 with a JSON body is read,
    /// not discarded, and what came back is in the log line.
    #[tokio::test]
    async fn a_refused_response_is_read_and_described() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/")
            .with_status(429)
            .with_header("retry-after", "1800")
            .with_header("anthropic-ratelimit-unified-status", "rejected")
            .with_body(r#"{"error":{"type":"rate_limit_error","message":"limit"}}"#)
            .create_async()
            .await;
        let resp = issue_to(&reqwest::Client::new(), "t", &server.url()).await.unwrap();
        let line = describe_response(resp).await;
        assert!(line.contains("HTTP 429") && line.contains("retry-after=1800"), "{line}");
        assert!(line.contains("rate_limit_error"), "{line}");
    }
}
