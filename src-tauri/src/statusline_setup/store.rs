//! The app's own copy of what it needs to undo an enable: the splice record
//! and a copy of the target file. It lives in the app data folder, so the
//! bridge's folder being deleted never loses the way back to the user's
//! original status line (spec §4.4: "the app database", kept here as a file
//! so it needs no schema migration).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use statusline_bridge::settings_edit::EnableRecord;
use statusline_bridge::target::Target;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoredState {
    Enabled,
    Released,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredTarget {
    pub id: String,
    /// Exactly what is in `targets/<id>.json`, so it can be rewritten.
    pub target: Target,
    pub record: EnableRecord,
    pub enabled_at_ms: i64,
    pub state: StoredState,
    /// Set by startup reconciliation when the bridge can't run.
    #[serde(default)]
    pub broken: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BridgeStore {
    pub targets: BTreeMap<String, StoredTarget>,
}

fn path(dir: &Path) -> PathBuf {
    dir.join("statusline_bridge.json")
}

pub fn load(dir: &Path) -> Result<BridgeStore> {
    match std::fs::read_to_string(path(dir)) {
        Ok(text) => serde_json::from_str(&text).context("parse statusline_bridge.json"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BridgeStore::default()),
        Err(e) => Err(e).context("read statusline_bridge.json"),
    }
}

pub fn save(dir: &Path, store: &BridgeStore) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let target = path(dir);
    let tmp = dir.join("statusline_bridge.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(store)?).context("write temp store")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp, &target).context("rename store into place")
}

#[cfg(test)]
mod tests {
    use super::*;
    use statusline_bridge::target::{TargetState, TARGET_VERSION};

    pub fn sample(id: &str) -> StoredTarget {
        StoredTarget {
            id: id.into(),
            target: Target {
                v: TARGET_VERSION,
                config_home: "/h/.claude".into(),
                settings_path: "/h/.claude/settings.json".into(),
                settings_real_path: "/h/.claude/settings.json".into(),
                original: None,
                original_value_text: None,
                file_was_created: false,
                installed_command: "'/b/bridge' --target 0123456789ab".into(),
                installed_at_ms: 1,
                state: TargetState::Enabled,
                last_run_ms: None,
                released_by: None,
                released_at_ms: None,
            },
            record: EnableRecord {
                old_text: String::new(),
                new_text: "x".into(),
                original_value_text: None,
                original_command: None,
                file_was_created: false,
                file_was_empty: false,
                installed_command: "'/b/bridge' --target 0123456789ab".into(),
            },
            enabled_at_ms: 1,
            state: StoredState::Enabled,
            broken: None,
        }
    }

    #[test]
    fn a_missing_file_is_an_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).unwrap().targets.is_empty());
    }

    #[test]
    fn target_row_round_trips_through_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = BridgeStore::default();
        store.targets.insert("0123456789ab".into(), sample("0123456789ab"));
        save(dir.path(), &store).unwrap();
        let back = load(dir.path()).unwrap();
        let row = &back.targets["0123456789ab"];
        assert_eq!(row.record.new_text, "x");
        assert_eq!(row.target.installed_command, "'/b/bridge' --target 0123456789ab");
        assert_eq!(row.state, StoredState::Enabled);
    }

    #[test]
    fn a_corrupt_store_is_an_error_not_silently_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("statusline_bridge.json"), "{ nope").unwrap();
        assert!(load(dir.path()).is_err(), "silently forgetting enabled targets would strand the user's settings");
    }
}
