//! Runs the real bridge binary on Windows against every shell Claude Code can
//! pick (spec §4.2, §4.3, §9). The guarantee under test is the same as in
//! `bridge_cli.rs`: whatever the observation code does, the user's status
//! line output and exit code come through unchanged.
//!
//! Each case runs once per available shell. `SWITCHBOARD_TEST_SHELL`
//! (feature `force-shell`) makes the bridge use a specific shell, so a machine
//! with Git installed still exercises PowerShell through the real binary.

#![cfg(windows)]

use statusline_bridge::format::{obs_dir, Observation};
use statusline_bridge::shell_windows::{candidate_shells, invocation, windows_command, RealFs, Shell, WinEnv};
use statusline_bridge::target::{self, Target, TargetState, TARGET_VERSION};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use tempfile::TempDir;

const ID: &str = "0123456789ab";
const SESSION: &str = "4b1e7c2a-0000-4000-8000-000000000001";
const FIXTURE: &[u8] = include_bytes!("fixtures/stdin_2_1_318.json");

/// The variables a shell needs to start and find itself. Everything else is
/// cleared so a developer's own environment cannot change the result.
const PASS_THROUGH: &[&str] = &[
    "SystemRoot",
    "windir",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "ProgramData",
    "LOCALAPPDATA",
    "APPDATA",
    "PATH",
    "PATHEXT",
    "TEMP",
    "TMP",
    "COMSPEC",
];

/// Every shell on this machine: what Claude Code would pick, plus Windows
/// PowerShell 5.1 when it is not already the PowerShell found.
fn shells() -> Vec<Shell> {
    let mut found = candidate_shells(&WinEnv::from_process(), &RealFs);
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let ps51 = format!(r"{system_root}\System32\WindowsPowerShell\v1.0\powershell.exe");
    if Path::new(&ps51).is_file() && !found.contains(&Shell::PowerShell(ps51.clone())) {
        found.push(Shell::PowerShell(ps51));
    }
    assert!(!found.is_empty(), "this machine has neither Git Bash nor PowerShell");
    found
}

fn force_value(shell: &Shell) -> String {
    match shell {
        Shell::Bash(path) => format!("bash|{path}"),
        Shell::PowerShell(path) => format!("powershell|{path}"),
    }
}

fn label(shell: &Shell) -> String {
    match shell {
        Shell::Bash(path) | Shell::PowerShell(path) => format!("{} ({path})", shell.display_name()),
    }
}

fn is_bash(shell: &Shell) -> bool {
    matches!(shell, Shell::Bash(_))
}

struct Env {
    root: TempDir,
    home: TempDir,
}

impl Env {
    fn new() -> Env {
        let env = Env { root: tempfile::tempdir().unwrap(), home: tempfile::tempdir().unwrap() };
        // A global config that names no account: a disk-stored token.
        std::fs::write(env.home.path().join(".claude.json"), r#"{"oauthAccount":null}"#).unwrap();
        env
    }

    fn root(&self) -> &Path {
        self.root.path()
    }

    fn install(&self, original: Option<&str>) {
        let t = Target {
            v: TARGET_VERSION,
            config_home: self.home.path().join(".claude").to_string_lossy().into(),
            settings_path: r"C:\unused\settings.json".into(),
            settings_real_path: r"C:\unused\settings.json".into(),
            original: original.map(|c| serde_json::json!({"type": "command", "command": c})),
            original_value_text: None,
            file_was_created: false,
            installed_command: format!("bridge.exe --target {ID}"),
            installed_at_ms: 1,
            state: TargetState::Enabled,
            last_run_ms: None,
            released_by: None,
            released_at_ms: None,
        };
        target::save(self.root(), ID, &t).unwrap();
    }

    fn scrub(&self, cmd: &mut Command) {
        cmd.env_clear();
        for name in PASS_THROUGH {
            if let Some(value) = std::env::var_os(name) {
                cmd.env(name, value);
            }
        }
        cmd.env("USERPROFILE", self.home.path())
            .env("SWITCHBOARD_STATUSLINE_DIR", self.root())
            .env("CLAUDE_PID", "4242");
    }

    /// The bridge, run the way Claude Code runs a status line, forced onto `shell`.
    fn bridge(&self, target_id: &str, shell: &Shell) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_switchboard-statusline"));
        cmd.args(["--target", target_id]);
        self.scrub(&mut cmd);
        cmd.env("SWITCHBOARD_TEST_SHELL", force_value(shell));
        cmd
    }

    /// `original` run directly in `shell`, in the same environment.
    fn direct(&self, shell: &Shell, original: &str) -> Command {
        let mut cmd = windows_command(&invocation(shell, original, false));
        self.scrub(&mut cmd);
        cmd
    }

    fn run(&self, shell: &Shell, stdin: &[u8]) -> Output {
        self.run_cmd(self.bridge(ID, shell), stdin)
    }

    fn run_cmd(&self, mut cmd: Command, stdin: &[u8]) -> Output {
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut pipe = child.stdin.take().unwrap();
        let bytes = stdin.to_vec();
        let writer = std::thread::spawn(move || {
            let _ = pipe.write_all(&bytes);
        });
        let out = child.wait_with_output().unwrap();
        writer.join().unwrap();
        out
    }

    fn observation(&self) -> Option<Observation> {
        let path: PathBuf = obs_dir(self.root()).join(format!("{SESSION}.4242.json"));
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.root().join("bridge.log")).unwrap_or_default()
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim_end().to_string()
}

#[test]
fn no_original_prints_nothing_exits_0_and_writes_an_observation() {
    for shell in shells() {
        let env = Env::new();
        env.install(None);
        let out = env.run(&shell, FIXTURE);
        let who = label(&shell);
        assert_eq!(out.status.code(), Some(0), "{who}");
        assert!(out.stdout.is_empty(), "{who}: stdout {:?}", text(&out.stdout));
        assert!(out.stderr.is_empty(), "{who}: stderr {:?}", text(&out.stderr));
        let obs = env.observation().unwrap_or_else(|| panic!("{who}: an observation file; log: {}", env.log()));
        assert_eq!(obs.claude_pid, 4242, "{who}");
        assert_eq!(obs.requests, 64, "{who}");
        assert_eq!(obs.rate_limits.unwrap().five_hour.unwrap().used_percentage, 62.0, "{who}");
        assert_eq!(obs.identity, statusline_bridge::format::Identity::None { reason: None }, "{who}");
    }
}

#[test]
fn original_stdout_passes_through_byte_exact() {
    for shell in shells() {
        let env = Env::new();
        let original = if is_bash(&shell) {
            "printf 'a\\nb'"
        } else {
            "[Console]::Out.Write(\"a`nb\")"
        };
        env.install(Some(original));
        let out = env.run(&shell, FIXTURE);
        assert_eq!(out.stdout, b"a\nb", "{}", label(&shell));
        assert_eq!(out.status.code(), Some(0), "{}", label(&shell));
    }
}

#[test]
fn original_that_prints_1mb_does_not_deadlock() {
    for shell in shells() {
        let env = Env::new();
        let original = if is_bash(&shell) {
            "head -c 1048576 /dev/zero | tr '\\0' 'x'"
        } else {
            "[Console]::Out.Write('x' * 1048576)"
        };
        env.install(Some(original));
        let started = Instant::now();
        let out = env.run(&shell, FIXTURE);
        assert_eq!(out.stdout.len(), 1_048_576, "{}", label(&shell));
        assert!(started.elapsed() < Duration::from_secs(30), "{}", label(&shell));
    }
}

#[test]
fn original_that_never_reads_stdin_does_not_stall_with_2mb_input() {
    for shell in shells() {
        let env = Env::new();
        env.install(Some("echo done"));
        // Valid JSON at the front, then 2 MB of padding after it.
        let mut stdin = FIXTURE.to_vec();
        stdin.extend(std::iter::repeat_n(b' ', 2 * 1024 * 1024));
        let who = label(&shell);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(env.run(&shell, &stdin));
        });
        // Shell startup and input handling can be slow on hosted Windows
        // runners. Bound the wait itself so a deadlock cannot hang the job.
        let out = rx.recv_timeout(Duration::from_secs(90)).unwrap_or_else(|e| panic!("{who}: bridge did not finish: {e}"));
        assert_eq!(text(&out.stdout), "done", "{who}");
    }
}

#[test]
fn original_receives_the_full_stdin() {
    for shell in shells() {
        let env = Env::new();
        let original = if is_bash(&shell) { "cat" } else { "[Console]::Out.Write([Console]::In.ReadToEnd())" };
        env.install(Some(original));
        // Larger than the 1 MB the bridge parses: the original still gets it all.
        let mut stdin = FIXTURE.to_vec();
        stdin.extend(std::iter::repeat_n(b'\n', 1_500_000));
        let out = env.run(&shell, &stdin);
        assert_eq!(out.stdout.len(), stdin.len(), "{}", label(&shell));
        assert_eq!(out.stdout, stdin, "{}", label(&shell));
    }
}

#[test]
fn original_exit_code_matches_running_it_directly_in_the_same_shell() {
    for shell in shells() {
        let originals: &[&str] = if is_bash(&shell) {
            &["exit 0", "exit 3", "false", "exit 255"]
        } else {
            &["exit 0", "exit 3", "cmd /c exit 3", "cmd /c exit 2"]
        };
        for original in originals {
            let env = Env::new();
            env.install(Some(original));
            let direct = env.run_cmd(env.direct(&shell, original), b"").status.code();
            let via = env.run(&shell, FIXTURE).status.code();
            assert_eq!(via, direct, "{}: {original:?}", label(&shell));
        }
    }
}

#[test]
fn stderr_is_inherited_and_the_depth_guard_is_set() {
    for shell in shells() {
        let env = Env::new();
        let original = if is_bash(&shell) {
            "echo oops >&2; printenv SWITCHBOARD_BRIDGE_DEPTH"
        } else {
            "[Console]::Error.Write(\"oops`n\"); [Console]::Out.Write($env:SWITCHBOARD_BRIDGE_DEPTH)"
        };
        env.install(Some(original));
        let out = env.run(&shell, FIXTURE);
        assert_eq!(text(&out.stderr), "oops", "{}", label(&shell));
        assert_eq!(text(&out.stdout), "1", "{}", label(&shell));
    }
}

#[test]
fn missing_target_prints_nothing_exits_0_and_logs() {
    for shell in shells() {
        let env = Env::new();
        let out = env.run_cmd(env.bridge("ffffffffffff", &shell), FIXTURE);
        assert_eq!(out.status.code(), Some(0), "{}", label(&shell));
        assert!(out.stdout.is_empty(), "{}", label(&shell));
        assert!(env.log().contains("ffffffffffff"), "{}: log: {}", label(&shell), env.log());
        assert!(env.observation().is_none(), "{}", label(&shell));
    }
}

#[test]
fn nested_invocation_exits_0_silently() {
    for shell in shells() {
        let env = Env::new();
        env.install(Some("echo should-not-run"));
        let mut cmd = env.bridge(ID, &shell);
        cmd.env("SWITCHBOARD_BRIDGE_DEPTH", "1");
        let out = env.run_cmd(cmd, FIXTURE);
        assert_eq!(out.status.code(), Some(0), "{}", label(&shell));
        assert!(out.stdout.is_empty(), "{}", label(&shell));
        assert!(env.observation().is_none(), "{}", label(&shell));
        assert!(env.log().contains("nested"), "{}", label(&shell));
    }
}

#[test]
fn last_seen_older_than_30_days_stops_observations_but_still_passes_through() {
    for shell in shells() {
        let env = Env::new();
        env.install(Some("echo still-works"));
        let last_seen = env.root().join("last_seen");
        std::fs::write(&last_seen, b"").unwrap();
        let old = std::time::SystemTime::now() - Duration::from_secs(31 * 24 * 3600);
        filetime::set_file_mtime(&last_seen, filetime::FileTime::from_system_time(old)).unwrap();
        let out = env.run(&shell, FIXTURE);
        assert_eq!(text(&out.stdout), "still-works", "{}", label(&shell));
        assert!(env.observation().is_none(), "{}: observations contain an email: they must stop", label(&shell));
    }
}

/// Review focus 4: awkward quoting in the user's original must reach the
/// shell as it would from Claude Code, so the bridge prints what the shell
/// prints when run directly. (Pinned against the shell, not against Bun's own
/// spawn quoting, which cannot run here.)
#[test]
fn hostile_quoting_matches_a_direct_run() {
    for shell in shells() {
        let originals: &[&str] = if is_bash(&shell) {
            &[
                r#"echo "a b" 'c d'"#,
                "echo $HOME",
                r#"echo 'back\slash' "q\"uote""#,
                "echo %PATH% !x",
                "echo naïve ✓",
                r#"printf '%s\n' "say \"hi\"""#,
            ]
        } else {
            &[
                r#"Write-Output "a b" 'c d'"#,
                "Write-Output $env:USERPROFILE",
                "Write-Output 'back\\slash' \"q`\"uote\"",
                "Write-Output '%PATH%' '!x'",
                "Write-Output 'naïve ✓'",
                r"Write-Output 'trailing\'",
            ]
        };
        for original in originals {
            let env = Env::new();
            env.install(Some(original));
            let direct = env.run_cmd(env.direct(&shell, original), b"");
            let via = env.run(&shell, FIXTURE);
            let who = format!("{}: {original:?}", label(&shell));
            assert_eq!(direct.status.code(), Some(0), "{who}: the original itself must work: {}", text(&direct.stderr));
            assert!(!direct.stdout.is_empty(), "{who}");
            assert_eq!(via.stdout, direct.stdout, "{who}");
            assert_eq!(via.status.code(), direct.status.code(), "{who}");
        }
    }
}

/// Found in review: quotes with no whitespace in the original. Claude Code
/// (Bun, libuv) wraps such an argument in quotes and escapes the inner ones;
/// Rust's std leaves it bare, and Git Bash then fails with an unterminated
/// quote. The bridge must hand the shell what Claude Code would.
#[test]
fn an_original_with_quotes_but_no_whitespace_runs_as_claude_code_would_run_it() {
    for shell in shells() {
        let originals: &[&str] = if is_bash(&shell) {
            &[r#""true""#, r#""exit""#]
        } else {
            &[r#""true""#, r#""ok""#]
        };
        for original in originals {
            let env = Env::new();
            env.install(Some(original));
            let out = env.run(&shell, FIXTURE);
            assert_eq!(
                out.status.code(),
                Some(0),
                "{}: {original:?}: {}",
                label(&shell),
                text(&out.stderr)
            );
        }
    }
}

/// The Job object: killing the bridge must end the original's whole tree. If a
/// grandchild survived, it would keep the inherited stdout pipe open and
/// `wait_with_output` would block until the 30 s sleep ended.
#[test]
fn killing_the_bridge_ends_a_long_original() {
    for shell in shells() {
        let env = Env::new();
        env.install(Some("sleep 30"));
        let mut child = env
            .bridge(ID, &shell)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(FIXTURE).unwrap();
        drop(stdin);
        // Long enough for the shell to start and `sleep` to begin.
        std::thread::sleep(Duration::from_millis(3000));
        child.kill().unwrap();
        let started = Instant::now();
        let _ = child.wait_with_output().unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{}: the original survived the bridge for {:?}",
            label(&shell),
            started.elapsed()
        );
    }
}
