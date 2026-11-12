//! Warming up through Claude Code itself.
//!
//! A direct `POST /v1/messages` with a subscription token is sometimes refused
//! by the API: a bare HTTP 429 with no usage-limit headers, on an account that
//! reads 0% of its 5-hour window. Claude Code's own request is the one the API
//! expects for those tokens, so the warm-up asks Claude Code to send it:
//! `claude -p "hi"` on the cheapest model.
//!
//! The run is isolated. It happens in a throwaway empty config directory, so it
//! cannot read the user's settings, hooks or plugins and leaves no session in
//! their history. The token reaches Claude Code only through the child's
//! environment (`CLAUDE_CODE_OAUTH_TOKEN`), never a file, and anything in the
//! parent's environment that could sign it in as someone else or route it
//! elsewhere is removed.

use super::config::WARMUP_MODEL;
use super::WarmupOutcome;
use crate::child_process::run_with_timeout;
use anyhow::{anyhow, Result};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// How long Claude Code gets to answer. One tiny request, but the first launch
/// of a CLI can be slow (a scanner, a cold disk).
const CLI_TIMEOUT: Duration = Duration::from_secs(90);

/// How much of what Claude Code said goes into an error message.
const MAX_MESSAGE_CHARS: usize = 300;

/// Claude Code could not be started at all (as opposed to starting and
/// failing). The caller falls back to a direct request on this.
#[derive(Debug)]
pub(crate) struct CouldNotStart(pub String);

impl fmt::Display for CouldNotStart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "could not start Claude Code ({})", self.0)
    }
}

impl std::error::Error for CouldNotStart {}

/// One prompt, the cheapest model, one structured result. Not `--bare` (it
/// never reads OAuth credentials) and no `--tools ""` (an empty argument does
/// not survive an npm `.cmd` shim on Windows).
pub(crate) fn claude_args() -> Vec<String> {
    ["-p", "hi", "--model", WARMUP_MODEL, "--output-format", "json"]
        .into_iter()
        .map(String::from)
        .collect()
}

/// Whether an environment variable of ours must not reach Claude Code: anything
/// that sets who it is signed in as (`ANTHROPIC_*`, a token, a provider switch)
/// or that belongs to an enclosing Claude Code session.
fn should_scrub(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    if name == "CLAUDE_CODE_GIT_BASH_PATH" {
        // Where Claude Code finds Git Bash on Windows: needed to run at all.
        return false;
    }
    name.starts_with("ANTHROPIC_")
        || name.starts_with("CLAUDE_CODE_")
        || matches!(name.as_str(), "CLAUDE_CONFIG_DIR" | "CLAUDECODE" | "CLAUDE_PID")
}

/// npm installs `claude` as a `.cmd` shim that starts node, which starts the
/// native binary. Run the native binary directly when it sits beside the shim:
/// one process to supervise instead of three, and no `cmd.exe` argument rules.
pub(crate) fn resolve_native(program: &Path) -> PathBuf {
    let is_shim = program
        .extension()
        .is_some_and(|e| ["cmd", "bat", "ps1"].iter().any(|s| e.eq_ignore_ascii_case(s)));
    if is_shim {
        if let Some(dir) = program.parent() {
            let native = dir
                .join("node_modules")
                .join("@anthropic-ai")
                .join("claude-code")
                .join("bin")
                .join("claude.exe");
            if native.is_file() {
                return native;
            }
        }
    }
    program.to_path_buf()
}

/// A fresh empty directory, removed when dropped.
struct IsolatedDir(PathBuf);

impl IsolatedDir {
    fn create() -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "claude-switchboard-warmup-{}-{:08x}",
            std::process::id(),
            rand::random::<u32>()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(IsolatedDir(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for IsolatedDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn clip(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() > MAX_MESSAGE_CHARS {
        format!("{}…", text.chars().take(MAX_MESSAGE_CHARS).collect::<String>())
    } else {
        text.to_string()
    }
}

/// The result object `--output-format json` prints: the whole of stdout, or the
/// last line of it that is a JSON object (a warning can come first).
fn parse_result(stdout: &str) -> Option<serde_json::Value> {
    let as_object =
        |text: &str| serde_json::from_str::<serde_json::Value>(text.trim()).ok().filter(|v| v.is_object());
    as_object(stdout).or_else(|| stdout.lines().rev().find_map(as_object))
}

/// What a finished Claude Code run means for the warm-up, from what it printed.
///
/// Claude Code reports an API failure as a result with `is_error: true`, the
/// HTTP status in `api_error_status` (null when there was none, as for "Not
/// logged in") and a human message in `result`, and exits 1. Only a clean exit
/// with `is_error: false` counts as a started window.
pub(crate) fn classify_cli_result(code: Option<i32>, stdout: &str, stderr: &str) -> Result<WarmupOutcome> {
    let Some(result) = parse_result(stdout) else {
        let code_text = code.map_or("none".to_string(), |c| c.to_string());
        let tail = clip(stderr);
        return Err(anyhow!(
            "Claude Code printed no readable result (exit code {code_text}){}",
            if tail.is_empty() { String::new() } else { format!(": {tail}") }
        ));
    };
    let is_error = result["is_error"].as_bool().unwrap_or(true);
    if code == Some(0) && !is_error {
        return Ok(WarmupOutcome::Success);
    }

    let message = result["result"].as_str().unwrap_or("").trim().to_string();
    let lower = message.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| lower.contains(n));
    let usage_limit = has(&["you've hit your", "usage limit", "limit reached"]);
    let signed_out = has(&[
        "please run /login",
        "not logged in",
        "oauth token revoked",
        "oauth access token is invalid",
        "token has expired",
        "invalid api key",
    ]);
    let network = has(&[
        "unable to connect",
        "econnrefused",
        "econnreset",
        "enotfound",
        "etimedout",
        "fetch failed",
        "timed out",
        "network error",
        "socket hang up",
    ]);
    match result["api_error_status"].as_u64() {
        Some(401 | 403) => Ok(WarmupOutcome::NeedsReauth),
        Some(429) if usage_limit => Ok(WarmupOutcome::AtRateLimit),
        Some(429) => Ok(WarmupOutcome::Refused {
            detail: format!("HTTP 429, not a usage limit; Claude Code reported: {}", clip(&message)),
        }),
        Some(500..=599) => Ok(WarmupOutcome::AnthropicServerError),
        Some(status) => Err(anyhow!("Claude Code reported HTTP {status}: {}", clip(&message))),
        None if signed_out => Ok(WarmupOutcome::NeedsReauth),
        None if usage_limit => Ok(WarmupOutcome::AtRateLimit),
        None if network => Ok(WarmupOutcome::NetworkError),
        None => Err(anyhow!("Claude Code could not warm up this account: {}", clip(&message))),
    }
}

/// The warm-up: through the Claude Code at `program` when there is one that
/// starts, otherwise through `direct` (a plain request to the API).
///
/// Only a Claude Code that could not be found or started falls back. One that
/// ran and failed has answered, and that answer is what the person sees: the
/// direct request is the one the API sometimes refuses, so sending it after a
/// failure would only bury the reason.
pub(crate) async fn warm_up_or_fall_back<F, Fut>(
    program: Option<&Path>,
    token: &str,
    direct: F,
) -> Result<WarmupOutcome>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<WarmupOutcome>>,
{
    warm_up_or_fall_back_with(program, token, &[], CLI_TIMEOUT, direct).await
}

async fn warm_up_or_fall_back_with<F, Fut>(
    program: Option<&Path>,
    token: &str,
    extra_env: &[(String, String)],
    timeout: Duration,
    direct: F,
) -> Result<WarmupOutcome>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<WarmupOutcome>>,
{
    match program {
        Some(program) => match warm_up_with(program, token, extra_env, timeout).await {
            Err(e) if e.downcast_ref::<CouldNotStart>().is_some() => {
                tracing::warn!(target: "switchboard.warmup", "{e:#}; warming up with a direct request instead");
            }
            answered => return answered,
        },
        None => {
            tracing::info!(target: "switchboard.warmup", "Claude Code was not found; warming up with a direct request");
        }
    }
    direct().await
}

/// [`warm_up`] with extra environment for the child and a chosen timeout (the
/// tests use both).
async fn warm_up_with(
    program: &Path,
    token: &str,
    extra_env: &[(String, String)],
    timeout: Duration,
) -> Result<WarmupOutcome> {
    let program = resolve_native(program);
    let token = token.to_owned();
    let extra_env = extra_env.to_vec();
    tokio::task::spawn_blocking(move || run_blocking(&program, &token, &extra_env, timeout))
        .await
        .map_err(|e| anyhow!("the Claude Code warm-up task failed: {e}"))?
}

fn run_blocking(
    program: &Path,
    token: &str,
    extra_env: &[(String, String)],
    timeout: Duration,
) -> Result<WarmupOutcome> {
    let dir = IsolatedDir::create().map_err(|e| anyhow!("could not make a scratch folder for Claude Code: {e}"))?;
    let mut cmd = Command::new(program);
    cmd.args(claude_args()).current_dir(dir.path());
    for (name, _) in std::env::vars_os() {
        if should_scrub(&name.to_string_lossy()) {
            cmd.env_remove(&name);
        }
    }
    cmd.env("CLAUDE_CODE_OAUTH_TOKEN", token)
        .env("CLAUDE_CONFIG_DIR", dir.path())
        .env("DISABLE_AUTOUPDATER", "1")
        // A persistent refusal should fail in seconds, not after ten backoffs.
        .env("CLAUDE_CODE_MAX_RETRIES", "2");
    for (name, value) in extra_env {
        cmd.env(name, value);
    }
    tracing::info!(target: "switchboard.warmup", "warming up through Claude Code ({})", program.display());
    let out = run_with_timeout(cmd, b"", timeout)
        .map_err(|e| anyhow::Error::new(CouldNotStart(format!("{}: {e}", program.display()))))?;
    let Some(code) = out.code else {
        return Err(anyhow!("Claude Code did not finish within {} seconds", timeout.as_secs()));
    };
    tracing::info!(target: "switchboard.warmup", "Claude Code finished: exit {code} in {} ms", out.elapsed.as_millis());
    classify_cli_result(Some(code), &String::from_utf8_lossy(&out.stdout), &String::from_utf8_lossy(&out.stderr))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    // ---- classification: the shapes `claude -p --output-format json` prints ----
    // (captured from Claude Code 2.1.322 with no credentials and with a bogus token)

    fn classify(code: i32, stdout: &str) -> Result<WarmupOutcome> {
        classify_cli_result(Some(code), stdout, "")
    }

    fn result_json(is_error: bool, status: Option<u16>, message: &str) -> String {
        serde_json::json!({
            "type": "result", "subtype": "success", "is_error": is_error,
            "api_error_status": status, "result": message,
        })
        .to_string()
    }

    #[test]
    fn a_clean_result_is_a_started_window() {
        assert_eq!(classify(0, &result_json(false, None, "Hi!")).unwrap(), WarmupOutcome::Success);
    }

    #[test]
    fn not_logged_in_and_bad_tokens_need_reauth() {
        let not_logged_in = result_json(true, None, "Not logged in · Please run /login");
        assert_eq!(classify(1, &not_logged_in).unwrap(), WarmupOutcome::NeedsReauth);
        let bogus = result_json(true, Some(401), "Failed to authenticate. API Error: 401 OAuth access token is invalid.");
        assert_eq!(classify(1, &bogus).unwrap(), WarmupOutcome::NeedsReauth);
        assert_eq!(classify(1, &result_json(true, Some(403), "forbidden")).unwrap(), WarmupOutcome::NeedsReauth);
        let revoked = result_json(true, None, "OAuth token revoked · Please run /login");
        assert_eq!(classify(1, &revoked).unwrap(), WarmupOutcome::NeedsReauth);
    }

    #[test]
    fn a_real_usage_limit_is_a_rate_limit() {
        let limit = result_json(true, Some(429), "You've hit your usage limit · resets 5pm (Pacific/Auckland)");
        assert_eq!(classify(1, &limit).unwrap(), WarmupOutcome::AtRateLimit);
    }

    /// The same bare 429 the direct request got: not a usage limit, and the
    /// person gets what Claude Code said.
    #[test]
    fn a_bare_429_is_a_refusal_and_says_what_claude_code_reported() {
        let bare = result_json(
            true,
            Some(429),
            r#"API Error: 429 {"type":"error","error":{"type":"rate_limit_error","message":"Error"}}"#,
        );
        match classify(1, &bare).unwrap() {
            WarmupOutcome::Refused { detail } => {
                assert!(detail.contains("429"), "{detail}");
                assert!(detail.contains("rate_limit_error"), "{detail}");
                assert!(detail.contains("Claude Code"), "{detail}");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn server_and_network_trouble_are_not_blamed_on_the_account() {
        for status in [500, 502, 503, 529] {
            assert_eq!(
                classify(1, &result_json(true, Some(status), "overloaded")).unwrap(),
                WarmupOutcome::AnthropicServerError,
                "{status}"
            );
        }
        for text in ["Unable to connect to API (ConnectionRefused)", "fetch failed: ENOTFOUND api.anthropic.com", "request timed out (ETIMEDOUT)"] {
            assert_eq!(classify(1, &result_json(true, None, text)).unwrap(), WarmupOutcome::NetworkError, "{text}");
        }
    }

    #[test]
    fn anything_else_is_an_error_that_carries_the_message() {
        let err = classify(1, &result_json(true, Some(400), "model: claude-haiku-9 not found")).unwrap_err().to_string();
        assert!(err.contains("not found") && err.contains("400"), "{err}");
        let err = classify(1, &result_json(true, None, "Something odd happened")).unwrap_err().to_string();
        assert!(err.contains("Something odd happened"), "{err}");
    }

    #[test]
    fn no_result_at_all_is_an_error_with_the_exit_code_and_stderr_never_a_success() {
        let err = classify_cli_result(Some(1), "", "node: command not found").unwrap_err().to_string();
        assert!(err.contains("exit code 1") && err.contains("node: command not found"), "{err}");
        // Exit 0 with nothing we can read is not evidence that a window started.
        assert!(classify(0, "ok").is_err());
        assert!(classify(0, "").is_err());
    }

    #[test]
    fn the_result_is_found_after_noise_and_a_long_message_is_cut() {
        let noisy = format!("warning: update available\n{}\n", result_json(false, None, "Hi"));
        assert_eq!(classify(0, &noisy).unwrap(), WarmupOutcome::Success);
        let long = "x".repeat(2000);
        let err = classify(1, &result_json(true, None, &long)).unwrap_err().to_string();
        assert!(err.chars().count() < 500, "{} chars", err.chars().count());
    }

    // ---- what is handed to Claude Code ----

    #[test]
    fn the_request_is_one_small_haiku_prompt_with_structured_output() {
        assert_eq!(
            claude_args(),
            ["-p", "hi", "--model", WARMUP_MODEL, "--output-format", "json"]
        );
    }

    #[test]
    fn nothing_that_could_change_who_claude_code_is_signed_in_as_survives() {
        for name in [
            "ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL", "anthropic_model",
            "CLAUDE_CODE_OAUTH_TOKEN", "CLAUDE_CODE_USE_BEDROCK", "CLAUDE_CODE_USE_VERTEX",
            "CLAUDE_CODE_SESSION_ID", "CLAUDE_CONFIG_DIR", "CLAUDECODE", "CLAUDE_PID",
        ] {
            assert!(should_scrub(name), "{name} must not reach the child");
        }
        for name in ["PATH", "SystemRoot", "HOME", "USERPROFILE", "CLAUDE_CODE_GIT_BASH_PATH", "TEMP"] {
            assert!(!should_scrub(name), "{name} is needed to run");
        }
    }

    #[test]
    fn an_npm_shim_is_swapped_for_the_native_binary_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        let shim = dir.path().join("claude.cmd");
        std::fs::write(&shim, "@echo off").unwrap();
        // No native binary next to it: run the shim as it is.
        assert_eq!(resolve_native(&shim), shim);
        let bin = dir.path().join("node_modules/@anthropic-ai/claude-code/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let native = bin.join("claude.exe");
        std::fs::write(&native, "x").unwrap();
        assert_eq!(resolve_native(&shim), native);
        // A binary that is not a shim is never swapped.
        let exe = dir.path().join("claude.exe");
        std::fs::write(&exe, "x").unwrap();
        assert_eq!(resolve_native(&exe), exe);
    }

    // ---- a real child process, with a stub standing in for `claude` ----

    /// A tiny `claude` whose behaviour is chosen by `STUB_MODE`. Built with
    /// rustc once per run, because an executable cannot be faked with a script.
    fn stub() -> &'static Path {
        static BUILT: OnceLock<PathBuf> = OnceLock::new();
        BUILT.get_or_init(|| {
            let dir = std::env::temp_dir().join(format!("claude-switchboard-stub-claude-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let source = dir.join("stub.rs");
            std::fs::write(&source, STUB_SOURCE).unwrap();
            let exe = dir.join(if cfg!(windows) { "claude.exe" } else { "claude" });
            let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
            let status = Command::new(rustc)
                .args(["--edition", "2021", "-O"])
                .arg(&source)
                .arg("-o")
                .arg(&exe)
                .status()
                .expect("rustc runs");
            assert!(status.success(), "building the stub");
            exe
        })
    }

    const STUB_SOURCE: &str = r##"
use std::time::Duration;
fn main() {
    match std::env::var("STUB_MODE").unwrap_or_default().as_str() {
        "ok" => println!(r#"{{"type":"result","is_error":false,"result":"hi"}}"#),
        "inspect" => {
            let dir = std::env::var("CLAUDE_CONFIG_DIR").unwrap_or_default();
            let seen = format!(
                "dir={dir}|cwd={}|token={}|leak={}|empty={}|retries={}|args={}",
                std::env::current_dir().unwrap().display(),
                std::env::var("CLAUDE_CODE_OAUTH_TOKEN").unwrap_or_default(),
                std::env::var("ANTHROPIC_SWITCHBOARD_TEST_LEAK").is_ok(),
                std::fs::read_dir(&dir).map(|d| d.count() == 0).unwrap_or(false),
                std::env::var("CLAUDE_CODE_MAX_RETRIES").unwrap_or_default(),
                std::env::args().skip(1).collect::<Vec<_>>().join(" "),
            );
            println!("{{\"is_error\":true,\"api_error_status\":null,\"result\":{:?}}}", format!("SEEN {seen}"));
            std::process::exit(1);
        }
        "hang" => std::thread::sleep(Duration::from_secs(120)),
        "garbage" => println!("not json"),
        _ => std::process::exit(3),
    }
}
"##;

    async fn run_stub(mode: &str, timeout: Duration) -> Result<WarmupOutcome> {
        warm_up_with(stub(), "tok-123", &[("STUB_MODE".into(), mode.into())], timeout).await
    }

    #[tokio::test]
    async fn a_successful_run_starts_the_window() {
        assert_eq!(run_stub("ok", Duration::from_secs(60)).await.unwrap(), WarmupOutcome::Success);
    }

    /// The token reaches Claude Code only through its environment; the run
    /// happens in a throwaway empty config dir (so it can neither read the
    /// user's settings and hooks nor leave a session in their history); nothing
    /// that routes or authenticates differently leaks in; and the dir is gone
    /// afterwards.
    #[tokio::test]
    async fn the_run_is_isolated_and_cleans_up_after_itself() {
        // SAFETY: a name no other test touches; set before the child is spawned.
        unsafe { std::env::set_var("ANTHROPIC_SWITCHBOARD_TEST_LEAK", "secret") };
        let err = run_stub("inspect", Duration::from_secs(60)).await.unwrap_err().to_string();
        unsafe { std::env::remove_var("ANTHROPIC_SWITCHBOARD_TEST_LEAK") };
        let seen = err.split("SEEN ").nth(1).expect(&err).to_string();
        let field = |name: &str| seen.split('|').find_map(|p| p.strip_prefix(&format!("{name}="))).unwrap_or("").to_string();
        assert_eq!(field("token"), "tok-123");
        assert_eq!(field("leak"), "false", "ANTHROPIC_* must be scrubbed: {seen}");
        assert_eq!(field("empty"), "true", "a fresh, empty config dir: {seen}");
        assert_eq!(field("retries"), "2", "fail fast on a persistent refusal: {seen}");
        assert_eq!(field("cwd"), field("dir"), "it runs inside its own dir: {seen}");
        assert!(field("args").starts_with("-p hi --model"), "{seen}");
        let dir = PathBuf::from(field("dir"));
        assert!(dir.starts_with(std::env::temp_dir()), "{}", dir.display());
        assert!(!dir.exists(), "the throwaway config dir must be removed: {}", dir.display());
    }

    #[tokio::test]
    async fn a_run_that_never_answers_is_stopped_at_the_timeout() {
        let started = std::time::Instant::now();
        let err = run_stub("hang", Duration::from_secs(2)).await.unwrap_err().to_string();
        assert!(err.contains("did not finish"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(20), "{:?}", started.elapsed());
    }

    #[tokio::test]
    async fn unreadable_output_is_an_error_even_with_exit_zero() {
        assert!(run_stub("garbage", Duration::from_secs(60)).await.is_err());
    }

    // ---- choosing between Claude Code and a direct request ----

    /// A direct request that must not be sent.
    async fn must_not_send() -> Result<WarmupOutcome> {
        panic!("the direct request was sent although Claude Code answered")
    }

    async fn with_stub(mode: &str) -> Result<WarmupOutcome> {
        warm_up_or_fall_back_with(
            Some(stub()),
            "tok",
            &[("STUB_MODE".into(), mode.into())],
            Duration::from_secs(60),
            must_not_send,
        )
        .await
    }

    #[tokio::test]
    async fn claude_code_is_used_when_it_runs_and_the_direct_request_is_not_sent() {
        assert_eq!(with_stub("ok").await.unwrap(), WarmupOutcome::Success);
    }

    /// If Claude Code ran and failed, that failure is the answer: trying the
    /// request the API refuses would only hide it.
    #[tokio::test]
    async fn a_claude_code_that_ran_and_failed_is_reported_not_papered_over() {
        let err = with_stub("garbage").await.unwrap_err().to_string();
        assert!(err.contains("no readable result"), "{err}");
    }

    #[tokio::test]
    async fn without_claude_code_the_direct_request_is_sent() {
        let outcome = warm_up_or_fall_back_with(None, "tok", &[], Duration::from_secs(5), || async {
            Ok(WarmupOutcome::AtRateLimit)
        })
        .await
        .unwrap();
        assert_eq!(outcome, WarmupOutcome::AtRateLimit, "the direct path's own answer");
    }

    #[tokio::test]
    async fn a_claude_code_that_cannot_start_falls_back_to_the_direct_request() {
        let outcome = warm_up_or_fall_back_with(
            Some(Path::new("definitely-not-claude-xyz")),
            "tok",
            &[],
            Duration::from_secs(5),
            || async { Ok(WarmupOutcome::Success) },
        )
        .await
        .unwrap();
        assert_eq!(outcome, WarmupOutcome::Success);
    }

    /// Manual check against the Claude Code installed on this machine, with no
    /// real credentials: a made-up token must come back as "sign in again", which
    /// proves discovery, the npm-shim handling, isolation and the real output
    /// shape together. Sends one request with the fake token to the API.
    /// Run with: cargo test real_claude_code -- --ignored
    #[tokio::test]
    #[ignore = "uses the installed Claude Code and the network"]
    async fn real_claude_code_reports_a_made_up_token_as_needs_reauth() {
        let program = crate::providers::launcher::resolve_claude_binary().expect("Claude Code is installed");
        println!("claude resolves to {}; native: {}", program.display(), resolve_native(&program).display());
        let started = std::time::Instant::now();
        let outcome = warm_up_with(
            &program,
            "sk-ant-oat01-this-is-not-a-real-token-0000000000000000000000000000",
            &[],
            CLI_TIMEOUT,
        )
        .await;
        println!("outcome {outcome:?} after {:?}", started.elapsed());
        assert_eq!(outcome.unwrap(), WarmupOutcome::NeedsReauth);
    }

    /// A Claude Code that cannot even start must be told apart from one that ran
    /// and failed, so the caller can fall back to a direct request.
    #[tokio::test]
    async fn a_program_that_cannot_start_is_reported_as_such() {
        let err = warm_up_with(Path::new("definitely-not-claude-xyz"), "tok", &[], Duration::from_secs(5))
            .await
            .unwrap_err();
        assert!(err.downcast_ref::<CouldNotStart>().is_some(), "{err:#}");
    }
}
