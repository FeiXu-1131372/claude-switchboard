//! Usage-driven rotation. Decisions use the existing cache and never spend
//! additional usage API requests or bypass its backoff rules.
use crate::app_state::{AppState, CachedUsage};
use crate::usage_api::Utilization;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

const COOLDOWN: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct RotationSettings {
    pub enabled: bool,
    pub five_hour_threshold: u8,
    pub seven_day_threshold: u8,
    /// Stable identity, so deleting an account cannot retarget this preference.
    pub preferred_account_id: Option<String>,
}

impl Default for RotationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            five_hour_threshold: 96,
            seven_day_threshold: 98,
            preferred_account_id: None,
        }
    }
}

impl RotationSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=100).contains(&self.five_hour_threshold)
            || !(1..=100).contains(&self.seven_day_threshold)
        {
            return Err("Account rotation thresholds must be between 1 and 100".into());
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct RotationRuntime {
    last_attempt: Option<Instant>,
    pub status: Option<String>,
}

fn fresh(cache: &CachedUsage, now: DateTime<Utc>, max_age: i64) -> bool {
    let age = now - cache.snapshot.fetched_at;
    cache.last_error.is_none()
        && age >= chrono::Duration::zero()
        && age <= chrono::Duration::seconds(max_age)
}

/// A nonzero bucket past its reset needs a new reading. Guessing that it is
/// empty could rotate to an account already consuming its next window.
fn utilization(bucket: Option<&Utilization>, now: DateTime<Utc>) -> Option<f64> {
    let bucket = bucket?;
    let value = bucket.utilization;
    if !value.is_finite()
        || !(0.0..=100.0).contains(&value)
        || (value > 0.0 && bucket.resets_at.is_some_and(|at| at <= now))
    {
        return None;
    }
    Some(value)
}

fn trigger(cache: &CachedUsage, settings: &RotationSettings, now: DateTime<Utc>) -> Option<String> {
    if let Some(value) = utilization(cache.snapshot.five_hour.as_ref(), now) {
        if value >= f64::from(settings.five_hour_threshold) {
            return Some(format!("5-hour usage reached {value:.0}%"));
        }
    }
    if let Some(value) = utilization(cache.snapshot.seven_day.as_ref(), now) {
        if value >= f64::from(settings.seven_day_threshold) {
            return Some(format!("Weekly usage reached {value:.0}%"));
        }
    }
    None
}

/// Prefer a user's chosen account when eligible; otherwise maximize the
/// smaller remaining margin across the two thresholds. Slot breaks ties.
fn choose_next(
    settings: &RotationSettings,
    active: u32,
    candidates: &[(u32, &CachedUsage)],
    now: DateTime<Utc>,
    max_age: i64,
) -> Option<u32> {
    candidates
        .iter()
        .filter_map(|&(slot, cache)| {
            if slot == active || !fresh(cache, now, max_age) {
                return None;
            }
            let five = utilization(cache.snapshot.five_hour.as_ref(), now)?;
            let week = utilization(cache.snapshot.seven_day.as_ref(), now)?;
            let score = (five / f64::from(settings.five_hour_threshold))
                .max(week / f64::from(settings.seven_day_threshold));
            if score >= 1.0 {
                return None;
            }
            let preferred =
                settings.preferred_account_id.as_deref() == Some(cache.account_id.as_str());
            Some((slot, preferred, score))
        })
        .min_by(|a, b| b.1.cmp(&a.1).then(a.2.total_cmp(&b.2)).then(a.0.cmp(&b.0)))
        .map(|(slot, _, _)| slot)
}

fn set_status(handle: &AppHandle, state: &AppState, message: String) {
    let mut runtime = state.rotation.lock();
    if runtime.status.as_ref() != Some(&message) {
        tracing::info!(target: "switchboard.rotation", "{message}");
        runtime.status = Some(message.clone());
        let _ = handle.emit("account_rotation_status", &message);
    }
}

pub async fn evaluate(handle: &AppHandle, state: &AppState) {
    // Serialize with imports, removals and manual swaps, including selection.
    let _account_change = state.account_change_lock.lock().await;
    let (settings, max_age) = {
        let settings = state.settings.read();
        (
            settings.account_rotation.clone(),
            settings
                .polling_interval_secs
                .saturating_mul(2)
                .clamp(120, 900) as i64,
        )
    };
    let Some(settings) = settings.filter(|s| s.enabled && s.validate().is_ok()) else {
        return;
    };
    let now = Utc::now();
    if state
        .active_since
        .read()
        .is_some_and(|since| (now - since).num_seconds() < 60)
        || state
            .rotation
            .lock()
            .last_attempt
            .is_some_and(|at| at.elapsed() < COOLDOWN)
    {
        return;
    }
    let Some(active) = *state.active_slot.read() else {
        return;
    };
    let Ok(accounts) = state.accounts.list() else {
        return;
    };
    let decision = {
        let cache = state.cached_usage_by_slot.read();
        let Some(current) = cache.get(&active).filter(|c| fresh(c, now, max_age)) else {
            return;
        };
        if !accounts
            .iter()
            .any(|a| a.slot == active && a.account_uuid == current.account_id)
        {
            return;
        }
        let Some(reason) = trigger(current, &settings, now) else {
            return;
        };
        let candidates: Vec<_> = accounts
            .iter()
            .filter(|a| {
                // Expired tokens with no refresh path cannot be used even when
                // a recently imported cached reading looks healthy.
                a.claude_code_oauth_blob["accessToken"]
                    .as_str()
                    .is_some_and(|t| !t.is_empty())
                    && (a.can_refresh() || a.token_expires_at.is_none_or(|at| at > now))
            })
            .filter_map(|a| {
                cache
                    .get(&a.slot)
                    .filter(|c| c.account_id == a.account_uuid)
                    .map(|c| (a.slot, c))
            })
            .collect();
        (
            choose_next(&settings, active, &candidates, now, max_age),
            reason,
        )
    };
    let (Some(target), reason) = decision else {
        set_status(
            handle,
            state,
            "Waiting for an available account with fresh usage below both thresholds.".into(),
        );
        return;
    };
    state.rotation.lock().last_attempt = Some(Instant::now());
    match crate::commands::perform_account_swap(target, state).await {
        Ok(report) => {
            let name = accounts
                .iter()
                .find(|a| a.slot == target)
                .map(|a| a.display_name())
                .unwrap_or_else(|| target.to_string());
            set_status(
                handle,
                state,
                format!(
                    "{reason}. Switched to {name} at {}.",
                    now.format("%H:%M UTC")
                ),
            );
            // A swap sets active_slot eagerly, so the poll loop will not see
            // a transition. Update every window and its headline immediately.
            let entries: Vec<_> = accounts
                .iter()
                .map(|a| crate::commands::entry_for(state, a, Some(target)))
                .collect();
            let _ = handle.emit("accounts_changed", &entries);
            if let Some(cached) = state.cached_usage_by_slot.read().get(&target).cloned() {
                *state.cached_usage.write() = Some(cached.clone());
                crate::tray::set_level(
                    handle,
                    cached.snapshot.five_hour.as_ref().map(|u| u.utilization),
                    cached.snapshot.seven_day.as_ref().map(|u| u.utilization),
                    cached.snapshot.five_hour.as_ref().and_then(|u| u.resets_at),
                    cached.snapshot.seven_day.as_ref().and_then(|u| u.resets_at),
                    false,
                );
                let _ = handle.emit(
                    "usage_updated",
                    serde_json::json!({ "slot": target, "cached": cached }),
                );
            }
            let _ = handle.emit("swap_completed", &report);
            use tauri_plugin_notification::NotificationExt;
            let _ = handle
                .notification()
                .builder()
                .title("Account rotated")
                .body(format!("{reason}. Claude Code is switching to {name}."))
                .show();
        }
        Err(error) => set_status(
            handle,
            state,
            format!("Automatic switch failed: {error}. Retrying after the cooldown."),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::SnapshotOrigin;
    use crate::auth::AuthSource;
    use crate::usage_api::UsageSnapshot;

    fn cache(five: f64, week: f64) -> CachedUsage {
        let now = Utc::now();
        let bucket = |utilization| {
            Some(Utilization {
                utilization,
                resets_at: Some(now + chrono::Duration::hours(1)),
            })
        };
        CachedUsage {
            snapshot: UsageSnapshot {
                five_hour: bucket(five),
                seven_day: bucket(week),
                seven_day_sonnet: None,
                seven_day_opus: None,
                extra_usage: None,
                fetched_at: now,
                unknown: Default::default(),
            },
            account_id: "account".into(),
            account_email: "test@example.com".into(),
            last_error: None,
            burn_rate: None,
            auth_source: AuthSource::OAuth,
            origin: SnapshotOrigin::Http,
            carried_fetched_at: None,
        }
    }

    #[test]
    fn either_threshold_triggers_at_the_boundary() {
        let s = RotationSettings::default();
        assert!(trigger(&cache(95.9, 97.9), &s, Utc::now()).is_none());
        assert!(trigger(&cache(96.0, 20.0), &s, Utc::now())
            .unwrap()
            .contains("5-hour"));
        assert!(trigger(&cache(20.0, 98.0), &s, Utc::now())
            .unwrap()
            .contains("Weekly"));
    }

    #[test]
    fn automatic_choice_balances_both_limits_and_excludes_active() {
        let a = cache(0.0, 0.0);
        let b = cache(10.0, 97.0);
        let c = cache(30.0, 30.0);
        assert_eq!(
            choose_next(
                &RotationSettings::default(),
                1,
                &[(1, &a), (2, &b), (3, &c)],
                Utc::now(),
                600
            ),
            Some(3)
        );
    }

    #[test]
    fn preferred_account_is_used_only_when_available() {
        let mut preferred = cache(50.0, 50.0);
        preferred.account_id = "preferred".into();
        let fallback = cache(10.0, 10.0);
        let s = RotationSettings {
            preferred_account_id: Some("preferred".into()),
            ..Default::default()
        };
        assert_eq!(
            choose_next(&s, 1, &[(2, &preferred), (3, &fallback)], Utc::now(), 600),
            Some(2)
        );
        preferred.snapshot.seven_day.as_mut().unwrap().utilization = 98.0;
        assert_eq!(
            choose_next(&s, 1, &[(2, &preferred), (3, &fallback)], Utc::now(), 600),
            Some(3)
        );
    }

    #[test]
    fn unknown_stale_error_future_and_reset_readings_are_unavailable() {
        let mut unknown = cache(0.0, 0.0);
        unknown.snapshot.seven_day = None;
        let mut stale = cache(0.0, 0.0);
        stale.snapshot.fetched_at -= chrono::Duration::minutes(30);
        let mut error = cache(0.0, 0.0);
        error.last_error = Some("auth_required".into());
        let mut future = cache(0.0, 0.0);
        future.snapshot.fetched_at += chrono::Duration::minutes(1);
        let mut reset = cache(20.0, 20.0);
        reset.snapshot.five_hour.as_mut().unwrap().resets_at =
            Some(Utc::now() - chrono::Duration::seconds(1));
        let mut invalid = cache(f64::NAN, 0.0);
        let s = RotationSettings::default();
        for c in [&unknown, &stale, &error, &future, &reset, &invalid] {
            assert_eq!(choose_next(&s, 1, &[(2, c)], Utc::now(), 600), None);
        }
        invalid.snapshot.five_hour.as_mut().unwrap().utilization = 0.0;
        invalid.snapshot.five_hour.as_mut().unwrap().resets_at = None;
        assert_eq!(
            choose_next(&s, 1, &[(2, &invalid)], Utc::now(), 600),
            Some(2)
        );
    }

    #[test]
    fn legacy_settings_and_validation() {
        let legacy: crate::app_state::Settings = serde_json::from_str("{}").unwrap();
        assert!(legacy.account_rotation.is_none());
        let s: RotationSettings = serde_json::from_str("{}").unwrap();
        assert!(!s.enabled);
        assert_eq!((s.five_hour_threshold, s.seven_day_threshold), (96, 98));
        assert!(RotationSettings {
            five_hour_threshold: 0,
            ..s.clone()
        }
        .validate()
        .is_err());
        assert!(RotationSettings {
            seven_day_threshold: 101,
            ..s
        }
        .validate()
        .is_err());
    }
}
