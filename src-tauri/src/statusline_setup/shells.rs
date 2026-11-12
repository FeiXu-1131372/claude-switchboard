//! Where the preflight runs things (spec §4.5).
//!
//! The preflight has to run a command string the way Claude Code would, in
//! every shell Claude Code can pick on this machine: `/bin/sh` on macOS, Git
//! Bash or PowerShell on Windows. This module is the one place that knows
//! which shells those are and how to start them; the manager only asks for
//! "run this command in that shell".

use crate::child_process::{run_with_timeout, RunOutput};
use std::io;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

#[cfg(windows)]
use statusline_bridge::shell_windows::{candidate_shells, invocation, windows_command, RealFs, Shell, WinEnv};

/// One shell Claude Code can run the status line in on this machine.
pub(super) struct PreflightShell {
    /// What the consent panel calls it.
    pub(super) name: String,
    #[cfg(windows)]
    shell: Shell,
}

/// Every shell the preflight must pass in. On Windows that is each shell
/// present, because installing Git later switches Claude Code from PowerShell
/// to bash (spec §4.5).
#[cfg(unix)]
pub(super) fn preflight_shells() -> Vec<PreflightShell> {
    vec![PreflightShell { name: "sh".into() }]
}

#[cfg(windows)]
pub(super) fn preflight_shells() -> Vec<PreflightShell> {
    candidate_shells(&WinEnv::from_process(), &RealFs)
        .into_iter()
        .map(|shell| PreflightShell { name: shell.display_name(), shell })
        .collect()
}

/// Run `command` in `shell` with the given stdin, giving up after `timeout`.
/// `root`, when given, points the bridge at our folder: it finds its own by
/// default (`~/.claude-switchboard/statusline`), so a manager with a custom
/// root stays self-consistent. In production the two are the same path.
#[cfg(unix)]
pub(super) fn run_in(
    _shell: &PreflightShell,
    command: &str,
    stdin: &[u8],
    timeout: Duration,
    root: Option<&Path>,
) -> io::Result<RunOutput> {
    run_sh(command, stdin, timeout, root)
}

#[cfg(windows)]
pub(super) fn run_in(
    shell: &PreflightShell,
    command: &str,
    stdin: &[u8],
    timeout: Duration,
    root: Option<&Path>,
) -> io::Result<RunOutput> {
    let respect_policy = WinEnv::from_process().respects_execution_policy();
    // Written the way Claude Code writes it (libuv quoting), so what the
    // preflight proves is what Claude Code will run.
    let mut cmd = windows_command(&invocation(&shell.shell, command, respect_policy));
    if let Some(root) = root {
        cmd.env("SWITCHBOARD_STATUSLINE_DIR", root);
    }
    drive(cmd, stdin, timeout)
}

/// Run `command` the way Claude Code does on macOS (`/bin/sh -c`).
#[cfg(unix)]
pub(super) fn run_sh(command: &str, stdin: &[u8], timeout: Duration, root: Option<&Path>) -> io::Result<RunOutput> {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c").arg(command);
    if let Some(root) = root {
        cmd.env("SWITCHBOARD_STATUSLINE_DIR", root);
    }
    drive(cmd, stdin, timeout)
}

/// Run a program directly, without a shell.
#[cfg_attr(unix, allow(dead_code))]
pub(super) fn run_program(program: &Path, args: &[&str], timeout: Duration) -> io::Result<RunOutput> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    drive(cmd, b"", timeout)
}

/// Run `cmd` with piped stdio and a timeout (see [`run_with_timeout`]), in an
/// environment that never carries a Claude Code session's identity or the
/// bridge's recursion guard.
fn drive(mut cmd: Command, stdin: &[u8], timeout: Duration) -> io::Result<RunOutput> {
    cmd.env_remove("CLAUDE_PID").env_remove("SWITCHBOARD_BRIDGE_DEPTH");
    run_with_timeout(cmd, stdin, timeout)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    use std::time::Instant;

    #[test]
    fn run_program_reports_the_exit_code_and_output() {
        let me = std::env::current_exe().unwrap();
        let out = run_program(&me, &["--list"], Duration::from_secs(60)).unwrap();
        assert_eq!(out.code, Some(0));
        assert!(!out.stdout.is_empty(), "the test binary lists its tests");
    }

    #[test]
    fn a_program_that_does_not_exist_is_an_error() {
        assert!(run_program(Path::new("definitely-not-a-program-xyz"), &[], Duration::from_secs(5)).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn a_program_that_hangs_is_killed_at_the_timeout() {
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let ping = Path::new(&system_root).join(r"System32\PING.EXE");
        let started = Instant::now();
        let out = run_program(&ping, &["-n", "30", "127.0.0.1"], Duration::from_secs(1)).unwrap();
        assert_eq!(out.code, None, "killed, so no exit code");
        assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
    }

    #[cfg(windows)]
    #[test]
    fn preflight_shells_names_each_installed_shell() {
        let shells = preflight_shells();
        assert!(!shells.is_empty(), "this machine has neither Git Bash nor PowerShell");
        for shell in &shells {
            assert!(
                ["Git Bash", "PowerShell 7", "Windows PowerShell"].contains(&shell.name.as_str()),
                "{}",
                shell.name
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn run_in_each_shell_runs_the_command_and_sets_the_bridge_root() {
        let root = Path::new(r"C:\bridge-root-for-test");
        for shell in preflight_shells() {
            let command = if shell.name == "Git Bash" {
                "echo shell-ok; printenv SWITCHBOARD_STATUSLINE_DIR"
            } else {
                "Write-Output shell-ok; Write-Output $env:SWITCHBOARD_STATUSLINE_DIR"
            };
            let out = run_in(&shell, command, b"", Duration::from_secs(20), Some(root)).unwrap();
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            assert_eq!(out.code, Some(0), "{}: {text} {}", shell.name, String::from_utf8_lossy(&out.stderr));
            assert!(text.contains("shell-ok"), "{}: {text}", shell.name);
            assert!(text.contains(r"C:\bridge-root-for-test"), "{}: {text}", shell.name);
        }
    }

    /// Found in review. With Git for Windows the direct child is the
    /// `Git\bin\bash.exe` launcher; killing it leaves the real bash and the
    /// command alive, still holding our pipes. Waiting for the pipes to close
    /// then blocked the caller until the command ended, and a caller that holds
    /// the manager's lock (`enable`) froze every later enable and disable.
    #[cfg(windows)]
    #[test]
    fn a_hung_command_is_abandoned_at_the_timeout_in_every_shell() {
        for shell in preflight_shells() {
            let started = Instant::now();
            let out = run_in(&shell, "sleep 40", b"", Duration::from_secs(2), None).unwrap();
            let waited = started.elapsed();
            assert_eq!(out.code, None, "{}: killed, so no exit code", shell.name);
            assert!(waited < Duration::from_secs(15), "{}: the caller was held for {waited:?}", shell.name);
        }
    }

    #[cfg(windows)]
    #[test]
    fn run_in_feeds_stdin_to_the_command() {
        for shell in preflight_shells() {
            let command = if shell.name == "Git Bash" { "cat" } else { "[Console]::Out.Write([Console]::In.ReadToEnd())" };
            let out = run_in(&shell, command, b"hello-stdin", Duration::from_secs(20), None).unwrap();
            assert_eq!(String::from_utf8_lossy(&out.stdout), "hello-stdin", "{}", shell.name);
        }
    }
}
