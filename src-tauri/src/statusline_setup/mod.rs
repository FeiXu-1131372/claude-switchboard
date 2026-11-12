//! Turning "Live usage from Claude Code" on and off (spec §4.4, §4.5, §6, §8).
//!
//! The bridge is a small program that becomes the user's `statusLine.command`.
//! This module installs it, proves it runs before anything is written, edits
//! `settings.json` through `statusline_bridge::settings_edit`, and keeps the
//! app's own copy of what is needed to put the user's original back.
//!
//! Settings edits happen only through explicit user actions. Startup
//! reconciliation repairs our own files and re-checks the bridge; it never
//! edits `settings.json`.

#[cfg(test)]
mod candidate_tests;
mod shells;
pub mod state;
mod store;
mod windows;
#[cfg(all(test, windows))]
mod windows_tests;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use state::{derive_state, BridgeState, ObsSummary, SettingsProbe, TargetFacts};
use statusline_bridge::format::{bridge_root, obs_dir, targets_dir, Observation};
use statusline_bridge::settings_edit::{self, DisableOutcome, EditRefused, OriginalStatusLine};
use statusline_bridge::target::{
    self, canonical_settings_path, is_bridge_command, target_id, Target, TargetState, TARGET_VERSION,
};
#[cfg(unix)]
use shells::run_sh;
use shells::PreflightShell;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};
use store::{BridgeStore, StoredState, StoredTarget};

/// Released targets are kept this long after their last run: a session whose
/// settings watcher never saw the change may still call the bridge.
const RELEASED_GC_DAYS: i64 = 7;

/// A probe (and a real-original run) that takes longer than this has failed.
const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a bridge gets to answer `--version`. The first launch of a freshly
/// written executable queues behind the system's scanner: 0.4-0.8 s alone on a
/// quiet Windows machine, a median of 7 s with 32 launching at once. Giving up
/// at 5 s turned a busy machine into "Couldn't install the bridge: the
/// installed bridge doesn't run".
const VERSION_CHECK_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub enum BridgeError {
    ClaudeCodeNotSetUp,
    SharedFileNotAccepted(String),
    /// The command already runs the bridge, but its target file is gone.
    SelfWrapNoTarget(String),
    Edit(EditRefused),
    Preflight(String),
    Install(String),
    NotFound,
    Other(String),
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BridgeError::ClaudeCodeNotSetUp => write!(f, "Claude Code isn't set up on this computer."),
            BridgeError::SharedFileNotAccepted(warning) => write!(f, "{warning} Tick the box to continue."),
            BridgeError::SelfWrapNoTarget(how) => write!(
                f,
                "The status line already runs the Switchboard bridge, but its saved settings are missing, so the original can't be found. {how}"
            ),
            BridgeError::Edit(e) => write!(f, "{e}"),
            BridgeError::Preflight(detail) => write!(f, "{detail}"),
            BridgeError::Install(detail) => write!(f, "Couldn't install the bridge: {detail}"),
            BridgeError::NotFound => write!(f, "That status line setting is no longer tracked."),
            BridgeError::Other(detail) => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for BridgeError {}

impl From<EditRefused> for BridgeError {
    fn from(e: EditRefused) -> Self {
        BridgeError::Edit(e)
    }
}

impl From<std::io::Error> for BridgeError {
    fn from(e: std::io::Error) -> Self {
        BridgeError::Other(e.to_string())
    }
}

impl From<anyhow::Error> for BridgeError {
    fn from(e: anyhow::Error) -> Self {
        BridgeError::Other(format!("{e:#}"))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ShellResult {
    pub shell: String,
    pub ok: bool,
    pub detail: String,
}

/// What the consent panel needs, gathered without changing any setting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct PreflightReport {
    /// The command that would go into `statusLine.command`.
    pub command: String,
    pub shells: Vec<ShellResult>,
    /// How much the bridge adds to each status-line refresh.
    pub latency_ms: u64,
    pub settings_path: String,
    pub bridge_path: String,
    /// The user's current command, `None` when there is none.
    pub original_command: Option<String>,
    pub original_direct: Option<String>,
    pub original_via_bridge: Option<String>,
    pub shared_warning: Option<String>,
}

impl PreflightReport {
    pub fn passed(&self) -> bool {
        !self.shells.is_empty() && self.shells.iter().all(|s| s.ok)
    }

    fn first_failure(&self) -> String {
        self.shells
            .iter()
            .find(|s| !s.ok)
            .map(|s| format!("The bridge couldn't run in {}: {}", s.shell, s.detail))
            .unwrap_or_else(|| "The bridge couldn't be checked.".into())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct BridgeTargetStatus {
    /// Empty when the config folder doesn't exist yet.
    pub id: String,
    pub config_home: String,
    pub settings_path: String,
    pub bridge_path: String,
    pub installed_command: Option<String>,
    pub original_command: Option<String>,
    pub shared_warning: Option<String>,
    pub state: BridgeState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DisableOutcomeDto {
    Restored,
    ReleasedUserChanged,
}

impl From<DisableOutcome> for DisableOutcomeDto {
    fn from(o: DisableOutcome) -> Self {
        match o {
            DisableOutcome::Restored => DisableOutcomeDto::Restored,
            DisableOutcome::ReleasedUserChanged => DisableOutcomeDto::ReleasedUserChanged,
        }
    }
}

#[derive(Debug, Default)]
pub struct ReconcileReport {
    /// `(target id, error)` for targets that just stopped working.
    pub newly_broken: Vec<(String, String)>,
    pub repaired_target_files: usize,
    pub garbage_collected: usize,
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn bridge_file_name() -> &'static str {
    if cfg!(windows) {
        "bridge.exe"
    } else {
        "bridge"
    }
}

/// The bridge binary shipped next to the app (Tauri's `externalBin`), or the
/// one `SWITCHBOARD_BUNDLED_BRIDGE` names (development and tests).
pub fn bundled_bridge() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("SWITCHBOARD_BUNDLED_BRIDGE") {
        return Some(PathBuf::from(p));
    }
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) { "switchboard-statusline.exe" } else { "switchboard-statusline" };
    let candidate = exe.parent()?.join(name);
    candidate.exists().then_some(candidate)
}

/// Single-quote for `/bin/sh`.
fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

fn clip(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_end();
    if text.chars().count() > 2000 {
        format!("{}…", text.chars().take(2000).collect::<String>())
    } else {
        text.to_string()
    }
}

/// `switchboard-statusline 0.1.0` -> `(0, 1, 0)`.
fn version_of(binary: &Path) -> Option<(u64, u64, u64)> {
    version_within(binary, VERSION_CHECK_TIMEOUT)
}

fn version_within(binary: &Path, timeout: Duration) -> Option<(u64, u64, u64)> {
    #[cfg(unix)]
    let out = run_sh(&format!("{} --version", shell_quote(binary)), b"", timeout, None).ok()?;
    // No shell: the bridge is a plain executable, and quoting its path for a
    // shell that may not be the one in use only adds a way to fail.
    #[cfg(windows)]
    let out = shells::run_program(binary, &["--version"], timeout).ok()?;
    if out.code != Some(0) {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let token = text.split_whitespace().last()?;
    let mut parts = token.split('.').map(|p| p.parse::<u64>().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

fn sample_stdin(session_id: &str) -> Vec<u8> {
    serde_json::json!({
        "session_id": session_id,
        "version": "preflight",
        "cost": {"total_api_duration_ms": 0},
        "prompt_cache": {"requests": 0},
        "model": {"id": "preflight", "display_name": "Preflight"},
    })
    .to_string()
    .into_bytes()
}

/// What a preflight needs to know about the settings it is checking.
struct PreflightInput<'a> {
    config_home: &'a Path,
    settings: &'a Path,
    original: &'a OriginalStatusLine,
    shared_warning: Option<String>,
}

/// Try `candidates` in order and stop at the first whose attempt `passed`.
/// When none does, the last attempt comes back so its failure can be shown;
/// an error ends the search at once. `None` for an empty list.
fn first_passing<T, E>(
    candidates: &[String],
    mut attempt: impl FnMut(&str) -> Result<T, E>,
    passed: impl Fn(&T) -> bool,
) -> Option<Result<T, E>> {
    let mut last = None;
    for candidate in candidates {
        let result = attempt(candidate);
        let stop = match &result {
            Ok(done) => passed(done),
            Err(_) => true,
        };
        if stop {
            return Some(result);
        }
        last = Some(result);
    }
    last
}

fn random_id() -> String {
    format!("{:012x}", rand::random::<u64>() & 0xFFFF_FFFF_FFFF)
}

fn same_dir(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

/// Put `new` at `dest`.
///
/// On Windows a running `bridge.exe` cannot be overwritten, and Claude Code
/// runs it on every status-line refresh. It can be renamed, though (spec
/// §4.1): when the plain replace is refused, move the old file aside as
/// `bridge.old-<ms>.exe`, rename the new one in, and put the old one back if
/// that fails. [`sweep_old_copies`] deletes the leftovers once nothing runs them.
#[cfg(windows)]
fn replace_running_file(new: &Path, dest: &Path) -> std::io::Result<()> {
    // A launch in the instant between two renames, or a scanner holding a
    // freshly written file, passes in a moment: about 5 s is allowed.
    fn retrying(mut op: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
        let mut last = None;
        for _ in 0..50 {
            match op() {
                Ok(()) => return Ok(()),
                Err(e) => last = Some(e),
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err(last.unwrap_or_else(|| std::io::Error::other("no attempt was made")))
    }

    // Nothing to move aside (a first install): the only thing in the way is
    // a scanner holding the new file.
    if !dest.exists() {
        return retrying(|| std::fs::rename(new, dest));
    }
    if std::fs::rename(new, dest).is_ok() {
        return Ok(());
    }
    let old = dest.with_file_name(format!("bridge.old-{}.exe", now_ms()));
    retrying(|| std::fs::rename(dest, &old))?;
    if let Err(e) = retrying(|| std::fs::rename(new, dest)) {
        // Leave a working bridge behind.
        let _ = retrying(|| std::fs::rename(&old, dest));
        return Err(e);
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_running_file(new: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::rename(new, dest)
}

/// Delete the `bridge.old-*.exe` copies [`replace_running_file`] left behind,
/// the ones nothing runs any more. A copy still running refuses to be
/// deleted and waits for the next sweep.
#[cfg(windows)]
fn sweep_old_copies(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("bridge.old-") && name.ends_with(".exe") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(not(windows))]
fn sweep_old_copies(_root: &Path) {}

pub struct BridgeManager {
    data_dir: PathBuf,
    root: PathBuf,
    bundled: Option<PathBuf>,
    default_config_home: Option<PathBuf>,
    /// One enable/disable at a time.
    op_lock: Mutex<()>,
}

impl BridgeManager {
    pub fn new(data_dir: PathBuf) -> Self {
        Self::with_paths(
            data_dir,
            bridge_root(),
            bundled_bridge(),
            crate::auth::paths::claude_config_home(),
        )
    }

    pub fn with_paths(
        data_dir: PathBuf,
        root: PathBuf,
        bundled: Option<PathBuf>,
        default_config_home: Option<PathBuf>,
    ) -> Self {
        BridgeManager { data_dir, root, bundled, default_config_home, op_lock: Mutex::new(()) }
    }

    pub fn bridge_path(&self) -> PathBuf {
        self.root.join(bridge_file_name())
    }

    fn command_for(&self, id: &str) -> String {
        format!("{} --target {id}", shell_quote(&self.bridge_path()))
    }

    /// The command that undoes the edit without the app, as a person types it.
    /// On Windows that is PowerShell, the default terminal: the call operator
    /// and double quotes (POSIX single quotes are a parse error there).
    fn restore_command(&self) -> String {
        #[cfg(windows)]
        {
            format!("& \"{}\" --restore --all", self.bridge_path().display())
        }
        #[cfg(not(windows))]
        {
            format!("{} --restore --all", shell_quote(&self.bridge_path()))
        }
    }

    /// The command strings to try for target `id`, best first. macOS has one;
    /// Windows has the forms of spec §4.5, because no single spelling of a
    /// path works unquoted in Git Bash, PowerShell 7 and PowerShell 5.1 for
    /// every profile path.
    fn command_candidates(&self, id: &str) -> Vec<String> {
        #[cfg(windows)]
        {
            let path = self.bridge_path();
            let short = windows::short_path(&path);
            let mut forms =
                windows::command_candidates(&path.to_string_lossy(), short.as_deref(), id, bridge_file_name());
            // The tilde form means the default folder: for any other root it
            // would run (or miss) a different bridge.
            if !same_dir(&self.root, &bridge_root()) {
                forms.retain(|c| !c.starts_with("~/"));
            }
            forms
        }
        #[cfg(not(windows))]
        {
            vec![self.command_for(id)]
        }
    }

    /// `(config home, its settings.json)` for the chosen or default folder.
    fn resolve_home(&self, config_home: Option<PathBuf>) -> Result<(PathBuf, PathBuf), BridgeError> {
        let home = config_home
            .or_else(|| self.default_config_home.clone())
            .ok_or(BridgeError::ClaudeCodeNotSetUp)?;
        if !self.claude_code_set_up(&home) {
            return Err(BridgeError::ClaudeCodeNotSetUp);
        }
        let settings = home.join("settings.json");
        Ok((home, settings))
    }

    fn claude_code_set_up(&self, home: &Path) -> bool {
        home.is_dir()
            || crate::auth::paths::claude_global_config().is_some_and(|p| p.exists())
    }

    fn load_store(&self) -> Result<BridgeStore, BridgeError> {
        Ok(store::load(&self.data_dir)?)
    }

    fn save_store(&self, store: &BridgeStore) -> Result<(), BridgeError> {
        Ok(store::save(&self.data_dir, store)?)
    }

    // ---- installation ----

    /// Make sure `root/bridge` exists, runs, and is at least as new as the
    /// bundled copy (never a downgrade: two builds can share the folder).
    pub fn install_bridge(&self) -> Result<PathBuf, BridgeError> {
        let bundled = self
            .bundled
            .as_ref()
            .ok_or_else(|| BridgeError::Install("the bridge isn't bundled with this build".into()))?;
        let dest = self.bridge_path();
        std::fs::create_dir_all(&self.root).map_err(|e| BridgeError::Install(e.to_string()))?;

        let installed = version_of(&dest);
        let shipped = version_of(bundled);
        let needs_copy = match (installed, shipped) {
            (None, _) => true,
            (Some(have), Some(want)) => have < want,
            (Some(_), None) => false,
        };
        if needs_copy {
            if shipped.is_none() {
                return Err(BridgeError::Install("the bundled bridge doesn't run".into()));
            }
            let tmp = self.root.join(format!("{}.new", bridge_file_name()));
            std::fs::copy(bundled, &tmp).map_err(|e| BridgeError::Install(e.to_string()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| BridgeError::Install(e.to_string()))?;
            }
            replace_running_file(&tmp, &dest).map_err(|e| BridgeError::Install(e.to_string()))?;
        }
        if version_of(&dest).is_none() {
            return Err(BridgeError::Install("the installed bridge doesn't run".into()));
        }
        sweep_old_copies(&self.root);
        // The bridge stops recording if this is ever 30 days old.
        let _ = std::fs::write(self.root.join("last_seen"), b"");
        Ok(dest)
    }

    // ---- preflight ----

    fn temp_target(
        &self,
        config_home: &Path,
        settings: &Path,
        original: Option<serde_json::Value>,
        command: &str,
    ) -> Target {
        Target {
            v: TARGET_VERSION,
            config_home: config_home.to_string_lossy().into(),
            settings_path: settings.to_string_lossy().into(),
            settings_real_path: settings.to_string_lossy().into(),
            original,
            original_value_text: None,
            file_was_created: false,
            installed_command: command.to_string(),
            installed_at_ms: now_ms(),
            state: TargetState::Enabled,
            last_run_ms: None,
            released_by: None,
            released_at_ms: None,
        }
    }

    /// Run the bridge as Claude Code would and report what happened, for the
    /// first command form that works in every shell. Nothing outside our own
    /// folder is written.
    fn run_preflight(&self, id: &str, input: &PreflightInput<'_>) -> Result<PreflightReport, BridgeError> {
        let shells = shells::preflight_shells();
        let candidates = self.command_candidates(id);
        first_passing(&candidates, |command| self.try_candidate(command, id, &shells, input), PreflightReport::passed)
            .unwrap_or_else(|| {
                Err(BridgeError::Preflight(
                    "The path to the bridge needs quoting, and no spelling of it is safe in every shell.".into(),
                ))
            })
    }

    /// One command form, tried in every shell (spec §4.5).
    ///
    /// The probe runs in each shell: the question is whether the command
    /// string reaches the bridge there. The user's real original runs once,
    /// in the shell Claude Code would pick (the first), because the bridge
    /// runs the original in that shell too, and a status line written for
    /// bash would fail in PowerShell with or without us.
    fn try_candidate(
        &self,
        command: &str,
        id: &str,
        shells: &[PreflightShell],
        input: &PreflightInput<'_>,
    ) -> Result<PreflightReport, BridgeError> {
        let (config_home, settings, original) = (input.config_home, input.settings, input.original);
        let mut report = PreflightReport {
            command: command.to_string(),
            shells: Vec::new(),
            latency_ms: 0,
            settings_path: settings.to_string_lossy().into(),
            bridge_path: self.bridge_path().to_string_lossy().into(),
            original_command: original.command.clone().filter(|c| !c.trim().is_empty()),
            original_direct: None,
            original_via_bridge: None,
            shared_warning: input.shared_warning.clone(),
        };
        let Some(primary) = shells.first() else {
            report.shells.push(ShellResult {
                shell: "a shell".into(),
                ok: false,
                detail: "neither Git Bash nor PowerShell was found".into(),
            });
            return Ok(report);
        };

        // 1. Probe: a deterministic original, through the bridge, in each shell.
        let nonce = random_id();
        let probe_id = random_id();
        let session = format!("switchboard-preflight-{nonce}");
        let expected = format!("switchboard-preflight-{nonce}");
        let probe_command = command.replace(id, &probe_id);
        let probe_target = self.temp_target(
            config_home,
            settings,
            Some(serde_json::json!({"type": "command", "command": format!("echo {expected}")})),
            &probe_command,
        );
        target::save(&self.root, &probe_id, &probe_target)?;
        let probes: Vec<_> = shells
            .iter()
            .map(|shell| {
                shells::run_in(shell, &probe_command, &sample_stdin(&session), PREFLIGHT_TIMEOUT, Some(&self.root))
            })
            .collect();
        let cleanup = |id: &str, session: &str| {
            let _ = std::fs::remove_file(targets_dir(&self.root).join(format!("{id}.json")));
            let _ = std::fs::remove_file(obs_dir(&self.root).join(format!("{session}.0.json")));
        };
        cleanup(&probe_id, &session);
        for (index, (shell, probe)) in shells.iter().zip(probes).enumerate() {
            let probe = probe?;
            let probe_ok = probe.code == Some(0) && clip(&probe.stdout) == expected;
            if !probe_ok {
                let stderr = clip(&probe.stderr);
                let detail = match probe.code {
                    None => "it did not finish within 10 seconds".to_string(),
                    Some(code) if !stderr.is_empty() => format!("exit {code}: {stderr}"),
                    Some(code) => {
                        format!("exit {code}, and it printed {:?} instead of the expected line", clip(&probe.stdout))
                    }
                };
                report.shells.push(ShellResult { shell: shell.name.clone(), ok: false, detail });
                continue;
            }
            if index == 0 {
                report.latency_ms = probe.elapsed.as_millis() as u64;
            }
            report.shells.push(ShellResult { shell: shell.name.clone(), ok: true, detail: String::new() });
        }
        if !report.passed() {
            return Ok(report);
        }

        // 2. The user's real original, through the bridge and directly, in
        // the shell Claude Code would pick.
        if let Some(original_command) = report.original_command.clone() {
            let real_id = random_id();
            let real_session = format!("switchboard-preflight-{}", random_id());
            let real_command = command.replace(id, &real_id);
            let real_target = self.temp_target(config_home, settings, original.value.clone(), &real_command);
            target::save(&self.root, &real_id, &real_target)?;
            let stdin = sample_stdin(&real_session);
            let direct = shells::run_in(primary, &original_command, &stdin, PREFLIGHT_TIMEOUT, None);
            let via = shells::run_in(primary, &real_command, &stdin, PREFLIGHT_TIMEOUT, Some(&self.root));
            cleanup(&real_id, &real_session);
            let (direct, via) = (direct?, via?);
            report.original_direct = Some(clip(&direct.stdout));
            report.original_via_bridge = Some(clip(&via.stdout));
            report.latency_ms = via.elapsed.saturating_sub(direct.elapsed).as_millis() as u64;
            if direct.code != via.code {
                if let Some(shell) = report.shells.first_mut() {
                    shell.ok = false;
                    shell.detail = format!(
                        "through the bridge your status line exited {:?}, but run directly it exits {:?}",
                        via.code, direct.code
                    );
                }
            }
        }
        Ok(report)
    }

    pub fn preflight(&self, config_home: Option<PathBuf>) -> Result<PreflightReport, BridgeError> {
        let (home, settings) = self.resolve_home(config_home)?;
        self.install_bridge()?;
        let state = settings_edit::read_state(&settings)?;
        let original = settings_edit::read_original(&settings)?;
        let id = target_id(&canonical_settings_path(&settings).map_err(EditRefused::from)?);
        let input = PreflightInput {
            config_home: &home,
            settings: &settings,
            original: &original,
            shared_warning: state.shared_warning,
        };
        self.run_preflight(&id, &input)
    }

    // ---- enable / disable ----

    pub fn enable(
        &self,
        config_home: Option<PathBuf>,
        accepted_shared_file: bool,
    ) -> Result<BridgeTargetStatus, BridgeError> {
        let _op = self.op_lock.lock().unwrap_or_else(|e| e.into_inner());
        let (home, settings) = self.resolve_home(config_home)?;

        // 1. Install and verify.
        let bridge = self.install_bridge()?;
        let id = target_id(&canonical_settings_path(&settings).map_err(EditRefused::from)?);
        let state = settings_edit::read_state(&settings)?;
        if let Some(warning) = &state.shared_warning {
            if !accepted_shared_file {
                return Err(BridgeError::SharedFileNotAccepted(warning.clone()));
            }
        }
        let original = settings_edit::read_original(&settings)?;

        // The command already runs the bridge: adopt the target we already
        // have, or refuse. Never record ourselves as the original.
        if let Some(current) = original.command.as_deref().filter(|c| is_bridge_command(c, &bridge)) {
            return self.adopt_existing(&id, &home, &settings, current, &state.real_path);
        }

        // 2. Write the target file before touching settings. It records the
        // best command form for now; the preflight below may pick a later one.
        let first_command = self.command_candidates(&id).into_iter().next().ok_or_else(|| {
            BridgeError::Preflight("The path to the bridge needs quoting, and no spelling of it is safe in every shell.".into())
        })?;
        let mut target = Target {
            v: TARGET_VERSION,
            config_home: home.to_string_lossy().into(),
            settings_path: settings.to_string_lossy().into(),
            settings_real_path: state.real_path.to_string_lossy().into(),
            original: original.value.clone(),
            original_value_text: original.value_text.clone(),
            file_was_created: !original.file_exists,
            installed_command: first_command,
            installed_at_ms: now_ms(),
            state: TargetState::Enabled,
            last_run_ms: None,
            released_by: None,
            released_at_ms: None,
        };
        target::save(&self.root, &id, &target)?;
        let undo_target = |anchor: Option<&PathBuf>| {
            let _ = std::fs::remove_file(targets_dir(&self.root).join(format!("{id}.json")));
            if let Some(a) = anchor {
                let _ = std::fs::remove_file(a);
            }
        };

        // 3. The anchor backup, a byte copy a person can restore by hand.
        let anchor = match settings_edit::write_anchor(&settings) {
            Ok(a) => a,
            Err(e) => {
                undo_target(None);
                return Err(e.into());
            }
        };

        // 4. Prove it runs, in every shell, before any setting changes.
        let input = PreflightInput {
            config_home: &home,
            settings: &settings,
            original: &original,
            shared_warning: state.shared_warning.clone(),
        };
        let report = match self.run_preflight(&id, &input) {
            Ok(r) => r,
            Err(e) => {
                undo_target(anchor.as_ref());
                return Err(e);
            }
        };
        if !report.passed() {
            undo_target(anchor.as_ref());
            return Err(BridgeError::Preflight(report.first_failure()));
        }
        // The form that passed is the one that gets installed.
        let command = report.command.clone();
        if target.installed_command != command {
            target.installed_command = command.clone();
            if let Err(e) = target::save(&self.root, &id, &target) {
                undo_target(anchor.as_ref());
                return Err(e.into());
            }
        }

        // 5. The edit itself.
        let record = match settings_edit::enable(&settings, &command, &bridge) {
            Ok(r) => r,
            Err(e) => {
                undo_target(anchor.as_ref());
                return Err(e.into());
            }
        };

        // 6. Remember how to undo it.
        let mut store = self.load_store()?;
        store.targets.insert(
            id.clone(),
            StoredTarget { id: id.clone(), target, record, enabled_at_ms: now_ms(), state: StoredState::Enabled, broken: None },
        );
        self.save_store(&store)?;
        drop(_op);
        self.status_of(&id, None)
    }

    fn adopt_existing(
        &self,
        id: &str,
        home: &Path,
        settings: &Path,
        current_command: &str,
        real_path: &Path,
    ) -> Result<BridgeTargetStatus, BridgeError> {
        let mut t = target::load(&self.root, id).map_err(|_| {
            BridgeError::SelfWrapNoTarget(format!(
                "Run `{}` or restore {} from its .switchboard-statusline-anchor backup.",
                self.restore_command(),
                settings.display()
            ))
        })?;
        t.state = TargetState::Enabled;
        t.released_by = None;
        t.released_at_ms = None;
        t.config_home = home.to_string_lossy().into();
        t.settings_real_path = real_path.to_string_lossy().into();
        t.installed_command = current_command.to_string();
        target::save(&self.root, id, &t)?;
        let record = settings_edit::EnableRecord {
            // The original splice isn't known: restore at the member level.
            old_text: String::new(),
            new_text: String::new(),
            original_value_text: t.original_value_text.clone(),
            original_command: t.original_command().map(str::to_string),
            file_was_created: t.file_was_created,
            file_was_empty: false,
            installed_command: current_command.to_string(),
        };
        let mut store = self.load_store()?;
        store.targets.insert(
            id.to_string(),
            StoredTarget { id: id.to_string(), target: t, record, enabled_at_ms: now_ms(), state: StoredState::Enabled, broken: None },
        );
        self.save_store(&store)?;
        self.status_of(id, None)
    }

    /// Put the user's original status line back.
    pub fn disable(&self, id: &str) -> Result<DisableOutcomeDto, BridgeError> {
        let _op = self.op_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut store = self.load_store()?;
        let outcome = match store.targets.get(id) {
            Some(row) => settings_edit::disable(Path::new(&row.target.settings_path), &row.record)?,
            None => {
                // The app's copy is gone: the target file alone can still restore.
                let t = target::load(&self.root, id).map_err(|_| BridgeError::NotFound)?;
                settings_edit::restore(&t)?
            }
        };
        let released_at = now_ms();
        if let Some(row) = store.targets.get_mut(id) {
            row.state = StoredState::Released;
            row.target.state = TargetState::Released;
            row.target.released_by = Some("switchboard".into());
            row.target.released_at_ms = Some(released_at);
            row.broken = None;
            // The bridge reads the target file, so it must say "released"
            // too and keep the original for sessions still running it.
            let _ = target::save(&self.root, id, &row.target);
        } else if let Ok(mut t) = target::load(&self.root, id) {
            t.state = TargetState::Released;
            t.released_by = Some("switchboard".into());
            t.released_at_ms = Some(released_at);
            let _ = target::save(&self.root, id, &t);
        }
        self.save_store(&store)?;
        Ok(outcome.into())
    }

    /// The "Restore original" button: the same path as turning it off.
    pub fn restore_original(&self, id: &str) -> Result<DisableOutcomeDto, BridgeError> {
        self.disable(id)
    }

    /// "Use the bridge again" after the user changed the command: wrap their
    /// new command as the original. Refused when it contains the bridge.
    pub fn rewrap(&self, id: &str) -> Result<BridgeTargetStatus, BridgeError> {
        let (home, settings) = {
            let store = self.load_store()?;
            let row = store.targets.get(id).ok_or(BridgeError::NotFound)?;
            (PathBuf::from(&row.target.config_home), PathBuf::from(&row.target.settings_path))
        };
        let original = settings_edit::read_original(&settings)?;
        let bridge = self.bridge_path();
        if original.command.as_deref().is_some_and(|c| is_bridge_command(c, &bridge)) {
            return Err(BridgeError::Preflight(
                "That status line already contains the Switchboard bridge, so it can't be wrapped again.".into(),
            ));
        }
        {
            // The old record described a command that is gone.
            let mut store = self.load_store()?;
            store.targets.remove(id);
            self.save_store(&store)?;
        }
        self.enable(Some(home), true)
    }

    // ---- status ----

    fn summarize_obs(&self, config_home: &Path) -> ObsSummary {
        let latest = crate::statusline_source::load_observations(&obs_dir(&self.root))
            .into_iter()
            .map(|(_, obs)| obs)
            .filter(|obs: &Observation| same_dir(Path::new(&obs.config_home), config_home))
            .max_by_key(|obs| obs.observed_at_ms);
        ObsSummary {
            last_heartbeat_ms: latest.as_ref().map(|o| o.observed_at_ms),
            last_no_data_reason: latest.and_then(|o| o.no_data_reason),
        }
    }

    fn settings_probe(&self, settings: &Path) -> SettingsProbe {
        match settings_edit::read_state(settings) {
            Ok(s) => SettingsProbe::Ok { command: s.command, disable_all_hooks: s.disable_all_hooks },
            Err(e) => SettingsProbe::Refused(e.to_string()),
        }
    }

    fn status_of(&self, id: &str, last_adopted: Option<DateTime<Utc>>) -> Result<BridgeTargetStatus, BridgeError> {
        let store = self.load_store()?;
        let row = store.targets.get(id).filter(|r| r.state == StoredState::Enabled).ok_or(BridgeError::NotFound)?;
        Ok(self.row_status(row, last_adopted))
    }

    fn row_status(&self, row: &StoredTarget, last_adopted: Option<DateTime<Utc>>) -> BridgeTargetStatus {
        let settings = PathBuf::from(&row.target.settings_path);
        let home = PathBuf::from(&row.target.config_home);
        let facts = TargetFacts {
            installed_command: row.target.installed_command.clone(),
            broken: row.broken.clone(),
        };
        let probe = self.settings_probe(&settings);
        let state = derive_state(Some(&facts), true, &self.summarize_obs(&home), &probe, last_adopted, Utc::now());
        BridgeTargetStatus {
            id: row.id.clone(),
            config_home: row.target.config_home.clone(),
            settings_path: row.target.settings_path.clone(),
            bridge_path: self.bridge_path().to_string_lossy().into(),
            installed_command: Some(row.target.installed_command.clone()),
            original_command: row.record.original_command.clone(),
            shared_warning: settings_edit::read_state(&settings).ok().and_then(|s| s.shared_warning),
            state,
        }
    }

    /// True when the bridge is installed for the app's own config folder and
    /// can still deliver a reading. A token account that can't ask `/usage`
    /// uses this to say "waiting for Claude Code" rather than "unavailable".
    pub fn feeds_usage(&self) -> bool {
        let Some(home) = &self.default_config_home else { return false };
        let Ok(store) = self.load_store() else { return false };
        store
            .targets
            .values()
            .filter(|r| r.state == StoredState::Enabled && same_dir(Path::new(&r.target.config_home), home))
            .any(|row| {
                matches!(
                    self.row_status(row, None).state,
                    BridgeState::Waiting
                        | BridgeState::Live { .. }
                        | BridgeState::Quiet { .. }
                        | BridgeState::RunningWithoutUsage { .. }
                )
            })
    }

    /// One entry per enabled target, plus one for the default config folder
    /// when it isn't enabled yet.
    pub fn status(&self, last_adopted: Option<DateTime<Utc>>) -> Result<Vec<BridgeTargetStatus>, BridgeError> {
        let store = self.load_store()?;
        let mut out: Vec<BridgeTargetStatus> = store
            .targets
            .values()
            .filter(|r| r.state == StoredState::Enabled)
            .map(|row| self.row_status(row, last_adopted))
            .collect();
        if let Some(home) = &self.default_config_home {
            if !out.iter().any(|s| same_dir(Path::new(&s.config_home), home)) {
                let settings = home.join("settings.json");
                let id = canonical_settings_path(&settings).map(|p| target_id(&p)).unwrap_or_default();
                let set_up = self.claude_code_set_up(home);
                let probe = if set_up { self.settings_probe(&settings) } else { SettingsProbe::Ok { command: None, disable_all_hooks: false } };
                let state = derive_state(None, set_up, &ObsSummary::default(), &probe, None, Utc::now());
                out.push(BridgeTargetStatus {
                    id,
                    config_home: home.to_string_lossy().into(),
                    settings_path: settings.to_string_lossy().into(),
                    bridge_path: self.bridge_path().to_string_lossy().into(),
                    installed_command: None,
                    original_command: None,
                    shared_warning: None,
                    state,
                });
            }
        }
        Ok(out)
    }

    // ---- startup ----

    /// Verify and repair our own files; never edits `settings.json`.
    pub fn reconcile_on_startup(&self) -> Result<ReconcileReport, BridgeError> {
        let mut report = ReconcileReport::default();
        let _op = self.op_lock.lock().unwrap_or_else(|e| e.into_inner());
        sweep_old_copies(&self.root);
        let mut store = self.load_store()?;
        let now = now_ms();
        let mut dirty = false;
        let have_enabled = store.targets.values().any(|r| r.state == StoredState::Enabled);

        // The bridge itself, once for all targets.
        let install_error = if have_enabled { self.install_bridge().err().map(|e| e.to_string()) } else { None };

        for (id, row) in store.targets.iter_mut() {
            if row.state != StoredState::Enabled {
                continue;
            }
            let mut error = install_error.clone();
            if error.is_none() {
                if target::load(&self.root, id).is_err() {
                    // A cleaner deleted it: rewrite it from our copy.
                    if target::save(&self.root, id, &row.target).is_ok() {
                        report.repaired_target_files += 1;
                    }
                }
                error = self.probe(id, &row.target);
            }
            if error != row.broken {
                if let Some(e) = &error {
                    if row.broken.is_none() {
                        report.newly_broken.push((id.clone(), e.clone()));
                    }
                }
                row.broken = error;
                dirty = true;
            }
        }

        // Released targets nobody has run for a week.
        let week_ms = RELEASED_GC_DAYS * 24 * 3600 * 1000;
        let stale: Vec<String> = store
            .targets
            .iter()
            .filter(|(id, row)| {
                row.state == StoredState::Released
                    && row.target.released_at_ms.is_some_and(|at| now - at > week_ms)
                    && target::load(&self.root, id)
                        .ok()
                        .and_then(|t| t.last_run_ms)
                        .is_none_or(|run| now - run > week_ms)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale {
            if let Some(row) = store.targets.remove(&id) {
                let _ = std::fs::remove_file(targets_dir(&self.root).join(format!("{id}.json")));
                let _ = std::fs::remove_file(format!("{}.switchboard-statusline-anchor", row.target.settings_real_path));
                report.garbage_collected += 1;
                dirty = true;
            }
        }
        if dirty {
            self.save_store(&store)?;
        }
        let _ = statusline_bridge::record::prune(&obs_dir(&self.root), SystemTime::now());
        Ok(report)
    }

    /// Does the installed bridge still run for this target? `None` = fine.
    fn probe(&self, id: &str, row_target: &Target) -> Option<String> {
        let probe_id = random_id();
        let nonce = random_id();
        let session = format!("switchboard-preflight-{nonce}");
        let expected = format!("switchboard-preflight-{nonce}");
        // The same command form that was installed, aimed at a probe target:
        // on Windows the installed form is whichever one passed the preflight.
        // A command the user has since wrapped (`<ours>; echo extra`) is not
        // one of our forms, and its extra output would fail the probe, so it
        // is probed as the first form, as macOS always did.
        let form = self.command_candidates(id).iter().position(|c| *c == row_target.installed_command).unwrap_or(0);
        let probe_command =
            self.command_candidates(&probe_id).into_iter().nth(form).unwrap_or_else(|| self.command_for(&probe_id));
        let t = self.temp_target(
            Path::new(&row_target.config_home),
            Path::new(&row_target.settings_path),
            Some(serde_json::json!({"type": "command", "command": format!("echo {expected}")})),
            &probe_command,
        );
        if let Err(e) = target::save(&self.root, &probe_id, &t) {
            return Some(format!("couldn't write a probe target: {e}"));
        }
        let shells = shells::preflight_shells();
        let results: Vec<_> = shells
            .iter()
            .map(|shell| {
                let run =
                    shells::run_in(shell, &probe_command, &sample_stdin(&session), PREFLIGHT_TIMEOUT, Some(&self.root));
                (shell.name.clone(), run)
            })
            .collect();
        let _ = std::fs::remove_file(targets_dir(&self.root).join(format!("{probe_id}.json")));
        let _ = std::fs::remove_file(obs_dir(&self.root).join(format!("{session}.0.json")));
        if results.is_empty() {
            return Some("neither Git Bash nor PowerShell was found".into());
        }
        // Which shell failed matters where there can be several.
        let named = |shell: &str, message: String| if cfg!(windows) { format!("{shell}: {message}") } else { message };
        for (shell, result) in results {
            match result {
                Ok(out) if out.code == Some(0) && clip(&out.stdout) == expected => {}
                Ok(out) => {
                    let stderr = clip(&out.stderr);
                    return Some(named(
                        &shell,
                        if stderr.is_empty() { format!("exit {:?}", out.code) } else { stderr },
                    ));
                }
                Err(e) => return Some(named(&shell, e.to_string())),
            }
        }
        None
    }
}

// These tests drive the manager with `/bin/sh` scripts, so they run on unix
// only. `windows_tests` covers the same ground on Windows with the real bridge
// and the real shells.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    use std::sync::OnceLock;
    use tempfile::{tempdir, TempDir};

    /// The real bridge, built once with the app's own toolchain.
    ///
    /// It gets its own target directory: the bridge's integration tests use
    /// `target/debug/switchboard-statusline` built with extra features, and a
    /// plain build of the same path from here would silently replace it.
    fn real_bridge() -> PathBuf {
        static BUILT: OnceLock<PathBuf> = OnceLock::new();
        BUILT
            .get_or_init(|| {
                let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
                let base = std::env::var_os("CARGO_TARGET_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| manifest.join("target"));
                let target_dir = base.join("statusline-bridge-for-app-tests");
                let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
                let status = Command::new(cargo)
                    .args(["build", "-p", "statusline-bridge", "--manifest-path"])
                    .arg(manifest.join("Cargo.toml"))
                    .arg("--target-dir")
                    .arg(&target_dir)
                    .status()
                    .expect("cargo runs");
                assert!(status.success(), "building the bridge");
                target_dir.join("debug").join("switchboard-statusline")
            })
            .clone()
    }

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    struct Fixture {
        _tmp: TempDir,
        manager: BridgeManager,
        root: PathBuf,
        home: PathBuf,
        settings: PathBuf,
        data: PathBuf,
    }

    fn fixture(bundled: Option<PathBuf>, settings_text: Option<&str>) -> Fixture {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("bridge-root");
        let home = tmp.path().join(".claude");
        let data = tmp.path().join("data");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        let settings = home.join("settings.json");
        if let Some(text) = settings_text {
            std::fs::write(&settings, text).unwrap();
        }
        let manager = BridgeManager::with_paths(data.clone(), root.clone(), bundled, Some(home.clone()));
        Fixture { _tmp: tmp, manager, root, home, settings, data }
    }

    const OWNER: &str = r#"{"a":1,"statusLine":{"type":"command","command":"printf hi","refreshInterval":60}}"#;

    #[test]
    fn install_copies_when_missing_and_never_downgrades() {
        let tools = tempdir().unwrap();
        let v = |ver: &str| format!("#!/bin/sh\ncase \"$1\" in --version) echo \"switchboard-statusline {ver}\";; esac\n");
        let bundled = script(tools.path(), "bundled", &v("0.2.0"));
        let f = fixture(Some(bundled.clone()), None);

        // Missing: copied.
        let installed = f.manager.install_bridge().unwrap();
        assert_eq!(version_of(&installed), Some((0, 2, 0)));
        assert!(f.root.join("last_seen").exists());

        // Older installed: replaced.
        script(&f.root, "bridge", &v("0.1.0"));
        f.manager.install_bridge().unwrap();
        assert_eq!(version_of(&f.manager.bridge_path()), Some((0, 2, 0)));

        // Newer installed (another build shares the folder): left alone.
        script(&f.root, "bridge", &v("0.9.0"));
        f.manager.install_bridge().unwrap();
        assert_eq!(version_of(&f.manager.bridge_path()), Some((0, 9, 0)));

        // A broken installed copy is repaired.
        script(&f.root, "bridge", "#!/bin/sh\nexit 1\n");
        f.manager.install_bridge().unwrap();
        assert_eq!(version_of(&f.manager.bridge_path()), Some((0, 2, 0)));
    }

    #[test]
    fn install_fails_clearly_without_a_bundled_bridge() {
        let f = fixture(None, None);
        assert!(matches!(f.manager.install_bridge(), Err(BridgeError::Install(_))));
    }

    fn failing_bridge(tools: &Path, body: &str) -> PathBuf {
        script(
            tools,
            "failing",
            &format!("#!/bin/sh\ncase \"$1\" in --version) echo \"switchboard-statusline 0.1.0\";; *) {body};; esac\n"),
        )
    }

    #[test]
    fn enable_undoes_everything_when_the_preflight_fails() {
        let tools = tempdir().unwrap();
        let f = fixture(Some(failing_bridge(tools.path(), "exit 1")), Some(OWNER));
        let before = std::fs::read(&f.settings).unwrap();

        let err = f.manager.enable(None, false).unwrap_err();
        assert!(matches!(err, BridgeError::Preflight(_)), "{err}");
        assert_eq!(std::fs::read(&f.settings).unwrap(), before, "settings byte-identical");
        let targets: Vec<_> = std::fs::read_dir(targets_dir(&f.root)).map(|d| d.flatten().collect()).unwrap_or_default();
        assert!(targets.iter().all(|e| e.path().extension().is_none_or(|x| x != "json")), "no target file left");
        assert!(!f.home.join("settings.json.switchboard-statusline-anchor").exists());
        assert!(f.manager.load_store().unwrap().targets.is_empty());
    }

    #[test]
    fn enable_refuses_a_failing_probe_with_the_shell_stderr() {
        let tools = tempdir().unwrap();
        let f = fixture(Some(failing_bridge(tools.path(), "echo boom-from-the-shell >&2; exit 126")), Some(OWNER));
        let err = f.manager.enable(None, false).unwrap_err().to_string();
        assert!(err.contains("boom-from-the-shell"), "{err}");
        assert!(err.contains("sh"), "{err}");
    }

    #[test]
    fn enable_with_the_real_bridge_runs_the_users_status_line_and_disable_restores_exactly() {
        let f = fixture(Some(real_bridge()), Some(OWNER));
        let before = std::fs::read(&f.settings).unwrap();

        let status = f.manager.enable(None, false).unwrap();
        assert_eq!(status.state, BridgeState::Waiting);
        assert_eq!(status.original_command.as_deref(), Some("printf hi"));
        let after: serde_json::Value = serde_json::from_slice(&std::fs::read(&f.settings).unwrap()).unwrap();
        assert_eq!(after["a"], 1);
        assert_eq!(after["statusLine"]["refreshInterval"], 60, "other statusLine keys stay");
        let installed = after["statusLine"]["command"].as_str().unwrap().to_string();
        assert_eq!(Some(installed.clone()), status.installed_command);

        // The installed command, run the way Claude Code runs it, prints the
        // user's own status line.
        let out = run_sh(&installed, &sample_stdin("e2e-session"), Duration::from_secs(10), Some(&f.root)).unwrap();
        assert_eq!(clip(&out.stdout), "hi");
        assert_eq!(out.code, Some(0));

        let target_file = targets_dir(&f.root).join(format!("{}.json", status.id));
        assert!(target_file.exists());
        assert!(f.home.join("settings.json.switchboard-statusline-anchor").exists());

        assert_eq!(f.manager.disable(&status.id).unwrap(), DisableOutcomeDto::Restored);
        assert_eq!(std::fs::read(&f.settings).unwrap(), before, "byte-identical after disable");
        let released = target::load(&f.root, &status.id).unwrap();
        assert_eq!(released.state, TargetState::Released, "kept for running sessions");
        assert!(released.original_value_text.is_some());
        // Off again.
        let list = f.manager.status(None).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].state, BridgeState::Off);
    }

    #[test]
    fn enable_when_there_is_no_original_prints_nothing_and_records_it() {
        let f = fixture(Some(real_bridge()), Some(r#"{"a":1}"#));
        let status = f.manager.enable(None, false).unwrap();
        assert_eq!(status.original_command, None);
        let installed = status.installed_command.unwrap();
        let out = run_sh(&installed, &sample_stdin("no-original"), Duration::from_secs(10), Some(&f.root)).unwrap();
        assert!(out.stdout.is_empty());
        f.manager.disable(&status.id).unwrap();
        assert_eq!(std::fs::read_to_string(&f.settings).unwrap(), r#"{"a":1}"#);
    }

    #[test]
    fn enable_requires_the_checkbox_for_shared_files() {
        let f = fixture(Some(real_bridge()), Some(OWNER));
        std::fs::create_dir(f.home.join(".git")).unwrap();
        let err = f.manager.enable(None, false).unwrap_err();
        assert!(matches!(err, BridgeError::SharedFileNotAccepted(_)), "{err}");
        assert_eq!(std::fs::read_to_string(&f.settings).unwrap(), OWNER, "nothing written");
        f.manager.enable(None, true).unwrap();
    }

    #[test]
    fn feeds_usage_is_true_only_while_the_bridge_is_installed_and_running() {
        let f = fixture(Some(real_bridge()), Some(OWNER));
        assert!(!f.manager.feeds_usage(), "off before it is enabled");

        let status = f.manager.enable(None, false).unwrap();
        assert!(f.manager.feeds_usage(), "enabled and waiting for the first reading");

        // The user pointed the status line somewhere else: nothing runs the
        // bridge, so no reading is coming.
        std::fs::write(&f.settings, r#"{"statusLine":{"type":"command","command":"my own"}}"#).unwrap();
        assert!(!f.manager.feeds_usage());

        f.manager.disable(&status.id).unwrap();
        assert!(!f.manager.feeds_usage(), "off again after disable");
    }

    #[test]
    fn disable_after_a_user_edit_leaves_settings_untouched() {
        let f = fixture(Some(real_bridge()), Some(OWNER));
        let status = f.manager.enable(None, false).unwrap();
        let theirs = r#"{"statusLine":{"type":"command","command":"my own"}}"#;
        std::fs::write(&f.settings, theirs).unwrap();

        // The card says so before anyone clicks anything.
        let list = f.manager.status(None).unwrap();
        assert_eq!(list[0].state, BridgeState::ChangedOutside);

        assert_eq!(f.manager.disable(&status.id).unwrap(), DisableOutcomeDto::ReleasedUserChanged);
        assert_eq!(std::fs::read_to_string(&f.settings).unwrap(), theirs);
    }

    #[test]
    fn rewrap_wraps_the_new_command_and_refuses_one_containing_the_bridge() {
        let f = fixture(Some(real_bridge()), Some(OWNER));
        let status = f.manager.enable(None, false).unwrap();

        // The user (or /statusline) edits the command around ours.
        let wrapped = format!("{}; echo extra", status.installed_command.clone().unwrap());
        std::fs::write(
            &f.settings,
            format!(r#"{{"statusLine":{{"type":"command","command":{}}}}}"#, serde_json::to_string(&wrapped).unwrap()),
        )
        .unwrap();
        let err = f.manager.rewrap(&status.id).unwrap_err().to_string();
        assert!(err.contains("already contains the Switchboard bridge"), "{err}");

        // A genuinely new command is wrapped.
        std::fs::write(&f.settings, r#"{"statusLine":{"type":"command","command":"printf new"}}"#).unwrap();
        let again = f.manager.rewrap(&status.id).unwrap();
        assert_eq!(again.original_command.as_deref(), Some("printf new"));
        let out = run_sh(&again.installed_command.unwrap(), &sample_stdin("rw"), Duration::from_secs(10), Some(&f.root)).unwrap();
        assert_eq!(clip(&out.stdout), "new");
    }

    #[test]
    fn enable_twice_with_the_database_wiped_adopts_the_existing_targets_original() {
        let f = fixture(Some(real_bridge()), Some(OWNER));
        let first = f.manager.enable(None, false).unwrap();
        std::fs::remove_file(f.data.join("statusline_bridge.json")).unwrap();

        // The settings already point at the bridge: adopt, never wrap ourselves.
        let second = f.manager.enable(None, false).unwrap();
        assert_eq!(second.id, first.id);
        let after: serde_json::Value = serde_json::from_slice(&std::fs::read(&f.settings).unwrap()).unwrap();
        assert_eq!(after["statusLine"]["command"], first.installed_command.clone().unwrap(), "unchanged, not wrapped");

        // And it can still be turned off, restoring the user's original.
        assert_eq!(f.manager.disable(&second.id).unwrap(), DisableOutcomeDto::Restored);
        let restored: serde_json::Value = serde_json::from_slice(&std::fs::read(&f.settings).unwrap()).unwrap();
        assert_eq!(restored["statusLine"]["command"], "printf hi");
        assert_eq!(restored["statusLine"]["refreshInterval"], 60);
    }

    #[test]
    fn enable_refuses_a_bridge_command_with_no_target_to_adopt() {
        let f = fixture(Some(real_bridge()), Some(OWNER));
        let status = f.manager.enable(None, false).unwrap();
        std::fs::remove_file(targets_dir(&f.root).join(format!("{}.json", status.id))).unwrap();
        std::fs::remove_file(f.data.join("statusline_bridge.json")).unwrap();
        let err = f.manager.enable(None, false).unwrap_err();
        assert!(matches!(err, BridgeError::SelfWrapNoTarget(_)), "{err}");
        assert!(err.to_string().contains("--restore --all"));
    }

    #[test]
    fn disable_falls_back_to_the_target_file_when_the_apps_copy_is_gone() {
        let f = fixture(Some(real_bridge()), Some(OWNER));
        let status = f.manager.enable(None, false).unwrap();
        std::fs::remove_file(f.data.join("statusline_bridge.json")).unwrap();
        assert_eq!(f.manager.disable(&status.id).unwrap(), DisableOutcomeDto::Restored);
        let restored: serde_json::Value = serde_json::from_slice(&std::fs::read(&f.settings).unwrap()).unwrap();
        assert_eq!(restored["statusLine"]["command"], "printf hi");
    }

    #[test]
    fn invalid_settings_are_refused_with_the_plain_message() {
        let f = fixture(Some(real_bridge()), Some("{ not json"));
        let err = f.manager.enable(None, false).unwrap_err().to_string();
        assert_eq!(err, "settings.json isn't valid JSON. Fix it, then try again.");
        assert_eq!(std::fs::read_to_string(&f.settings).unwrap(), "{ not json");
        // The card reports it instead of pretending everything is fine.
        match &f.manager.status(None).unwrap()[0].state {
            BridgeState::EditRefused { message } => assert!(message.contains("valid JSON")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn claude_code_not_set_up_when_there_is_no_config_at_all() {
        let tmp = tempdir().unwrap();
        let missing = tmp.path().join("nope");
        let manager = BridgeManager::with_paths(tmp.path().join("d"), tmp.path().join("r"), Some(real_bridge()), Some(missing));
        // (The global config of the machine running the tests may exist, so
        // only assert the state when this folder truly has nothing.)
        if !crate::auth::paths::claude_global_config().is_some_and(|p| p.exists()) {
            assert!(matches!(manager.enable(None, false), Err(BridgeError::ClaudeCodeNotSetUp)));
            assert_eq!(manager.status(None).unwrap()[0].state, BridgeState::ClaudeCodeNotSetUp);
        }
    }

    // ---- reconcile ----

    #[test]
    fn reconcile_rewrites_a_missing_target_file_from_the_stored_copy() {
        let f = fixture(Some(real_bridge()), Some(OWNER));
        let status = f.manager.enable(None, false).unwrap();
        let file = targets_dir(&f.root).join(format!("{}.json", status.id));
        std::fs::remove_file(&file).unwrap();

        let report = f.manager.reconcile_on_startup().unwrap();
        assert_eq!(report.repaired_target_files, 1);
        assert!(file.exists());
        assert!(report.newly_broken.is_empty());
        // And the status line works again.
        let out = run_sh(&status.installed_command.unwrap(), &sample_stdin("heal"), Duration::from_secs(10), Some(&f.root)).unwrap();
        assert_eq!(clip(&out.stdout), "hi");
    }

    #[test]
    fn reconcile_repairs_a_missing_bridge_binary_without_asking() {
        let f = fixture(Some(real_bridge()), Some(OWNER));
        f.manager.enable(None, false).unwrap();
        std::fs::remove_file(f.manager.bridge_path()).unwrap();
        let report = f.manager.reconcile_on_startup().unwrap();
        assert!(report.newly_broken.is_empty());
        assert!(version_of(&f.manager.bridge_path()).is_some());
    }

    #[test]
    fn reconcile_marks_broken_when_the_probe_fails_and_reports_it_once() {
        let f = fixture(Some(real_bridge()), Some(OWNER));
        let status = f.manager.enable(None, false).unwrap();
        // The installed bridge starts failing (a newer copy shares the folder,
        // so it is not replaced, but it can't run).
        script(&f.root, "bridge", "#!/bin/sh\ncase \"$1\" in --version) echo \"switchboard-statusline 9.9.9\";; *) echo cannot-run >&2; exit 126;; esac\n");

        let first = f.manager.reconcile_on_startup().unwrap();
        assert_eq!(first.newly_broken.len(), 1);
        assert!(first.newly_broken[0].1.contains("cannot-run"));
        match &f.manager.status(None).unwrap()[0].state {
            BridgeState::Broken { error } => assert!(error.contains("cannot-run")),
            other => panic!("{other:?}"),
        }
        // Reported once: a second start notifies nobody again.
        let second = f.manager.reconcile_on_startup().unwrap();
        assert!(second.newly_broken.is_empty());

        // "Restore original" is the way out, and it works while broken.
        assert_eq!(f.manager.restore_original(&status.id).unwrap(), DisableOutcomeDto::Restored);
    }

    #[test]
    fn reconcile_clears_broken_once_the_bridge_runs_again() {
        let f = fixture(Some(real_bridge()), Some(OWNER));
        f.manager.enable(None, false).unwrap();
        script(&f.root, "bridge", "#!/bin/sh\ncase \"$1\" in --version) echo \"switchboard-statusline 9.9.9\";; *) exit 126;; esac\n");
        f.manager.reconcile_on_startup().unwrap();
        std::fs::remove_file(f.manager.bridge_path()).unwrap();
        f.manager.reconcile_on_startup().unwrap();
        assert!(!matches!(f.manager.status(None).unwrap()[0].state, BridgeState::Broken { .. }));
    }

    #[test]
    fn reconcile_garbage_collects_released_targets_after_7_days_without_a_run() {
        let f = fixture(Some(real_bridge()), Some(OWNER));
        let status = f.manager.enable(None, false).unwrap();
        f.manager.disable(&status.id).unwrap();
        let file = targets_dir(&f.root).join(format!("{}.json", status.id));
        let anchor = f.home.join("settings.json.switchboard-statusline-anchor");

        // Fresh release: kept.
        assert_eq!(f.manager.reconcile_on_startup().unwrap().garbage_collected, 0);
        assert!(file.exists() && anchor.exists());

        // Released 8 days ago and never run since: collected.
        let mut store = f.manager.load_store().unwrap();
        store.targets.get_mut(&status.id).unwrap().target.released_at_ms = Some(now_ms() - 8 * 24 * 3600 * 1000);
        f.manager.save_store(&store).unwrap();
        assert_eq!(f.manager.reconcile_on_startup().unwrap().garbage_collected, 1);
        assert!(!file.exists() && !anchor.exists());
        assert!(f.manager.load_store().unwrap().targets.is_empty());
    }

    #[test]
    fn reconcile_keeps_a_released_target_that_a_session_still_runs() {
        let f = fixture(Some(real_bridge()), Some(OWNER));
        let status = f.manager.enable(None, false).unwrap();
        f.manager.disable(&status.id).unwrap();
        let mut store = f.manager.load_store().unwrap();
        store.targets.get_mut(&status.id).unwrap().target.released_at_ms = Some(now_ms() - 8 * 24 * 3600 * 1000);
        f.manager.save_store(&store).unwrap();
        // A session ran it an hour ago.
        let mut t = target::load(&f.root, &status.id).unwrap();
        t.last_run_ms = Some(now_ms() - 3600 * 1000);
        target::save(&f.root, &status.id, &t).unwrap();
        assert_eq!(f.manager.reconcile_on_startup().unwrap().garbage_collected, 0);
    }

    #[test]
    fn preflight_reports_the_command_paths_outputs_and_latency_without_changing_anything() {
        let f = fixture(Some(real_bridge()), Some(OWNER));
        let before = std::fs::read(&f.settings).unwrap();
        let report = f.manager.preflight(None).unwrap();
        assert!(report.passed(), "{report:?}");
        assert!(report.command.contains("--target"));
        assert_eq!(report.original_command.as_deref(), Some("printf hi"));
        assert_eq!(report.original_direct.as_deref(), Some("hi"));
        assert_eq!(report.original_via_bridge.as_deref(), Some("hi"));
        assert!(report.settings_path.ends_with("settings.json"));
        assert_eq!(std::fs::read(&f.settings).unwrap(), before, "preflight never edits settings");
        assert!(f.manager.load_store().unwrap().targets.is_empty());
        // Our own temporary targets are cleaned up.
        let leftovers = std::fs::read_dir(targets_dir(&f.root))
            .map(|d| d.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "json")).count())
            .unwrap_or(0);
        assert_eq!(leftovers, 0, "temporary probe targets must be removed");
    }

    #[test]
    fn preflight_fails_when_the_bridge_changes_the_originals_exit_code() {
        // A bridge that runs the probe fine but turns every other exit into 0.
        let tools = tempdir().unwrap();
        let real = real_bridge();
        let wrapper = script(
            tools.path(),
            "wrapper",
            &format!(
                "#!/bin/sh\ncase \"$1\" in --version) exec {real} --version;; esac\n\"{real}\" \"$@\" ; code=$?\nif [ \"$code\" = 3 ]; then exit 0; fi\nexit $code\n",
                real = real.display()
            ),
        );
        let f = fixture(Some(wrapper), Some(r#"{"statusLine":{"type":"command","command":"exit 3"}}"#));
        let report = f.manager.preflight(None).unwrap();
        assert!(!report.passed());
        assert!(report.shells[0].detail.contains("exits"), "{:?}", report.shells);
    }

    #[test]
    fn bundled_bridge_honours_the_env_override() {
        // Not asserting the real lookup (it depends on the test runner), only
        // that the override wins.
        std::env::set_var("SWITCHBOARD_BUNDLED_BRIDGE", "/tmp/x-bridge");
        assert_eq!(bundled_bridge(), Some(PathBuf::from("/tmp/x-bridge")));
        std::env::remove_var("SWITCHBOARD_BUNDLED_BRIDGE");
    }

    #[test]
    fn shell_quote_survives_single_quotes() {
        assert_eq!(shell_quote(Path::new("/a b/c")), "'/a b/c'");
        assert_eq!(shell_quote(Path::new("/it's/c")), "'/it'\\''s/c'");
        let out = run_sh(&format!("printf %s {}", shell_quote(Path::new("/it's here/c"))), b"", Duration::from_secs(5), None).unwrap();
        assert_eq!(clip(&out.stdout), "/it's here/c");
    }
}
