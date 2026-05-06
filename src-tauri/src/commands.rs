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

            state_clone.force_refresh.notify_one();
            Ok(slot)
        }
        .await;

        match result {
            Ok(slot) => {
                let _ = app.emit("oauth_complete", slot);
            }
            Err(e) => {
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
            let interval = std::time::Duration::from_secs(
                state.settings.read().polling_interval_secs.max(60),
            );
            let slot_ids: Vec<u32> = accounts.iter().map(|a| a.slot).collect();
            *state.schedule_by_slot.write() =
                crate::poll_loop::seed_schedules(&slot_ids, active, now, interval);
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

fn entry_for(state: &AppState, acc: &ManagedAccount, active: Option<u32>) -> AccountListEntry {
    let cache = state.cached_usage_by_slot.read();
    let cached = cache.get(&acc.slot).cloned();
    let last_error = cached.as_ref().and_then(|c| c.last_error.clone());
    AccountListEntry {
        slot: acc.slot,
        email: acc.email.clone(),
        org_name: acc.organization_name.clone(),
        org_uuid: acc.organization_uuid.clone(),
        subscription_type: acc.subscription_type.clone(),
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
    state.force_refresh.notify_one();
    Ok(slot)
}

#[command]
#[specta::specta]
pub async fn remove_account(
    slot: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state.accounts.remove(slot).map_err(err_to_string)?;
    state.cached_usage_by_slot.write().remove(&slot);
    state.backoff_by_slot.write().remove(&slot);
    Ok(())
}

#[command]
#[specta::specta]
pub async fn swap_to_account(
    slot: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<SwapReport, String> {
    state
        .accounts
        .swap_to(slot)
        .await
        .map_err(|e| e.to_string())?;

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

    if let Ok(Some(target)) = state.accounts.get(slot) {
        let prev = state.keychain_guardian.lock().replace(
            crate::auth::keychain_guardian::KeychainGuardian::arm_with_claude_code_creds(
                target.claude_code_oauth_blob.clone(),
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
