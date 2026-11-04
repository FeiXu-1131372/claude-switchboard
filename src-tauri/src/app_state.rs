use crate::auth::accounts::AccountManager;
use crate::auth::keychain_guardian::KeychainGuardian;
use crate::auth::{AuthOrchestrator, AuthSource};
use crate::jsonl_parser::PricingTable;
use crate::store::Db;
use crate::usage_api::{UsageClient, UsageSnapshot};
use chrono::{DateTime, Utc};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Notify;

/// Marker struct for the in-app warm-up scheduler. The tokio JoinHandle is
/// held by lib.rs at task spawn time and does not need to be reachable from
/// Tauri commands. Kept as a named struct so future tasks (T22+) can add
/// UI-visible state here (e.g. `last_outcome_by_account`) without churning
/// AppState's field layout.
#[derive(Default)]
pub struct WarmupState {
    // Reserved for future per-account status badges (Plan B T22+).
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct Settings {
    pub polling_interval_secs: u64,
    /// Base spacing between consecutive per-slot polls within one round.
    /// The poll loop compresses below this when (slots × gap) wouldn't fit
    /// in `polling_interval_secs`. Bounded by the `update_settings` command
    /// to a safe range (see commands.rs).
    pub stagger_gap_secs: u64,
    pub thresholds: Vec<u8>,
    pub theme: String,
    pub launch_at_login: bool,
    pub crash_reports: bool,
    pub preferred_auth_source: Option<AuthSource>,
    /// `None` means "use the platform default" (`launcher::default_terminal()`).
    /// `#[serde(default)]` keeps settings written before this field readable.
    #[serde(default)]
    pub terminal: Option<crate::providers::launcher::Terminal>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            polling_interval_secs: 300,
            stagger_gap_secs: crate::poll_loop::DEFAULT_STAGGER_GAP_SECS,
            thresholds: vec![75, 90],
            theme: "system".into(),
            launch_at_login: false,
            crash_reports: false,
            preferred_auth_source: None,
            terminal: None,
        }
    }
}

#[cfg(test)]
mod settings_tests {
    use super::Settings;

    #[test]
    fn settings_without_terminal_field_still_deserialize() {
        let json = r#"{
            "polling_interval_secs": 300,
            "stagger_gap_secs": 30,
            "thresholds": [75, 90],
            "theme": "system",
            "launch_at_login": false,
            "crash_reports": false,
            "preferred_auth_source": null
        }"#;
        let s: Settings = serde_json::from_str(json).expect("legacy settings must still parse");
        assert!(s.terminal.is_none());
    }

    #[test]
    fn terminal_choice_round_trips() {
        let s = Settings {
            terminal: Some(crate::providers::launcher::Terminal::Iterm2),
            ..Settings::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.terminal, Some(crate::providers::launcher::Terminal::Iterm2));
    }
}

/// Linear projection of where 5h utilization will land at the current
/// window's reset_at, based on observed slope so far this window. Borrowed
/// from ccusage's burn-rate idea — answers "should I keep coding?" with a
/// concrete number instead of just the bare current %. None when we don't
/// yet have enough samples (need at least 2 polls ≥ 2 minutes apart).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct BurnRateProjection {
    /// Slope of five_hour.utilization, in percentage points per minute.
    /// Positive means consumption is rising; negative is rare but possible
    /// if Anthropic adjusts the metric mid-window.
    pub utilization_per_min: f64,
    /// Projected utilization at five_hour.resets_at if the current pace
    /// continues. Not clamped — values >100 are meaningful (means you'd
    /// hit the cap before reset).
    pub projected_at_reset: f64,
}

/// Where a usage snapshot came from. Every origin but `Http` is a local
/// source that costs no `/usage` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotOrigin {
    #[default]
    Http,
    /// The upstream CLI's persisted `/usage` response in its global config.
    CliCache,
    /// `statusline-usage.json` written by an external poller.
    SharedFile,
    /// A `rate_limits` observation recorded by the statusline bridge.
    Statusline,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct CachedUsage {
    pub snapshot: UsageSnapshot,
    pub account_id: String,
    pub account_email: String,
    pub last_error: Option<String>,
    #[serde(default)]
    pub burn_rate: Option<BurnRateProjection>,
    pub auth_source: AuthSource,
    /// Which source produced `snapshot`'s headline 5h/7d numbers.
    #[serde(default)]
    pub origin: SnapshotOrigin,
    /// For a `Statusline` snapshot: when the slower buckets it carries over
    /// (Sonnet, Opus, extra usage) were last fetched. They are not in the
    /// status line's `rate_limits`, so they keep their own age.
    #[serde(default)]
    #[specta(type = Option<String>)]
    pub carried_fetched_at: Option<DateTime<Utc>>,
}

impl CachedUsage {
    #[allow(dead_code)]
    pub fn is_stale(&self, now: DateTime<Utc>) -> bool {
        (now - self.snapshot.fetched_at) > chrono::Duration::minutes(15)
            || now < self.snapshot.fetched_at
            || self.last_error.is_some()
    }
}

pub struct AppState {
    pub db: Arc<Db>,
    pub auth: Arc<AuthOrchestrator>,
    pub usage: Arc<UsageClient>,
    /// Shared HTTP client — used by the usage API, token exchange, and the
    /// warm-up dispatcher. Stored here so `scheduler_glue` can reach it
    /// without going through UsageClient's private `inner` field.
    pub http_client: Arc<reqwest::Client>,
    pub pricing: Arc<PricingTable>,
    pub settings: RwLock<Settings>,
    pub cached_usage: RwLock<Option<CachedUsage>>,
    pub force_refresh: Notify,
    pub accounts: Arc<AccountManager>,
    /// Serialize token replacement with account switches across windows.
    pub account_change_lock: tokio::sync::Mutex<()>,
    pub cached_usage_by_slot: RwLock<HashMap<u32, CachedUsage>>,
    pub active_slot: RwLock<Option<u32>>,
    /// When the live Claude Code account last changed — a swap, an external
    /// `cswap`, or a Claude Code / cowork instance started on another machine.
    ///
    /// Guards the shared-file local source: `statusline-usage.json` carries no
    /// account identity, so a snapshot written before this instant describes
    /// the previous account and must not be adopted for the new one.
    pub active_since: RwLock<Option<DateTime<Utc>>>,
    /// Woken by the observation-directory watcher: a statusline observation
    /// changed, so the poll loop should re-evaluate the local sources now
    /// (without the keychain read a full `poll_all` tick does).
    pub local_source_changed: Notify,
    /// When a statusline observation was last adopted for any managed
    /// account. Drives the "Live" state of the settings card.
    pub statusline_last_adopted: RwLock<Option<DateTime<Utc>>>,
    /// Per slot, when a statusline snapshot last went into the history table
    /// (they are throttled so the trend charts don't get a row per second).
    pub statusline_last_persisted: RwLock<HashMap<u32, DateTime<Utc>>>,
    pub backoff_by_slot: RwLock<HashMap<u32, BackoffState>>,
    pub schedule_by_slot: RwLock<HashMap<u32, ScheduleState>>,
    pub keychain_guardian: Mutex<Option<KeychainGuardian>>,
    pub warmup: WarmupState,
    /// Set once a real `TrayIconEvent` has reached `tauri_plugin_positioner`.
    /// `Position::TrayCenter` panics (and aborts the process under the
    /// release profile's `panic = "abort"`) if no tray position has been
    /// recorded yet — which can happen on a cold launch, since the popover's
    /// webview mounts and requests a resize before the user has ever
    /// interacted with the tray icon. `resize_window` checks this before
    /// asking for `TrayCenter`.
    pub tray_position_known: AtomicBool,
    /// Cached session-browser scan, invalidated when any transcript's mtime
    /// advances. `ExpandedReport` remounts tab components on every tab
    /// change, so an unmemoized scan would re-parse ~100 MB per switch.
    pub sessions_cache: RwLock<Option<(std::time::SystemTime, Vec<crate::sessions::SessionSummary>)>>,
    /// Turns "Live usage from Claude Code" (the statusline bridge) on and off.
    pub statusline_setup: Arc<crate::statusline_setup::BridgeManager>,
}

#[derive(Debug, Clone, Copy)]
pub struct BackoffState {
    pub until: Instant,
    pub reason: BackoffReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackoffReason {
    /// 429 with a positive `Retry-After`. The server named when it will take
    /// the next call; calling earlier can restart the ban, so user actions
    /// (manual refresh, swap) must not lift it.
    RateLimited,
    /// 401/403 or a failed token refresh. Lifted by any user action, since
    /// the user may have just fixed the credentials.
    AuthFailure,
}

impl AppState {
    /// Lift `slot`'s auth-failure backoff so a user action retries now,
    /// keeping any 429 ban (see [`BackoffReason::RateLimited`]).
    pub fn lift_auth_backoff(&self, slot: u32) {
        let mut backoff = self.backoff_by_slot.write();
        if backoff
            .get(&slot)
            .is_some_and(|b| b.reason == BackoffReason::AuthFailure)
        {
            backoff.remove(&slot);
        }
    }

    /// [`Self::lift_auth_backoff`] for every slot.
    pub fn lift_all_auth_backoffs(&self) {
        self.backoff_by_slot
            .write()
            .retain(|_, b| b.reason == BackoffReason::RateLimited);
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ScheduleState {
    /// Earliest moment this slot is eligible for a usage fetch. The poll
    /// loop will not fetch this slot before this instant. Updated after
    /// each successful fetch to `now + polling_interval_secs`.
    pub next_poll_at: Instant,
}

impl AppState {
    pub fn snapshot(&self) -> Option<CachedUsage> {
        let active = *self.active_slot.read();
        if let Some(slot) = active {
            if let Some(c) = self.cached_usage_by_slot.read().get(&slot) {
                return Some(c.clone());
            }
        }
        self.cached_usage.read().clone()
    }
}
