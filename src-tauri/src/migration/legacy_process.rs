//! Find and quit a still-running v0.3.x `Claude Limits.app` process.
//! Distinct from `process_detection.rs`, which targets upstream Claude
//! Code / VS Code processes only.

use anyhow::Result;
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, Signal, System};

/// Heuristic match for a v0.3.x running process.
///
/// macOS: binary path under `*/Claude Limits.app/Contents/MacOS/claude-limits`,
/// or process name `claude-limits` (the default Tauri executable name).
///
/// Windows: process name `claude-limits.exe`.
pub fn find_legacy_pids(sys: &System) -> Vec<Pid> {
    let mut hits = Vec::new();
    for (pid, p) in sys.processes() {
        let name = p.name().to_string_lossy();
        let exe_path = p
            .exe()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default();

        let by_name = matches!(name.as_ref(), "claude-limits" | "claude-limits.exe");
        let by_path =
            exe_path.contains("Claude Limits.app/Contents/MacOS/claude-limits");

        if by_name || by_path {
            hits.push(*pid);
        }
    }
    hits
}

/// Send SIGTERM (or `WM_CLOSE`-equivalent via `Signal::Term`) to each pid,
/// then wait up to `grace_secs` for them to exit. Falls back to `Signal::Kill`
/// if any are still alive after the grace.
///
/// Returns `Ok(())` once all processes have exited or been killed.
pub fn quit_legacy_processes(grace_secs: u64) -> Result<()> {
    let mut sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
    );
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let pids = find_legacy_pids(&sys);
    if pids.is_empty() {
        return Ok(());
    }

    for pid in &pids {
        if let Some(p) = sys.process(*pid) {
            let _ = p.kill_with(Signal::Term);
        }
    }

    let deadline = std::time::Instant::now()
        + std::time::Duration::from_secs(grace_secs);
    while std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(250));
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        if find_legacy_pids(&sys).is_empty() {
            return Ok(());
        }
    }

    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    for pid in find_legacy_pids(&sys) {
        if let Some(p) = sys.process(pid) {
            let _ = p.kill_with(Signal::Kill);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_legacy_pids_returns_consistent_result() {
        let mut sys = System::new();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        let pids = find_legacy_pids(&sys);

        // On a clean system this is empty; on a dev machine with v0.3.x
        // running it's >= 1. In either case, every PID returned must
        // correspond to a process that actually exists in the System.
        for pid in &pids {
            assert!(
                sys.process(*pid).is_some(),
                "find_legacy_pids returned a PID that doesn't exist: {pid:?}",
            );
        }

        // The function never panics or returns garbage. That's the contract.
        let _ = pids.len();
    }

    #[test]
    fn quit_legacy_processes_is_noop_when_nothing_to_quit() {
        // Should return immediately with Ok(()) on a clean system.
        let res = quit_legacy_processes(1);
        assert!(res.is_ok());
    }
}
