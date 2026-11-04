//! The observation file the bridge writes and the app reads (spec §5.1).
//!
//! One file per Claude Code **process**, `obs/<session_id>.<claude_pid>.json`.
//! Reading is tolerant (unknown fields are ignored, optional fields default),
//! because the app and the bridge can be different versions on one machine.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The only observation version the app adopts.
pub const OBS_VERSION: u32 = 1;

/// Which account an observation belongs to (spec §3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Identity {
    /// The config dir's `oauthAccount`.
    Oauth {
        account_uuid: String,
        email: String,
        #[serde(default)]
        organization_uuid: Option<String>,
    },
    /// No identity: a disk-stored long-lived token has a null `oauthAccount`
    /// (`reason: None`); routed or otherwise unattributable auth carries a
    /// `reason` and never matches an account.
    None {
        #[serde(default)]
        reason: Option<String>,
    },
}

/// One rate-limit window as Claude Code reports it on the status line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Window {
    /// 0–100, the same scale as `/usage` utilization.
    pub used_percentage: f64,
    /// Epoch seconds.
    pub resets_at: i64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RateLimits {
    #[serde(default)]
    pub five_hour: Option<Window>,
    #[serde(default)]
    pub seven_day: Option<Window>,
}

impl RateLimits {
    pub fn is_empty(&self) -> bool {
        self.five_hour.is_none() && self.seven_day.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub v: u32,
    pub session_id: String,
    pub claude_pid: u32,
    pub config_home: String,
    pub identity: Identity,
    /// When the bridge last ran for this process (a heartbeat).
    pub observed_at_ms: i64,
    /// `cost.total_api_duration_ms`.
    #[serde(default)]
    pub api_ms: i64,
    /// `prompt_cache.requests`, 0 when absent.
    #[serde(default)]
    pub requests: i64,
    /// When the rate-limit data arrived, not when the status line last ran.
    pub data_at_ms: i64,
    #[serde(default)]
    pub no_data_reason: Option<String>,
    #[serde(default)]
    pub rate_limits: Option<RateLimits>,
    /// The app only adopts confirmed observations (spec §5.3).
    #[serde(default)]
    pub confirmed: bool,
    #[serde(default)]
    pub identity_since_ms: i64,
    #[serde(default)]
    pub baseline_api_ms: Option<i64>,
    #[serde(default)]
    pub baseline_requests: Option<i64>,
    #[serde(default)]
    pub growths_since_baseline: u32,
    #[serde(default)]
    pub pre_switch_rate_limits: Option<RateLimits>,
    #[serde(default)]
    pub cc_version: Option<String>,
}

/// File name for a process's observation. A `session_id` outside
/// `[A-Za-z0-9-]{1,64}` is replaced by the first 32 hex chars of its SHA-256,
/// so a hostile id can never escape the directory. A missing pid is `0`.
pub fn obs_file_name(session_id: &str, claude_pid: Option<u32>) -> String {
    let safe = !session_id.is_empty()
        && session_id.len() <= 64
        && session_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-');
    let stem = if safe {
        session_id.to_string()
    } else {
        let digest = Sha256::digest(session_id.as_bytes());
        digest.iter().take(16).map(|b| format!("{b:02x}")).collect()
    };
    format!("{stem}.{}.json", claude_pid.unwrap_or(0))
}

/// `~/.claude-switchboard/statusline`, or `$SWITCHBOARD_STATUSLINE_DIR`.
pub fn bridge_root() -> PathBuf {
    bridge_root_from(
        std::env::var_os("SWITCHBOARD_STATUSLINE_DIR").as_deref(),
        home_dir().as_deref(),
    )
}

pub fn bridge_root_from(env_override: Option<&OsStr>, home: Option<&Path>) -> PathBuf {
    if let Some(dir) = env_override.filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    home.unwrap_or_else(|| Path::new("/"))
        .join(".claude-switchboard")
        .join("statusline")
}

pub fn obs_dir(root: &Path) -> PathBuf {
    root.join("obs")
}

pub fn targets_dir(root: &Path) -> PathBuf {
    root.join("targets")
}

#[cfg(unix)]
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(windows)]
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC_EXAMPLE: &str = r#"{
      "v": 1,
      "session_id": "4b1e",
      "claude_pid": 23140,
      "config_home": "C:/Users/xue/.claude",
      "identity": { "kind": "oauth", "account_uuid": "u-1", "email": "a@b.c", "organization_uuid": "o-1" },
      "observed_at_ms": 1761580000123,
      "api_ms": 182345,
      "requests": 41,
      "data_at_ms": 1761579998000,
      "no_data_reason": null,
      "rate_limits": {
        "five_hour": { "used_percentage": 41.3, "resets_at": 1761591600 },
        "seven_day": { "used_percentage": 12.0, "resets_at": 1762023600 }
      },
      "confirmed": true,
      "identity_since_ms": 1761570000000,
      "baseline_api_ms": 120000,
      "baseline_requests": 30,
      "growths_since_baseline": 2,
      "pre_switch_rate_limits": null,
      "cc_version": "2.1.311"
    }"#;

    #[test]
    fn observation_round_trips_the_spec_example() {
        let obs: Observation = serde_json::from_str(SPEC_EXAMPLE).unwrap();
        let back = serde_json::to_value(&obs).unwrap();
        let original: serde_json::Value = serde_json::from_str(SPEC_EXAMPLE).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let mut v: serde_json::Value = serde_json::from_str(SPEC_EXAMPLE).unwrap();
        v["from_the_future"] = serde_json::json!({"x": 1});
        v["identity"]["extra"] = serde_json::json!(true);
        let obs: Observation = serde_json::from_value(v).unwrap();
        assert_eq!(obs.session_id, "4b1e");
    }

    #[test]
    fn identity_none_parses_with_and_without_reason() {
        let a: Identity = serde_json::from_str(r#"{"kind":"none"}"#).unwrap();
        assert_eq!(a, Identity::None { reason: None });
        let b: Identity = serde_json::from_str(r#"{"kind":"none","reason":"routed"}"#).unwrap();
        assert_eq!(b, Identity::None { reason: Some("routed".into()) });
    }

    #[test]
    fn obs_file_name_keeps_safe_ids() {
        assert_eq!(obs_file_name("4b1e-aa", Some(23140)), "4b1e-aa.23140.json");
    }

    #[test]
    fn obs_file_name_hashes_unsafe_ids() {
        for bad in ["../etc/passwd", "a/b", &"x".repeat(65), ""] {
            let name = obs_file_name(bad, Some(1));
            let stem = name.strip_suffix(".1.json").unwrap();
            assert_eq!(stem.len(), 32, "{bad:?} -> {name}");
            assert!(stem.bytes().all(|b| b.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn missing_pid_is_zero() {
        assert_eq!(obs_file_name("abc", None), "abc.0.json");
    }

    #[test]
    fn bridge_root_honours_env_override() {
        let got = bridge_root_from(Some(OsStr::new("/tmp/x")), Some(Path::new("/home/u")));
        assert_eq!(got, PathBuf::from("/tmp/x"));
        let default = bridge_root_from(None, Some(Path::new("/home/u")));
        assert_eq!(default, PathBuf::from("/home/u/.claude-switchboard/statusline"));
    }
}
