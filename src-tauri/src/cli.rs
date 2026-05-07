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

/// Run `--tick`. Headless: open DB without file lock, log a placeholder, exit.
/// Full per-account dispatcher walk is wired in Plan B Task 15 once
/// AccountManager is reachable from the headless context.
pub async fn run_tick(data_dir: &Path) -> Result<()> {
    use crate::store::Db;

    let _db = Db::open_for_tick(data_dir)?;
    tracing::info!(
        "[--tick] dispatcher placeholder — full account walk wired in T15"
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
