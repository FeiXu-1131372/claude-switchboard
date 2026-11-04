//! The target file: everything the bridge needs to run the user's original
//! status line and to put it back, with no app installed (spec §4.4, §4.6).
//!
//! `targets/<id>.json` lives next to the bridge. `<id>` is the first 12 hex
//! characters of the SHA-256 of the **canonical** settings path, so every
//! spelling of one file (symlinks, junctions, `..`) maps to one target.

use crate::format::targets_dir;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const TARGET_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetState {
    /// `settings.json` points at the bridge.
    Enabled,
    /// The original was put back, or the user changed the command. Kept for
    /// a while because a running session may not have reloaded its settings.
    Released,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub v: u32,
    pub config_home: String,
    pub settings_path: String,
    pub settings_real_path: String,
    /// The user's original `statusLine` value, exactly as it was (`None` when
    /// there was none).
    pub original: Option<serde_json::Value>,
    /// The exact text of the original `statusLine` value, so `--restore` can
    /// put it back byte for byte without the app's database.
    #[serde(default)]
    pub original_value_text: Option<String>,
    /// Enabling created `settings.json` (it did not exist before).
    #[serde(default)]
    pub file_was_created: bool,
    pub installed_command: String,
    pub installed_at_ms: i64,
    pub state: TargetState,
    #[serde(default)]
    pub last_run_ms: Option<i64>,
    #[serde(default)]
    pub released_by: Option<String>,
    #[serde(default)]
    pub released_at_ms: Option<i64>,
}

impl Target {
    /// The command string of the original status line, if it has one.
    pub fn original_command(&self) -> Option<&str> {
        self.original
            .as_ref()?
            .get("command")?
            .as_str()
            .filter(|c| !c.trim().is_empty())
    }
}

/// An id that may name a file: exactly 12 lowercase hex characters. Checked
/// before any path is built from a command-line argument.
pub fn valid_id(id: &str) -> bool {
    id.len() == 12 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The canonical form of a settings path: symlinks resolved. For a file that
/// does not exist yet, the nearest existing ancestor is canonicalized and the
/// rest appended, so the id is stable before and after the file is created.
pub fn canonical_settings_path(path: &Path) -> io::Result<PathBuf> {
    if let Ok(real) = path.canonicalize() {
        return Ok(real);
    }
    let mut missing = Vec::new();
    let mut cursor = path;
    loop {
        match cursor.canonicalize() {
            Ok(mut real) => {
                for part in missing.iter().rev() {
                    real.push(part);
                }
                return Ok(real);
            }
            Err(_) => {
                let name = cursor
                    .file_name()
                    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no existing ancestor"))?;
                missing.push(name.to_owned());
                cursor = cursor
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no existing ancestor"))?;
            }
        }
    }
}

pub fn target_id(canonical_settings_path: &Path) -> String {
    let text = canonical_settings_path.to_string_lossy();
    // Windows paths are case-insensitive: fold, so two spellings share an id.
    #[cfg(windows)]
    let text = text.to_lowercase();
    let digest = Sha256::digest(text.as_bytes());
    digest.iter().take(6).map(|b| format!("{b:02x}")).collect()
}

fn target_path(root: &Path, id: &str) -> io::Result<PathBuf> {
    if !valid_id(id) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid target id"));
    }
    Ok(targets_dir(root).join(format!("{id}.json")))
}

pub fn load(root: &Path, id: &str) -> io::Result<Target> {
    let text = fs::read_to_string(target_path(root, id)?)?;
    serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn lock_dir(root: &Path) -> io::Result<fs::File> {
    let dir = targets_dir(root);
    create_private_dir(&dir)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(".lock"))?;
    // Two builds can share this folder; the single-instance plugin only
    // covers one app identifier.
    lock.lock_exclusive()?;
    Ok(lock)
}

fn create_private_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(dir)
    }
}

fn write_locked(root: &Path, id: &str, target: &Target) -> io::Result<()> {
    let path = target_path(root, id)?;
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let result = (|| {
        let mut file = opts.open(&tmp)?;
        file.write_all(serde_json::to_string_pretty(target)?.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, &path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

pub fn save(root: &Path, id: &str, target: &Target) -> io::Result<()> {
    let _lock = lock_dir(root)?;
    write_locked(root, id, target)
}

/// Record that the bridge ran, at most once a minute: the field only drives
/// garbage collection after days, and this runs on every status-line refresh.
pub fn touch_last_run(root: &Path, id: &str, now_ms: i64) -> io::Result<()> {
    let _lock = lock_dir(root)?;
    let mut target = load(root, id)?;
    if target.last_run_ms.is_some_and(|t| now_ms - t < 60_000 && now_ms >= t) {
        return Ok(());
    }
    target.last_run_ms = Some(now_ms);
    write_locked(root, id, &target)
}

/// Whether `cmd` runs the bridge, in any spelling (tilde, absolute, forward
/// or back slashes, quoted or not, with or without `.exe`, an 8.3 short
/// name), or names one of its targets with `--target <12 hex>` anywhere.
/// Enabling must never record such a command as the "original", or the
/// bridge would run itself on every refresh (spec §4.6).
pub fn is_bridge_command(cmd: &str, bridge_path: &Path) -> bool {
    let norm = |s: &str| -> String {
        s.replace('\\', "/")
            .chars()
            .filter(|c| !matches!(c, '\'' | '"'))
            .collect::<String>()
            .to_lowercase()
    };
    let cmd = norm(cmd);
    if has_target_flag(&cmd) {
        return true;
    }
    let bridge = norm(&bridge_path.to_string_lossy());
    let bridge = bridge.strip_suffix(".exe").unwrap_or(&bridge);
    // The tilde and absolute spellings differ only in the home prefix.
    let tail = ".claude-switchboard/statusline/bridge";
    cmd.contains(bridge)
        || cmd.contains(tail)
        // `C:\Users\XUE~1\CLAUDE~1\STATUSLINE\bridge.exe` style short names.
        || (cmd.contains("claude~") && cmd.contains("/statusline/bridge"))
}

fn has_target_flag(cmd: &str) -> bool {
    let mut rest = cmd;
    while let Some(i) = rest.find("--target") {
        let after = rest[i + "--target".len()..].trim_start_matches([' ', '=', '\t']);
        let id: String = after.chars().take(12).collect();
        let boundary = after.chars().nth(12).is_none_or(|c| !c.is_ascii_alphanumeric());
        if valid_id(&id) && boundary {
            return true;
        }
        rest = &rest[i + "--target".len()..];
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn bridge() -> PathBuf {
        PathBuf::from("/Users/u/.claude-switchboard/statusline/bridge")
    }

    fn sample() -> Target {
        Target {
            v: TARGET_VERSION,
            config_home: "/Users/u/.claude".into(),
            settings_path: "/Users/u/.claude/settings.json".into(),
            settings_real_path: "/Users/u/.claude/settings.json".into(),
            original: Some(serde_json::json!({"type": "command", "command": "bash ~/.claude/statusline.sh"})),
            original_value_text: Some("{\"type\": \"command\", \"command\": \"bash ~/.claude/statusline.sh\"}".into()),
            file_was_created: false,
            installed_command: "'/Users/u/.claude-switchboard/statusline/bridge' --target 0123456789ab".into(),
            installed_at_ms: 1_800_000_000_000,
            state: TargetState::Enabled,
            last_run_ms: None,
            released_by: None,
            released_at_ms: None,
        }
    }

    #[test]
    fn is_bridge_command_matches_tilde_absolute_backslash_quoted_and_extensionless_spellings() {
        let spellings = [
            "/Users/u/.claude-switchboard/statusline/bridge",
            "'/Users/u/.claude-switchboard/statusline/bridge'",
            "\"/Users/u/.claude-switchboard/statusline/bridge\" extra",
            "~/.claude-switchboard/statusline/bridge",
            "~/.claude-switchboard/statusline/bridge.exe",
            "C:\\Users\\u\\.claude-switchboard\\statusline\\bridge.exe",
            "C:/Users/u/.claude-switchboard/statusline/bridge.exe --restore",
            "C:\\Users\\U~1\\CLAUDE~1\\statusline\\bridge.exe",
            "/OTHER/.CLAUDE-SWITCHBOARD/STATUSLINE/BRIDGE",
            "echo hi; '/x/y/z' --target 0123456789ab",
        ];
        for s in spellings {
            assert!(is_bridge_command(s, &bridge()), "should match: {s}");
        }
    }

    #[test]
    fn is_bridge_command_matches_target_flag_anywhere_in_the_string() {
        assert!(is_bridge_command("some-wrapper --target 0123456789ab; echo", &bridge()));
        assert!(is_bridge_command("x --target=0123456789ab", &bridge()));
    }

    #[test]
    fn is_bridge_command_rejects_unrelated_commands() {
        let unrelated = [
            "bash ~/.claude/statusline.sh",
            "node /opt/statusline/bridge.js",
            "echo --target",
            "tool --target 0123456789",          // 10 hex: not an id
            "tool --target 0123456789abc",       // 13 chars: not an id
            "tool --target ZZZZZZZZZZZZ",
            "",
        ];
        for s in unrelated {
            assert!(!is_bridge_command(s, &bridge()), "should not match: {s}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn same_file_through_a_symlink_gets_one_id() {
        let dir = tempdir().unwrap();
        let real = dir.path().join("real");
        fs::create_dir(&real).unwrap();
        fs::write(real.join("settings.json"), "{}").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let a = canonical_settings_path(&real.join("settings.json")).unwrap();
        let b = canonical_settings_path(&link.join("settings.json")).unwrap();
        assert_eq!(a, b);
        assert_eq!(target_id(&a), target_id(&b));
    }

    #[test]
    fn id_is_stable_before_and_after_the_file_exists() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let before = target_id(&canonical_settings_path(&path).unwrap());
        fs::write(&path, "{}").unwrap();
        let after = target_id(&canonical_settings_path(&path).unwrap());
        assert_eq!(before, after);
        assert!(valid_id(&before));
    }

    #[test]
    fn a_missing_nested_directory_still_resolves() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("not").join("yet").join("settings.json");
        let resolved = canonical_settings_path(&path).unwrap();
        assert!(resolved.ends_with("not/yet/settings.json"));
    }

    #[test]
    fn save_then_load_round_trips() {
        let root = tempdir().unwrap();
        save(root.path(), "0123456789ab", &sample()).unwrap();
        assert_eq!(load(root.path(), "0123456789ab").unwrap(), sample());
    }

    #[cfg(unix)]
    #[test]
    fn target_files_are_0600() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempdir().unwrap();
        save(root.path(), "0123456789ab", &sample()).unwrap();
        let mode = fs::metadata(root.path().join("targets/0123456789ab.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn ids_that_could_escape_the_directory_are_refused() {
        let root = tempdir().unwrap();
        for bad in ["../../etc/passwd", "0123456789AB", "short", "0123456789abc", ""] {
            assert!(load(root.path(), bad).is_err(), "{bad:?}");
            assert!(save(root.path(), bad, &sample()).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn concurrent_saves_serialize_on_the_lock() {
        let root = tempdir().unwrap();
        save(root.path(), "0123456789ab", &sample()).unwrap();
        let path = root.path().to_path_buf();
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for j in 0..10 {
                        let mut t = sample();
                        t.last_run_ms = Some(i * 100 + j);
                        save(&path, "0123456789ab", &t).unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        // Whichever write won, the file is whole and parses.
        let t = load(&path, "0123456789ab").unwrap();
        assert!(t.last_run_ms.is_some());
        let leftovers = fs::read_dir(path.join("targets"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "tmp"))
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn touch_last_run_is_throttled_to_once_a_minute() {
        let root = tempdir().unwrap();
        save(root.path(), "0123456789ab", &sample()).unwrap();
        touch_last_run(root.path(), "0123456789ab", 1_000_000).unwrap();
        touch_last_run(root.path(), "0123456789ab", 1_030_000).unwrap();
        assert_eq!(load(root.path(), "0123456789ab").unwrap().last_run_ms, Some(1_000_000));
        touch_last_run(root.path(), "0123456789ab", 1_060_000).unwrap();
        assert_eq!(load(root.path(), "0123456789ab").unwrap().last_run_ms, Some(1_060_000));
    }

    #[test]
    fn original_command_ignores_blank_commands() {
        let mut t = sample();
        assert_eq!(t.original_command(), Some("bash ~/.claude/statusline.sh"));
        t.original = Some(serde_json::json!({"type": "command", "command": "   "}));
        assert_eq!(t.original_command(), None);
        t.original = None;
        assert_eq!(t.original_command(), None);
    }
}
