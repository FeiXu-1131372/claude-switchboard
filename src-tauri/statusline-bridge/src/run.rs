//! The bridge's run-time sequence (spec §4.2).
//!
//! Claude Code runs this as its `statusLine.command`. It must behave exactly
//! like the user's original command as far as stdout and the exit code go:
//! the original writes straight to our inherited stdout (no copy loop, no
//! pipe that could fill up), and everything we do for Switchboard is
//! best-effort inside `catch_unwind`.

use crate::config_paths::{config_home, global_config_path, EnvView};
use crate::format::{bridge_root, obs_dir, obs_file_name, Observation};
use crate::identity;
use crate::record::{next_record, prune, write_record, StdinFacts};
use crate::target::{self, Target};
use std::io::{Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Set by the bridge on the original's environment. A bridge that starts
/// with it set was launched by another bridge and does nothing.
pub const DEPTH_ENV: &str = "SWITCHBOARD_BRIDGE_DEPTH";

/// The app writes `last_seen` on every start. A bridge whose app has been
/// gone this long stops recording (observations hold an email address) and
/// only passes the original through.
const LAST_SEEN_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 3600);

const LOG_MAX_BYTES: u64 = 256 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Version,
    Run { target_id: String },
    Restore { target: Option<String>, all: bool, by: Option<String> },
}

pub fn parse_args(args: &[String]) -> Result<Command, String> {
    let mut target = None;
    let mut restore = false;
    let mut all = false;
    let mut by = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--version" | "-V" => return Ok(Command::Version),
            "--restore" => restore = true,
            "--all" => all = true,
            "--target" => {
                target = Some(iter.next().ok_or("--target needs an id")?.clone());
            }
            "--by" => {
                by = Some(iter.next().ok_or("--by needs a name")?.clone());
            }
            other => {
                if let Some(id) = other.strip_prefix("--target=") {
                    target = Some(id.to_string());
                } else {
                    return Err(format!("unknown argument: {other}"));
                }
            }
        }
    }
    if restore {
        return Ok(Command::Restore { target, all, by });
    }
    match target {
        Some(target_id) => Ok(Command::Run { target_id }),
        None => Err("missing --target <id>".into()),
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Append one line to `bridge.log`, best effort. The log is cut back when it
/// grows past 256 KiB.
pub fn log(root: &Path, message: &str) {
    let path = root.join("bridge.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_MAX_BYTES) {
        let _ = std::fs::remove_file(&path);
    }
    let _ = std::fs::create_dir_all(root);
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{} {message}", now_ms());
    }
}

/// Entry point for the binary. Returns the process exit code.
pub fn main_entry(args: &[String]) -> i32 {
    let root = bridge_root();
    match parse_args(args) {
        Ok(Command::Version) => {
            println!("switchboard-statusline {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Ok(Command::Run { target_id }) => {
            let env = EnvView::from_process().unwrap_or_default();
            execute(&target_id, &env, &root)
        }
        Ok(Command::Restore { target, all, by }) => crate::restore::run(&root, target.as_deref(), all, by.as_deref()),
        Err(e) => {
            // Never fail a status line over our own argument handling.
            log(&root, &format!("bad arguments: {e}"));
            0
        }
    }
}

/// Run one status-line refresh: forward to the original, record the
/// observation. Returns the exit code the original produced (0 if none).
pub fn execute(target_id: &str, env: &EnvView, root: &Path) -> i32 {
    // 1. Recursion guard.
    if std::env::var_os(DEPTH_ENV).is_some() {
        log(root, "nested invocation: doing nothing");
        return 0;
    }

    // 2. All of stdin, kept whole for the original.
    let mut stdin_bytes = Vec::new();
    let _ = std::io::stdin().lock().read_to_end(&mut stdin_bytes);

    // 3. The target. Without it we cannot know the original: print nothing,
    // exit 0, so Claude Code shows an empty status line and keeps working.
    let target = match target::load(root, target_id) {
        Ok(t) => t,
        Err(e) => {
            log(root, &format!("target {target_id} unavailable: {e}"));
            return 0;
        }
    };
    let _ = target::touch_last_run(root, target_id, now_ms());

    // 4. Start the original now so its latency overlaps with step 5.
    let mut child = match target.original_command() {
        Some(command) => match spawn_original(command, &stdin_bytes) {
            Ok(child) => Some(child),
            Err(e) => {
                log(root, &format!("could not start the original: {e}"));
                return 127;
            }
        },
        None => None,
    };

    // 5. Observation: best effort. Nothing here may change stdout or the
    // exit code, so every failure, including a panic, ends in the log.
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = catch_unwind(AssertUnwindSafe(|| observe(&stdin_bytes, env, root, &target)));
    std::panic::set_hook(previous_hook);
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(e)) => log(root, &format!("observation failed: {e}")),
        Err(payload) => {
            let what = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".into());
            log(root, &format!("observation panicked: {what}"));
        }
    }

    // 6. The original's exit code is ours.
    match child.as_mut() {
        Some(child) => wait_for(child),
        None => 0,
    }
}

fn observe(stdin_bytes: &[u8], env: &EnvView, root: &Path, target: &Target) -> Result<(), String> {
    #[cfg(feature = "inject-panic")]
    if std::env::var_os("SWITCHBOARD_TEST_PANIC").is_some() {
        panic!("injected panic");
    }

    if app_has_been_gone_too_long(root) {
        return Ok(());
    }
    let facts = StdinFacts::parse(stdin_bytes).map_err(|e| e.to_string())?;

    let home = config_home(env, facts.transcript_path.as_deref());
    let global = global_config_path(env, &home);
    let ident = identity::resolve(env, &global);

    let claude_pid: u32 = std::env::var("CLAUDE_PID")
        .ok()
        .and_then(|p| p.trim().parse().ok())
        .unwrap_or(0);
    let dir = obs_dir(root);
    let file = dir.join(obs_file_name(&facts.session_id, Some(claude_pid)));
    let previous: Option<Observation> = std::fs::read_to_string(&file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());

    let record = next_record(
        previous.as_ref(),
        &facts,
        ident,
        claude_pid,
        &home.to_string_lossy(),
        now_ms(),
    );
    let _ = &target.config_home; // the target names the config home it was installed for
    write_record(&dir, &record).map_err(|e| format!("write: {e}"))?;
    let _ = prune(&dir, SystemTime::now());
    Ok(())
}

fn app_has_been_gone_too_long(root: &Path) -> bool {
    std::fs::metadata(root.join("last_seen"))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age > LAST_SEEN_MAX_AGE)
}

#[cfg(unix)]
fn spawn_original(command: &str, stdin_bytes: &[u8]) -> std::io::Result<std::process::Child> {
    use std::process::{Command, Stdio};
    // Same shell Claude Code uses on macOS: `/bin/sh -c`. The child stays in
    // this process's group (as a detached child of Claude Code we lead it),
    // so Claude Code's group kill reaches both of us.
    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .env(DEPTH_ENV, "1")
        .stdin(Stdio::piped())
        // The original writes straight to our own stdout and stderr.
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        let bytes = stdin_bytes.to_vec();
        // From a separate thread, so an original that never reads stdin
        // cannot stall us. A broken pipe just ends the thread.
        std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        });
    }
    Ok(child)
}

// UNTESTED(windows): the shell choice and command building are tested on every
// platform in `shell_windows`; the spawn itself has not run on Windows.
#[cfg(windows)]
fn spawn_original(command: &str, stdin_bytes: &[u8]) -> std::io::Result<std::process::Child> {
    crate::shell_windows::spawn_original(command, stdin_bytes, DEPTH_ENV)
}

#[cfg(not(any(unix, windows)))]
fn spawn_original(_command: &str, _stdin_bytes: &[u8]) -> std::io::Result<std::process::Child> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "running the original is not implemented on this platform",
    ))
}

fn wait_for(child: &mut std::process::Child) -> i32 {
    match child.wait() {
        Ok(status) => {
            if let Some(code) = status.code() {
                return code;
            }
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                if let Some(signal) = status.signal() {
                    return 128 + signal;
                }
            }
            1
        }
        Err(_) => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_run_version_and_restore() {
        assert_eq!(
            parse_args(&args(&["--target", "0123456789ab"])),
            Ok(Command::Run { target_id: "0123456789ab".into() })
        );
        assert_eq!(
            parse_args(&args(&["--target=0123456789ab"])),
            Ok(Command::Run { target_id: "0123456789ab".into() })
        );
        assert_eq!(parse_args(&args(&["--version"])), Ok(Command::Version));
        assert_eq!(
            parse_args(&args(&["--restore", "--all", "--by", "uninstaller"])),
            Ok(Command::Restore { target: None, all: true, by: Some("uninstaller".into()) })
        );
        assert_eq!(
            parse_args(&args(&["--restore", "--target", "0123456789ab"])),
            Ok(Command::Restore { target: Some("0123456789ab".into()), all: false, by: None })
        );
    }

    #[test]
    fn rejects_bad_arguments() {
        assert!(parse_args(&args(&[])).is_err());
        assert!(parse_args(&args(&["--bogus"])).is_err());
        assert!(parse_args(&args(&["--target"])).is_err());
    }
}
