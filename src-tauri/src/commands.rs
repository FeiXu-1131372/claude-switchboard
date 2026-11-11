use crate::app_state::{AppState, CachedUsage, Settings};
use crate::auth::accounts::{AddSource, ManagedAccount};
use crate::jsonl_parser::hidden_calls::session_file_of;
use crate::process_detection::{self, RunningClaudeCode};
use crate::store::StoredSessionEvent;
use chrono::{DateTime, Duration, Utc};
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
pub struct DailyModelBucket {
    pub date: String,
    pub models: Vec<ModelStats>,
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

/// Where a report covering `days` days begins: local midnight at the start of
/// the first of them, with today as the last. Seven days is today and the six
/// before it.
///
/// Reports are bucketed by local calendar day, so a window measured back from
/// this instant would open partway through a day: the oldest bar would be a
/// fragment, and two reports of the same length would only agree if they
/// happened to be asked at the same moment.
fn report_window_start<Tz: chrono::TimeZone>(days: u32, now: &DateTime<Tz>) -> DateTime<Utc> {
    let first_day = now.date_naive() - Duration::days(i64::from(days.max(1)) - 1);
    let midnight = first_day.and_time(chrono::NaiveTime::MIN);
    let zone = now.timezone();
    zone.from_local_datetime(&midnight)
        .earliest()
        // A zone that moves its clocks forward at midnight has no 00:00 that
        // day; the day starts an hour later.
        .or_else(|| zone.from_local_datetime(&(midnight + Duration::hours(1))).earliest())
        .map(|start| start.with_timezone(&Utc))
        .unwrap_or_else(|| now.with_timezone(&Utc) - Duration::days(i64::from(days.max(1))))
}

/// The usage every report is summed from: whole local days, ending now.
fn report_events(
    days: u32,
    state: &State<'_, Arc<AppState>>,
) -> Result<Vec<StoredSessionEvent>, String> {
    let from = report_window_start(days, &chrono::Local::now());
    state.db.events_between(from, Utc::now()).map_err(err_to_string)
}

/// Compaction boundaries in the same window as `get_session_costs`, so the
/// Cost tab can mark which sessions had their context reset partway through.
#[command]
#[specta::specta]
pub async fn get_compactions(
    days: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<crate::store::StoredCompaction>, String> {
    let from = report_window_start(days, &chrono::Local::now());
    state.db.compactions_between(from, Utc::now()).map_err(err_to_string)
}

/// What each conversation cost on each of the last `days` days, newest first.
#[command]
#[specta::specta]
pub async fn get_session_costs(
    days: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<crate::session_costs::SessionCostRow>, String> {
    let events = report_events(days, &state)?;
    Ok(crate::session_costs::aggregate(&events, &state.pricing, &chrono::Local))
}

#[command]
#[specta::specta]
pub async fn get_daily_trends(
    days: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<DailyBucket>, String> {
    let events = report_events(days, &state)?;
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

fn is_synthetic_placeholder(e: &StoredSessionEvent) -> bool {
    e.model.starts_with('<')
        && e.input_tokens == 0
        && e.output_tokens == 0
        && e.cache_read_tokens == 0
        && e.cache_creation_5m_tokens == 0
        && e.cache_creation_1h_tokens == 0
}

#[command]
#[specta::specta]
pub async fn get_model_breakdown(
    days: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<ModelStats>, String> {
    let events = report_events(days, &state)?;
    use std::collections::HashMap;
    let mut by_model: HashMap<String, ModelStats> = HashMap::new();
    for e in events {
        if is_synthetic_placeholder(&e) {
            continue;
        }
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
    let mut models: Vec<ModelStats> = by_model.into_values().collect();
    models.sort_by(|a, b| {
        b.cost_usd
            .total_cmp(&a.cost_usd)
            .then_with(|| (b.input_tokens + b.output_tokens).cmp(&(a.input_tokens + a.output_tokens)))
            .then_with(|| a.model.cmp(&b.model))
    });
    Ok(models)
}

#[command]
#[specta::specta]
pub async fn get_daily_model_breakdown(
    days: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<DailyModelBucket>, String> {
    let events = report_events(days, &state)?;
    use std::collections::{BTreeMap, HashMap};
    let mut by_day: BTreeMap<String, HashMap<String, ModelStats>> = BTreeMap::new();
    for e in events {
        if is_synthetic_placeholder(&e) {
            continue;
        }
        let date = e
            .ts
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d")
            .to_string();
        let by_model = by_day.entry(date).or_default();
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
    Ok(by_day
        .into_iter()
        .map(|(date, models)| {
            let mut models: Vec<ModelStats> = models.into_values().collect();
            models.sort_by(|a, b| b.cost_usd.total_cmp(&a.cost_usd));
            DailyModelBucket { date, models }
        })
        .collect())
}

#[command]
#[specta::specta]
pub async fn get_project_breakdown(
    days: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<ProjectStats>, String> {
    let events = report_events(days, &state)?;
    Ok(project_breakdown(&events))
}

/// One row per project: what it cost and how many sessions it ran.
///
/// A session is one conversation, so its subagents' transcripts count toward
/// the session that started them and a session that made forty API calls is
/// still one session. Rows come back most expensive first, ties by name, so
/// the order is the same on every call.
fn project_breakdown(events: &[StoredSessionEvent]) -> Vec<ProjectStats> {
    use std::collections::{HashMap, HashSet};
    let mut by_project: HashMap<&str, (HashSet<String>, f64)> = HashMap::new();
    for e in events {
        let (sessions, cost) = by_project.entry(e.project.as_str()).or_default();
        sessions.insert(session_file_of(&e.source_file));
        *cost += e.cost_usd;
    }
    let mut rows: Vec<ProjectStats> = by_project
        .into_iter()
        .map(|(project, (sessions, total_cost_usd))| ProjectStats {
            project: project.to_string(),
            session_count: sessions.len() as u64,
            total_cost_usd,
        })
        .collect();
    rows.sort_by(|a, b| {
        b.total_cost_usd
            .total_cmp(&a.total_cost_usd)
            .then_with(|| a.project.cmp(&b.project))
    });
    rows
}

#[command]
#[specta::specta]
pub async fn get_cache_stats(
    days: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<CacheStats, String> {
    let pricing = state.pricing.clone();
    let events = report_events(days, &state)?;
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
    // Savings are per-model: cache-read tokens × (input − cache-read rate).
    // The previous flat $2.7/MTok was only right for Sonnet — it understated
    // Opus ($4.50) and Fable ($9.00) savings and priced unknown models' cache
    // reads as free when they simply can't be estimated.
    let savings: f64 = events
        .iter()
        .map(|e| {
            pricing.cache_savings_per_mtok(&e.model).unwrap_or(0.0)
                * (e.cache_read_tokens as f64)
                / 1_000_000.0
        })
        .sum();
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

            state_clone.backoff_by_slot.write().remove(&slot);
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
                state.lift_auth_backoff(active);
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
            state.lift_all_auth_backoffs();
            // Manual refresh is a rare user-initiated burst: use the minimum
            // stagger so every slot refreshes within seconds rather than
            // riding the steady-state 30s stagger, which left later slots
            // stale for minutes after clicking "refresh".
            *state.schedule_by_slot.write() = crate::poll_loop::seed_schedules(
                &slot_ids,
                active,
                now,
                interval,
                std::time::Duration::from_secs(5),
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
    if let Some(rotation) = &s.account_rotation {
        rotation.validate()?;
    }
    state.db.save_settings(&s).map_err(|e| e.to_string())?;
    *state.settings.write() = s;
    state.force_refresh.notify_one();
    Ok(())
}

#[command]
#[specta::specta]
pub async fn get_account_rotation_status(state: State<'_, Arc<AppState>>) -> Result<Option<String>, String> {
    Ok(state.rotation.lock().status.clone())
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

/// Target top-left for compact-mode resizes: the TOP edge stays glued
/// (menu-bar popovers hang downward from the tray icon; a center-fixed
/// resize visibly detaches the popover from the menu bar on every
/// collapse/expand). Width is recentered — a no-op when width is unchanged
/// (the common case; heights are the only thing that differs today).
fn compact_target_xy(from_x: f64, from_y: f64, from_w: f64, target_w: f64) -> (f64, f64) {
    (from_x + (from_w - target_w) / 2.0, from_y)
}

#[command]
#[specta::specta]
pub async fn resize_window(mode: String, app: tauri::AppHandle) -> Result<(), String> {    use tauri::{LogicalPosition, LogicalSize, Manager, Position, Size};

    // `Position::TrayCenter` panics (aborting the process under this crate's
    // `panic = "abort"` release profile) if `tauri_plugin_positioner` hasn't
    // recorded a tray position yet — which happens on a cold launch, since
    // the popover's webview mounts and calls this before the user has ever
    // interacted with the tray icon. Skip the tray-relative repositioning
    // until a real `TrayIconEvent` has arrived; the window is still sized
    // correctly, just not yet re-anchored, and the next call (e.g. the
    // user's first tray click, which itself supplies that position) fixes it.
    let tray_position_known = app
        .state::<Arc<AppState>>()
        .tray_position_known
        .load(std::sync::atomic::Ordering::Relaxed);

    let Some(w) = app.get_webview_window("popover") else {
        return Ok(());
    };

    let target_size = match mode.as_str() {
        "compact" => (360.0_f64, 380.0_f64),
        // Hero numbers + details disclosure + footer only — the popover's
        // default glance view. Details expand back to full compact height.
        "compact-minimal" => (360.0_f64, 208.0_f64),
        "expanded" => (960.0_f64, 640.0_f64),
        _ => return Ok(()),
    };

    // Apply flag changes upfront so the rest of the animation runs in the
    // target mode's resize profile (resizable + always-on-top affect how
    // window-managers respond to subsequent set_size calls on some
    // platforms).
    match mode.as_str() {
        "compact" | "compact-minimal" => {
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
    // Read on every platform, but only *used* by the animation loop below,
    // which is `cfg(not(windows))`. Windows resizes in one step, so there it is
    // genuinely unused — and renaming it to `_from_h` breaks every other
    // platform's build instead of silencing one warning.
    #[cfg_attr(target_os = "windows", allow(unused_variables))]
    let from_h = cur_size.height as f64 / scale;
    let from_x = cur_pos.x as f64 / scale;
    let from_y = cur_pos.y as f64 / scale;

    // Where to end up. Compact modes keep the TOP edge glued to its current
    // position — a menu-bar popover hangs downward from the tray icon, so a
    // height change must rise/extend from the bottom. (The previous
    // center-fixed math detached the popover from the menu bar on every
    // collapse/expand.) Expanded glides to the monitor's center so the
    // bigger window doesn't shoot off-screen from the tray-anchored view.
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
            None => compact_target_xy(from_x, from_y, from_w, target_size.0),
        }
    } else {
        compact_target_xy(from_x, from_y, from_w, target_size.0)
    };

    #[cfg(not(target_os = "windows"))]
    {
        // Target duration for the animation. Cubic ease-out so the motion
        // feels native (fast start, gentle settle), matching macOS
        // Control Center / window-resize timing. Time-based interpolation
        // ensures smooth animation across OS timer resolution limits.
        let start_time = std::time::Instant::now();
        let duration = std::time::Duration::from_millis(280);

        loop {
            let elapsed = start_time.elapsed();
            if elapsed >= duration {
                break;
            }
            let t = elapsed.as_secs_f64() / duration.as_secs_f64();
            let eased = 1.0 - (1.0 - t).powi(3);
            let nw = from_w + (target_size.0 - from_w) * eased;
            let nh = from_h + (target_size.1 - from_h) * eased;
            let nx = from_x + (to_x - from_x) * eased;
            let ny = from_y + (to_y - from_y) * eased;

            let _ = w.set_size(Size::Logical(LogicalSize::new(nw, nh)));
            let _ = w.set_position(Position::Logical(LogicalPosition::new(nx, ny)));
            tokio::time::sleep(std::time::Duration::from_millis(8)).await;
        }

        // Snap exactly to final target at the end.
        let _ = w.set_size(Size::Logical(LogicalSize::new(target_size.0, target_size.1)));
        let _ = w.set_position(Position::Logical(LogicalPosition::new(to_x, to_y)));

        // Compact modes re-anchor to the tray after the animation so the
        // popover lives where the user's eye expects it (this is also the
        // horizontal centering correction — the top edge is already glued by
        // compact_target_xy). Expanded was already animated to monitor
        // center, no follow-up needed.
        if tray_position_known && (mode == "compact" || mode == "compact-minimal") {
            crate::move_to_tray_center(&w);
        }
    }

    #[cfg(target_os = "windows")]
    {
        // On Windows, rapid resizing is extremely laggy. We instead animate the DOM.
        // We just need to ensure the window is large enough for the DOM animation.
        if mode == "expanded" {
            let _ = w.set_size(Size::Logical(LogicalSize::new(target_size.0, target_size.1)));
            let _ = w.set_position(Position::Logical(LogicalPosition::new(to_x, to_y)));
        } else {
            // Wait for the DOM animation (280ms) to finish before shrinking the OS window.
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                let _ = w.set_size(Size::Logical(LogicalSize::new(target_size.0, target_size.1)));
                let _ = w.set_position(Position::Logical(LogicalPosition::new(to_x, to_y)));
                if tray_position_known {
                    crate::move_to_tray_center(&w);
                }
            });
        }
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
    /// Display label, falling back to the verified email when available.
    pub email: String,
    /// Stable local account identifier (a UUID or token fingerprint).
    /// Pass this as `accountId` to warmup-related Tauri commands.
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

/// `last_error` of an account that can't ask `/usage` (an inference-only
/// token) and has nothing to show.
pub(crate) const USAGE_NOT_SUPPORTED: &str = "usage_not_supported";
/// Same account, but the status-line bridge is running and this is the login
/// Claude Code is using, so a reading is expected after some activity.
pub(crate) const AWAITING_STATUSLINE: &str = "awaiting_statusline";

/// Which of the two markers an empty token account carries. Only the active
/// login can be fed: the bridge matches an observation to a token account
/// only while that account is the one Claude Code is signed in with.
pub(crate) fn local_only_marker(bridge_feeds_usage: bool, is_active: bool) -> &'static str {
    if bridge_feeds_usage && is_active {
        AWAITING_STATUSLINE
    } else {
        USAGE_NOT_SUPPORTED
    }
}

/// The error an account row carries. A token account that holds a reading
/// (one the bridge adopted) is not "unsupported": it reports whatever its
/// cache says. `marker` is only evaluated when the account has nothing.
pub(crate) fn row_last_error(
    supports_usage: bool,
    cached: Option<&CachedUsage>,
    marker: impl FnOnce() -> &'static str,
) -> Option<String> {
    let cached_error = || cached.and_then(|c| c.last_error.clone());
    if supports_usage {
        return cached_error();
    }
    match cached {
        Some(c) if crate::poll_loop::has_usage_data(&c.snapshot) => cached_error(),
        _ => Some(marker().to_string()),
    }
}

pub(crate) fn entry_for(
    state: &AppState,
    acc: &ManagedAccount,
    active: Option<u32>,
) -> AccountListEntry {
    let cached = state.cached_usage_by_slot.read().get(&acc.slot).cloned();
    let is_active = Some(acc.slot) == active;
    let last_error = row_last_error(acc.supports_usage(), cached.as_ref(), || {
        local_only_marker(state.statusline_setup.feeds_usage(), is_active)
    });

    AccountListEntry {
        slot: acc.slot,
        email: acc.display_name(),
        account_uuid: acc.account_uuid.clone(),
        org_name: acc.organization_name.clone(),
        org_uuid: acc.organization_uuid.clone(),
        // The plan as it is now, not the label saved when the account was
        // added: that one goes stale on the first upgrade (pro → max).
        subscription_type: acc.plan(),
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
    state.backoff_by_slot.write().remove(&slot);
    state.force_refresh.notify_one();
    Ok(slot)
}

#[command]
#[specta::specta]
pub async fn import_long_lived_token(
    access_token: String,
    label: Option<String>,
    replace_slot: Option<u32>,
    state: State<'_, Arc<AppState>>,
) -> Result<u32, String> {
    let access_token = zeroize::Zeroizing::new(access_token);
    let _account_change = state.account_change_lock.lock().await;
    if replace_slot.is_some() && replace_slot == *state.active_slot.read() {
        return Err("Switch to another account before replacing the active token.".into());
    }
    let previous = replace_slot.and_then(|slot| state.accounts.get(slot).ok().flatten());
    let slot = state.accounts.import_long_lived_token(
        &access_token, label.as_deref(), replace_slot, &state.auth.identity,
    ).await.map_err(err_to_string)?;
    if let Err(e) = mirror_account_to_sqlite(&state, slot) {
        tracing::warn!("token import: SQLite mirror failed: {e:#}");
    }
    if let Some(previous) = previous {
        if let Ok(Some(updated)) = state.accounts.get(slot) {
            if previous.account_uuid != updated.account_uuid {
                let _ = state.db.delete_account(&previous.account_uuid);
            }
        }
    }
    state.cached_usage_by_slot.write().remove(&slot);
    state.backoff_by_slot.write().remove(&slot);
    state.force_refresh.notify_one();
    tracing::info!(target: "switchboard.accounts", "imported long-lived token (slot={slot})");
    Ok(slot)
}

#[command]
#[specta::specta]
pub async fn remove_account(
    slot: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let _account_change = state.account_change_lock.lock().await;
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
    let _account_change = state.account_change_lock.lock().await;
    perform_account_swap(slot, &state).await
}

/// Caller holds account_change_lock, shared by manual and automatic switches.
pub(crate) async fn perform_account_swap(slot: u32, state: &AppState) -> Result<SwapReport, String> {
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
        let near_expiry = target.can_refresh() && target.token_expires_at
            .is_some_and(|expiry| expiry <= chrono::Utc::now() + chrono::Duration::minutes(2));
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

    if let Some(guardian) = state.keychain_guardian.lock().take() {
        guardian.cancel();
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

    // The statusline daemon's shared snapshot carries no account identity, so
    // anything it wrote before this instant describes the account we just
    // swapped away from. Without this, the new active slot adopts the previous
    // account's numbers and both rows show identical usage for up to one
    // polling interval.
    *state.active_since.write() = Some(Utc::now());

    // Drop auth-failure backoff: the swap may have fixed the token that
    // earned it. Without this, an unlucky run of failures can leave every
    // slot waiting out a 30-min window with no successful fetch. 429 bans
    // stay — a swap writes the same account's tokens into the live slot, so
    // the ban still applies, and calling into it can restart it. Banned
    // slots keep showing hydrated or local-source data meanwhile.
    state.lift_all_auth_backoffs();

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
    state.lift_auth_backoff(slot);
    let active = *state.active_slot.read();
    if let Some(account) = state.accounts.get(slot).map_err(err_to_string)? {
        if !account.can_refresh() {
            state.force_refresh.notify_one();
            return Ok(());
        }
    }
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
    #[serde(default)]
    pub last_success_at: Option<i64>,
    #[serde(default)]
    pub last_result: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub next_scheduled_at: Option<i64>,
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
    let activity = crate::scheduler::claim::run_state(&conn, &account_id).map_err(err_to_string)?;
    let next_scheduled_at = (enabled != 0).then(|| crate::scheduler::catchup::next_expected_fire(&schedule, chrono::Local::now()))
        .flatten().map(|at| at.timestamp());
    Ok(WarmupAccountState {
        warmup_enabled: enabled != 0,
        schedule,
        last_warmup_at: activity.last_attempt_at.or(last_warmup_at),
        last_success_at: activity.last_success_at,
        last_result: activity.last_result,
        last_error: activity.last_error,
        next_scheduled_at,
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
    reconcile_sqlite_mirror(&state.accounts, &state.db)
}

fn reconcile_sqlite_mirror(
    accounts_mgr: &crate::auth::accounts::AccountManager,
    db: &crate::store::Db,
) -> anyhow::Result<()> {
    let accounts = accounts_mgr.list()?;
    let active_uuids: std::collections::HashSet<&str> =
        accounts.iter().map(|a| a.account_uuid.as_str()).collect();

    for acc in &accounts {
        if let Err(e) = db.upsert_account(&crate::store::StoredAccount {
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

    // Backwards-compatibility guard: if a legacy single-account `credentials.json`
    // is still on disk (e.g. offline boot prevented `migrate_legacy` from completing),
    // do not prune existing SQLite account rows or their history.
    if accounts_mgr.data_dir.join("credentials.json").exists() {
        return Ok(());
    }

    // Prune any orphan SQLite account rows left behind if a prior delete failed
    // before dependent snapshot rows were cleaned up.
    let sqlite_ids: Vec<String> = {
        let conn = db.conn();
        let mut stmt = conn.prepare("SELECT id FROM accounts")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for id in sqlite_ids {
        if !active_uuids.contains(id.as_str()) {
            if let Err(e) = db.delete_account(&id) {
                tracing::warn!("reconcile_sqlite_account_mirror: failed to prune orphan {id}: {e:#}");
            } else {
                tracing::info!("reconcile_sqlite_account_mirror: pruned orphan account {id}");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(project: &str, source_file: &str, cost_usd: f64) -> StoredSessionEvent {
        StoredSessionEvent {
            ts: Utc::now(),
            project: project.into(),
            model: "claude-fable-5-1".into(),
            input_tokens: 10,
            output_tokens: 10,
            cache_read_tokens: 0,
            cache_creation_5m_tokens: 0,
            cache_creation_1h_tokens: 0,
            cost_usd,
            source_file: source_file.into(),
            source_line: 1,
            event_id: format!("{source_file}:{cost_usd}"),
        }
    }

    #[test]
    fn a_report_window_is_whole_local_days_ending_today() {
        use chrono::{FixedOffset, TimeZone};
        let nz = FixedOffset::east_opt(13 * 3600).unwrap();
        let now = nz.with_ymd_and_hms(2026, 11, 11, 15, 30, 0).unwrap();
        // Seven days is today and the six before it, from local midnight.
        assert_eq!(
            report_window_start(7, &now),
            nz.with_ymd_and_hms(2026, 11, 5, 0, 0, 0).unwrap()
        );
        assert_eq!(
            report_window_start(1, &now),
            nz.with_ymd_and_hms(2026, 11, 11, 0, 0, 0).unwrap()
        );
        assert_eq!(
            report_window_start(365, &now),
            nz.with_ymd_and_hms(2025, 11, 12, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn a_report_window_of_no_days_is_still_today() {
        use chrono::{FixedOffset, TimeZone};
        let west = FixedOffset::west_opt(8 * 3600).unwrap();
        let now = west.with_ymd_and_hms(2026, 3, 1, 0, 5, 0).unwrap();
        assert_eq!(report_window_start(0, &now), report_window_start(1, &now));
        assert_eq!(
            report_window_start(1, &now),
            west.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn a_session_is_counted_once_however_many_calls_it_made() {
        let events = vec![
            ev("atlas", "/p/atlas/s1.jsonl", 1.0),
            ev("atlas", "/p/atlas/s1.jsonl", 2.0),
            ev("atlas", "/p/atlas/s1/subagents/agent-ab.jsonl", 0.5),
            ev("atlas", "/p/atlas/s2.jsonl", 1.0),
        ];
        let out = project_breakdown(&events);
        assert_eq!(out[0].session_count, 2);
        assert!((out[0].total_cost_usd - 4.5).abs() < 1e-9);
    }

    #[test]
    fn projects_come_back_most_expensive_first_then_by_name() {
        let out = project_breakdown(&[
            ev("b", "/b.jsonl", 1.0),
            ev("a", "/a.jsonl", 1.0),
            ev("c", "/c.jsonl", 9.0),
        ]);
        assert_eq!(
            out.iter().map(|p| p.project.as_str()).collect::<Vec<_>>(),
            ["c", "a", "b"]
        );
    }

    #[test]
    fn a_project_with_only_subagent_calls_in_the_window_still_counts_its_session() {
        let out = project_breakdown(&[ev("atlas", "p/atlas/s1/subagents/agent-ab.jsonl", 0.5)]);
        assert_eq!(out[0].session_count, 1);
    }

    fn cached(five_hour: Option<f64>, last_error: Option<&str>) -> CachedUsage {
        use crate::app_state::SnapshotOrigin;
        use crate::auth::AuthSource;
        use crate::usage_api::types::{UsageSnapshot, Utilization};
        CachedUsage {
            snapshot: UsageSnapshot {
                five_hour: five_hour.map(|u| Utilization { utilization: u, resets_at: None }),
                seven_day: None,
                seven_day_sonnet: None,
                seven_day_opus: None,
                seven_day_fable: None,
                extra_usage: None,
                fetched_at: Utc::now(),
                unknown: Default::default(),
            },
            account_id: "token:abc".into(),
            account_email: "Catch".into(),
            last_error: last_error.map(str::to_string),
            burn_rate: None,
            auth_source: AuthSource::OAuth,
            origin: SnapshotOrigin::Http,
            carried_fetched_at: None,
        }
    }

    #[test]
    fn an_empty_token_account_waits_only_while_it_is_the_active_login_and_the_bridge_runs() {
        assert_eq!(local_only_marker(true, true), AWAITING_STATUSLINE);
        // The bridge can't feed a token account that isn't signed in.
        assert_eq!(local_only_marker(true, false), USAGE_NOT_SUPPORTED);
        // No bridge: nothing will ever arrive.
        assert_eq!(local_only_marker(false, true), USAGE_NOT_SUPPORTED);
        assert_eq!(local_only_marker(false, false), USAGE_NOT_SUPPORTED);
    }

    #[test]
    fn a_token_account_with_an_adopted_reading_is_not_reported_unsupported() {
        // Before: every refresh of the account list stamped
        // `usage_not_supported` over a reading the bridge had adopted, and
        // the row swapped its meters for "unavailable".
        let adopted = cached(Some(7.0), None);
        assert_eq!(row_last_error(false, Some(&adopted), || USAGE_NOT_SUPPORTED), None);
    }

    #[test]
    fn a_token_account_with_nothing_gets_the_marker_and_one_without_a_cache_too() {
        let placeholder = cached(None, Some(USAGE_NOT_SUPPORTED));
        assert_eq!(
            row_last_error(false, Some(&placeholder), || AWAITING_STATUSLINE),
            Some(AWAITING_STATUSLINE.to_string()),
            "the marker is recomputed, not copied from the stale placeholder",
        );
        assert_eq!(
            row_last_error(false, None, || USAGE_NOT_SUPPORTED),
            Some(USAGE_NOT_SUPPORTED.to_string()),
        );
    }

    #[test]
    fn an_account_that_can_ask_usage_reports_its_own_error_and_never_looks_up_the_bridge() {
        let failing = cached(Some(7.0), Some("rate_limited"));
        let looked_up = std::cell::Cell::new(false);
        let err = row_last_error(true, Some(&failing), || {
            looked_up.set(true);
            USAGE_NOT_SUPPORTED
        });
        assert_eq!(err.as_deref(), Some("rate_limited"));
        assert!(!looked_up.get());
        assert_eq!(row_last_error(true, None, || USAGE_NOT_SUPPORTED), None);
    }

    #[test]
    fn compact_resize_keeps_top_edge_glued() {
        // Collapsing 380 → 208 must not move the top edge — a menu-bar
        // popover hangs from the tray; center-fixed math visibly detaches it.
        let (_, y) = compact_target_xy(100.0, 24.0, 360.0, 360.0);
        assert_eq!(y, 24.0);
    }

    #[test]
    fn compact_resize_recenters_width() {
        // If the width ever changes, the window recenters horizontally…
        let (x, _) = compact_target_xy(100.0, 24.0, 400.0, 360.0);
        assert_eq!(x, 120.0);
        // …but at equal widths x is untouched (the common case).
        let (x, _) = compact_target_xy(100.0, 24.0, 360.0, 360.0);
        assert_eq!(x, 100.0);
    }

    #[test]
    fn reconcile_prunes_orphans_unless_legacy_credentials_pending() {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::store::Db::open(dir.path()).unwrap();
        let mgr = crate::auth::accounts::AccountManager::new(dir.path().to_path_buf());

        db.upsert_account(&crate::store::StoredAccount {
            id: "legacy-acc".into(),
            email: "legacy@example.com".into(),
            display_name: None,
        })
        .unwrap();
        db.insert_snapshot("legacy-acc", Utc::now(), "{}").unwrap();

        // While legacy `credentials.json` is still pending migration, keep SQLite rows intact.
        let legacy_creds = dir.path().join("credentials.json");
        std::fs::write(&legacy_creds, b"{}").unwrap();
        reconcile_sqlite_mirror(&mgr, &db).unwrap();
        assert!(db.latest_snapshot("legacy-acc").unwrap().is_some());

        // Once legacy migration is complete (`credentials.json` absent), orphan rows are pruned cleanly.
        std::fs::remove_file(&legacy_creds).unwrap();
        reconcile_sqlite_mirror(&mgr, &db).unwrap();
        assert!(db.latest_snapshot("legacy-acc").unwrap().is_none());
    }
}

// ---------------------------------------------------------------------------
// Custom model providers
// ---------------------------------------------------------------------------

use crate::providers::launcher::{self, LaunchSpec, LaunchSurface, Terminal};
use crate::providers::model::Provider;
use crate::providers::presets::{self, PresetInfo};
use crate::providers::{default_env, DefaultProviderState};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum SetDefaultOutcome {
    Applied,
    /// `settings.json` already carries provider env we do not own. The UI
    /// must confirm before we overwrite hand-written configuration.
    NeedsConfirmation { unmanaged_keys: Vec<String> },
}

fn claude_settings_path() -> Result<PathBuf, String> {
    crate::auth::paths::claude_config_home()
        .map(|d| d.join("settings.json"))
        .ok_or_else(|| "could not resolve the Claude config directory".to_string())
}

#[command]
#[specta::specta]
pub async fn list_providers(state: State<'_, Arc<AppState>>) -> Result<Vec<Provider>, String> {
    state.db.list_providers().map_err(|e| e.to_string())
}

#[command]
#[specta::specta]
pub async fn upsert_provider(
    provider: Provider,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state.db.upsert_provider(&provider).map_err(|e| e.to_string())
}

#[command]
#[specta::specta]
pub async fn delete_provider(id: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    // Deleting the active default would leave settings.json holding orphaned
    // env, so undo the default first. `ON DELETE SET NULL` is only a backstop:
    // a nulled provider_id beside a populated managed_env is an orphaned
    // manifest with no UI path to undo the mutation it describes.
    if let Ok(Some(d)) = state.db.get_default_provider() {
        if d.provider_id == id {
            let path = claude_settings_path()?;
            // What we wrote is the provider's own resolved env — §4.1 requires
            // every edit to an active default to go through clear-then-apply,
            // so the stored manifest always corresponds to the current row.
            let written = state
                .db
                .get_provider(&id)
                .map_err(|e| e.to_string())?
                .map(|p| p.resolved_env())
                .unwrap_or_default();
            default_env::clear(&path, &d.managed_env, &written).map_err(|e| e.to_string())?;
            state.db.clear_default_provider().map_err(|e| e.to_string())?;
        }
    }
    state.db.delete_provider(&id).map_err(|e| e.to_string())
}

#[command]
#[specta::specta]
pub async fn list_provider_presets() -> Result<Vec<PresetInfo>, String> {
    Ok(presets::all_info())
}

#[command]
#[specta::specta]
pub async fn list_available_terminals() -> Result<Vec<Terminal>, String> {
    Ok(launcher::available_terminals())
}

#[command]
#[specta::specta]
/// `surface` is optional so callers written before VS Code tabs existed keep
/// launching into a terminal.
pub async fn launch_provider_session(
    provider_id: String,
    cwd: String,
    terminal: Terminal,
    resume_session_id: Option<String>,
    permission_mode: Option<String>,
    surface: Option<LaunchSurface>,
    state: State<'_, Arc<AppState>>,
) -> Result<String, String> {
    let provider = state
        .db
        .get_provider(&provider_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no provider with id {provider_id}"))?;
    let spec = LaunchSpec {
        provider,
        cwd: PathBuf::from(cwd),
        terminal,
        resume_session_id,
        permission_mode,
        surface: surface.unwrap_or_default(),
    };
    launcher::launch(&spec).map_err(|e| format!("{e:#}"))
}

/// Whether a VS Code tab can be offered at all: both the `code` CLI and the
/// Claude Code extension have to be present. Probed at settings time so an
/// unavailable choice surfaces before the user commits to a session.
#[command]
#[specta::specta]
pub async fn vscode_tab_available() -> Result<bool, String> {
    Ok(launcher::vscode::is_available())
}

/// The shell one-liner equivalent of a launch, for users whose terminal we
/// do not drive. Deliberately returns the script path rather than inlining
/// secrets into a string the user will paste somewhere.
#[command]
#[specta::specta]
pub async fn get_provider_launch_command(
    provider_id: String,
    cwd: String,
    terminal: Terminal,
    state: State<'_, Arc<AppState>>,
) -> Result<String, String> {
    let provider = state
        .db
        .get_provider(&provider_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no provider with id {provider_id}"))?;
    let spec = LaunchSpec {
        provider,
        cwd: PathBuf::from(cwd.clone()),
        terminal,
        resume_session_id: None,
        permission_mode: None,
        // A copyable one-liner is inherently a shell command, so this path is
        // terminal-only regardless of the configured surface.
        surface: LaunchSurface::Terminal,
    };
    let script =
        launcher::write_script(&spec, &launcher::script_dir()).map_err(|e| format!("{e:#}"))?;
    let (program, args) = launcher::build_command(terminal, &script, &PathBuf::from(cwd));
    Ok(format!("{program} {}", args.join(" ")))
}

#[command]
#[specta::specta]
pub async fn get_default_provider(
    state: State<'_, Arc<AppState>>,
) -> Result<Option<DefaultProviderState>, String> {
    state.db.get_default_provider().map_err(|e| e.to_string())
}

#[command]
#[specta::specta]
pub async fn set_default_provider(
    provider_id: String,
    force: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<SetDefaultOutcome, String> {
    let provider = state
        .db
        .get_provider(&provider_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no provider with id {provider_id}"))?;
    let path = claude_settings_path()?;
    let prev = state.db.get_default_provider().map_err(|e| e.to_string())?;

    // ORDER IS LOAD-BEARING (spec §4.1): the foreign-key check must run
    // BEFORE clear(prev), against the *previous* manifest.
    //
    // Clearing first restores the user's original values into the file, which
    // the check would then report as foreign — so every ordinary A→B switch
    // would prompt, and declining would leave the previous default already
    // undone with no record of it.
    if !force {
        let known = prev.as_ref().map(|p| p.managed_env.clone()).unwrap_or_default();
        let foreign =
            default_env::foreign_provider_keys(&path, &known).map_err(|e| e.to_string())?;
        if !foreign.is_empty() {
            return Ok(SetDefaultOutcome::NeedsConfirmation { unmanaged_keys: foreign });
        }
    }

    // Only now undo the previous default, so its keys cannot linger and the
    // new manifest is computed against the user's true prior state (§4.1).
    if let Some(prev) = prev {
        let prev_written = state
            .db
            .get_provider(&prev.provider_id)
            .map_err(|e| e.to_string())?
            .map(|p| p.resolved_env())
            .unwrap_or_default();
        default_env::clear(&path, &prev.managed_env, &prev_written).map_err(|e| e.to_string())?;
    }

    let env = provider.resolved_env();
    let manifest = default_env::apply(&path, &env).map_err(|e| e.to_string())?;
    state
        .db
        .set_default_provider(&provider_id, &manifest, Utc::now().timestamp())
        .map_err(|e| e.to_string())?;
    Ok(SetDefaultOutcome::Applied)
}

/// Returns the keys left untouched because the user edited them by hand while
/// the default was active (§4.2) — the UI reports these rather than silently
/// reverting someone's edit.
#[command]
#[specta::specta]
pub async fn clear_default_provider(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<String>, String> {
    let Some(d) = state.db.get_default_provider().map_err(|e| e.to_string())? else {
        return Ok(Vec::new());
    };
    let path = claude_settings_path()?;
    let written = state
        .db
        .get_provider(&d.provider_id)
        .map_err(|e| e.to_string())?
        .map(|p| p.resolved_env())
        .unwrap_or_default();
    let skipped = default_env::clear(&path, &d.managed_env, &written).map_err(|e| e.to_string())?;
    state.db.clear_default_provider().map_err(|e| e.to_string())?;
    Ok(skipped)
}

// ---------------------------------------------------------------------------
// Session browser
// ---------------------------------------------------------------------------

use crate::sessions::{recap, scan, SessionSummary};

const MAX_SESSIONS: usize = 200;

#[command]
#[specta::specta]
pub async fn list_resumable_sessions(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<SessionSummary>, String> {
    let Some(root) = crate::jsonl_parser::walker::claude_projects_root() else {
        return Ok(Vec::new());
    };
    let files = scan::discover_session_files(&root);

    // Newest mtime is the cache key: any new or appended transcript advances it.
    let newest = files
        .first()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok())
        .unwrap_or(std::time::UNIX_EPOCH);

    if let Some((cached_at, rows)) = state.sessions_cache.read().as_ref() {
        if *cached_at == newest {
            return Ok(rows.clone());
        }
    }

    // The cap applies to sessions AFTER filtering, not files scanned — a
    // pre-filter cap would let one session's subagent transcripts evict real
    // sessions. Scanning is cheap; the result list is what needs bounding.
    // One grouped scan of session_events for the whole list — a per-session
    // query would be 200 round-trips to show one column.
    let totals = state.db.session_totals().unwrap_or_default();

    let mut rows: Vec<SessionSummary> = Vec::new();
    for f in &files {
        if let Some(mut s) = recap::parse_session(f) {
            // `session_events.source_file` is stored relative to the projects
            // root (P1-7, so the value is not machine-specific), so the
            // lookup key has to be built the same way.
            if let Some((tokens, cost)) = f
                .strip_prefix(&root)
                .ok()
                .and_then(|rel| totals.get(rel.to_string_lossy().as_ref()))
            {
                s.total_tokens = *tokens;
                s.total_cost_usd = *cost;
            }
            rows.push(s);
            if rows.len() >= MAX_SESSIONS {
                break;
            }
        }
    }

    *state.sessions_cache.write() = Some((newest, rows.clone()));
    Ok(rows)
}

// ---- Live usage from Claude Code (the statusline bridge) ----
//
// Each runs on a blocking thread: they spawn shells and rewrite a file the
// user and Claude Code also write. Every error reaches the UI as text.

async fn bridge_call<T: Send + 'static>(
    state: &State<'_, Arc<AppState>>,
    f: impl FnOnce(Arc<crate::statusline_setup::BridgeManager>) -> Result<T, crate::statusline_setup::BridgeError> + Send + 'static,
) -> Result<T, String> {
    let manager = state.statusline_setup.clone();
    tauri::async_runtime::spawn_blocking(move || f(manager))
        .await
        .map_err(|e| format!("The status line task failed: {e}"))?
        .map_err(|e| e.to_string())
}

#[command]
#[specta::specta]
pub async fn get_statusline_bridge_status(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<crate::statusline_setup::BridgeTargetStatus>, String> {
    let adopted = *state.statusline_last_adopted.read();
    bridge_call(&state, move |m| m.status(adopted)).await
}

#[command]
#[specta::specta]
pub async fn preflight_statusline_bridge(
    config_home: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<crate::statusline_setup::PreflightReport, String> {
    bridge_call(&state, move |m| m.preflight(config_home.map(std::path::PathBuf::from))).await
}

#[command]
#[specta::specta]
pub async fn enable_statusline_bridge(
    config_home: Option<String>,
    accepted_shared_file: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<crate::statusline_setup::BridgeTargetStatus, String> {
    let status = bridge_call(&state, move |m| {
        m.enable(config_home.map(std::path::PathBuf::from), accepted_shared_file)
    })
    .await?;
    // Token accounts show "waiting for Claude Code" while the bridge runs.
    state.force_refresh.notify_one();
    Ok(status)
}

#[command]
#[specta::specta]
pub async fn disable_statusline_bridge(
    target_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<crate::statusline_setup::DisableOutcomeDto, String> {
    let outcome = bridge_call(&state, move |m| m.disable(&target_id)).await?;
    state.force_refresh.notify_one();
    Ok(outcome)
}

#[command]
#[specta::specta]
pub async fn rewrap_statusline_bridge(
    target_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<crate::statusline_setup::BridgeTargetStatus, String> {
    bridge_call(&state, move |m| m.rewrap(&target_id)).await
}

/// The Broken state's **Retry**: repair our own files and re-run the same
/// check the app runs at startup, then report the fresh status. Never edits
/// `settings.json`.
#[command]
#[specta::specta]
pub async fn recheck_statusline_bridge(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<crate::statusline_setup::BridgeTargetStatus>, String> {
    let adopted = *state.statusline_last_adopted.read();
    bridge_call(&state, move |m| {
        m.reconcile_on_startup()?;
        m.status(adopted)
    })
    .await
}

#[command]
#[specta::specta]
pub async fn restore_original_statusline(
    target_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<crate::statusline_setup::DisableOutcomeDto, String> {
    let outcome = bridge_call(&state, move |m| m.restore_original(&target_id)).await?;
    state.force_refresh.notify_one();
    Ok(outcome)
}
