//! CLI entry points for headless modes.
//! - `claude-switchboard --tick`: run scheduler dispatcher for all eligible
//!   accounts and exit, without starting Tauri.
//! - `claude-switchboard --migrate`: re-launch the GUI which re-runs
//!   migration idempotently.

use anyhow::Result;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliMode {
    Tick,
    Migrate,
    RegisterScheduler,
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
            "--register-scheduler" => return CliMode::RegisterScheduler,
            _ => {}
        }
    }
    CliMode::Gui
}

/// Run one shared dispatcher pass without constructing a GUI or holding its
/// exclusive database lock. Failures are saved per account and return a nonzero
/// exit code to Task Scheduler; healthy accounts continue to be processed.
pub async fn run_tick(data_dir: &Path) -> Result<()> {
    use crate::{
        auth::{accounts::AccountManager, AuthOrchestrator},
        store::Db,
        warmup::runner,
    };
    let db = Db::open_for_tick(data_dir)?;
    let http = std::sync::Arc::new(
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()?,
    );
    let accounts = AccountManager::new(data_dir.to_path_buf());
    let auth = AuthOrchestrator::new(data_dir.to_path_buf(), http.clone());
    let result = runner::scheduled(
        &db,
        &runner::AccountBackend {
            db: &db,
            accounts: &accounts,
            auth: &auth,
            http: &http,
        },
        chrono::Local::now(),
    )
    .await?;
    tracing::info!(target: "switchboard.warmup", "headless tick: {} attempted, {} failed", result.attempted, result.failed);
    anyhow::ensure!(
        result.failed == 0,
        "{} scheduled warm-up(s) failed",
        result.failed
    );
    Ok(())
}

/// Explicit data directory for portable installs and isolated checks. Only
/// headless mode consumes this option; GUI account storage is unchanged.
pub fn tick_data_dir(args: &[String]) -> Result<Option<std::path::PathBuf>> {
    match args.iter().position(|a| a == "--data-dir") {
        Some(i) => {
            let path = args
                .get(i + 1)
                .filter(|p| !p.starts_with("--"))
                .ok_or_else(|| anyhow::anyhow!("--data-dir requires a path"))?;
            Ok(Some(std::path::PathBuf::from(path)))
        }
        None => Ok(None),
    }
}

pub fn register_scheduler() -> Result<()> {
    crate::os_scheduler::for_current_platform()
        .ok_or_else(|| anyhow::anyhow!("Scheduling is unavailable on this platform"))?
        .register(&std::env::current_exe()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_tick_directory_and_registration_mode() {
        assert_eq!(
            parse_args(["--register-scheduler"]),
            CliMode::RegisterScheduler
        );
        assert!(tick_data_dir(&["--tick".into(), "--data-dir".into()]).is_err());
        assert_eq!(
            tick_data_dir(&["--tick".into(), "--data-dir".into(), "a folder".into()]).unwrap(),
            Some("a folder".into())
        );
        assert_eq!(tick_data_dir(&["--tick".into()]).unwrap(), None);
    }

    #[test]
    fn parses_tick_flag() {
        assert_eq!(parse_args(["claude-switchboard", "--tick"]), CliMode::Tick,);
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
