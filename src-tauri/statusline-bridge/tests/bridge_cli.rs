//! Runs the real bridge binary against hostile originals. The guarantee under
//! test: whatever the observation code does, the user's status line output
//! and exit code come through unchanged (spec §4.2, §9).

#![cfg(unix)]

use statusline_bridge::format::{obs_dir, Observation};
use statusline_bridge::target::{self, Target, TargetState, TARGET_VERSION};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use tempfile::TempDir;

const ID: &str = "0123456789ab";
const SESSION: &str = "4b1e7c2a-0000-4000-8000-000000000001";
const FIXTURE: &[u8] = include_bytes!("fixtures/stdin_2_1_318.json");

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
            settings_path: "/unused/settings.json".into(),
            settings_real_path: "/unused/settings.json".into(),
            original: original.map(|c| serde_json::json!({"type": "command", "command": c})),
            original_value_text: None,
            file_was_created: false,
            installed_command: format!("bridge --target {ID}"),
            installed_at_ms: 1,
            state: TargetState::Enabled,
            last_run_ms: None,
            released_by: None,
            released_at_ms: None,
        };
        target::save(self.root(), ID, &t).unwrap();
    }

    fn command(&self, target_id: &str) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_switchboard-statusline"));
        cmd.args(["--target", target_id])
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap())
            .env("HOME", self.home.path())
            .env("SWITCHBOARD_STATUSLINE_DIR", self.root())
            .env("CLAUDE_PID", "4242");
        cmd
    }

    fn run(&self, stdin: &[u8]) -> Output {
        self.run_cmd(self.command(ID), stdin)
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

#[test]
fn no_original_prints_nothing_exits_0_and_writes_an_observation() {
    let env = Env::new();
    env.install(None);
    let out = env.run(FIXTURE);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty(), "stdout: {:?}", String::from_utf8_lossy(&out.stdout));
    assert!(out.stderr.is_empty(), "stderr: {:?}", String::from_utf8_lossy(&out.stderr));
    let obs = env.observation().expect("an observation file");
    assert_eq!(obs.claude_pid, 4242);
    assert_eq!(obs.requests, 64);
    assert_eq!(obs.rate_limits.unwrap().five_hour.unwrap().used_percentage, 62.0);
    // The fixture carries rate_limits on the first record: unconfirmed.
    assert!(!obs.confirmed);
    assert_eq!(obs.identity, statusline_bridge::format::Identity::None { reason: None });
}

#[test]
fn original_stdout_passes_through_byte_exact() {
    let env = Env::new();
    env.install(Some("printf 'a\\nb'"));
    let out = env.run(FIXTURE);
    assert_eq!(out.stdout, b"a\nb");
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn original_that_prints_1mb_does_not_deadlock() {
    let env = Env::new();
    env.install(Some("head -c 1048576 /dev/zero | tr '\\0' 'x'"));
    let started = Instant::now();
    let out = env.run(FIXTURE);
    assert_eq!(out.stdout.len(), 1_048_576);
    assert!(started.elapsed() < Duration::from_secs(20));
}

#[test]
fn original_that_never_reads_stdin_does_not_stall_with_2mb_input() {
    let env = Env::new();
    env.install(Some("echo done"));
    // Valid JSON at the front, then 2 MB of padding after it.
    let mut stdin = FIXTURE.to_vec();
    stdin.extend(std::iter::repeat_n(b' ', 2 * 1024 * 1024));
    let started = Instant::now();
    let out = env.run(&stdin);
    assert_eq!(out.stdout, b"done\n");
    assert!(started.elapsed() < Duration::from_secs(10), "stalled for {:?}", started.elapsed());
}

#[test]
fn original_cat_receives_the_full_stdin() {
    let env = Env::new();
    env.install(Some("cat"));
    // Larger than the 1 MB the bridge parses: the original still gets it all.
    let mut stdin = FIXTURE.to_vec();
    stdin.extend(std::iter::repeat_n(b'\n', 1_500_000));
    let out = env.run(&stdin);
    assert_eq!(out.stdout, stdin);
}

#[test]
fn original_exit_3_is_the_bridge_exit_code() {
    let env = Env::new();
    env.install(Some("exit 3"));
    assert_eq!(env.run(FIXTURE).status.code(), Some(3));
}

#[test]
fn original_stderr_is_inherited_and_the_depth_guard_is_set() {
    let env = Env::new();
    env.install(Some("echo oops >&2; printenv SWITCHBOARD_BRIDGE_DEPTH"));
    let out = env.run(FIXTURE);
    assert_eq!(out.stderr, b"oops\n");
    assert_eq!(out.stdout, b"1\n");
}

#[test]
fn a_signalled_original_maps_to_128_plus_signal() {
    let env = Env::new();
    env.install(Some("kill -TERM $$"));
    assert_eq!(env.run(FIXTURE).status.code(), Some(128 + 15));
}

#[test]
fn missing_target_prints_nothing_exits_0_and_logs() {
    let env = Env::new();
    let out = env.run_cmd(env.command("ffffffffffff"), FIXTURE);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert!(env.log().contains("ffffffffffff"), "log: {}", env.log());
    assert!(env.observation().is_none());
}

#[test]
fn a_hostile_target_id_is_refused_without_touching_the_filesystem() {
    let env = Env::new();
    let out = env.run_cmd(env.command("../../etc/passwd"), FIXTURE);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
}

#[test]
fn nested_invocation_exits_0_silently() {
    let env = Env::new();
    env.install(Some("echo should-not-run"));
    let mut cmd = env.command(ID);
    cmd.env("SWITCHBOARD_BRIDGE_DEPTH", "1");
    let out = env.run_cmd(cmd, FIXTURE);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert!(env.observation().is_none());
    assert!(env.log().contains("nested"));
}

#[test]
fn stdin_is_forwarded_even_when_it_is_not_json() {
    let env = Env::new();
    env.install(Some("cat"));
    let out = env.run(b"this is not json\n");
    assert_eq!(out.stdout, b"this is not json\n");
    assert_eq!(out.status.code(), Some(0));
    assert!(env.observation().is_none());
    assert!(env.log().contains("not JSON"), "log: {}", env.log());
}

#[test]
fn last_seen_older_than_30_days_stops_observations_but_still_passes_through() {
    let env = Env::new();
    env.install(Some("echo still-works"));
    let last_seen = env.root().join("last_seen");
    std::fs::write(&last_seen, b"").unwrap();
    let old = std::time::SystemTime::now() - Duration::from_secs(31 * 24 * 3600);
    filetime::set_file_mtime(&last_seen, filetime::FileTime::from_system_time(old)).unwrap();
    let out = env.run(FIXTURE);
    assert_eq!(out.stdout, b"still-works\n");
    assert!(env.observation().is_none(), "observations contain an email: they must stop");
}

#[test]
fn a_fresh_last_seen_keeps_observing() {
    let env = Env::new();
    env.install(Some("true"));
    std::fs::write(env.root().join("last_seen"), b"").unwrap();
    env.run(FIXTURE);
    assert!(env.observation().is_some());
}

#[test]
fn a_second_run_updates_the_same_file_and_keeps_state() {
    let env = Env::new();
    env.install(None);
    env.run(FIXTURE);
    let first = env.observation().unwrap();
    env.run(FIXTURE);
    let second = env.observation().unwrap();
    assert!(second.observed_at_ms >= first.observed_at_ms);
    assert_eq!(second.identity_since_ms, first.identity_since_ms);
}

#[cfg(feature = "inject-panic")]
#[test]
fn a_panic_in_observation_code_changes_neither_stdout_nor_exit_code() {
    let env = Env::new();
    env.install(Some("printf 'kept'; exit 5"));
    let mut cmd = env.command(ID);
    cmd.env("SWITCHBOARD_TEST_PANIC", "1");
    let out = env.run_cmd(cmd, FIXTURE);
    assert_eq!(out.stdout, b"kept");
    assert_eq!(out.status.code(), Some(5));
    assert!(out.stderr.is_empty(), "the panic message must not leak: {:?}", String::from_utf8_lossy(&out.stderr));
    assert!(env.log().contains("panicked"));
}

#[test]
#[ignore = "latency benchmark: run explicitly with --profile bridge"]
fn latency_p50_is_under_15ms_for_the_no_original_path() {
    let env = Env::new();
    env.install(None);
    let mut samples: Vec<Duration> = (0..50)
        .map(|_| {
            let started = Instant::now();
            env.run(FIXTURE);
            started.elapsed()
        })
        .collect();
    samples.sort();
    let p50 = samples[samples.len() / 2];
    eprintln!("p50 = {p50:?}, max = {:?}", samples.last().unwrap());
    assert!(p50 < Duration::from_millis(15), "p50 = {p50:?}");
}
