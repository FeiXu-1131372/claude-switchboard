use crate::app_state::{
    AppState, BackoffReason, BackoffState, BurnRateProjection, CachedUsage, SnapshotOrigin,
};
use crate::auth::AuthSource;
use crate::auth::accounts::ManagedAccount;
use crate::notifier;
use crate::tray;
use crate::usage_api::{FetchOutcome, UsageSnapshot};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

static STALE_EMITTED: AtomicBool = AtomicBool::new(false);

/// Default gap between consecutive slot polls in a staggered round when
/// the user hasn't customised `Settings::stagger_gap_secs`. Mirrors the
/// `Settings::default()` value so a non-test caller that bypasses settings
/// still gets the historical behaviour.
pub const DEFAULT_STAGGER_GAP_SECS: u64 = 30;

/// Backoff applied when a slot fails with 401/403 (`FetchOutcome::Unauthorized`)
/// or an OAuth token refresh failure (`invalid_grant`). Prevents hammering
/// `/api/oauth/usage` or `/v1/oauth/token` every poll cycle (which triggers
/// 1-hour 429 bans from Anthropic) while waiting for the user to re-authenticate.
/// Cleared immediately on OAuth completion, manual refresh, or account swap.
pub const AUTH_FAILURE_BACKOFF: Duration = Duration::from_secs(30 * 60);

/// `last_error` shown while a slot is rate-limited.
const RATE_LIMITED_ERROR: &str = "rate-limited (429)";

/// Added to a server-supplied `Retry-After`. A retry landing at the very end
/// of a 1-hour ban was observed to start a fresh 1-hour ban (2026-10-27).
const RETRY_AFTER_MARGIN: Duration = Duration::from_secs(60);

/// Upper bound on a 429 backoff, so a misconfigured server can't lock a slot
/// out indefinitely. The real bans observed are 1 hour.
const MAX_RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(2 * 60 * 60);

/// Oldest local snapshot adopted while a slot sits out a 429 ban. Matches the
/// upstream CLI's own cut-off for its persisted `cachedUsageUtilization`.
const MAX_LOCAL_AGE_DURING_BAN: Duration = Duration::from_secs(60 * 60);

/// What a slot that cannot call `/usage` (no `user:profile` scope) does with
/// the local sources' answer.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)] // UsageSnapshot is intentionally value-typed; boxing adds indirection for no benefit here
enum LocalOnly {
    /// A local snapshot newer than what the slot shows: apply it.
    Adopt(UsageSnapshot, SnapshotOrigin),
    /// A local source has data, but nothing newer than what is shown. Carries
    /// its `fetched_at` so the next poll lands when that data ages out.
    KeepShown(DateTime<Utc>),
    /// No local source has data: show the "usage not supported" placeholder.
    NotSupported,
}

/// Decide the outcome for a slot whose only possible usage source is local.
/// These accounts never reach the HTTP path: `/api/oauth/usage` answers 403
/// `oauth_scope_insufficient` for an inference-only token.
fn resolve_local_only(
    local: Option<(UsageSnapshot, SnapshotOrigin)>,
    newer_than_shown: bool,
) -> LocalOnly {
    match local {
        Some((snap, origin)) if newer_than_shown => LocalOnly::Adopt(snap, origin),
        Some((snap, _)) => LocalOnly::KeepShown(snap.fetched_at),
        None => LocalOnly::NotSupported,
    }
}

/// Pull the polling interval + base stagger gap from settings as Durations,
/// clamping each to a safe lower bound that mirrors the validation in
/// `update_settings` (defends against any settings row written before that
/// validation existed).
fn settings_durations(state: &crate::app_state::AppState) -> (Duration, Duration) {
    let s = state.settings.read();
    (
        Duration::from_secs(s.polling_interval_secs.max(60)),
        Duration::from_secs(s.stagger_gap_secs.clamp(5, 120)),
    )
}

/// Compute the per-slot stagger gap. Compresses below `base_gap` when the
/// configured polling interval can't fit (`slots * base_gap`).
pub fn stagger_gap(slot_count: usize, interval: Duration, base_gap: Duration) -> Duration {
    if slot_count == 0 {
        return base_gap;
    }
    let max_total = interval;
    let needed_total = base_gap * (slot_count.saturating_sub(1) as u32);
    if needed_total <= max_total {
        base_gap
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
    base_gap: Duration,
) -> HashMap<u32, crate::app_state::ScheduleState> {
    use crate::app_state::ScheduleState;

    let gap = stagger_gap(slots.len(), interval, base_gap);
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
    earliest_deadline(&state.schedule_by_slot.read(), &state.backoff_by_slot.read())
        .unwrap_or(now + Duration::from_secs(60))
}

/// The earliest moment any slot can actually be fetched: its poll deadline,
/// or its backoff end when that is later. Without the backoff term, a slot
/// whose deadline passed mid-ban stays "due" but unpickable, and the loop
/// spins with zero sleep until the ban ends.
fn earliest_deadline(
    schedule: &HashMap<u32, crate::app_state::ScheduleState>,
    backoff: &HashMap<u32, BackoffState>,
) -> Option<Instant> {
    schedule
        .iter()
        .map(|(slot, sched)| match backoff.get(slot) {
            Some(b) if b.until > sched.next_poll_at => b.until,
            _ => sched.next_poll_at,
        })
        .min()
}

pub fn spawn(handle: AppHandle, state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        let mut burn_buffers: HashMap<u32, VecDeque<(DateTime<Utc>, f64)>> = HashMap::new();
        loop {
            let _ = poll_all(&handle, &state, &mut burn_buffers).await;
            crate::account_rotation::evaluate(&handle, &state).await;
            let now = Instant::now();
            let wake_at = next_wake_time(&state, now);
            // Cap the sleep at 60 s so we still re-reconcile active_slot
            // periodically even when no slot is due (covers the
            // "live creds change without a swap" path).
            let max_sleep = Duration::from_secs(60);
            let sleep_for = wake_at.saturating_duration_since(now).min(max_sleep);
            let deadline = tokio::time::Instant::now() + sleep_for;
            loop {
                tokio::select! {
                    _ = tokio::time::sleep_until(deadline) => break,
                    _ = state.force_refresh.notified() => break,
                    // A statusline observation changed: refresh the local
                    // sources only. A full `poll_all` tick would re-read the
                    // keychain on every status-line refresh.
                    _ = state.local_source_changed.notified() => {
                        let accounts = state.accounts.list().unwrap_or_default();
                        let active_slot = *state.active_slot.read();
                        adopt_fresh_statusline(
                            &handle, &state, &mut burn_buffers, &accounts, active_slot,
                        )
                        .await;
                        crate::account_rotation::evaluate(&handle, &state).await;
                    }
                }
            }
        }
    });
}

/// Fetch one slot's usage, from a local source when one is fresh enough and
/// over HTTP otherwise. Returns the local snapshot's `fetched_at` when a local
/// source answered, so the caller can schedule the next poll for when that
/// data ages out rather than a full interval from now.
async fn fetch_and_apply_one(
    handle: &AppHandle,
    state: &AppState,
    burn_buffers: &mut HashMap<u32, VecDeque<(DateTime<Utc>, f64)>>,
    slot: u32,
    accounts: &[ManagedAccount],
) -> Option<DateTime<Utc>> {
    let acc = match accounts.iter().find(|a| a.slot == slot).cloned() {
        Some(a) => a,
        None => return None, // slot disappeared between poll_all's accounts read and now
    };
    // Re-read active_slot here; in the rare race where swap_to_account
    // fires between poll_all's reconciliation and this point, we accept
    // a one-cycle cosmetic flicker on tray/auth_source rather than
    // adding a lock or capturing the value at the call site.
    let active_slot = *state.active_slot.read();

    if !acc.supports_usage() {
        // No `user:profile` scope, so `/usage` is off the table. A local
        // source (the statusline bridge's observations) may still know.
        let interval = Duration::from_secs(state.settings.read().polling_interval_secs.max(60));
        let local = local_snapshot(state, &acc, active_slot, interval);
        let newer = local
            .as_ref()
            .is_none_or(|(snap, _)| is_newer_than_shown(state, slot, snap.fetched_at));
        return match resolve_local_only(local, newer) {
            LocalOnly::Adopt(snap, origin) => {
                let fetched_at = snap.fetched_at;
                tracing::debug!(target: "switchboard.poll", "slot {slot}: adopted {origin:?} snapshot");
                apply_fetch_outcome(
                    handle,
                    state,
                    burn_buffers,
                    slot,
                    &acc,
                    active_slot,
                    FetchOutcome::Ok(snap),
                    origin,
                )
                .await;
                Some(fetched_at)
            }
            LocalOnly::KeepShown(fetched_at) => Some(fetched_at),
            LocalOnly::NotSupported => {
                // Keep data an earlier adoption put on the row: it ages
                // visibly through `fetched_at` instead of vanishing.
                let shown = state
                    .cached_usage_by_slot
                    .read()
                    .get(&slot)
                    .is_some_and(|c| has_usage_data(&c.snapshot));
                if !shown {
                    let marker = crate::commands::local_only_marker(
                        state.statusline_setup.feeds_usage(),
                        Some(slot) == active_slot,
                    );
                    let cached = placeholder_cached(&acc, marker);
                    state.cached_usage_by_slot.write().insert(slot, cached.clone());
                    if Some(slot) == active_slot {
                        *state.cached_usage.write() = Some(cached.clone());
                        tray::set_level(handle, tray::TrayUsage::default(), false);
                    }
                    let _ = handle
                        .emit("usage_updated", json!({ "slot": slot, "cached": cached }));
                }
                None
            }
        };
    }

    // Local-source waterfall: when another local consumer already holds data
    // for this account younger than one polling interval, use it instead of
    // spending the account's /usage rate budget on a duplicate call. If that
    // data is no newer than what the slot already shows, the slot is fresh
    // enough as it is — skip the call without re-applying. A dead source
    // ages out within a cycle and hands control back to the HTTP path.
    let interval = Duration::from_secs(state.settings.read().polling_interval_secs.max(60));
    if let Some((snap, origin)) = local_snapshot(state, &acc, active_slot, interval) {
        let fetched_at = snap.fetched_at;
        // A statusline snapshot carries no Sonnet/Opus/extra-usage buckets of
        // its own. Once the ones it inherited are 15 minutes old, the slot
        // makes its one real call (which still honors any backoff).
        let floor_forces_http = origin == SnapshotOrigin::Statusline
            && !crate::statusline_source::satisfies_poll(
                crate::statusline_source::carried_of(state.cached_usage_by_slot.read().get(&slot))
                    .map(|(_, at)| at),
                Utc::now(),
            );
        if is_newer_than_shown(state, slot, fetched_at) {
            tracing::debug!(target: "switchboard.poll", "slot {slot}: adopted {origin:?} snapshot");
            apply_fetch_outcome(
                handle,
                state,
                burn_buffers,
                slot,
                &acc,
                active_slot,
                FetchOutcome::Ok(snap),
                origin,
            )
            .await;
        }
        if !floor_forces_http {
            return Some(fetched_at);
        }
    }

    let token_result = state
        .auth
        .token_for_slot(slot, active_slot, &state.accounts)
        .await;
    let outcome = match token_result {
        Ok(tok) => Some(state.usage.fetch(&tok).await),
        Err(e) => {
            tracing::warn!(
                "token_for_slot({slot}) failed: {e}; backing off {:?}",
                AUTH_FAILURE_BACKOFF
            );
            state.backoff_by_slot.write().insert(
                slot,
                BackoffState {
                    until: Instant::now() + AUTH_FAILURE_BACKOFF,
                    reason: BackoffReason::AuthFailure,
                },
            );
            let _ = handle.emit(
                "auth_required_for_slot",
                json!({ "slot": slot, "email": acc.email }),
            );
            // Most common cause is the refresh token being revoked
            // (Anthropic returns invalid_grant). Mark the slot's cache
            // with auth_required so the UI shows "token expired —
            // re-authenticate" without waiting for a manual refresh.
            // Network blips fall under the same label here; the next
            // successful refresh will clear last_error.
            let mut entry = state
                .cached_usage_by_slot
                .write()
                .remove(&slot)
                .unwrap_or_else(|| placeholder_cached(&acc, "auth_required"));
            entry.last_error = Some("auth_required".into());
            state.cached_usage_by_slot.write().insert(slot, entry.clone());
            let _ = handle.emit(
                "usage_updated",
                json!({ "slot": slot, "cached": entry }),
            );
            return None;
        }
    };
    let outcome = outcome?;
    apply_fetch_outcome(
        handle,
        state,
        burn_buffers,
        slot,
        &acc,
        active_slot,
        outcome,
        SnapshotOrigin::Http,
    )
    .await;
    None
}

/// Apply a fetch outcome for one slot: update the per-slot cache, emit
/// `usage_updated`, and on success also persist + update tray/notifier.
/// Shared by the HTTP path and the local-source waterfall; `origin` says
/// which one produced an `Ok` snapshot.
#[allow(clippy::too_many_arguments)]
async fn apply_fetch_outcome(
    handle: &AppHandle,
    state: &AppState,
    burn_buffers: &mut HashMap<u32, VecDeque<(DateTime<Utc>, f64)>>,
    slot: u32,
    acc: &ManagedAccount,
    active_slot: Option<u32>,
    outcome: FetchOutcome,
    origin: SnapshotOrigin,
) {
    match outcome {
        FetchOutcome::Ok(mut snapshot) => {
            // Every snapshot enters the cache through here, so this is the
            // one place that decides whether the account has a Fable row.
            // The tray, the notifier and both windows then agree.
            snapshot.hide_fable_without_access(crate::auth::accounts::plan_includes_fable(
                acc.plan().as_deref(),
            ));
            // A local snapshot adopted mid-ban leaves the ban in force (it
            // only bounds this app's own calls). Keep the 429 label while the
            // data is older than a poll interval, so the row reads "showing
            // last good data from 20m ago" rather than passing it off as live.
            let banned = origin != SnapshotOrigin::Http
                && state
                    .backoff_by_slot
                    .read()
                    .get(&slot)
                    .is_some_and(|b| b.reason == BackoffReason::RateLimited);
            let interval = Duration::from_secs(state.settings.read().polling_interval_secs.max(60));
            let last_error = (banned
                && !is_within_age(snapshot.fetched_at, interval, Utc::now()))
            .then(|| RATE_LIMITED_ERROR.to_string());
            let buf = burn_buffers.entry(slot).or_default();
            let burn_rate = update_burn_rate(buf, &snapshot, snapshot.fetched_at);
            // Read what the slot showed before this snapshot replaces it: a
            // statusline snapshot inherits its carried buckets' age from it,
            // and its history row is gated on a headline change.
            let (carried_fetched_at, persist) = {
                let shown = state.cached_usage_by_slot.read();
                let shown = shown.get(&slot);
                if origin == SnapshotOrigin::Statusline {
                    let now = Utc::now();
                    let changed = crate::statusline_source::headline_changed(
                        shown.map(|c| &c.snapshot),
                        &snapshot,
                    );
                    let last = state.statusline_last_persisted.read().get(&slot).copied();
                    (
                        crate::statusline_source::carried_of(shown).map(|(_, at)| at),
                        crate::statusline_source::should_persist(last, now, changed),
                    )
                } else {
                    (None, true)
                }
            };
            let cached = CachedUsage {
                snapshot: snapshot.clone(),
                account_id: acc.account_uuid.clone(),
                account_email: acc.email.clone(),
                last_error,
                burn_rate,
                auth_source: if Some(slot) == active_slot {
                    AuthSource::ClaudeCode
                } else {
                    AuthSource::OAuth
                },
                origin,
                carried_fetched_at,
            };
            state.cached_usage_by_slot.write().insert(slot, cached.clone());
            if origin == SnapshotOrigin::Http {
                state.backoff_by_slot.write().remove(&slot);
            }
            if origin == SnapshotOrigin::Statusline {
                *state.statusline_last_adopted.write() = Some(Utc::now());
            }
            // Persist the raw snapshot so a cold start can rehydrate
            // last-known-good data (see hydrated_caches). Best-effort: a
            // storage hiccup must never interrupt polling. Note the
            // re-serialize drops forward-compat `unknown` fields — the UI
            // and hydration only rely on the typed buckets.
            if persist {
                match serde_json::to_string(&snapshot) {
                    Ok(payload) => {
                        if let Err(e) = state.db.insert_snapshot(
                            &acc.account_uuid,
                            snapshot.fetched_at,
                            &payload,
                        ) {
                            tracing::warn!("persist snapshot for slot {slot} failed: {e}");
                        } else if origin == SnapshotOrigin::Statusline {
                            state
                                .statusline_last_persisted
                                .write()
                                .insert(slot, Utc::now());
                        }
                    }
                    Err(e) => tracing::warn!("serialize snapshot for slot {slot} failed: {e}"),
                }
            }
            let _ = handle.emit(
                "usage_updated",
                json!({ "slot": slot, "cached": cached }),
            );

            if Some(slot) == active_slot {
                *state.cached_usage.write() = Some(cached.clone());
                tray::set_level(handle, (&snapshot).into(), false);
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
            tracing::warn!(
                "slot {slot} returned unauthorized/forbidden; backing off {:?}",
                AUTH_FAILURE_BACKOFF
            );
            state.backoff_by_slot.write().insert(
                slot,
                BackoffState {
                    until: Instant::now() + AUTH_FAILURE_BACKOFF,
                    reason: BackoffReason::AuthFailure,
                },
            );
            let _ = handle.emit(
                "auth_required_for_slot",
                json!({ "slot": slot, "email": acc.email }),
            );
            let mut entry = state
                .cached_usage_by_slot
                .write()
                .remove(&slot)
                .unwrap_or_else(|| placeholder_cached(acc, "auth_required"));
            entry.last_error = Some("auth_required".into());
            state.cached_usage_by_slot.write().insert(slot, entry.clone());
            let _ = handle.emit(
                "usage_updated",
                json!({ "slot": slot, "cached": entry }),
            );
        }
        FetchOutcome::RateLimited(retry_after) => {
            match backoff_for_429(retry_after) {
                Some(delay) => {
                    tracing::warn!(
                        "slot {slot} rate-limited; backing off {:?} (server retry-after={:?})",
                        delay,
                        retry_after,
                    );
                    state
                        .backoff_by_slot
                        .write()
                        .insert(slot, BackoffState {
                            until: Instant::now() + delay,
                            reason: BackoffReason::RateLimited,
                        });
                }
                None => {
                    // Retry-After: 0 or absent — retry at the next scheduled
                    // poll; the error state clears on the next success.
                    tracing::warn!(
                        "slot {slot} rate-limited; retrying at next scheduled poll (server retry-after={retry_after:?})",
                    );
                }
            }
            let mut entry = state
                .cached_usage_by_slot
                .write()
                .remove(&slot)
                .unwrap_or_else(|| placeholder_cached(acc, RATE_LIMITED_ERROR));
            entry.last_error = Some(RATE_LIMITED_ERROR.into());
            state.cached_usage_by_slot.write().insert(slot, entry.clone());
            let _ = handle.emit(
                "usage_updated",
                json!({ "slot": slot, "cached": entry }),
            );
        }
        FetchOutcome::Transient(e) => {
            let mut entry = state
                .cached_usage_by_slot
                .write()
                .remove(&slot)
                .unwrap_or_else(|| placeholder_cached(acc, &e));
            entry.last_error = Some(e);
            state.cached_usage_by_slot.write().insert(slot, entry.clone());
            let _ = handle.emit(
                "usage_updated",
                json!({ "slot": slot, "cached": entry }),
            );
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
    let mut accounts = state.accounts.list().unwrap_or_default();
    let active_slot = live.as_ref().and_then(|l| {
        accounts
            .iter()
            .find(|a| a.matches_credentials(&l.claude_code_oauth_blob, &l.oauth_account_blob))
            .map(|a| a.slot)
    });
    let prev_active_slot = std::mem::replace(&mut *state.active_slot.write(), active_slot);

    // Claude Code re-fetches the active account's profile as it runs, and
    // that profile is where a plan change shows up. Keep the stored copy in
    // step, or the row keeps the plan the account had when it was added.
    let profile_changed = match (&live, active_slot) {
        (Some(live), Some(slot)) => {
            sync_active_profile(state, &mut accounts, slot, &live.oauth_account_blob)
        }
        _ => false,
    };

    // Catches the paths that never go through `swap_to_account`: an external
    // `cswap`, or a Claude Code / cowork instance started on another machine.
    // Any shared snapshot older than this moment describes the account that
    // was live before the change.
    if prev_active_slot != active_slot {
        *state.active_since.write() = Some(Utc::now());
    }

    // Notify the frontend whenever the active slot transitions. The
    // frontend's `accounts` array only carries `is_active` flags from the
    // last `list_accounts` call; without this event, an out-of-band CC
    // login (or the startup race where `init()` reads `list_accounts`
    // before this loop's first tick) leaves the AccountsPanel showing no
    // active highlight even though the backend knows which slot is live.
    // The same goes for the plan badge when the active profile changed.
    if prev_active_slot != active_slot || profile_changed {
        let entries: Vec<crate::commands::AccountListEntry> = accounts
            .iter()
            .map(|a| crate::commands::entry_for(state, a, active_slot))
            .collect();
        let _ = handle.emit("accounts_changed", &entries);
    }

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
            let (interval, base_gap) = settings_durations(state);
            let slot_ids: Vec<u32> = accounts.iter().map(|a| a.slot).collect();
            *sched = seed_schedules(
                &slot_ids,
                active_slot,
                Instant::now(),
                interval,
                base_gap,
            );
        } else {
            // Append newly added slots so they trail the existing
            // schedule (one stagger gap behind the latest deadline).
            // This avoids placing a new slot ahead of an existing slot's
            // next fetch, which would break the configured base gap.
            let now = Instant::now();
            let (interval, base_gap) = settings_durations(state);
            let gap = stagger_gap(current_slots.len(), interval, base_gap);
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

    // 4. A 429 ban only bounds this app's own /usage calls: keep banned
    //    slots' numbers current from local sources meanwhile.
    adopt_local_during_bans(handle, state, burn_buffers, &accounts, active_slot).await;
    // Backstop for the observation watcher (which can miss events on network
    // volumes or before the directory exists).
    adopt_fresh_statusline(handle, state, burn_buffers, &accounts, active_slot).await;

    // 5. Pick at most one due slot, fetch it, advance its deadline.
    let now = Instant::now();
    if let Some(slot) = pick_due_slot(state, now) {
        let local_fetched_at = fetch_and_apply_one(handle, state, burn_buffers, slot, &accounts).await;
        let interval = Duration::from_secs(
            state.settings.read().polling_interval_secs.max(60),
        );
        let wait = next_poll_wait(interval, local_fetched_at, Utc::now());
        if let Some(entry) = state.schedule_by_slot.write().get_mut(&slot) {
            entry.next_poll_at = Instant::now() + wait;
        }
    }

    Ok(())
}

/// Store the live profile slice on the active slot when it changed, and
/// patch `accounts` so this tick already sees it. Returns whether it changed.
/// Best-effort: a failed write is retried on the next tick.
fn sync_active_profile(
    state: &AppState,
    accounts: &mut [ManagedAccount],
    slot: u32,
    live_profile: &serde_json::Value,
) -> bool {
    let Some(acc) = accounts.iter_mut().find(|a| a.slot == slot) else {
        return false;
    };
    // Compared in memory first: the common case is "nothing changed", and
    // that must not take the accounts file lock on every tick.
    if acc.oauth_account_blob == *live_profile {
        return false;
    }
    match state.accounts.sync_live_profile(slot, live_profile) {
        Ok(true) => {
            let before = acc.plan();
            acc.oauth_account_blob = live_profile.clone();
            let after = acc.plan();
            if before != after {
                tracing::info!(
                    target: "switchboard.auth",
                    "slot {slot}: plan changed from {before:?} to {after:?}"
                );
            }
            true
        }
        Ok(false) => false,
        Err(e) => {
            tracing::warn!("slot {slot}: storing the live profile failed: {e:#}");
            false
        }
    }
}

/// How long until a slot's next poll. A local snapshot already spent part of
/// the interval, so poll again when it ages out — otherwise a source that
/// goes quiet lets the shown data reach nearly twice the interval.
fn next_poll_wait(
    interval: Duration,
    local_fetched_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Duration {
    // A future-skewed timestamp fails `to_std` and gets the full interval.
    local_fetched_at
        .and_then(|fetched_at| (now - fetched_at).to_std().ok())
        .map_or(interval, |age| interval.saturating_sub(age))
}

/// Adopt local snapshots newer than what each 429-banned slot shows. Runs
/// every loop tick (at most 60 s apart), so a ban no longer freezes the
/// numbers for its full hour whenever another local consumer is active.
/// When nothing newer turns up, restores the 429 label once the shown data
/// ages past a poll interval (see [`relabel_aged_during_ban`]).
async fn adopt_local_during_bans(
    handle: &AppHandle,
    state: &AppState,
    burn_buffers: &mut HashMap<u32, VecDeque<(DateTime<Utc>, f64)>>,
    accounts: &[ManagedAccount],
    active_slot: Option<u32>,
) {
    let now = Instant::now();
    let banned: Vec<u32> = state
        .backoff_by_slot
        .read()
        .iter()
        .filter(|(_, b)| b.reason == BackoffReason::RateLimited && now < b.until)
        .map(|(&slot, _)| slot)
        .collect();
    let interval = Duration::from_secs(state.settings.read().polling_interval_secs.max(60));
    for slot in banned {
        let Some(acc) = accounts.iter().find(|a| a.slot == slot) else { continue };
        if !acc.supports_usage() {
            continue;
        }
        let adoptable = local_snapshot(state, acc, active_slot, MAX_LOCAL_AGE_DURING_BAN)
            .filter(|(snap, _)| is_newer_than_shown(state, slot, snap.fetched_at));
        if let Some((snap, origin)) = adoptable {
            tracing::info!(target: "switchboard.poll", "slot {slot}: rate-limited; adopted {origin:?} snapshot");
            apply_fetch_outcome(
                handle,
                state,
                burn_buffers,
                slot,
                acc,
                active_slot,
                FetchOutcome::Ok(snap),
                origin,
            )
            .await;
            continue;
        }
        let relabeled = state
            .cached_usage_by_slot
            .write()
            .get_mut(&slot)
            .and_then(|cached| {
                relabel_aged_during_ban(cached, interval, Utc::now()).then(|| cached.clone())
            });
        if let Some(cached) = relabeled {
            if Some(slot) == active_slot {
                *state.cached_usage.write() = Some(cached.clone());
            }
            let _ = handle.emit("usage_updated", json!({ "slot": slot, "cached": cached }));
        }
    }
}

/// Adopt statusline observations newer than what each slot shows. Runs on
/// every watcher wake-up and every loop tick, so the popover follows Claude
/// Code's own numbers within about a second instead of once per interval.
async fn adopt_fresh_statusline(
    handle: &AppHandle,
    state: &AppState,
    burn_buffers: &mut HashMap<u32, VecDeque<(DateTime<Utc>, f64)>>,
    accounts: &[ManagedAccount],
    active_slot: Option<u32>,
) {
    let interval = Duration::from_secs(state.settings.read().polling_interval_secs.max(60));
    let now = Instant::now();
    for acc in accounts {
        let banned = state
            .backoff_by_slot
            .read()
            .get(&acc.slot)
            .is_some_and(|b| b.reason == BackoffReason::RateLimited && now < b.until);
        let max_age = if banned { MAX_LOCAL_AGE_DURING_BAN } else { interval };
        let Some(snap) = crate::statusline_source::snapshot_for(state, acc, active_slot, max_age)
        else {
            continue;
        };
        if !is_newer_than_shown(state, acc.slot, snap.fetched_at) {
            continue;
        }
        tracing::debug!(target: "switchboard.poll", "slot {}: adopted Statusline snapshot", acc.slot);
        apply_fetch_outcome(
            handle,
            state,
            burn_buffers,
            acc.slot,
            acc,
            active_slot,
            FetchOutcome::Ok(snap),
            SnapshotOrigin::Statusline,
        )
        .await;
    }
}

/// A local snapshot adopted mid-ban while under a poll interval old shows as
/// live (no `last_error`). Once it ages past the interval with the ban still
/// in force, label it rate-limited again, so the row reads "showing last good
/// data from 40m ago" instead of passing it off as current. Returns whether
/// the entry changed; never overwrites another error.
fn relabel_aged_during_ban(cached: &mut CachedUsage, interval: Duration, now: DateTime<Utc>) -> bool {
    if cached.last_error.is_some()
        || !has_usage_data(&cached.snapshot)
        || is_within_age(cached.snapshot.fetched_at, interval, now)
    {
        return false;
    }
    cached.last_error = Some(RATE_LIMITED_ERROR.to_string());
    true
}

/// Decide the backoff for a 429 response, or `None` for "no extra backoff —
/// retry at the next normally scheduled poll".
///
/// The Anthropic usage endpoint returns `Retry-After: 0` on most 429s: the
/// server is asking for no delay beyond the caller's own cadence. The
/// per-slot poll schedule already spaces retries by `polling_interval_secs`
/// (advanced unconditionally after each fetch attempt), so honoring the zero
/// keeps the request rate identical to the success path — no hammering.
/// The previous implementation treated 0 as "no guidance" and escalated
/// 2→4→8→10 min, which stretched each transient 429 into a multi-minute
/// "usage unavailable" window for no benefit.
///
/// A positive `Retry-After` is a ban — typically 3600s, counting down on a
/// fixed window — and is honored in full plus [`RETRY_AFTER_MARGIN`], capped
/// at [`MAX_RATE_LIMIT_BACKOFF`]. An earlier 10-minute cap retried into the
/// ban five times per hour, and a retry at its edge restarted it.
fn backoff_for_429(retry_after: Option<Duration>) -> Option<Duration> {
    match retry_after {
        Some(d) if d > Duration::ZERO => {
            Some(d.saturating_add(RETRY_AFTER_MARGIN).min(MAX_RATE_LIMIT_BACKOFF))
        }
        _ => None,
    }
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
            seven_day_fable: None,
            extra_usage: None,
            fetched_at: Utc::now(),
            unknown: Default::default(),
        },
        account_id: acc.account_uuid.clone(),
        account_email: acc.display_name(),
        last_error: Some(err.to_string()),
        burn_rate: None,
        auth_source: AuthSource::OAuth,
        origin: SnapshotOrigin::Http,
        carried_fetched_at: None,
    }
}

/// Location of the shared usage snapshot written by an external poller, such
/// as a statusline daemon that polls `/api/oauth/usage` for the live Claude
/// Code account. Absent unless the user set one up.
/// `SWITCHBOARD_SHARED_USAGE_FILE` overrides for testing.
fn shared_usage_file_path() -> std::path::PathBuf {
    if let Some(p) = std::env::var_os("SWITCHBOARD_SHARED_USAGE_FILE") {
        return std::path::PathBuf::from(p);
    }
    crate::auth::paths::claude_config_home()
        .unwrap_or_else(|| std::path::PathBuf::from("/"))
        .join("statusline-usage.json")
}

/// Adopt a shared usage snapshot written by an external poller when it's
/// fresher than `max_age`. Returns `None` when the file is missing, stale,
/// unparsable, or lacks the injected `fetched_at` epoch-seconds marker that
/// identifies the daemon's format (a bare /usage payload has no timestamp,
/// so a file without one can't be freshness-checked and is ignored).
///
/// Why this exists: the /usage endpoint's rate budget is per-account and is
/// shared by every consumer, so a poller already fetching for the live
/// account makes this app's own call for it pure overhead.
pub fn read_shared_snapshot(
    path: &std::path::Path,
    max_age: Duration,
    active_since: Option<DateTime<Utc>>,
) -> Option<UsageSnapshot> {
    let raw = std::fs::read_to_string(path).ok()?;
    let mut value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let obj = value.as_object_mut()?;
    let fetched_at = obj.remove("fetched_at")?.as_i64()?;
    let fetched_at = DateTime::from_timestamp(fetched_at, 0)?;
    if !is_within_age(fetched_at, max_age, Utc::now()) {
        return None;
    }
    // The file carries NO account identity — it describes whichever account
    // was live when the daemon wrote it. A snapshot written before the active
    // account last changed therefore describes the *previous* account, and
    // adopting it would put that account's numbers on the new active slot
    // while the previous account's own slot fetches the same numbers with its
    // own token — two rows showing identical usage until the file ages out.
    //
    // `active_since` is when the live account last changed (swap, external
    // `cswap`, or a Claude Code / cowork instance started elsewhere). `None`
    // means no change has been observed this run, so there is nothing to
    // invalidate against.
    if let Some(since) = active_since {
        if fetched_at < since {
            return None;
        }
    }
    let mut snap: UsageSnapshot = serde_json::from_value(value).ok()?;
    snap.fetched_at = fetched_at;
    Some(snap)
}

/// Adopt the upstream CLI's persisted `/usage` response for `account_uuid`
/// when younger than `max_age`. CLI 2.1.311+ keeps one in its global config
/// as `cachedUsageUtilization: {fetchedAtMs, accountUuid?, utilization}`,
/// where `utilization` is the raw `/usage` body. It writes one only after
/// its own `/usage` call, at most once a minute, and only for the account it
/// is logged in as.
///
/// Unlike [`read_shared_snapshot`] this source names its account, so it is
/// valid for any slot — after a swap, a cache left behind by the previous
/// account still lands on that account's own row. An entry without an
/// `accountUuid` is ignored: it can't be attributed.
pub fn read_cli_usage_cache(
    path: &std::path::Path,
    account_uuid: &str,
    max_age: Duration,
) -> Option<UsageSnapshot> {
    if account_uuid.is_empty() {
        return None;
    }
    let raw = std::fs::read_to_string(path).ok()?;
    let config: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let cache = config.get("cachedUsageUtilization")?;
    if cache.get("accountUuid")?.as_str()? != account_uuid {
        return None;
    }
    let fetched_at_ms = cache.get("fetchedAtMs")?;
    let fetched_at_ms = fetched_at_ms
        .as_i64()
        .or_else(|| fetched_at_ms.as_f64().map(|ms| ms as i64))?;
    let fetched_at = DateTime::from_timestamp_millis(fetched_at_ms)?;
    if !is_within_age(fetched_at, max_age, Utc::now()) {
        return None;
    }
    let mut snap: UsageSnapshot = serde_json::from_value(cache.get("utilization")?.clone()).ok()?;
    snap.fetched_at = fetched_at;
    Some(snap)
}

/// Whether `fetched_at` is at most `max_age` old, rejecting clock-skewed
/// future timestamps beyond a 30 s grace.
pub(crate) fn is_within_age(fetched_at: DateTime<Utc>, max_age: Duration, now: DateTime<Utc>) -> bool {
    let Ok(max_age) = ChronoDuration::from_std(max_age) else { return false };
    fetched_at <= now + ChronoDuration::seconds(30) && now - fetched_at <= max_age
}

/// The freshest snapshot for `acc` that a local source holds within
/// `max_age`, or `None` when no source has one.
fn local_snapshot(
    state: &AppState,
    acc: &ManagedAccount,
    active_slot: Option<u32>,
    max_age: Duration,
) -> Option<(UsageSnapshot, SnapshotOrigin)> {
    let cli_cache = crate::auth::paths::claude_global_config()
        .and_then(|path| read_cli_usage_cache(&path, &acc.account_uuid, max_age))
        .map(|snap| (snap, SnapshotOrigin::CliCache));
    // The shared file carries no account identity; it describes the live
    // account, so only the active slot may adopt it.
    let shared_file = (Some(acc.slot) == active_slot)
        .then(|| {
            read_shared_snapshot(&shared_usage_file_path(), max_age, *state.active_since.read())
        })
        .flatten()
        .map(|snap| (snap, SnapshotOrigin::SharedFile));
    let statusline = crate::statusline_source::snapshot_for(state, acc, active_slot, max_age)
        .map(|snap| (snap, SnapshotOrigin::Statusline));
    pick_freshest([cli_cache, shared_file, statusline])
}

/// The newest of the local sources' answers.
fn pick_freshest(
    candidates: impl IntoIterator<Item = Option<(UsageSnapshot, SnapshotOrigin)>>,
) -> Option<(UsageSnapshot, SnapshotOrigin)> {
    candidates
        .into_iter()
        .flatten()
        .max_by_key(|(snap, _)| snap.fetched_at)
}

/// Whether a snapshot fetched at `fetched_at` is newer than the data `slot`
/// currently shows. A placeholder entry (an error with no buckets) shows no
/// data, so anything beats it.
fn is_newer_than_shown(state: &AppState, slot: u32, fetched_at: DateTime<Utc>) -> bool {
    match state.cached_usage_by_slot.read().get(&slot) {
        Some(cached) if has_usage_data(&cached.snapshot) => fetched_at > cached.snapshot.fetched_at,
        _ => true,
    }
}

pub(crate) fn has_usage_data(snapshot: &UsageSnapshot) -> bool {
    snapshot.five_hour.is_some()
        || snapshot.seven_day.is_some()
        || snapshot.seven_day_sonnet.is_some()
        || snapshot.seven_day_fable.is_some()
        || snapshot.seven_day_opus.is_some()
        || snapshot.extra_usage.is_some()
}

/// Build per-slot cache entries from the most recent persisted API snapshot
/// for each account. Called once at startup so the UI has last-known-good
/// data before the first poll completes — without this, a cold start during
/// a rate-limit storm shows "usage unavailable" (empty placeholder) until a
/// fetch finally succeeds, which can take minutes.
///
/// Accounts without a persisted snapshot, or whose latest payload fails to
/// decode, are skipped (the poll loop will fill them in on its first tick).
pub fn hydrated_caches(
    db: &crate::store::Db,
    accounts: &[ManagedAccount],
) -> HashMap<u32, CachedUsage> {
    let mut out = HashMap::new();
    for acc in accounts {
        let payload = match db.latest_snapshot(&acc.account_uuid) {
            Ok(Some((_fetched_at, payload))) => payload,
            Ok(None) => continue,
            Err(e) => {
                tracing::warn!("hydrate: latest_snapshot({}) failed: {e}", acc.account_uuid);
                continue;
            }
        };
        match serde_json::from_str::<UsageSnapshot>(&payload) {
            Ok(mut snapshot) => {
                // Same rule as a fresh snapshot (see `apply_fetch_outcome`):
                // the plan may have become known since this one was saved.
                snapshot.hide_fable_without_access(
                    crate::auth::accounts::plan_includes_fable(acc.plan().as_deref()),
                );
                out.insert(
                    acc.slot,
                    CachedUsage {
                        snapshot,
                        account_id: acc.account_uuid.clone(),
                        account_email: acc.email.clone(),
                        last_error: None,
                        burn_rate: None,
                        auth_source: AuthSource::OAuth,
                        origin: SnapshotOrigin::Http,
                        carried_fetched_at: None,
                    },
                );
            }
            Err(e) => {
                tracing::warn!(
                    "hydrate: decode snapshot for {} failed: {e}",
                    acc.account_uuid
                );
            }
        }
    }
    out
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

    fn d(secs: u64) -> Duration {
        Duration::from_secs(secs)
    }

    #[test]
    fn stagger_gap_returns_base_when_interval_fits() {
        // 3 slots, 300 s interval, 30 s base → needs 60 s for stagger, fits.
        let gap = stagger_gap(3, d(300), d(30));
        assert_eq!(gap, d(30));
    }

    #[test]
    fn stagger_gap_compresses_when_interval_too_short() {
        // 4 slots, 60 s interval, 30 s base → 90 s needed > 60 s → 60/4 = 15 s.
        let gap = stagger_gap(4, d(60), d(30));
        assert_eq!(gap, d(15));
    }

    #[test]
    fn stagger_gap_honors_custom_base() {
        // 3 slots, 300 s interval, 60 s base → 2*60 = 120 ≤ 300, gap stays 60.
        let gap = stagger_gap(3, d(300), d(60));
        assert_eq!(gap, d(60));
    }

    #[test]
    fn stagger_gap_zero_slots_returns_base() {
        assert_eq!(stagger_gap(0, d(60), d(30)), d(30));
        assert_eq!(stagger_gap(0, d(60), d(15)), d(15));
    }

    #[test]
    fn seed_schedules_active_first_then_inactive_in_slot_id_order() {
        let now = Instant::now();
        let sched = seed_schedules(&[3, 1, 2], Some(2), now, d(300), d(30));

        assert_eq!(sched[&2].next_poll_at, now);
        assert_eq!(sched[&1].next_poll_at, now + d(30));
        assert_eq!(sched[&3].next_poll_at, now + d(60));
    }

    #[test]
    fn seed_schedules_no_active_slot_orders_by_slot_id() {
        let now = Instant::now();
        let sched = seed_schedules(&[3, 1, 2], None, now, d(300), d(30));

        assert_eq!(sched[&1].next_poll_at, now);
        assert_eq!(sched[&2].next_poll_at, now + d(30));
        assert_eq!(sched[&3].next_poll_at, now + d(60));
    }

    #[test]
    fn seed_schedules_active_not_in_slots_ignored() {
        let now = Instant::now();
        let sched = seed_schedules(&[1, 2], Some(99), now, d(300), d(30));

        assert_eq!(sched[&1].next_poll_at, now);
        assert_eq!(sched[&2].next_poll_at, now + d(30));
        assert!(!sched.contains_key(&99));
    }

    #[test]
    fn seed_schedules_empty_slots_returns_empty_map() {
        let now = Instant::now();
        let sched = seed_schedules(&[], Some(1), now, d(300), d(30));
        assert!(sched.is_empty());
    }

    #[test]
    fn seed_schedules_custom_base_gap_propagates_to_offsets() {
        // 3 slots, 300 s interval, 60 s base → offsets at 0, 60, 120.
        let now = Instant::now();
        let sched = seed_schedules(&[1, 2, 3], None, now, d(300), d(60));
        assert_eq!(sched[&1].next_poll_at, now);
        assert_eq!(sched[&2].next_poll_at, now + d(60));
        assert_eq!(sched[&3].next_poll_at, now + d(120));
    }

    #[test]
    fn backoff_for_429_zero_retry_after_means_no_backoff() {
        // The usage endpoint returns `Retry-After: 0` on most 429s — the
        // server is asking for no delay beyond our normal poll cadence (the
        // per-slot schedule already spaces retries by the polling interval),
        // so no backoff entry should be created.
        assert_eq!(backoff_for_429(Some(Duration::ZERO)), None);
    }

    #[test]
    fn backoff_for_429_missing_retry_after_means_no_backoff() {
        assert_eq!(backoff_for_429(None), None);
    }

    #[test]
    fn backoff_for_429_positive_retry_after_is_honored_in_full_plus_margin() {
        assert_eq!(backoff_for_429(Some(d(120))), Some(d(180)));
        assert_eq!(backoff_for_429(Some(d(5))), Some(d(65)));
        // The observed 1-hour ban: a retry inside it, or right at its edge,
        // can restart it — so wait it out completely.
        assert_eq!(backoff_for_429(Some(d(3600))), Some(d(3660)));
    }

    #[test]
    fn backoff_for_429_caps_absurd_retry_after() {
        assert_eq!(backoff_for_429(Some(d(86_400))), Some(d(2 * 60 * 60)));
        // Adding the margin must not overflow (a panic aborts in release).
        assert_eq!(backoff_for_429(Some(d(u64::MAX))), Some(d(2 * 60 * 60)));
    }

    mod deadlines {
        use super::*;
        use crate::app_state::ScheduleState;

        fn banned_until(until: Instant) -> BackoffState {
            BackoffState { until, reason: BackoffReason::RateLimited }
        }

        #[test]
        fn a_banned_slot_wakes_the_loop_at_its_ban_end_not_its_past_deadline() {
            let now = Instant::now();
            let schedule = HashMap::from([
                (1, ScheduleState { next_poll_at: now - d(10) }),
                (2, ScheduleState { next_poll_at: now + d(100) }),
            ]);
            let backoff = HashMap::from([(1, banned_until(now + d(3000)))]);
            assert_eq!(earliest_deadline(&schedule, &backoff), Some(now + d(100)));

            let only_banned = HashMap::from([(1, ScheduleState { next_poll_at: now - d(10) })]);
            assert_eq!(earliest_deadline(&only_banned, &backoff), Some(now + d(3000)));
        }

        #[test]
        fn an_expired_backoff_leaves_the_deadline_alone() {
            let now = Instant::now();
            let schedule = HashMap::from([(1, ScheduleState { next_poll_at: now + d(50) })]);
            let backoff = HashMap::from([(1, banned_until(now + d(20)))]);
            assert_eq!(earliest_deadline(&schedule, &backoff), Some(now + d(50)));
            assert_eq!(earliest_deadline(&HashMap::new(), &backoff), None);
        }

        #[test]
        fn next_poll_wait_subtracts_the_local_snapshots_age() {
            let now = Utc::now();
            let interval = d(300);
            assert_eq!(next_poll_wait(interval, None, now), interval);
            assert_eq!(
                next_poll_wait(interval, Some(now - ChronoDuration::seconds(290)), now),
                d(10)
            );
            assert_eq!(
                next_poll_wait(interval, Some(now - ChronoDuration::seconds(400)), now),
                Duration::ZERO
            );
            // Future-skewed timestamp: no age to subtract.
            assert_eq!(
                next_poll_wait(interval, Some(now + ChronoDuration::seconds(5)), now),
                interval
            );
        }
    }

    mod relabel {
        use super::*;

        fn cached(fetched_at: DateTime<Utc>, last_error: Option<&str>) -> CachedUsage {
            CachedUsage {
                snapshot: UsageSnapshot {
                    five_hour: Some(crate::usage_api::Utilization {
                        utilization: 40.0,
                        resets_at: None,
                    }),
                    seven_day: None,
                    seven_day_sonnet: None,
                    seven_day_opus: None,
                    seven_day_fable: None,
                    extra_usage: None,
                    fetched_at,
                    unknown: Default::default(),
                },
                origin: SnapshotOrigin::Http,
                carried_fetched_at: None,
                account_id: "uuid".into(),
                account_email: "a@example.com".into(),
                last_error: last_error.map(str::to_string),
                burn_rate: None,
                auth_source: AuthSource::OAuth,
            }
        }

        #[test]
        fn restores_the_429_label_once_the_data_ages_past_an_interval() {
            let now = Utc::now();
            let mut c = cached(now - ChronoDuration::seconds(400), None);
            assert!(relabel_aged_during_ban(&mut c, d(300), now));
            assert_eq!(c.last_error.as_deref(), Some(RATE_LIMITED_ERROR));
        }

        #[test]
        fn leaves_young_data_and_other_errors_alone() {
            let now = Utc::now();
            let mut young = cached(now - ChronoDuration::seconds(100), None);
            assert!(!relabel_aged_during_ban(&mut young, d(300), now));
            assert_eq!(young.last_error, None);

            let mut auth = cached(now - ChronoDuration::seconds(400), Some("auth_required"));
            assert!(!relabel_aged_during_ban(&mut auth, d(300), now));
            assert_eq!(auth.last_error.as_deref(), Some("auth_required"));
        }
    }

    mod freshest {
        use super::*;

        fn snap(fetched_at: DateTime<Utc>) -> UsageSnapshot {
            UsageSnapshot {
                five_hour: None,
                seven_day: None,
                seven_day_sonnet: None,
                seven_day_opus: None,
                seven_day_fable: None,
                extra_usage: None,
                fetched_at,
                unknown: Default::default(),
            }
        }

        #[test]
        fn a_statusline_snapshot_beats_an_older_cli_cache() {
            let now = Utc::now();
            let older = now - ChronoDuration::seconds(40);
            let (picked, origin) = pick_freshest([
                Some((snap(older), SnapshotOrigin::CliCache)),
                None,
                Some((snap(now), SnapshotOrigin::Statusline)),
            ])
            .unwrap();
            assert_eq!(origin, SnapshotOrigin::Statusline);
            assert_eq!(picked.fetched_at, now);
        }

        #[test]
        fn no_source_yields_none() {
            assert!(pick_freshest([None, None, None]).is_none());
        }
    }

    mod local_only {
        use super::*;

        fn snap(fetched_at: DateTime<Utc>) -> UsageSnapshot {
            UsageSnapshot {
                five_hour: Some(crate::usage_api::Utilization {
                    utilization: 33.0,
                    resets_at: None,
                }),
                seven_day: None,
                seven_day_sonnet: None,
                seven_day_opus: None,
                seven_day_fable: None,
                extra_usage: None,
                fetched_at,
                unknown: Default::default(),
            }
        }

        #[test]
        fn local_only_adopts_a_newer_local_snapshot() {
            let at = Utc::now();
            match resolve_local_only(Some((snap(at), SnapshotOrigin::Statusline)), true) {
                LocalOnly::Adopt(s, origin) => {
                    assert_eq!(s.fetched_at, at);
                    assert_eq!(origin, SnapshotOrigin::Statusline);
                }
                other => panic!("expected Adopt, got {other:?}"),
            }
        }

        #[test]
        fn local_only_keeps_shown_data_when_local_is_not_newer() {
            let at = Utc::now();
            match resolve_local_only(Some((snap(at), SnapshotOrigin::Statusline)), false) {
                LocalOnly::KeepShown(fetched_at) => assert_eq!(fetched_at, at),
                other => panic!("expected KeepShown, got {other:?}"),
            }
        }

        #[test]
        fn local_only_without_local_data_is_not_supported() {
            assert!(matches!(
                resolve_local_only(None, true),
                LocalOnly::NotSupported
            ));
        }
    }

    #[test]
    fn has_usage_data_is_false_for_placeholders() {
        let snap = UsageSnapshot {
            five_hour: None,
            seven_day: None,
            seven_day_sonnet: None,
            seven_day_opus: None,
            seven_day_fable: None,
            extra_usage: None,
            fetched_at: Utc::now(),
            unknown: Default::default(),
        };
        assert!(!has_usage_data(&snap));
    }

    #[test]
    fn is_within_age_bounds() {
        let now = Utc::now();
        let max = d(120);
        assert!(is_within_age(now - ChronoDuration::seconds(120), max, now));
        assert!(!is_within_age(now - ChronoDuration::seconds(121), max, now));
        // 30 s of clock-skew grace, then reject.
        assert!(is_within_age(now + ChronoDuration::seconds(30), max, now));
        assert!(!is_within_age(now + ChronoDuration::seconds(31), max, now));
    }

    mod hydrate {
        use super::*;
        use crate::auth::accounts::{AddSource, ManagedAccount};
        use crate::store::Db;
        use tempfile::tempdir;

        fn acc(slot: u32, uuid: &str) -> ManagedAccount {
            ManagedAccount {
                slot,
                email: format!("slot{slot}@example.com"),
                label: None,
                account_uuid: uuid.to_string(),
                organization_uuid: None,
                organization_name: None,
                subscription_type: None,
                source: AddSource::OAuth,
                claude_code_oauth_blob: serde_json::json!({}),
                oauth_account_blob: serde_json::json!({}),
                token_expires_at: Some(Utc::now()),
                added_at: Utc::now(),
                last_seen_active: None,
            }
        }

        const PAYLOAD: &str = r#"{
            "five_hour": { "utilization": 42.5, "resets_at": "2026-04-24T18:00:00Z" },
            "seven_day": { "utilization": 63.1, "resets_at": "2026-04-30T09:00:00Z" }
        }"#;

        fn db_with_account(uuid: &str) -> (tempfile::TempDir, Db) {
            let dir = tempdir().unwrap();
            let db = Db::open(dir.path()).unwrap();
            // api_snapshots.account_id is FK'd to accounts(id) — mirror the
            // account first, as mirror_account_to_sqlite does in production.
            db.upsert_account(&crate::store::StoredAccount {
                id: uuid.to_string(),
                email: "a@example.com".into(),
                display_name: None,
            })
            .unwrap();
            (dir, db)
        }

        #[test]
        fn returns_persisted_snapshot_for_account_with_last_error_cleared() {
            let (_dir, db) = db_with_account("uuid-2");
            db.insert_snapshot("uuid-2", Utc::now(), PAYLOAD).unwrap();

            let caches = hydrated_caches(&db, &[acc(2, "uuid-2")]);
            let entry = caches.get(&2).expect("slot 2 hydrated");
            assert_eq!(
                entry.snapshot.five_hour.as_ref().unwrap().utilization,
                42.5
            );
            assert_eq!(entry.last_error, None);
            assert_eq!(entry.account_email, "slot2@example.com");
        }

        /// A snapshot saved before the plan was known still carries the
        /// Fable row the API sends to everyone.
        const PAYLOAD_WITH_UNUSED_FABLE: &str = r#"{
            "five_hour": { "utilization": 0.0, "resets_at": null },
            "seven_day": { "utilization": 85.0, "resets_at": null },
            "seven_day_fable": { "utilization": 0.0, "resets_at": null }
        }"#;

        fn acc_on_plan(slot: u32, uuid: &str, organization_type: &str) -> ManagedAccount {
            let mut account = acc(slot, uuid);
            account.oauth_account_blob = serde_json::json!({
                "accountUuid": uuid,
                "organizationType": organization_type,
            });
            account
        }

        #[test]
        fn hides_the_unused_fable_row_of_a_plan_without_fable() {
            let (_dir, db) = db_with_account("uuid-2");
            db.insert_snapshot("uuid-2", Utc::now(), PAYLOAD_WITH_UNUSED_FABLE)
                .unwrap();

            let pro = hydrated_caches(&db, &[acc_on_plan(2, "uuid-2", "claude_pro")]);
            let snapshot = &pro.get(&2).expect("slot 2 hydrated").snapshot;
            assert!(snapshot.seven_day_fable.is_none());
            assert!(snapshot.seven_day.is_some());

            let max = hydrated_caches(&db, &[acc_on_plan(2, "uuid-2", "claude_max")]);
            assert!(max.get(&2).unwrap().snapshot.seven_day_fable.is_some());
        }

        #[test]
        fn skips_accounts_without_snapshots() {
            let dir = tempdir().unwrap();
            let db = Db::open(dir.path()).unwrap();
            let caches = hydrated_caches(&db, &[acc(2, "uuid-2")]);
            assert!(caches.is_empty());
        }

        #[test]
        fn skips_corrupt_payloads_instead_of_failing() {
            let (_dir, db) = db_with_account("uuid-2");
            db.insert_snapshot("uuid-2", Utc::now(), "not json").unwrap();
            let caches = hydrated_caches(&db, &[acc(2, "uuid-2")]);
            assert!(caches.is_empty());
        }
    }

    mod shared_snapshot {
        use super::*;
        use tempfile::tempdir;

        fn write(dir: &tempfile::TempDir, body: &str) -> std::path::PathBuf {
            let p = dir.path().join("statusline-usage.json");
            std::fs::write(&p, body).unwrap();
            p
        }

        fn payload(fetched_at: i64) -> String {
            format!(
                r#"{{"five_hour": {{"utilization": 42.5, "resets_at": "2026-04-24T18:00:00Z"}}, "seven_day": null, "fetched_at": {fetched_at}}}"#
            )
        }

        #[test]
        fn adopts_fresh_valid_snapshot() {
            let dir = tempdir().unwrap();
            let now = Utc::now().timestamp();
            let p = write(&dir, &payload(now));
            let snap = read_shared_snapshot(&p, Duration::from_secs(120), None)
                .expect("fresh snapshot adopted");
            assert_eq!(snap.five_hour.unwrap().utilization, 42.5);
            assert_eq!(snap.fetched_at.timestamp(), now);
        }

        #[test]
        fn tolerates_unknown_vendor_fields() {
            // The live file carries extra buckets and codename fields the
            // typed snapshot doesn't know — forward-compat must hold.
            let dir = tempdir().unwrap();
            let now = Utc::now().timestamp();
            let body = format!(
                r#"{{"five_hour": {{"utilization": 2.0, "resets_at": null}}, "seven_day": {{"utilization": 26.0, "resets_at": "2026-07-25T02:59:59Z"}}, "seven_day_cowork": null, "tangelo": null, "limits": [], "fetched_at": {now}}}"#
            );
            let p = write(&dir, &body);
            assert!(read_shared_snapshot(&p, Duration::from_secs(120), None).is_some());
        }

        #[test]
        fn rejects_stale_snapshot() {
            let dir = tempdir().unwrap();
            let old = (Utc::now() - ChronoDuration::minutes(10)).timestamp();
            let p = write(&dir, &payload(old));
            assert!(read_shared_snapshot(&p, Duration::from_secs(120), None).is_none());
        }

        #[test]
        fn rejects_future_timestamp() {
            let dir = tempdir().unwrap();
            let future = (Utc::now() + ChronoDuration::minutes(5)).timestamp();
            let p = write(&dir, &payload(future));
            assert!(read_shared_snapshot(&p, Duration::from_secs(120), None).is_none());
        }

        #[test]
        fn missing_file_returns_none() {
            let dir = tempdir().unwrap();
            let p = dir.path().join("does-not-exist.json");
            assert!(read_shared_snapshot(&p, Duration::from_secs(120), None).is_none());
        }

        #[test]
        fn corrupt_json_returns_none() {
            let dir = tempdir().unwrap();
            let p = write(&dir, "not json at all");
            assert!(read_shared_snapshot(&p, Duration::from_secs(120), None).is_none());
        }

        #[test]
        fn missing_fetched_at_returns_none() {
            // Without the daemon's injected epoch marker we can't judge
            // freshness — treat as a foreign file and ignore it.
            let dir = tempdir().unwrap();
            let p = write(&dir, r#"{"five_hour": {"utilization": 42.5, "resets_at": null}}"#);
            assert!(read_shared_snapshot(&p, Duration::from_secs(120), None).is_none());
        }

        /// The file carries NO account identity — no email, no account_uuid,
        /// nothing. It describes whichever account was live when the daemon
        /// wrote it. So a snapshot written BEFORE the active account changed
        /// describes the *previous* account, and adopting it puts the old
        /// account's numbers on the new active slot — while the old account's
        /// own slot fetches the same numbers with its own token. Both rows
        /// then show identical usage until the file ages out.
        #[test]
        fn rejects_a_snapshot_written_before_the_active_account_changed() {
            let dir = tempdir().unwrap();
            let now = Utc::now();
            // Daemon wrote this 60s ago, while account A was live.
            let written = (now - ChronoDuration::seconds(60)).timestamp();
            let p = write(&dir, &payload(written));

            // Still comfortably inside the freshness window on its own.
            assert!(
                read_shared_snapshot(&p, Duration::from_secs(300), None).is_some(),
                "precondition: the snapshot is fresh enough by age alone"
            );

            // The user swapped to account B 30s ago — after the file was
            // written. The snapshot cannot describe B.
            let swapped_at = now - ChronoDuration::seconds(30);
            assert!(
                read_shared_snapshot(&p, Duration::from_secs(300), Some(swapped_at)).is_none(),
                "a snapshot predating the swap describes the previous account"
            );
        }

        #[test]
        fn accepts_a_snapshot_written_after_the_active_account_changed() {
            let dir = tempdir().unwrap();
            let now = Utc::now();
            let swapped_at = now - ChronoDuration::seconds(60);
            // Daemon re-polled after the swap, so this describes the new account.
            let written = (now - ChronoDuration::seconds(10)).timestamp();
            let p = write(&dir, &payload(written));
            assert!(
                read_shared_snapshot(&p, Duration::from_secs(300), Some(swapped_at)).is_some(),
                "a snapshot written after the swap is valid for the new account"
            );
        }

        /// No recorded change (fresh boot, never swapped) must not disable the
        /// fast path — that would silently spend every account's scarce /usage
        /// budget on duplicate calls.
        #[test]
        fn no_recorded_change_still_adopts() {
            let dir = tempdir().unwrap();
            let p = write(&dir, &payload(Utc::now().timestamp()));
            assert!(read_shared_snapshot(&p, Duration::from_secs(120), None).is_some());
        }
    }

    mod cli_usage_cache {
        use super::*;
        use tempfile::tempdir;

        const UUID: &str = "fa86dfa1-0000-0000-0000-000000000000";

        /// A global config shaped like the upstream CLI 2.1.311's: unrelated
        /// keys around a `cachedUsageUtilization` entry.
        fn write_config(dir: &tempfile::TempDir, cache: serde_json::Value) -> std::path::PathBuf {
            let p = dir.path().join(".claude.json");
            let config = serde_json::json!({
                "numStartups": 12,
                "oauthAccount": { "accountUuid": UUID, "emailAddress": "a@example.com" },
                "cachedUsageUtilization": cache,
            });
            std::fs::write(&p, config.to_string()).unwrap();
            p
        }

        fn cache(account_uuid: Option<&str>, fetched_at: DateTime<Utc>) -> serde_json::Value {
            let mut cache = serde_json::json!({
                "fetchedAtMs": fetched_at.timestamp_millis(),
                "utilization": {
                    "five_hour": { "utilization": 42.5, "resets_at": "2026-10-27T18:00:00Z" },
                    "seven_day": { "utilization": 63.1, "resets_at": "2026-10-30T09:00:00Z" },
                    "seven_day_opus": null,
                    "cinder_cove": null
                }
            });
            if let Some(uuid) = account_uuid {
                cache["accountUuid"] = serde_json::json!(uuid);
            }
            cache
        }

        #[test]
        fn adopts_fresh_entry_for_its_account() {
            let dir = tempdir().unwrap();
            let fetched_at = Utc::now() - ChronoDuration::seconds(30);
            let p = write_config(&dir, cache(Some(UUID), fetched_at));
            let snap = read_cli_usage_cache(&p, UUID, d(180)).expect("fresh entry adopted");
            assert_eq!(snap.five_hour.unwrap().utilization, 42.5);
            assert_eq!(snap.seven_day.unwrap().utilization, 63.1);
            assert_eq!(snap.fetched_at.timestamp_millis(), fetched_at.timestamp_millis());
        }

        #[test]
        fn rejects_another_accounts_entry() {
            let dir = tempdir().unwrap();
            let p = write_config(&dir, cache(Some(UUID), Utc::now()));
            assert!(read_cli_usage_cache(&p, "some-other-uuid", d(180)).is_none());
        }

        #[test]
        fn rejects_entry_without_account_identity() {
            let dir = tempdir().unwrap();
            let p = write_config(&dir, cache(None, Utc::now()));
            assert!(read_cli_usage_cache(&p, UUID, d(180)).is_none());
        }

        #[test]
        fn empty_account_uuid_never_matches() {
            let dir = tempdir().unwrap();
            let p = write_config(&dir, cache(Some(""), Utc::now()));
            assert!(read_cli_usage_cache(&p, "", d(180)).is_none());
        }

        #[test]
        fn rejects_stale_and_future_entries() {
            let dir = tempdir().unwrap();
            let p = write_config(&dir, cache(Some(UUID), Utc::now() - ChronoDuration::minutes(10)));
            assert!(read_cli_usage_cache(&p, UUID, d(180)).is_none());
            let p = write_config(&dir, cache(Some(UUID), Utc::now() + ChronoDuration::minutes(5)));
            assert!(read_cli_usage_cache(&p, UUID, d(180)).is_none());
        }

        #[test]
        fn missing_entry_or_file_returns_none() {
            let dir = tempdir().unwrap();
            let p = dir.path().join(".claude.json");
            std::fs::write(&p, r#"{"numStartups": 1}"#).unwrap();
            assert!(read_cli_usage_cache(&p, UUID, d(180)).is_none());
            assert!(read_cli_usage_cache(&dir.path().join("absent.json"), UUID, d(180)).is_none());
        }
    }
}
