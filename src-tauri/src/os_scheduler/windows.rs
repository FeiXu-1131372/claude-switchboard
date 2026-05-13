//! Windows Task Scheduler wrapper. Uses schtasks.exe to register a per-user
//! task that fires `<binary> --tick` every minute. Per spec §7.

use anyhow::{Context, Result};
use std::path::Path;

use super::OsScheduler;

const TASK_NAME: &str = "Claude Switchboard Tick";

pub struct SchTasksScheduler;

impl Default for SchTasksScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl SchTasksScheduler {
    pub fn new() -> Self {
        Self
    }
}

impl OsScheduler for SchTasksScheduler {
    fn register(&self, binary_path: &Path) -> Result<()> {
        let bin_str = binary_path.to_string_lossy().to_string();
        // /F overwrites if it already exists; per-user (no /RU SYSTEM); 1-min cadence.
        let status = std::process::Command::new("schtasks")
            .args([
                "/Create",
                "/F",
                "/SC", "MINUTE",
                "/MO", "1",
                "/TN", TASK_NAME,
                "/TR", &format!("\"{}\" --tick", bin_str),
            ])
            .status()
            .context("invoke schtasks /Create")?;
        if !status.success() {
            anyhow::bail!("schtasks /Create failed: status {status}");
        }
        Ok(())
    }

    fn unregister(&self) -> Result<()> {
        let _ = std::process::Command::new("schtasks")
            .args(["/Delete", "/F", "/TN", TASK_NAME])
            .status();
        Ok(())
    }

    fn is_registered(&self) -> Result<bool> {
        let out = std::process::Command::new("schtasks")
            .args(["/Query", "/TN", TASK_NAME])
            .output()
            .context("invoke schtasks /Query")?;
        Ok(out.status.success())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_name_is_branded() {
        assert!(TASK_NAME.contains("Switchboard"));
    }
}
