//! The manager against the real bridge and the real shells on Windows
//! (spec §4.4, §4.5, §8, §9). Runs only on Windows; the Unix equivalents are
//! in `mod.rs`.

use super::*;
use std::process::Command;
use std::sync::OnceLock;
use tempfile::{tempdir, TempDir};

/// The real bridge, built once with the app's own toolchain, in its own target
/// directory (feature variants of the same crate must not collide).
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
            target_dir.join("debug").join("switchboard-statusline.exe")
        })
        .clone()
}

struct Fixture {
    _tmp: TempDir,
    manager: BridgeManager,
    root: PathBuf,
    home: PathBuf,
    settings: PathBuf,
}

fn fixture(settings_text: Option<&str>) -> Fixture {
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
    let manager = BridgeManager::with_paths(data, root.clone(), Some(real_bridge()), Some(home.clone()));
    Fixture { _tmp: tmp, manager, root, home, settings }
}

const OWNER: &str = r#"{"a":1,"statusLine":{"type":"command","command":"echo hi","refreshInterval":60}}"#;

fn id_of(f: &Fixture) -> String {
    target_id(&canonical_settings_path(&f.settings).unwrap())
}

#[test]
fn preflight_picks_the_first_form_every_shell_accepts() {
    let f = fixture(Some(OWNER));
    f.manager.install_bridge().unwrap();
    let id = id_of(&f);
    let original = settings_edit::read_original(&f.settings).unwrap();
    let input = PreflightInput { config_home: &f.home, settings: &f.settings, original: &original, shared_warning: None };
    let report = f.manager.run_preflight(&id, &input).unwrap();
    assert!(report.passed(), "{report:?}");
    assert!(report.command.ends_with(&format!("--target {id}")), "{}", report.command);
    assert!(!report.command.contains('\\'), "forward slashes work in every shell: {}", report.command);
    assert!(!report.shells.is_empty());
    assert_eq!(report.original_direct.as_deref(), Some("hi"));
    assert_eq!(report.original_via_bridge.as_deref(), Some("hi"));
}

#[test]
fn a_command_form_one_shell_cannot_run_is_not_accepted() {
    let f = fixture(Some(OWNER));
    f.manager.install_bridge().unwrap();
    let id = id_of(&f);
    let original = settings_edit::read_original(&f.settings).unwrap();
    let input = PreflightInput { config_home: &f.home, settings: &f.settings, original: &original, shared_warning: None };
    // Unquoted backslashes: PowerShell runs this, bash eats the backslashes.
    let command = format!("{} --target {id}", f.manager.bridge_path().display());
    let shells = shells::preflight_shells();
    let report = f.manager.try_candidate(&command, &id, &shells, &input).unwrap();
    for result in &report.shells {
        if result.shell == "Git Bash" {
            assert!(!result.ok, "bash cannot run a backslash path: {result:?}");
        } else {
            assert!(result.ok, "{result:?}");
        }
    }
    if shells.iter().any(|s| s.name == "Git Bash") {
        assert!(!report.passed(), "one failing shell fails the preflight");
    }
}

#[test]
fn a_custom_root_never_offers_the_tilde_form() {
    let f = fixture(Some(OWNER));
    f.manager.install_bridge().unwrap();
    let id = id_of(&f);
    assert!(f.manager.command_candidates(&id).iter().all(|c| !c.starts_with("~/")));

    let default = BridgeManager::with_paths(f.root.clone(), statusline_bridge::format::bridge_root(), None, None);
    assert!(default.command_candidates(&id).last().is_some_and(|c| c.starts_with("~/")));
}

// ---- enable / disable end to end ----

/// A stand-in bridge: answers `--version` with `version`, and every other
/// call with `stderr` and `code`. Built with rustc because an `.exe` cannot be
/// faked with a script.
fn fake_bridge(dir: &Path, name: &str, version: &str, stderr: &str, code: i32) -> PathBuf {
    slow_fake_bridge(dir, name, version, 0, stderr, code)
}

/// As [`fake_bridge`], but `--version` takes `version_delay_ms` to answer: the
/// first launch of a freshly written exe is slow while antivirus scans it.
fn slow_fake_bridge(dir: &Path, name: &str, version: &str, version_delay_ms: u64, stderr: &str, code: i32) -> PathBuf {
    let source = dir.join(format!("{name}.rs"));
    std::fs::write(
        &source,
        format!(
            "fn main() {{ let a: Vec<String> = std::env::args().collect(); \
             if a.get(1).map(|s| s.as_str()) == Some(\"--version\") {{ \
             std::thread::sleep(std::time::Duration::from_millis({version_delay_ms})); \
             println!(\"switchboard-statusline {version}\"); return; }} \
             eprintln!(\"{stderr}\"); std::process::exit({code}); }}"
        ),
    )
    .unwrap();
    let exe = dir.join(format!("{name}.exe"));
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let status = Command::new(rustc)
        .args(["--edition", "2021", "-O"])
        .arg(&source)
        .arg("-o")
        .arg(&exe)
        .status()
        .expect("rustc runs");
    assert!(status.success(), "building the fake bridge");
    exe
}

fn no_json_targets(root: &Path) -> bool {
    std::fs::read_dir(targets_dir(root))
        .map(|d| d.flatten().all(|e| e.path().extension().is_none_or(|x| x != "json")))
        .unwrap_or(true)
}

fn sample() -> Vec<u8> {
    sample_stdin("windows-e2e-session")
}

#[test]
fn enable_with_the_real_bridge_runs_the_users_status_line_and_disable_restores_exactly() {
    let f = fixture(Some(OWNER));
    let before = std::fs::read(&f.settings).unwrap();

    let status = f.manager.enable(None, false).unwrap();
    assert_eq!(status.state, BridgeState::Waiting);
    assert_eq!(status.original_command.as_deref(), Some("echo hi"));
    let after: serde_json::Value = serde_json::from_slice(&std::fs::read(&f.settings).unwrap()).unwrap();
    assert_eq!(after["a"], 1);
    assert_eq!(after["statusLine"]["refreshInterval"], 60, "other statusLine keys stay");
    let installed = after["statusLine"]["command"].as_str().unwrap().to_string();
    assert_eq!(Some(installed.clone()), status.installed_command);
    assert!(installed.ends_with(&format!("--target {}", status.id)), "{installed}");

    // The installed command, run the way Claude Code runs it in each shell it
    // can pick, prints the user's own status line.
    for shell in shells::preflight_shells() {
        let out = shells::run_in(&shell, &installed, &sample(), Duration::from_secs(20), Some(&f.root)).unwrap();
        assert_eq!(out.code, Some(0), "{}: {}", shell.name, clip(&out.stderr));
        assert_eq!(clip(&out.stdout), "hi", "{}", shell.name);
    }

    assert!(targets_dir(&f.root).join(format!("{}.json", status.id)).exists());
    assert!(f.home.join("settings.json.switchboard-statusline-anchor").exists());

    assert_eq!(f.manager.disable(&status.id).unwrap(), DisableOutcomeDto::Restored);
    assert_eq!(std::fs::read(&f.settings).unwrap(), before, "byte-identical after disable");
    let released = target::load(&f.root, &status.id).unwrap();
    assert_eq!(released.state, TargetState::Released, "kept for running sessions");
    let list = f.manager.status(None).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].state, BridgeState::Off);
}

#[test]
fn enable_when_there_is_no_original_prints_nothing_and_records_it() {
    let f = fixture(Some(r#"{"a":1}"#));
    let status = f.manager.enable(None, false).unwrap();
    assert_eq!(status.original_command, None);
    let installed = status.installed_command.clone().unwrap();
    for shell in shells::preflight_shells() {
        let out = shells::run_in(&shell, &installed, &sample(), Duration::from_secs(20), Some(&f.root)).unwrap();
        assert!(out.stdout.is_empty(), "{}", shell.name);
    }
    f.manager.disable(&status.id).unwrap();
    assert_eq!(std::fs::read_to_string(&f.settings).unwrap(), r#"{"a":1}"#);
}

#[test]
fn enable_undoes_everything_when_the_preflight_fails() {
    let tools = tempdir().unwrap();
    let failing = fake_bridge(tools.path(), "failing", "0.1.0", "boom-from-the-shell", 126);
    let f = fixture(Some(OWNER));
    let manager = BridgeManager::with_paths(f.manager.data_dir.clone(), f.root.clone(), Some(failing), Some(f.home.clone()));
    let before = std::fs::read(&f.settings).unwrap();

    let err = manager.enable(None, false).unwrap_err();
    assert!(matches!(err, BridgeError::Preflight(_)), "{err}");
    let message = err.to_string();
    assert!(message.contains("boom-from-the-shell"), "{message}");
    assert!(message.contains("couldn't run in"), "{message}");
    assert_eq!(std::fs::read(&f.settings).unwrap(), before, "settings byte-identical");
    assert!(no_json_targets(&f.root), "no target file left");
    assert!(!f.home.join("settings.json.switchboard-statusline-anchor").exists());
    assert!(manager.load_store().unwrap().targets.is_empty());
}

#[test]
fn preflight_reports_each_shell_and_changes_nothing() {
    let f = fixture(Some(OWNER));
    let before = std::fs::read(&f.settings).unwrap();
    let report = f.manager.preflight(None).unwrap();
    assert!(report.passed(), "{report:?}");
    let expected: Vec<String> = shells::preflight_shells().into_iter().map(|s| s.name).collect();
    let got: Vec<String> = report.shells.iter().map(|s| s.shell.clone()).collect();
    assert_eq!(got, expected);
    assert_eq!(report.original_command.as_deref(), Some("echo hi"));
    assert!(report.settings_path.ends_with("settings.json"));
    assert_eq!(std::fs::read(&f.settings).unwrap(), before, "preflight never edits settings");
    assert!(f.manager.load_store().unwrap().targets.is_empty());
    assert!(no_json_targets(&f.root), "temporary probe targets must be removed");
}

#[test]
fn feeds_usage_is_true_only_while_the_bridge_is_installed_and_running() {
    let f = fixture(Some(OWNER));
    assert!(!f.manager.feeds_usage(), "off before it is enabled");
    let status = f.manager.enable(None, false).unwrap();
    assert!(f.manager.feeds_usage(), "enabled and waiting for the first reading");
    std::fs::write(&f.settings, r#"{"statusLine":{"type":"command","command":"my own"}}"#).unwrap();
    assert!(!f.manager.feeds_usage(), "nothing runs the bridge now");
    f.manager.disable(&status.id).unwrap();
    assert!(!f.manager.feeds_usage(), "off again after disable");
}

// ---- startup reconciliation ----

#[test]
fn reconcile_rewrites_a_missing_target_file_from_the_stored_copy() {
    let f = fixture(Some(OWNER));
    let status = f.manager.enable(None, false).unwrap();
    let file = targets_dir(&f.root).join(format!("{}.json", status.id));
    std::fs::remove_file(&file).unwrap();

    let report = f.manager.reconcile_on_startup().unwrap();
    assert_eq!(report.repaired_target_files, 1);
    assert!(file.exists());
    assert!(report.newly_broken.is_empty(), "{:?}", report.newly_broken);
    let installed = status.installed_command.unwrap();
    for shell in shells::preflight_shells() {
        let out = shells::run_in(&shell, &installed, &sample(), Duration::from_secs(20), Some(&f.root)).unwrap();
        assert_eq!(clip(&out.stdout), "hi", "{}", shell.name);
    }
}

#[test]
fn reconcile_repairs_a_missing_bridge_binary_without_asking() {
    let f = fixture(Some(OWNER));
    f.manager.enable(None, false).unwrap();
    std::fs::remove_file(f.manager.bridge_path()).unwrap();
    let report = f.manager.reconcile_on_startup().unwrap();
    assert!(report.newly_broken.is_empty(), "{:?}", report.newly_broken);
    assert!(version_of(&f.manager.bridge_path()).is_some());
}

#[test]
fn reconcile_marks_broken_when_the_installed_bridge_stops_running_and_reports_once() {
    let tools = tempdir().unwrap();
    // Newer than the bundled copy, so reconcile leaves it alone, but unable to run.
    let broken = fake_bridge(tools.path(), "cannot", "9.9.9", "cannot-run", 126);
    let f = fixture(Some(OWNER));
    let status = f.manager.enable(None, false).unwrap();
    std::fs::copy(&broken, f.manager.bridge_path()).unwrap();

    let first = f.manager.reconcile_on_startup().unwrap();
    assert_eq!(first.newly_broken.len(), 1);
    assert!(first.newly_broken[0].1.contains("cannot-run"), "{:?}", first.newly_broken);
    match &f.manager.status(None).unwrap()[0].state {
        BridgeState::Broken { error } => assert!(error.contains("cannot-run"), "{error}"),
        other => panic!("{other:?}"),
    }
    let second = f.manager.reconcile_on_startup().unwrap();
    assert!(second.newly_broken.is_empty(), "reported once");
    assert_eq!(f.manager.restore_original(&status.id).unwrap(), DisableOutcomeDto::Restored);
}

// ---- replacing a bridge that is running (spec §4.1) ----

#[test]
fn replace_running_file_succeeds_while_the_bridge_is_running() {
    let f = fixture(Some(OWNER));
    f.manager.install_bridge().unwrap();
    let id = id_of(&f);
    let dest = f.manager.bridge_path();

    // The bridge reads all of stdin before anything else, so with the pipe
    // left open it stays running, its image mapped, like a status-line refresh
    // in progress.
    let mut child = Command::new(&dest)
        .args(["--target", &id])
        .env("SWITCHBOARD_STATUSLINE_DIR", &f.root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));

    let newer = f.root.join("bridge.exe.new");
    let mut bytes = std::fs::read(real_bridge()).unwrap();
    bytes.extend_from_slice(b"newer");
    std::fs::write(&newer, &bytes).unwrap();

    replace_running_file(&newer, &dest).expect("a running bridge can still be replaced");
    assert_eq!(std::fs::read(&dest).unwrap(), bytes, "the new copy is in place");
    let old_copies = || -> Vec<String> {
        std::fs::read_dir(&f.root)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("bridge.old-"))
            .collect()
    };
    assert_eq!(old_copies().len(), 1, "the running image was renamed away: {:?}", old_copies());

    drop(child.stdin.take());
    child.wait().unwrap();
    sweep_old_copies(&f.root);
    assert!(old_copies().is_empty(), "no longer running, so it can be deleted: {:?}", old_copies());
}

/// The first launch of a freshly copied exe queues behind antivirus. Measured
/// here: 0.4-0.8 s alone, a median of 7 s with 32 launching at once. A version
/// check that gives up at 5 s turned a busy machine into "Couldn't install the
/// bridge: the installed bridge doesn't run".
#[test]
fn a_slow_first_launch_is_not_mistaken_for_a_bridge_that_does_not_run() {
    let tools = tempdir().unwrap();
    let slow = slow_fake_bridge(tools.path(), "slow", "0.1.0", 6_000, "unused", 1);
    assert_eq!(version_of(&slow), Some((0, 1, 0)));
}

#[test]
fn a_bridge_that_never_answers_is_still_given_up_on() {
    let tools = tempdir().unwrap();
    let hung = slow_fake_bridge(tools.path(), "hung", "0.1.0", 120_000, "unused", 1);
    let started = std::time::Instant::now();
    assert_eq!(version_within(&hung, Duration::from_secs(1)), None);
    assert!(started.elapsed() < Duration::from_secs(15), "{:?}", started.elapsed());
}

/// Found in review: a scanner holds a freshly written file for a moment. On a
/// first install `bridge.exe` does not exist yet, so the failed rename was
/// taken for "a running image", the move-aside found nothing to move, and the
/// user saw "The system cannot find the file specified".
#[test]
fn first_install_waits_out_a_scanner_that_briefly_holds_the_new_file() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = tempdir().unwrap();
    let new = dir.path().join("bridge.exe.new");
    let dest = dir.path().join("bridge.exe");
    std::fs::write(&new, b"bridge").unwrap();
    // No sharing at all, for 300 ms, as an antivirus scan holds it.
    let held = std::fs::OpenOptions::new().read(true).share_mode(0).open(&new).unwrap();
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        drop(held);
    });
    replace_running_file(&new, &dest).expect("waits for the scanner instead of giving up");
    release.join().unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"bridge");
}

/// Found in review: adopting a command the user wrapped (`<ours>; echo extra`)
/// stores it verbatim, and the startup probe ran that stored text, so its
/// extra output made a working bridge look Broken. macOS probes the canonical
/// form, and so must this.
#[test]
fn reconcile_does_not_call_an_adopted_wrapped_command_broken() {
    let f = fixture(Some(OWNER));
    let status = f.manager.enable(None, false).unwrap();
    let wrapped = format!("{}; echo extra", status.installed_command.clone().unwrap());
    std::fs::write(
        &f.settings,
        format!(r#"{{"statusLine":{{"type":"command","command":{}}}}}"#, serde_json::to_string(&wrapped).unwrap()),
    )
    .unwrap();
    // Turning the switch on again adopts the existing target and its command.
    let again = f.manager.enable(None, false).unwrap();
    assert_eq!(again.installed_command.as_deref(), Some(wrapped.as_str()));

    let report = f.manager.reconcile_on_startup().unwrap();
    assert!(report.newly_broken.is_empty(), "{:?}", report.newly_broken);
}

/// Found in review: the instructions for undoing the edit without the app must
/// run where a Windows user will type them. POSIX single quotes are a parse
/// error in PowerShell, which is the default terminal on Windows 11.
#[test]
fn the_restore_instructions_in_the_error_run_in_powershell() {
    let f = fixture(Some(OWNER));
    let status = f.manager.enable(None, false).unwrap();
    std::fs::remove_file(targets_dir(&f.root).join(format!("{}.json", status.id))).unwrap();
    std::fs::remove_file(f.manager.data_dir.join("statusline_bridge.json")).unwrap();

    let err = f.manager.enable(None, false).unwrap_err();
    assert!(matches!(err, BridgeError::SelfWrapNoTarget(_)), "{err}");
    let want = format!("& \"{}\" --restore --all", f.manager.bridge_path().display());
    assert!(err.to_string().contains(&want), "{err}");
    assert!(!err.to_string().contains("'C:"), "no POSIX single-quoted path: {err}");
}

#[test]
fn install_bridge_replaces_an_older_copy_even_while_it_runs() {
    let tools = tempdir().unwrap();
    let older = fake_bridge(tools.path(), "older", "0.0.1", "old", 1);
    let f = fixture(Some(OWNER));
    std::fs::create_dir_all(&f.root).unwrap();
    std::fs::copy(&older, f.manager.bridge_path()).unwrap();
    assert_eq!(version_of(&f.manager.bridge_path()), Some((0, 0, 1)));

    f.manager.install_bridge().unwrap();
    assert_eq!(version_of(&f.manager.bridge_path()), version_of(&real_bridge()), "updated to the bundled version");
}
