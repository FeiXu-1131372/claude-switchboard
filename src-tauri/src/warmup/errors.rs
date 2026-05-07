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
    /// 429 — already at limit. Window running and at cap.
    AtRateLimit,
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
}
