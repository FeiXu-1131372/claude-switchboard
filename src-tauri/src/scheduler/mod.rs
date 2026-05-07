//! Warm-up scheduling: presets, dispatcher, transactional claim, catch-up.
//! Public entry point is `tick_for_account` (per spec §7).

pub mod catchup;
pub mod claim;
pub mod presets;

use anyhow::Result;
use chrono::{DateTime, Local, Utc};
use rusqlite::Connection;

pub use presets::{HhMm, Schedule};

use crate::warmup::{self, errors::WarmupOutcome};

/// Schedule eligibility check (Step 1 from §7).
///
/// For OS-level/in-app fires, returns true if the schedule is due *now*.
/// Catch-up callers pass `is_due_override = true` so the schedule check is
/// bypassed; they've already computed the most_recent_expected_fire.
pub fn is_due(schedule: &Schedule, now: DateTime<Local>) -> bool {
    // For minute-granularity schedules, "due now" means there's a fire time
    // within the past 60 seconds (the tick cadence).
    let one_minute_ago = now - chrono::Duration::seconds(60);
    match schedule {
        Schedule::Off => false,
        Schedule::Every5h { .. } | Schedule::Custom { .. } => {
            match catchup::most_recent_expected_fire(schedule, now) {
                Some(t) => t >= one_minute_ago && t <= now,
                None => false,
            }
        }
    }
}

/// Tick for one account. Implements Steps 1-4 from spec §7:
/// 1. Schedule eligibility (or override for catch-up).
/// 2. Transactional claim.
/// 3. Active-window precondition.
/// 4. HTTP issue.
pub async fn tick_for_account<F>(
    conn: &Connection,
    account_id: &str,
    schedule: &Schedule,
    is_due_override: bool,
    five_hour_resets_at: Option<DateTime<Utc>>,
    oauth_token_loader: F,
    http: &reqwest::Client,
) -> Result<Option<WarmupOutcome>>
where
    F: FnOnce() -> Result<String>,
{
    // Step 1
    if !is_due_override && !is_due(schedule, Local::now()) {
        return Ok(None);
    }

    // Step 2
    let now_secs = chrono::Utc::now().timestamp();
    if !claim::try_claim(conn, account_id, now_secs)? {
        return Ok(None);
    }

    // Steps 3 + 4
    let outcome =
        warmup::warmup_account(five_hour_resets_at, oauth_token_loader, http)
            .await?;

    Ok(Some(outcome))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn is_due_returns_false_for_off() {
        let s = Schedule::Off;
        let now = Local
            .with_ymd_and_hms(2026, 5, 7, 14, 0, 0)
            .single()
            .unwrap();
        assert!(!is_due(&s, now));
    }
}
