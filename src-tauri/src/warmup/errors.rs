//! Maps HTTP / network outcomes to WarmupOutcome variants per spec §6's
//! failure-handling table.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub enum WarmupOutcome {
    /// 200 OK — the call started a fresh 5h window.
    Success,
    /// Active-window precondition: bucket already running (no HTTP issued).
    SkippedAlreadyActive,
    /// 401 / 403 — token expired or revoked. Slot needs reauth.
    NeedsReauth,
    /// 429 with the headers of a subscription usage limit: the account really
    /// is at a limit.
    AtRateLimit,
    /// The request was refused, and not because of a usage limit: a 429 that
    /// carries none of the `anthropic-ratelimit-*` headers a limit does. Seen
    /// in the wild on an account at 0% of its 5-hour window. `detail` says what
    /// came back (status, request id) in a form a person can read.
    Refused { detail: String },
    /// 5xx — Anthropic server. Retry-once policy already applied.
    AnthropicServerError,
    /// Network failure or timeout.
    NetworkError,
    /// Other / unknown HTTP status.
    OtherFailure { status: u16 },
}

/// Classify an HTTP status code into a `WarmupOutcome`.
pub fn classify_status(status: u16) -> WarmupOutcome {
    match status {
        200 => WarmupOutcome::Success,
        401 | 403 => WarmupOutcome::NeedsReauth,
        429 => WarmupOutcome::AtRateLimit,
        500..=599 => WarmupOutcome::AnthropicServerError,
        other => WarmupOutcome::OtherFailure { status: other },
    }
}

/// Classify a response by status and headers. Like [`classify_status`], except
/// that a 429 is only a usage limit when it says so: a subscription limit comes
/// with `anthropic-ratelimit-*` headers (Claude Code builds its limit messages
/// from them) or a `retry-after`. A bare 429 is a refusal.
pub fn classify_response(status: u16, headers: &reqwest::header::HeaderMap) -> WarmupOutcome {
    if status == 429 && !has_quota_signal(headers) {
        let detail = match headers.get("request-id").and_then(|v| v.to_str().ok()) {
            Some(id) => format!("HTTP 429, not a usage limit; request {id}"),
            None => "HTTP 429, not a usage limit".to_string(),
        };
        return WarmupOutcome::Refused { detail };
    }
    classify_status(status)
}

fn has_quota_signal(headers: &reqwest::header::HeaderMap) -> bool {
    headers.contains_key("retry-after")
        || headers.keys().any(|name| name.as_str().starts_with("anthropic-ratelimit-"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_success() {
        assert_eq!(classify_status(200), WarmupOutcome::Success);
    }

    #[test]
    fn classify_auth_failures() {
        assert_eq!(classify_status(401), WarmupOutcome::NeedsReauth);
        assert_eq!(classify_status(403), WarmupOutcome::NeedsReauth);
    }

    #[test]
    fn classify_rate_limit() {
        assert_eq!(classify_status(429), WarmupOutcome::AtRateLimit);
    }

    #[test]
    fn classify_server_error_range() {
        for s in [500, 502, 503, 504, 599] {
            assert_eq!(classify_status(s), WarmupOutcome::AnthropicServerError);
        }
    }

    #[test]
    fn classify_other() {
        assert_eq!(
            classify_status(418),
            WarmupOutcome::OtherFailure { status: 418 },
        );
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

    /// A subscription usage limit is announced by the `anthropic-ratelimit-*`
    /// headers: Claude Code reads its limit messages from them.
    #[test]
    fn a_429_with_rate_limit_headers_is_a_real_usage_limit() {
        let h = headers(&[("anthropic-ratelimit-unified-status", "rejected"), ("request-id", "req_1")]);
        assert_eq!(classify_response(429, &h), WarmupOutcome::AtRateLimit);
    }

    #[test]
    fn a_429_with_retry_after_is_a_real_limit_too() {
        assert_eq!(classify_response(429, &headers(&[("retry-after", "120")])), WarmupOutcome::AtRateLimit);
    }

    /// What eric.xu's account got on 2026-11-12, four times, while its 5-hour
    /// window read 0%: a generic `rate_limit_error`, no quota headers at all.
    #[test]
    fn a_429_with_no_quota_signal_is_a_refusal_not_a_limit() {
        let h = headers(&[("request-id", "req_011CgxJct4aADvpUp8uhsiaL"), ("x-should-retry", "true")]);
        match classify_response(429, &h) {
            WarmupOutcome::Refused { detail } => {
                assert!(detail.contains("HTTP 429"), "{detail}");
                assert!(detail.contains("not a usage limit"), "{detail}");
                assert!(detail.contains("req_011CgxJct4aADvpUp8uhsiaL"), "the request id is what support asks for: {detail}");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
        match classify_response(429, &headers(&[])) {
            WarmupOutcome::Refused { detail } => assert!(!detail.contains("request"), "{detail}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn every_other_status_classifies_as_before() {
        let h = headers(&[]);
        assert_eq!(classify_response(200, &h), WarmupOutcome::Success);
        assert_eq!(classify_response(401, &h), WarmupOutcome::NeedsReauth);
        assert_eq!(classify_response(529, &h), WarmupOutcome::AnthropicServerError);
        assert_eq!(classify_response(418, &h), WarmupOutcome::OtherFailure { status: 418 });
    }
}
