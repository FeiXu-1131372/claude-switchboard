use crate::app_state::{AppState, BackoffState, BurnRateProjection, CachedUsage};
use crate::auth::AuthSource;
use crate::notifier;
use crate::tray;
use crate::usage_api::{next_backoff, FetchOutcome, UsageSnapshot};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

static STALE_EMITTED: AtomicBool = AtomicBool::new(false);

/// Default gap between consecutive slot polls in a staggered round.
/// Compressed automatically when (slots * STAGGER_GAP_SECS) > polling_interval_secs.
pub const STAGGER_GAP_SECS: u64 = 30;

/// Compute the per-slot stagger gap. Compresses below 30 s when the
/// configured polling interval can't fit (slots * 30 s).
pub fn stagger_gap(slot_count: usize, interval: Duration) -> Duration {
    if slot_count == 0 {
        return Duration::from_secs(STAGGER_GAP_SECS);
    }
    let default_gap = Duration::from_secs(STAGGER_GAP_SECS);
    let max_total = interval;
    let needed_total = default_gap * (slot_count.saturating_sub(1) as u32);
    if needed_total <= max_total {
        default_gap
    } else {
        max_total / slot_count as u32
    }
}

/// Lay out per-slot poll deadlines so the active slot fires first and
/// inactive slots trail at fixed offsets. Slot-id ordering for inactive
/// slots makes the schedule deterministic regardless of input order.
pub fn seed_schedules(
    slots: &[u32],
    active_slot: Option<u32>,
    now: Instant,
    interval: Duration,
) -> HashMap<u32, crate::app_state::ScheduleState> {
    use crate::app_state::ScheduleState;

    let gap = stagger_gap(slots.len(), interval);
    let mut ordered: Vec<u32> = match active_slot {
        Some(active) if slots.contains(&active) => {
            let mut v = vec![active];
            let mut rest: Vec<u32> = slots.iter().copied().filter(|&s| s != active).collect();
            rest.sort_unstable();
            v.extend(rest);
            v
        }
        _ => {
            let mut v: Vec<u32> = slots.to_vec();
            v.sort_unstable();
            v
        }
    };
    ordered.dedup();

    ordered
        .into_iter()
        .enumerate()
        .map(|(i, slot)| {
            let next_poll_at = now + gap * (i as u32);
            (slot, ScheduleState { next_poll_at })
        })
        .collect()
}

/// Choose the slot with the earliest already-expired `next_poll_at`,
/// skipping any slot currently in 429 backoff. Returns None when no
/// slot is ready to fetch.
pub fn pick_due_slot(state: &crate::app_state::AppState, now: Instant) -> Option<u32> {
    let schedule = state.schedule_by_slot.read();
    let backoff = state.backoff_by_slot.read();
    schedule
        .iter()
        .filter(|(_slot, sched)| sched.next_poll_at <= now)
        .filter(|(slot, _sched)| backoff.get(slot).is_none_or(|b| now >= b.until))
        .min_by_key(|(_slot, sched)| sched.next_poll_at)
        .map(|(&slot, _)| slot)
}

/// Earliest future deadline across all scheduled slots, used to pick a
/// sleep target when nothing is currently due. Falls back to 60 s out
/// when the schedule is empty (e.g., before any account is added).
pub fn next_wake_time(state: &crate::app_state::AppState, now: Instant) -> Instant {
    let schedule = state.schedule_by_slot.read();
    schedule
        .values()
        .map(|s| s.next_poll_at)
        .min()
        .unwrap_or(now + Duration::from_secs(60))
}

pub fn spawn(handle: AppHandle, state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        let mut burn_buffers: HashMap<u32, VecDeque<(DateTime<Utc>, f64)>> = HashMap::new();
        loop {
            let _ = poll_all(&handle, &state, &mut burn_buffers).await;
            let now = Instant::now();
            let wake_at = next_wake_time(&state, now);
            // Cap the sleep at 60 s so we still re-reconcile active_slot
            // periodically even when no slot is due (covers the
            // "live creds change without a swap" path).
            let max_sleep = Duration::from_secs(60);
            let sleep_for = wake_at.saturating_duration_since(now).min(max_sleep);
            tokio::select! {
                _ = tokio::time::sleep(sleep_for) => {}
                _ = state.force_refresh.notified() => {}
            }
        }
    });
}

async fn fetch_and_apply_one(
    handle: &AppHandle,
    state: &AppState,
    burn_buffers: &mut HashMap<u32, VecDeque<(DateTime<Utc>, f64)>>,
    slot: u32,
) {
    let accounts = state.accounts.list().unwrap_or_default();
    let acc = match accounts.iter().find(|a| a.slot == slot).cloned() {
        Some(a) => a,
        None => return, // slot disappeared mid-tick (remove)
    };
    let active_slot = *state.active_slot.read();

    let token_result = state
        .auth
        .token_for_slot(slot, active_slot, &state.accounts)
        .await;
    let outcome = match token_result {
        Ok(tok) => Some(state.usage.fetch(&tok).await),
        Err(e) => {
            tracing::warn!("token_for_slot({slot}) failed: {e}");
            let _ = handle.emit(
                "auth_required_for_slot",
                json!({ "slot": slot, "email": acc.email }),
            );
            return;
        }
    };
    let Some(outcome) = outcome else { return };

    match outcome {
        FetchOutcome::Ok(snapshot) => {
            let buf = burn_buffers.entry(slot).or_default();
            let burn_rate = update_burn_rate(buf, &snapshot, Utc::now());
            let cached = CachedUsage {
                snapshot: snapshot.clone(),
                account_id: acc.account_uuid.clone(),
                account_email: acc.email.clone(),
                last_error: None,
                burn_rate,
                auth_source: if Some(slot) == active_slot {
                    AuthSource::ClaudeCode
                } else {
                    AuthSource::OAuth
                },
            };
            state.cached_usage_by_slot.write().insert(slot, cached.clone());
            state.backoff_by_slot.write().remove(&slot);
            let _ = handle.emit(
                "usage_updated",
                json!({ "slot": slot, "cached": cached }),
            );

            if Some(slot) == active_slot {
                *state.cached_usage.write() = Some(cached.clone());
                tray::set_level(
                    handle,
                    snapshot.five_hour.as_ref().map(|u| u.utilization),
                    snapshot.seven_day.as_ref().map(|u| u.utilization),
                    snapshot.five_hour.as_ref().and_then(|u| u.resets_at),
                    snapshot.seven_day.as_ref().and_then(|u| u.resets_at),
                    false,
                );
                let thresholds = state.settings.read().thresholds.clone();
                if let Ok(fired) = notifier::evaluate(
                    &state.db,
                    &cached.account_id,
                    &snapshot,
                    &thresholds,
                    Utc::now(),
                ) {
                    for f in fired {
                        use tauri_plugin_notification::NotificationExt;
                        let _ = handle
                            .notification()
                            .builder()
                            .title(f.title)
                            .body(f.body)
                            .show();
                    }
                }
                STALE_EMITTED.store(false, Ordering::Relaxed);
            }
        }
        FetchOutcome::Unauthorized => {
            let _ = handle.emit(
                "auth_required_for_slot",
                json!({ "slot": slot, "email": acc.email }),
            );
            let mut entry = state
                .cached_usage_by_slot
                .write()
                .remove(&slot)
                .unwrap_or_else(|| placeholder_cached(&acc, "auth_required"));
            entry.last_error = Some("auth_required".into());
            state.cached_usage_by_slot.write().insert(slot, entry);
        }
        FetchOutcome::RateLimited(retry_after) => {
            let prev_delay = state
                .backoff_by_slot
                .read()
                .get(&slot)
                .map(|b| b.last_delay)
                .unwrap_or(Duration::from_secs(60));
            let delay = match retry_after {
                Some(d) if d > Duration::ZERO => clamp_backoff(d),
                _ => next_backoff(prev_delay),
            };
            tracing::warn!(
                "slot {slot} rate-limited; backing off {:?} (server retry-after={:?})",
                delay,
                retry_after,
            );
            state.backoff_by_slot.write().insert(
                slot,
                BackoffState {
                    until: Instant::now() + delay,
                    last_delay: delay,
                },
            );
            let mut entry = state
                .cached_usage_by_slot
                .write()
                .remove(&slot)
                .unwrap_or_else(|| placeholder_cached(&acc, "rate-limited (429)"));
            entry.last_error = Some("rate-limited (429)".into());
            state.cached_usage_by_slot.write().insert(slot, entry);
        }
        FetchOutcome::Transient(e) => {
            let mut entry = state
                .cached_usage_by_slot
                .write()
                .remove(&slot)
                .unwrap_or_else(|| placeholder_cached(&acc, &e));
            entry.last_error = Some(e);
            state.cached_usage_by_slot.write().insert(slot, entry);
        }
    }
}

async fn poll_all(
    handle: &AppHandle,
    state: &AppState,
    burn_buffers: &mut HashMap<u32, VecDeque<(DateTime<Utc>, f64)>>,
) -> Result<(), anyhow::Error> {
    // 1. Reconcile active slot from live CC creds.
    let live = state.auth.read_live_claude_code().await.ok().flatten();
    let accounts = state.accounts.list().unwrap_or_default();
    let active_slot = live.as_ref().and_then(|l| {
        accounts
            .iter()
            .find(|a| a.account_uuid == l.account_uuid)
            .map(|a| a.slot)
    });
    *state.active_slot.write() = active_slot;

    // 2. Empty-state + unmanaged-active signals.
    if accounts.is_empty() && live.is_none() {
        let _ = handle.emit("requires_setup", ());
    }
    if let Some(live) = &live {
        if active_slot.is_none() {
            let _ = handle.emit(
                "unmanaged_active_account",
                json!({
                    "email": live.email,
                    "account_uuid": live.account_uuid,
                }),
            );
        }
    }

    // 3. Lazy-seed the schedule on first call (or when slots have been
    //    added/removed without going through swap_to_account, which seeds
    //    explicitly). If schedule_by_slot is empty but we have managed
    //    accounts, seed; if accounts have been removed since last tick,
    //    drop their entries; if accounts have been added, append at the
    //    tail of the round one stagger gap behind the latest deadline.
    {
        let mut sched = state.schedule_by_slot.write();
        let existing_slots: std::collections::HashSet<u32> =
            sched.keys().copied().collect();
        let current_slots: std::collections::HashSet<u32> =
            accounts.iter().map(|a| a.slot).collect();

        // Drop schedule entries for slots that no longer exist.
        sched.retain(|slot, _| current_slots.contains(slot));

        if sched.is_empty() && !accounts.is_empty() {
            let interval = Duration::from_secs(
                state.settings.read().polling_interval_secs.max(60),
            );
            let slot_ids: Vec<u32> = accounts.iter().map(|a| a.slot).collect();
            *sched = seed_schedules(&slot_ids, active_slot, Instant::now(), interval);
        } else {
            // Append newly added slots so they trail the existing
            // schedule (one stagger gap behind the latest deadline).
            // This avoids placing a new slot ahead of an existing slot's
            // next fetch, which would break the 30 s gap.
            let now = Instant::now();
            let interval = Duration::from_secs(
                state.settings.read().polling_interval_secs.max(60),
            );
            let gap = stagger_gap(current_slots.len(), interval);
            let latest = sched
                .values()
                .map(|s| s.next_poll_at)
                .max()
                .unwrap_or(now);
            let mut next_at = latest + gap;
            for slot in current_slots.difference(&existing_slots) {
                sched.insert(
                    *slot,
                    crate::app_state::ScheduleState { next_poll_at: next_at },
                );
                next_at += gap;
            }
        }
    }

    // 4. Pick at most one due slot, fetch it, advance its deadline.
    let now = Instant::now();
    if let Some(slot) = pick_due_slot(state, now) {
        fetch_and_apply_one(handle, state, burn_buffers, slot).await;
        let interval = Duration::from_secs(
            state.settings.read().polling_interval_secs.max(60),
        );
        if let Some(entry) = state.schedule_by_slot.write().get_mut(&slot) {
            entry.next_poll_at = Instant::now() + interval;
        }
    }

    Ok(())
}

fn clamp_backoff(d: Duration) -> Duration {
    let min = Duration::from_secs(60);
    let max = Duration::from_secs(30 * 60);
    d.clamp(min, max)
}

fn placeholder_cached(
    acc: &crate::auth::accounts::ManagedAccount,
    err: &str,
) -> CachedUsage {
    CachedUsage {
        snapshot: UsageSnapshot {
            five_hour: None,
            seven_day: None,
            seven_day_sonnet: None,
            seven_day_opus: None,
            extra_usage: None,
            fetched_at: Utc::now(),
            unknown: Default::default(),
        },
        account_id: acc.account_uuid.clone(),
        account_email: acc.email.clone(),
        last_error: Some(err.to_string()),
        burn_rate: None,
        auth_source: AuthSource::OAuth,
    }
}

fn update_burn_rate(
    buf: &mut VecDeque<(DateTime<Utc>, f64)>,
    snapshot: &UsageSnapshot,
    now: DateTime<Utc>,
) -> Option<BurnRateProjection> {
    let five_hour = snapshot.five_hour.as_ref()?;
    let resets_at = five_hour.resets_at?;
    let window_start = resets_at - ChronoDuration::hours(5);
    while let Some(&(ts, _)) = buf.front() {
        if ts < window_start {
            buf.pop_front();
        } else {
            break;
        }
    }
    buf.push_back((now, five_hour.utilization));
    if buf.len() < 2 {
        return None;
    }
    let &(t0, u0) = buf.front()?;
    let &(t1, u1) = buf.back()?;
    let span_minutes = (t1 - t0).num_seconds() as f64 / 60.0;
    if span_minutes < 2.0 {
        return None;
    }
    let slope = (u1 - u0) / span_minutes;
    let mins_until_reset = ((resets_at - now).num_seconds() as f64 / 60.0).max(0.0);
    Some(BurnRateProjection {
        utilization_per_min: slope,
        projected_at_reset: u1 + slope * mins_until_reset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn stagger_gap_default_30s_when_interval_fits() {
        // 3 slots, 300 s interval → needs 60 s for stagger, fits comfortably.
        let gap = stagger_gap(3, Duration::from_secs(300));
        assert_eq!(gap, Duration::from_secs(30));
    }

    #[test]
    fn stagger_gap_compresses_when_interval_too_short() {
        // 4 slots, 60 s interval → 90 s needed > 60 s → compress to 60/4 = 15 s.
        let gap = stagger_gap(4, Duration::from_secs(60));
        assert_eq!(gap, Duration::from_secs(15));
    }

    #[test]
    fn stagger_gap_zero_slots_returns_default() {
        let gap = stagger_gap(0, Duration::from_secs(60));
        assert_eq!(gap, Duration::from_secs(30));
    }

    #[test]
    fn seed_schedules_active_first_then_inactive_in_slot_id_order() {
        let now = Instant::now();
        let interval = Duration::from_secs(300);
        let sched = seed_schedules(&[3, 1, 2], Some(2), now, interval);

        assert_eq!(sched[&2].next_poll_at, now);
        assert_eq!(sched[&1].next_poll_at, now + Duration::from_secs(30));
        assert_eq!(sched[&3].next_poll_at, now + Duration::from_secs(60));
    }

    #[test]
    fn seed_schedules_no_active_slot_orders_by_slot_id() {
        let now = Instant::now();
        let interval = Duration::from_secs(300);
        let sched = seed_schedules(&[3, 1, 2], None, now, interval);

        assert_eq!(sched[&1].next_poll_at, now);
        assert_eq!(sched[&2].next_poll_at, now + Duration::from_secs(30));
        assert_eq!(sched[&3].next_poll_at, now + Duration::from_secs(60));
    }

    #[test]
    fn seed_schedules_active_not_in_slots_ignored() {
        // Active is 99 but only slots 1, 2 are managed: fall back to id order.
        let now = Instant::now();
        let interval = Duration::from_secs(300);
        let sched = seed_schedules(&[1, 2], Some(99), now, interval);

        assert_eq!(sched[&1].next_poll_at, now);
        assert_eq!(sched[&2].next_poll_at, now + Duration::from_secs(30));
        assert_eq!(sched.contains_key(&99), false);
    }

    #[test]
    fn seed_schedules_empty_slots_returns_empty_map() {
        let now = Instant::now();
        let sched = seed_schedules(&[], Some(1), now, Duration::from_secs(300));
        assert!(sched.is_empty());
    }
}
