//! CLI entry points for headless modes.
//! - `claude-switchboard --tick`    : run scheduler dispatcher for all
//!                                    eligible accounts and exit. Future
//!                                    Plan B tasks fill in the per-account
//!                                    walk; this stub validates the path.
//! - `claude-switchboard --migrate` : re-launch the GUI which re-runs
//!                                    migration idempotently.

use anyhow::Result;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliMode {
    Tick,
    Migrate,
    Gui, // default — start the Tauri runtime as usual
}

pub fn parse_args<I, S>(args: I) -> CliMode
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    for a in args {
        match a.as_ref() {
            "--tick" => return CliMode::Tick,
            "--migrate" => return CliMode::Migrate,
            _ => {}
        }
    }
    CliMode::Gui
}

/// Run `--tick`. Headless: open DB, attempt the per-account warm-up walk.
///
/// ## Trade-off: headless AppState reconstruction
///
/// The full `scheduler_glue::walk_due_accounts` path requires an `Arc<AppState>`,
/// which holds an `AccountManager`, `AuthOrchestrator`, `Arc<reqwest::Client>`,
/// and a per-slot snapshot cache (`cached_usage_by_slot`).
///
/// The snapshot cache starts empty for a headless tick — there is no running
/// poll loop to populate it. This means `five_hour.resets_at` will always be
/// `None` for a headless invocation, which is acceptable: the warm-up module
/// treats `None` as "window inactive", so it will issue the warm-up call
/// (correct behaviour for a launchd-driven pre-window fire).
///
/// Reconstructing the remaining AppState fields (AccountManager, Auth, HTTP
/// client) is straightforward but requires wiring them into this entry point
/// independently of `lib.rs::run()`, which builds them inside the Tauri
/// Builder setup closure. That refactor is deferred to a future task.
///
/// **For now** the in-app 30-second dispatcher (spawned in `lib.rs`) handles
/// all warm-up firing while the GUI is open. The launchd `--tick` path is a
/// documented no-op until the headless AppState reconstruction task lands.
pub async fn run_tick(data_dir: &Path) -> Result<()> {
    use crate::store::Db;

    let _db = Db::open_for_tick(data_dir)?;
    tracing::info!(
        "[--tick] headless AppState reconstruction not yet wired; \
         in-app dispatcher (lib.rs) handles warm-up while GUI is open"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tick_flag() {
        assert_eq!(
            parse_args(["claude-switchboard", "--tick"]),
            CliMode::Tick,
        );
    }

    #[test]
    fn parses_migrate_flag() {
        assert_eq!(
            parse_args(["claude-switchboard", "--migrate"]),
            CliMode::Migrate,
        );
    }

    #[test]
    fn defaults_to_gui_when_no_flag() {
        assert_eq!(parse_args(["claude-switchboard"]), CliMode::Gui);
    }

    #[test]
    fn ignores_other_args() {
        assert_eq!(
            parse_args(["claude-switchboard", "--unknown", "--tick"]),
            CliMode::Tick,
        );
    }
}
