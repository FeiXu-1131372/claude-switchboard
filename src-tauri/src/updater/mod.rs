//! Auto-update orchestration: scheduler, events, install.
//!
//! The actual download + signature verification + install is handled by
//! `tauri-plugin-updater`; this module owns the policy (when to check,
//! what to emit, how to persist last-checked time) and the two Tauri
//! commands the frontend invokes.

mod persistence;
mod scheduler;
mod version;

use chrono::Utc;
use serde::Serialize;
use specta::Type;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum UpdatePhase {
    Check,
    Download,
    Verify,
    Install,
}

/// In-flight guard so concurrent triggers (timer + manual click) collapse
/// into a single check.
#[derive(Default)]
pub struct UpdaterGuard {
    pub busy: AtomicBool,
}

const EV_CHECKING: &str = "update://checking";
const EV_UP_TO_DATE: &str = "update://up-to-date";
const EV_AVAILABLE: &str = "update://available";
const EV_PROGRESS: &str = "update://progress";
const EV_READY: &str = "update://ready";
const EV_FAILED: &str = "update://failed";

pub fn data_dir(_app: &AppHandle) -> PathBuf {
    crate::store::default_dir()
}

/// Single check cycle. Emits events as it progresses. Never panics.
pub async fn check_and_emit(app: &AppHandle) {
    // Reentrancy guard. If a check is already running, do nothing.
    let guard = app.state::<Arc<UpdaterGuard>>();
    if guard.busy.swap(true, Ordering::AcqRel) {
        tracing::debug!("update check already in flight; skipping");
        return;
    }
    let _release = ReleaseOnDrop { guard: guard.inner().clone() };

    let _ = app.emit(EV_CHECKING, ());

    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => {
            emit_failed(app, UpdatePhase::Check, format!("updater unavailable: {e}"));
            return;
        }
    };

    let maybe_update = match updater.check().await {
        Ok(u) => u,
        Err(e) => {
            emit_failed(app, UpdatePhase::Check, format!("check failed: {e}"));
            return;
        }
    };

    // Persist successful check time (whether or not an update exists).
    persistence::write_last_checked_at(&data_dir(app), Utc::now());

    let Some(update) = maybe_update else {
        let _ = app.emit(EV_UP_TO_DATE, serde_json::json!({ "checkedAt": Utc::now() }));
        return;
    };

    let new_version = update.version.clone();
    let _ = app.emit(
        EV_AVAILABLE,
        serde_json::json!({
            "version": new_version,
            "notes": update.body.clone().unwrap_or_default(),
            "pubDate": update.date.map(|d| d.to_string()).unwrap_or_default(),
        }),
    );

    // Throttled progress emit: at most 5 per second.
    let mut last_progress_emit: Option<std::time::Instant> = None;
    let mut downloaded: u64 = 0;
    let app_for_progress = app.clone();

    let download_result = update
        .download(
            move |chunk_len, content_len| {
                downloaded += chunk_len as u64;
                let should_emit = match last_progress_emit {
                    None => true,
                    Some(t) => t.elapsed() >= std::time::Duration::from_millis(200),
                };
                if should_emit {
                    last_progress_emit = Some(std::time::Instant::now());
                    let _ = app_for_progress.emit(
                        EV_PROGRESS,
                        serde_json::json!({
                            "downloaded": downloaded,
                            "total": content_len.unwrap_or(0),
                        }),
                    );
                }
            },
            || {},
        )
        .await;

    let bytes = match download_result {
        Ok(b) => b,
        Err(e) => {
            // tauri-plugin-updater's `download` performs signature verification
            // internally; a sig failure surfaces here. Distinguish by string match
            // — coarse but the plugin doesn't expose a typed error variant.
            let msg = e.to_string();
            let phase = if msg.contains("signature") || msg.contains("verify") {
                UpdatePhase::Verify
            } else {
                UpdatePhase::Download
            };
            emit_failed(app, phase, msg);
            return;
        }
    };

    if let Err(e) = update.install(bytes) {
        emit_failed(app, UpdatePhase::Install, format!("install staging failed: {e}"));
        return;
    }

    let _ = app.emit(EV_READY, serde_json::json!({ "version": new_version }));
}

/// Triggers `update.install()` to actually run the staged installer.
/// Tauri's plugin restarts the app for us.
pub async fn install_now(app: &AppHandle) -> Result<(), String> {
    let guard = app.state::<Arc<UpdaterGuard>>();
    if guard.busy.swap(true, Ordering::AcqRel) {
        return Err("an update operation is already in progress".into());
    }
    let _release = ReleaseOnDrop { guard: guard.inner().clone() };

    // We re-fetch + re-download because Tauri's API doesn't expose a
    // "install the previously-staged bytes" entry point. The download
    // is small (~10MB) and the bytes are already on a CDN; this is fine.
    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => {
            let msg = e.to_string();
            emit_failed(app, UpdatePhase::Install, msg.clone());
            return Err(msg);
        }
    };
    let maybe_update = match updater.check().await {
        Ok(u) => u,
        Err(e) => {
            let msg = e.to_string();
            emit_failed(app, UpdatePhase::Install, msg.clone());
            return Err(msg);
        }
    };
    let Some(update) = maybe_update else {
        let msg = "no update available".to_string();
        emit_failed(app, UpdatePhase::Install, msg.clone());
        return Err(msg);
    };
    let bytes = match update.download(|_, _| {}, || {}).await {
        Ok(b) => b,
        Err(e) => {
            let msg = e.to_string();
            // download failures during install_now should still surface as install-phase
            // failures from the user's perspective — they clicked Install, not Check.
            emit_failed(app, UpdatePhase::Install, msg.clone());
            return Err(msg);
        }
    };
    if let Err(e) = update.install(bytes) {
        let msg = e.to_string();
        emit_failed(app, UpdatePhase::Install, msg.clone());
        return Err(msg);
    }
    // Tauri's installer relaunches the app on success; control rarely returns here.
    app.restart();
}

/// Background task: on launch (after `delay_until_next_check`) and every
/// 6h thereafter, run a single check cycle.
pub fn run_scheduler(app: AppHandle) {
    #[cfg(debug_assertions)]
    {
        tracing::info!("updater scheduler disabled in dev build");
        let _ = app;
    }

    #[cfg(not(debug_assertions))]
    tauri::async_runtime::spawn(async move {
        loop {
            let legacy_dir = app.path().app_data_dir().ok();
            let last = persistence::read_last_checked_with_fallback(
                &data_dir(&app),
                legacy_dir.as_deref(),
            );
            let delay = scheduler::delay_until_next_check(Utc::now(), last);
            let std_delay = delay
                .to_std()
                .unwrap_or(std::time::Duration::from_secs(0));
            if !std_delay.is_zero() {
                tokio::time::sleep(std_delay).await;
            }
            check_and_emit(&app).await;
            // Sleep the full interval before next cycle.
            tokio::time::sleep(std::time::Duration::from_secs(
                (scheduler::CHECK_INTERVAL_HOURS as u64) * 3600,
            ))
            .await;
        }
    });
}

fn emit_failed(app: &AppHandle, phase: UpdatePhase, message: String) {
    tracing::warn!(?phase, %message, "update cycle failed");
    let _ = app.emit(
        EV_FAILED,
        serde_json::json!({
            "phase": match phase {
                UpdatePhase::Check => "check",
                UpdatePhase::Download => "download",
                UpdatePhase::Verify => "verify",
                UpdatePhase::Install => "install",
            },
            "message": message,
        }),
    );
}

struct ReleaseOnDrop {
    guard: Arc<UpdaterGuard>,
}
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.guard.busy.store(false, Ordering::Release);
    }
}

