use crate::app_state::{AppState, CachedUsage, Settings};
use crate::auth::accounts::{AddSource, ManagedAccount};
use crate::process_detection::{self, RunningClaudeCode};
use crate::store::StoredSessionEvent;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tauri::{command, State};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RefreshScope {
    /// Re-fetch only the currently active slot. Inactive slots stay on
    /// their staggered schedule. Triggered by the popover home view's
    /// refresh icon.
    Active,
    /// Re-fetch every managed slot, staggered by 30 s starting from now.
    /// Triggered by the AccountsPanel header refresh button.
    All,
}

#[derive(Debug, Serialize, Deserialize, specta::Type)]
pub struct DailyBucket {
    pub date: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Serialize, Deserialize, specta::Type)]
pub struct ModelStats {
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Serialize, Deserialize, specta::Type)]
pub struct ProjectStats {
    pub project: String,
    pub session_count: u64,
    pub total_cost_usd: f64,
}

#[derive(Debug, Serialize, Deserialize, specta::Type)]
pub struct CacheStats {
    pub total_cache_read_tokens: u64,
    pub total_cache_creation_tokens: u64,
    pub estimated_savings_usd: f64,
    pub hit_ratio: f64,
}

fn err_to_string<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[command]
#[specta::specta]
pub async fn get_current_usage(state: State<'_, Arc<AppState>>) -> Result<Option<CachedUsage>, String> {
    Ok(state.snapshot())
}

#[command]
#[specta::specta]
pub async fn get_pricing(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<crate::jsonl_parser::pricing::PricingEntry>, String> {
    Ok(state.pricing.entries().to_vec())
}

#[command]
#[specta::specta]
pub async fn get_session_history(
    days: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<StoredSessionEvent>, String> {
    let to = Utc::now();
    let from = to - Duration::days(days as i64);
    state.db.events_between(from, to).map_err(err_to_string)
}

#[command]
#[specta::specta]
pub async fn get_daily_trends(
    days: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<DailyBucket>, String> {
    let events = get_session_history(days, state).await?;
    use std::collections::BTreeMap;
    let mut by_day: BTreeMap<String, DailyBucket> = BTreeMap::new();
    for e in events {
        let date = e
            .ts
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d")
            .to_string();
        let slot = by_day
            .entry(date.clone())
            .or_insert_with(|| DailyBucket {
                date,
                input_tokens: 0,
                output_tokens: 0,
                cost_usd: 0.0,
            });
        slot.input_tokens += e.input_tokens;
        slot.output_tokens += e.output_tokens;
        slot.cost_usd += e.cost_usd;
    }
    Ok(by_day.into_values().collect())
}

#[command]
#[specta::specta]
pub async fn get_model_breakdown(
    days: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<ModelStats>, String> {
    let events = get_session_history(days, state).await?;
    use std::collections::HashMap;
    let mut by_model: HashMap<String, ModelStats> = HashMap::new();
    for e in events {
        let entry = by_model
            .entry(e.model.clone())
            .or_insert_with(|| ModelStats {
                model: e.model.clone(),
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                cost_usd: 0.0,
            });
        entry.input_tokens += e.input_tokens;
        entry.output_tokens += e.output_tokens;
        entry.cache_read_tokens += e.cache_read_tokens;
        entry.cache_creation_tokens += e.cache_creation_5m_tokens + e.cache_creation_1h_tokens;
        entry.cost_usd += e.cost_usd;
    }
    Ok(by_model.into_values().collect())
}

#[command]
#[specta::specta]
pub async fn get_project_breakdown(
    days: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<ProjectStats>, String> {
    let events = get_session_history(days, state).await?;
    use std::collections::HashMap;
    let mut by_project: HashMap<String, ProjectStats> = HashMap::new();
    for e in events {
        let entry = by_project
            .entry(e.project.clone())
            .or_insert_with(|| ProjectStats {
                project: e.project.clone(),
                session_count: 0,
                total_cost_usd: 0.0,
            });
        entry.session_count += 1;
        entry.total_cost_usd += e.cost_usd;
    }
    Ok(by_project.into_values().collect())
}

#[command]
#[specta::specta]
pub async fn get_cache_stats(
    days: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<CacheStats, String> {
    let events = get_session_history(days, state).await?;
    let mut read = 0u64;
    let mut created = 0u64;
    for e in &events {
        read += e.cache_read_tokens;
        created += e.cache_creation_5m_tokens + e.cache_creation_1h_tokens;
    }
    let total = read + created;
    let hit_ratio = if total > 0 {
        (read as f64) / (total as f64)
    } else {
        0.0
    };
    let savings = (read as f64 / 1_000_000.0) * 2.7;
    Ok(CacheStats {
        total_cache_read_tokens: read,
        total_cache_creation_tokens: created,
        estimated_savings_usd: savings,
        hit_ratio,
    })
}

#[command]
#[specta::specta]
pub async fn start_oauth_flow(
    long_lived: bool,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<String, String> {
    tracing::info!(
        target: "switchboard.auth",
        "OAuth flow starting (long_lived={long_lived})"
    );
    use crate::auth::oauth_paste_back::{
        build_authorize_url, generate_pkce, start_local_callback_server,
        LONG_LIVED_EXPIRES_IN_SECS,
    };

    let pkce = generate_pkce();
    let (port, rx) = start_local_callback_server().await.map_err(err_to_string)?;
    let redirect_uri = format!("http://localhost:{port}/callback");
    let url = build_authorize_url(&pkce, &redirect_uri, long_lived).map_err(err_to_string)?;
    let expires_in = if long_lived { Some(LONG_LIVED_EXPIRES_IN_SECS) } else { None };

    let state_clone = Arc::clone(state.inner());

    tokio::spawn(async move {
        use tauri::Emitter;

        let result: Result<u32, String> = async {
            let (code, callback_state) = rx
                .await
                .map_err(|_| "OAuth server closed before callback arrived".to_string())?
                .map_err(err_to_string)?;

            if callback_state != pkce.state {
                return Err("State mismatch — possible replay attack".to_string());
            }

            let token = state_clone
                .auth
                .exchange
                .exchange_code(&code, &pkce.verifier, &redirect_uri, &pkce.state, expires_in)
                .await
                .map_err(err_to_string)?;

            let userinfo = state_clone
                .auth
                .identity
                .fetch(&token.access_token)
                .await
                .map_err(err_to_string)?;

            let slot = state_clone
                .accounts
                .add_from_oauth(token, userinfo)
                .await
                .map_err(err_to_string)?;

            // Mirror the new account into the SQLite `accounts` table so
            // warm-up commands (which key off SQLite, not accounts.json)
            // can find a row. Failure here is non-fatal: the add succeeded;
            // warm-up will just default to disabled until the next mirror
            // (set_warmup_enabled also INSERT-OR-IGNOREs as a backstop).
            if let Err(e) = mirror_account_to_sqlite(&state_clone, slot) {
                tracing::warn!("oauth_complete: SQLite mirror failed: {e:#}");
            }

            state_clone.force_refresh.notify_one();
            Ok(slot)
        }
        .await;

        match result {
            Ok(slot) => {
                tracing::info!(
                    target: "switchboard.auth",
                    "OAuth flow complete (slot={slot})"
                );
                let _ = app.emit("oauth_complete", slot);
            }
            Err(e) => {
                tracing::warn!(
                    target: "switchboard.auth",
                    "OAuth flow failed: {e}"
                );
                let _ = app.emit("oauth_error", e);
            }
        }
    });

    Ok(url)
}

#[command]
#[specta::specta]
pub async fn force_refresh(
    scope: RefreshScope,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    use crate::app_state::ScheduleState;

    tracing::info!(target: "switchboard.poll", "force_refresh scope={scope:?}");
    let now = Instant::now();
    match scope {
        RefreshScope::Active => {
            if let Some(active) = *state.active_slot.read() {
                state.schedule_by_slot.write().insert(
                    active,
                    ScheduleState { next_poll_at: now },
                );
            }
        }
        RefreshScope::All => {
            let accounts = state.accounts.list().map_err(err_to_string)?;
            let active = *state.active_slot.read();
            let (interval, base_gap) = {
                let s = state.settings.read();
                (
                    std::time::Duration::from_secs(s.polling_interval_secs.max(60)),
                    std::time::Duration::from_secs(s.stagger_gap_secs.clamp(5, 120)),
                )
            };
            let slot_ids: Vec<u32> = accounts.iter().map(|a| a.slot).collect();
            *state.schedule_by_slot.write() = crate::poll_loop::seed_schedules(
                &slot_ids, active, now, interval, base_gap,
            );
        }
    }
    state.force_refresh.notify_one();
    Ok(())
}

#[command]
#[specta::specta]
pub async fn has_claude_code_creds() -> Result<bool, String> {
    Ok(crate::auth::claude_code_creds::has_creds().await)
}

#[command]
#[specta::specta]
pub async fn update_settings(s: Settings, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    if s.polling_interval_secs < 60 {
        return Err("polling_interval_secs must be at least 60".to_string());
    }
    // 5s lower bound prevents thrashing the upstream usage endpoint when many
    // slots are present; 120s upper bound keeps the round-trip across all
    // slots inside the polling interval at reasonable account counts.
    if !(5..=120).contains(&s.stagger_gap_secs) {
        return Err("stagger_gap_secs must be between 5 and 120".to_string());
    }
    if s.thresholds.iter().any(|&t| t > 100) {
        return Err("threshold values must be between 0 and 100".to_string());
    }
    state.db.save_settings(&s).map_err(|e| e.to_string())?;
    *state.settings.write() = s;
    Ok(())
}

#[command]
#[specta::specta]
pub async fn get_settings(state: State<'_, Arc<AppState>>) -> Result<Settings, String> {
    Ok(state.settings.read().clone())
}

#[cfg(debug_assertions)]
#[command]
#[specta::specta]
pub async fn debug_force_threshold(
    bucket: String,
    pct: u8,
    _state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    tracing::info!("debug_force_threshold({bucket}, {pct})");
    Ok(())
}

#[command]
#[specta::specta]
pub async fn resize_window(mode: String, app: tauri::AppHandle) -> Result<(), String> {
    use tauri::{LogicalPosition, LogicalSize, Manager, Position, Size};

    let Some(w) = app.get_webview_window("popover") else {
        return Ok(());
    };

    let target_size = match mode.as_str() {
        "compact" => (360.0_f64, 380.0_f64),
        "expanded" => (960.0_f64, 640.0_f64),
        _ => return Ok(()),
    };

    // Apply flag changes upfront so the rest of the animation runs in the
    // target mode's resize profile (resizable + always-on-top affect how
    // window-managers respond to subsequent set_size calls on some
    // platforms).
    match mode.as_str() {
        "compact" => {
            let _ = w.set_always_on_top(true);
            let _ = w.set_resizable(false);
        }
        "expanded" => {
            let _ = w.set_resizable(true);
            let _ = w.set_always_on_top(false);
        }
        _ => {}
    }

    // Capture the starting frame in logical coordinates so the math is
    // resolution-independent across retina/non-retina displays.
    let scale = w.scale_factor().map_err(|e| e.to_string())?;
    let cur_size = w.outer_size().map_err(|e| e.to_string())?;
    let cur_pos = w.outer_position().map_err(|e| e.to_string())?;
    let from_w = cur_size.width as f64 / scale;
    let from_h = cur_size.height as f64 / scale;
    let from_x = cur_pos.x as f64 / scale;
    let from_y = cur_pos.y as f64 / scale;

    // Where to end up. Compact stays anchored at the current center (the
    // post-animation TrayCenter snap below handles the reposition cleanly).
    // Expanded glides to the monitor's center so the bigger window doesn't
    // shoot off-screen when called from the tray-anchored compact view.
    let (to_x, to_y) = if mode == "expanded" {
        match w.current_monitor().map_err(|e| e.to_string())? {
            Some(m) => {
                let m_size = m.size();
                let m_pos = m.position();
                let mw = m_size.width as f64 / scale;
                let mh = m_size.height as f64 / scale;
                let mx = m_pos.x as f64 / scale;
                let my = m_pos.y as f64 / scale;
                (mx + (mw - target_size.0) / 2.0, my + (mh - target_size.1) / 2.0)
            }
            None => {
                // Fallback: keep the center fixed.
                let cx = from_x + from_w / 2.0;
                let cy = from_y + from_h / 2.0;
                (cx - target_size.0 / 2.0, cy - target_size.1 / 2.0)
            }
        }
    } else {
        let cx = from_x + from_w / 2.0;
        let cy = from_y + from_h / 2.0;
        (cx - target_size.0 / 2.0, cy - target_size.1 / 2.0)
    };

    // ~280ms total over 24 frames ≈ 12ms/frame. Cubic ease-out so the
    // motion feels native (fast start, gentle settle), matching macOS
    // Control Center / window-resize timing.
    const STEPS: u32 = 24;
    const STEP_MS: u64 = 12;

    for i in 1..=STEPS {
        let t = i as f64 / STEPS as f64;
        let eased = 1.0 - (1.0 - t).powi(3);
        let nw = from_w + (target_size.0 - from_w) * eased;
        let nh = from_h + (target_size.1 - from_h) * eased;
        let nx = from_x + (to_x - from_x) * eased;
        let ny = from_y + (to_y - from_y) * eased;

        let _ = w.set_size(Size::Logical(LogicalSize::new(nw, nh)));
        let _ = w.set_position(Position::Logical(LogicalPosition::new(nx, ny)));
        tokio::time::sleep(std::time::Duration::from_millis(STEP_MS)).await;
    }

    // Compact mode re-anchors to the tray after the animation so the
    // popover lives where the user's eye expects it. Expanded was already
    // animated to monitor center, no follow-up needed.
    if mode == "compact" {
        use tauri_plugin_positioner::{Position as TrayPos, WindowExt};
        let _ = w.move_window(TrayPos::TrayCenter);
    }

    Ok(())
}

#[command]
#[specta::specta]
pub async fn check_for_updates_now(app: tauri::AppHandle) -> Result<(), String> {
    crate::updater::check_and_emit(&app).await;
    Ok(())
}

#[command]
#[specta::specta]
pub async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    crate::updater::install_now(&app).await
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct AccountListEntry {
    pub slot: u32,
    pub email: String,
    /// The stable UUID that identifies this account in the SQLite `accounts`
    /// table. Pass this as `accountId` to all warmup-related Tauri commands.
    pub account_uuid: String,
    pub org_name: Option<String>,
    pub org_uuid: Option<String>,
    pub subscription_type: Option<String>,
    pub source: AddSource,
    pub is_active: bool,
    pub cached_usage: Option<CachedUsage>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SwapReport {
    pub new_active_slot: u32,
    pub running: RunningClaudeCode,
}

pub(crate) fn entry_for(
    state: &AppState,
    acc: &ManagedAccount,
    active: Option<u32>,
) -> AccountListEntry {
    let cache = state.cached_usage_by_slot.read();
    let cached = cache.get(&acc.slot).cloned();
    let last_error = cached.as_ref().and_then(|c| c.last_error.clone());

    // Prefer the live subscriptionType from the blob (which capture_live_into_slot
    // keeps current on every swap) over the snapshot stored at import time —
    // the latter goes stale when the user upgrades their plan (e.g. pro → max).
    let live_sub = acc
        .claude_code_oauth_blob
        .get("subscriptionType")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    AccountListEntry {
        slot: acc.slot,
        email: acc.email.clone(),
        account_uuid: acc.account_uuid.clone(),
        org_name: acc.organization_name.clone(),
        org_uuid: acc.organization_uuid.clone(),
        subscription_type: live_sub.or_else(|| acc.subscription_type.clone()),
        source: acc.source,
        is_active: Some(acc.slot) == active,
        cached_usage: cached,
        last_error,
    }
}

#[command]
#[specta::specta]
pub async fn list_accounts(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<AccountListEntry>, String> {
    let accounts = state.accounts.list().map_err(err_to_string)?;
    let active = *state.active_slot.read();
    Ok(accounts
        .iter()
        .map(|a| entry_for(&state, a, active))
        .collect())
}

#[command]
#[specta::specta]
pub async fn add_account_from_claude_code(
    state: State<'_, Arc<AppState>>,
) -> Result<u32, String> {
    let slot = state
        .accounts
        .add_from_claude_code()
        .await
        .map_err(err_to_string)?;
    tracing::info!(
        target: "switchboard.accounts",
        "added account from upstream-CLI (slot={slot})"
    );
    if let Err(e) = mirror_account_to_sqlite(&state, slot) {
        tracing::warn!("add_from_claude_code: SQLite mirror failed: {e:#}");
    }
    state.force_refresh.notify_one();
    Ok(slot)
}

#[command]
#[specta::specta]
pub async fn remove_account(
    slot: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    // Look up the account_uuid before removal so we can also drop the
    // SQLite mirror row. accounts.json holds the canonical mapping
    // (slot → account_uuid); after .remove() it's gone.
    let account_uuid = state
        .accounts
        .get(slot)
        .map_err(err_to_string)?
        .map(|a| a.account_uuid);

    state.accounts.remove(slot).map_err(err_to_string)?;
    state.cached_usage_by_slot.write().remove(&slot);
    state.backoff_by_slot.write().remove(&slot);
    state.schedule_by_slot.write().remove(&slot);

    if let Some(uuid) = account_uuid {
        if let Err(e) = state.db.delete_account(&uuid) {
            tracing::warn!("remove_account: SQLite delete failed: {e:#}");
        }
        tracing::info!(
            target: "switchboard.accounts",
            "removed account slot={slot} account={uuid}"
        );
    } else {
        tracing::info!(
            target: "switchboard.accounts",
            "removed account slot={slot}"
        );
    }
    Ok(())
}

#[command]
#[specta::specta]
pub async fn swap_to_account(
    slot: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<SwapReport, String> {
    tracing::info!(target: "switchboard.swap", "swap_to_account(slot={slot}) starting");
    // Refresh the target slot's token if it's expired or about to expire
    // before handing its blob to swap_to. swap_to writes whatever is
    // stored in accounts.json as the live CC credentials; if the stored
    // accessToken is already past expiry, the next poll-loop tick reads
    // those just-written creds, fetches usage, and gets 401 — surfacing
    // "token expired — re-authenticate" the instant the user switches.
    //
    // refresh_inactive is the same routine that keeps inactive slots
    // current during polling, so reusing it here keeps the refresh story
    // in one place. We swallow refresh errors: if the stored AT happens
    // to still be live, the swap still succeeds; if it's also dead, the
    // post-swap poll will surface auth_required the same way it does
    // today, and the user can re-authenticate.
    if let Ok(Some(target)) = state.accounts.get(slot) {
        let near_expiry = target.token_expires_at
            <= chrono::Utc::now() + chrono::Duration::minutes(2);
        if near_expiry {
            tracing::info!(
                target: "switchboard.swap",
                "slot {slot} stored AT near expiry; pre-refreshing before swap"
            );
            if let Err(e) = state
                .accounts
                .refresh_inactive(slot, &state.auth.exchange)
                .await
            {
                tracing::warn!("pre-swap refresh of slot {slot} failed: {e:#}");
            }
        }
    }

    state
        .accounts
        .swap_to(slot)
        .await
        .map_err(|e| e.to_string())?;
    tracing::info!(target: "switchboard.swap", "swap_to_account(slot={slot}) complete");

    // swap_to commits both CC creds and the global oauthAccount blob for
    // `slot`; reconcile active_slot eagerly so the next list_accounts call
    // (the UI hits this immediately after we return) sees correct is_active
    // flags without waiting on the poll-loop tick.
    *state.active_slot.write() = Some(slot);

    // Drop per-slot backoff state. The previous backoff was earned by a
    // different token (the prior active slot's live CC blob, or a stale
    // OAuth refresh token) — a swap rotates which token authenticates each
    // slot's usage fetch, so prior 429s no longer apply. Without this, an
    // unlucky run of throttling can leave every slot waiting out a 30-min
    // window with no successful fetch, which strands the popover on the
    // empty LoadingShell because state.snapshot() has nothing to return.
    state.backoff_by_slot.write().clear();

    // Re-seed per-slot schedules so the new active slot polls first
    // (next_poll_at = now), with previously-active and other inactive
    // slots staggered behind it. Without this, the new active would
    // wait out whatever deadline was set when it was inactive.
    {
        let accounts = state.accounts.list().map_err(err_to_string)?;
        let (interval, base_gap) = {
            let s = state.settings.read();
            (
                std::time::Duration::from_secs(s.polling_interval_secs.max(60)),
                std::time::Duration::from_secs(s.stagger_gap_secs.clamp(5, 120)),
            )
        };
        let slot_ids: Vec<u32> = accounts.iter().map(|a| a.slot).collect();
        *state.schedule_by_slot.write() = crate::poll_loop::seed_schedules(
            &slot_ids,
            Some(slot),
            std::time::Instant::now(),
            interval,
            base_gap,
        );
    }

    if let Ok(Some(target)) = state.accounts.get(slot) {
        let prev = state.keychain_guardian.lock().replace(
            crate::auth::keychain_guardian::KeychainGuardian::arm_with_claude_code_creds(
                target.claude_code_oauth_blob.clone(),
                target.oauth_account_blob.clone(),
                target.account_uuid.clone(),
            ),
        );
        if let Some(p) = prev {
            p.cancel();
        }
    }

    let running = process_detection::detect();
    state.force_refresh.notify_one();
    Ok(SwapReport {
        new_active_slot: slot,
        running,
    })
}

#[command]
#[specta::specta]
pub async fn detect_running_claude_code() -> Result<RunningClaudeCode, String> {
    Ok(process_detection::detect())
}

#[command]
#[specta::specta]
pub async fn refresh_account(
    slot: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let active = *state.active_slot.read();
    if Some(slot) == active {
        state.force_refresh.notify_one();
        return Ok(());
    }
    state
        .accounts
        .refresh_inactive(slot, &state.auth.exchange)
        .await
        .map_err(err_to_string)?;
    state.force_refresh.notify_one();
    Ok(())
}

// ---------------------------------------------------------------------------
// Warmup pillar commands (Plan B T16)
// ---------------------------------------------------------------------------

/// Trigger a manual warm-up for a specific account (UI "Warm up now" button).
#[command]
#[specta::specta]
pub async fn warmup_account_now(
    state: State<'_, Arc<AppState>>,
    account_id: String,
) -> Result<crate::warmup::errors::WarmupOutcome, String> {
    ensure_sqlite_account_row(&state, &account_id).map_err(|e| e.to_string())?;
    crate::scheduler_glue::manual_warmup(state.inner(), &account_id)
        .await
        .map_err(|e| format!("{e:#}"))
}

/// Set the per-account schedule preset.
#[command]
#[specta::specta]
pub async fn set_account_schedule(
    state: State<'_, Arc<AppState>>,
    account_id: String,
    schedule: crate::scheduler::Schedule,
) -> Result<(), String> {
    ensure_sqlite_account_row(&state, &account_id).map_err(|e| e.to_string())?;
    let conn = state.db.conn();
    let json = serde_json::to_string(&schedule).map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE accounts SET schedule = ?1 WHERE id = ?2",
        rusqlite::params![json, account_id],
    )
    .map_err(|e| e.to_string())?;
    tracing::info!(
        target: "switchboard.warmup",
        "set_account_schedule({account_id}, {schedule:?})"
    );
    Ok(())
}

/// Toggle warm-up on/off for a specific account.
#[command]
#[specta::specta]
pub async fn set_warmup_enabled(
    state: State<'_, Arc<AppState>>,
    account_id: String,
    enabled: bool,
) -> Result<(), String> {
    ensure_sqlite_account_row(&state, &account_id).map_err(|e| e.to_string())?;
    let conn = state.db.conn();
    conn.execute(
        "UPDATE accounts SET warmup_enabled = ?1 WHERE id = ?2",
        rusqlite::params![enabled as i64, account_id],
    )
    .map_err(|e| e.to_string())?;
    tracing::info!(
        target: "switchboard.warmup",
        "set_warmup_enabled({account_id}, {enabled})"
    );
    Ok(())
}

/// Grant the global warm-up consent (called by WarmupConsentModal on Accept).
#[command]
#[specta::specta]
pub async fn grant_warmup_consent(
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let conn = state.db.conn();
    conn.execute(
        "UPDATE settings SET value = '1' WHERE key = 'warmup_consent_granted'",
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Revoke global consent (also disables warm-up on every account).
#[command]
#[specta::specta]
pub async fn revoke_warmup_consent(
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let conn = state.db.conn();
    conn.execute_batch(
        "UPDATE settings SET value = '0' WHERE key = 'warmup_consent_granted'; \
         UPDATE accounts SET warmup_enabled = 0;",
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Read the consent flag.
#[command]
#[specta::specta]
pub async fn get_warmup_consent_granted(
    state: State<'_, Arc<AppState>>,
) -> Result<bool, String> {
    let conn = state.db.conn();
    let v: String = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'warmup_consent_granted'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(v == "1")
}

/// Register OS-level scheduler (writes plist / schtasks task).
#[command]
#[specta::specta]
pub async fn os_scheduler_register() -> Result<(), String> {
    let bin = std::env::current_exe().map_err(|e| e.to_string())?;
    let s = crate::os_scheduler::for_current_platform()
        .ok_or_else(|| "OS-level scheduling not supported on this platform".to_string())?;
    s.register(&bin).map_err(|e| format!("{e:#}"))
}

/// Unregister OS-level scheduler.
#[command]
#[specta::specta]
pub async fn os_scheduler_unregister() -> Result<(), String> {
    let s = crate::os_scheduler::for_current_platform()
        .ok_or_else(|| "OS-level scheduling not supported on this platform".to_string())?;
    s.unregister().map_err(|e| format!("{e:#}"))
}

/// Check if OS-level scheduler is currently registered.
#[command]
#[specta::specta]
pub async fn os_scheduler_is_registered() -> Result<bool, String> {
    let s = crate::os_scheduler::for_current_platform()
        .ok_or_else(|| "OS-level scheduling not supported on this platform".to_string())?;
    s.is_registered().map_err(|e| format!("{e:#}"))
}

/// Per-account warm-up state returned by `get_warmup_state`.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct WarmupAccountState {
    pub warmup_enabled: bool,
    pub schedule: crate::scheduler::Schedule,
    pub last_warmup_at: Option<i64>,
}

/// Fetch the warm-up state for a specific account. Used by the UI row to
/// initialise the WarmupToggle / ScheduleSelector on mount.
#[command]
#[specta::specta]
pub async fn get_warmup_state(
    state: State<'_, Arc<AppState>>,
    account_id: String,
) -> Result<WarmupAccountState, String> {
    // Make sure a row exists so a brand-new account doesn't bubble a
    // "no rows returned" error up to the UI on mount.
    ensure_sqlite_account_row(&state, &account_id).map_err(|e| e.to_string())?;

    let conn = state.db.conn();
    let (enabled, schedule_json, last_warmup_at): (i64, String, Option<i64>) = conn
        .query_row(
            "SELECT warmup_enabled, schedule, last_warmup_at FROM accounts WHERE id = ?1",
            rusqlite::params![account_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|e| e.to_string())?;
    let schedule: crate::scheduler::Schedule =
        serde_json::from_str(&schedule_json).map_err(|e| e.to_string())?;
    Ok(WarmupAccountState {
        warmup_enabled: enabled != 0,
        schedule,
        last_warmup_at,
    })
}

// ---------------------------------------------------------------------------
// SQLite mirror helpers
//
// `accounts.json` (managed by AccountManager) is the canonical source of
// "which accounts exist." The SQLite `accounts` table is a sidecar that holds
// warm-up state (warmup_enabled, schedule, last_warmup_at) — split from the
// JSON store because the transactional claim in scheduler::claim needs SQL,
// and we deliberately keep credential blobs out of the DB.
//
// These helpers keep the two stores in sync.
// ---------------------------------------------------------------------------

/// Insert (or no-op-update) the SQLite mirror row for a slot. Called after
/// every successful AccountManager add so warm-up queries can find a row.
pub(crate) fn mirror_account_to_sqlite(
    state: &Arc<AppState>,
    slot: u32,
) -> anyhow::Result<()> {
    let acc = state
        .accounts
        .get(slot)?
        .ok_or_else(|| anyhow::anyhow!("slot {slot} not in accounts.json"))?;
    state.db.upsert_account(&crate::store::StoredAccount {
        id: acc.account_uuid,
        email: acc.email,
        display_name: None,
    })?;
    Ok(())
}

/// Ensure a SQLite row exists for `account_uuid`. Used as a defensive guard
/// at the top of every warm-up command so a missing mirror row (e.g. account
/// added before this fix shipped, or mirror failed asynchronously) doesn't
/// cause `set_warmup_enabled` to silently match 0 rows or `load_schedule` to
/// return `QueryReturnedNoRows`.
fn ensure_sqlite_account_row(
    state: &State<'_, Arc<AppState>>,
    account_uuid: &str,
) -> anyhow::Result<()> {
    let accounts = state.accounts.list()?;
    let acc = accounts
        .into_iter()
        .find(|a| a.account_uuid == account_uuid)
        .ok_or_else(|| {
            anyhow::anyhow!("account {account_uuid} not found in accounts.json")
        })?;
    state.db.upsert_account(&crate::store::StoredAccount {
        id: acc.account_uuid,
        email: acc.email,
        display_name: None,
    })?;
    Ok(())
}

/// Reconcile every account in accounts.json into the SQLite mirror at
/// startup. Idempotent — existing rows keep their warm-up state because
/// upsert_account's ON CONFLICT clause only touches email/display_name/
/// last_seen_at, not the warm-up columns.
pub fn reconcile_sqlite_account_mirror(state: &Arc<AppState>) -> anyhow::Result<()> {
    let accounts = state.accounts.list()?;
    for acc in accounts {
        if let Err(e) = state.db.upsert_account(&crate::store::StoredAccount {
            id: acc.account_uuid.clone(),
            email: acc.email.clone(),
            display_name: None,
        }) {
            tracing::warn!(
                "reconcile_sqlite_account_mirror: failed for {}: {e:#}",
                acc.account_uuid
            );
        }
    }
    Ok(())
}
