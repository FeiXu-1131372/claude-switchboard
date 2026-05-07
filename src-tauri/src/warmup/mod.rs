//! Warm-up: issues a 1-token /v1/messages call to start the 5h window
//! deliberately. Per spec §6.

pub mod api_call;
pub mod config;
pub mod errors;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

pub use errors::WarmupOutcome;

/// Whether the slot's 5h bucket is currently active. Read from the most-recent
/// usage snapshot. Spec §6 active-window precondition.
pub fn is_five_hour_window_active(
    five_hour_resets_at: Option<DateTime<Utc>>,
) -> bool {
    five_hour_resets_at.is_some()
}

/// Per-account warm-up entry point. The caller (`scheduler::tick_for_account`)
/// is responsible for the transactional claim. This function:
///   1. Reads the most-recent snapshot for the account (passed in).
///   2. Applies the active-window precondition.
///   3. If the window is inactive, fetches the OAuth token and issues the call.
///   4. Returns a `WarmupOutcome`.
///
/// `oauth_token_loader` is injected so unit tests don't have to talk to the
/// real keychain / accounts.json.
pub async fn warmup_account<F>(
    five_hour_resets_at: Option<DateTime<Utc>>,
    oauth_token_loader: F,
    http: &reqwest::Client,
) -> Result<WarmupOutcome>
where
    F: FnOnce() -> Result<String>,
{
    if is_five_hour_window_active(five_hour_resets_at) {
        return Ok(WarmupOutcome::SkippedAlreadyActive);
    }

    let token = oauth_token_loader().context("load OAuth token")?;

    match api_call::issue(http, &token).await {
        Ok(resp) => Ok(errors::classify_status(resp.status().as_u16())),
        Err(e) if e.is_timeout() => Ok(WarmupOutcome::NetworkError),
        Err(e) if e.is_connect() => Ok(WarmupOutcome::NetworkError),
        Err(_) => Ok(WarmupOutcome::NetworkError),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_window_check_returns_true_when_resets_at_some() {
        let now = Utc::now();
        let future = now + chrono::Duration::hours(2);
        assert!(is_five_hour_window_active(Some(future)));
        assert!(!is_five_hour_window_active(None));
    }

    #[tokio::test]
    async fn warmup_skips_when_window_active() {
        let future = Some(Utc::now() + chrono::Duration::hours(2));
        let http = reqwest::Client::new();
        let outcome = warmup_account(
            future,
            || panic!("oauth_token_loader should not be called when active"),
            &http,
        )
        .await
        .unwrap();
        assert_eq!(outcome, WarmupOutcome::SkippedAlreadyActive);
    }

    #[tokio::test]
    async fn warmup_propagates_token_loader_error() {
        let http = reqwest::Client::new();
        let res = warmup_account(
            None,
            || anyhow::bail!("token vault unavailable"),
            &http,
        )
        .await;
        assert!(res.is_err());
    }
}
