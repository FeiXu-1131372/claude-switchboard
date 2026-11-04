//! `bridge --restore`: put the user's original status line back using only
//! the target files (spec §4.4). Needs no app and no database, so it keeps
//! working after Switchboard is uninstalled.

use crate::format::targets_dir;
use crate::settings_edit::{self, DisableOutcome};
use crate::target::{self, valid_id, TargetState};
use std::path::Path;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn all_ids(root: &Path) -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(targets_dir(root))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let id = name.strip_suffix(".json")?.to_string();
            valid_id(&id).then_some(id)
        })
        .collect();
    ids.sort();
    ids
}

/// Restore one target. Returns a one-line report, `Err` when it failed.
pub fn restore_one(root: &Path, id: &str, by: &str) -> Result<String, String> {
    let mut t = target::load(root, id).map_err(|e| format!("{id}: can't read the target: {e}"))?;
    if t.state == TargetState::Released {
        return Ok(format!("{id}: already released"));
    }
    let outcome = settings_edit::restore(&t).map_err(|e| format!("{id}: {e}"))?;
    t.state = TargetState::Released;
    t.released_by = Some(by.to_string());
    t.released_at_ms = Some(now_ms());
    target::save(root, id, &t).map_err(|e| format!("{id}: restored, but couldn't record it: {e}"))?;
    Ok(match outcome {
        DisableOutcome::Restored => format!("{id}: restored {}", t.settings_path),
        DisableOutcome::ReleasedUserChanged => {
            format!("{id}: {} was changed since, so it was left alone", t.settings_path)
        }
    })
}

pub fn run(root: &Path, target: Option<&str>, all: bool, by: Option<&str>) -> i32 {
    let by = by.unwrap_or("user");
    let ids: Vec<String> = match (target, all) {
        (Some(id), _) if valid_id(id) => vec![id.to_string()],
        (Some(id), _) => {
            eprintln!("switchboard-statusline: {id:?} is not a target id");
            return 2;
        }
        (None, true) => all_ids(root),
        (None, false) => {
            eprintln!("switchboard-statusline: --restore needs --target <id> or --all");
            return 2;
        }
    };
    if ids.is_empty() {
        println!("Nothing to restore.");
        return 0;
    }
    let mut failed = false;
    for id in ids {
        match restore_one(root, &id, by) {
            Ok(line) => println!("{line}"),
            Err(line) => {
                eprintln!("{line}");
                crate::run::log(root, &format!("restore failed: {line}"));
                failed = true;
            }
        }
    }
    i32::from(failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::target::{Target, TARGET_VERSION};
    use tempfile::tempdir;

    const ID: &str = "0123456789ab";
    const CMD: &str = "'/Users/u/.claude-switchboard/statusline/bridge' --target 0123456789ab";

    fn setup(root: &Path, settings: &Path, original: &str) -> Target {
        std::fs::write(
            settings,
            format!(r#"{{"a":1,"statusLine":{{"type":"command","command":{}}}}}"#, serde_json::to_string(CMD).unwrap()),
        )
        .unwrap();
        let t = Target {
            v: TARGET_VERSION,
            config_home: "/h".into(),
            settings_path: settings.to_string_lossy().into(),
            settings_real_path: settings.to_string_lossy().into(),
            original: serde_json::from_str(original).ok(),
            original_value_text: Some(original.into()),
            file_was_created: false,
            installed_command: CMD.into(),
            installed_at_ms: 1,
            state: TargetState::Enabled,
            last_run_ms: None,
            released_by: None,
            released_at_ms: None,
        };
        target::save(root, ID, &t).unwrap();
        t
    }

    #[test]
    fn restore_all_puts_the_original_back_and_marks_the_target_released() {
        let root = tempdir().unwrap();
        let dir = tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        setup(root.path(), &settings, r#"{"type":"command","command":"bash ~/.claude/statusline.sh"}"#);

        assert_eq!(run(root.path(), None, true, Some("uninstaller")), 0);
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(v["statusLine"]["command"], "bash ~/.claude/statusline.sh");
        let t = target::load(root.path(), ID).unwrap();
        assert_eq!(t.state, TargetState::Released);
        assert_eq!(t.released_by.as_deref(), Some("uninstaller"));
        assert!(t.released_at_ms.is_some());
        // The original is kept: a running session may still call the bridge.
        assert!(t.original_value_text.is_some());
    }

    #[test]
    fn restoring_twice_is_harmless() {
        let root = tempdir().unwrap();
        let dir = tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        setup(root.path(), &settings, r#"{"type":"command","command":"x"}"#);
        assert_eq!(run(root.path(), Some(ID), false, None), 0);
        assert_eq!(run(root.path(), Some(ID), false, None), 0);
    }

    #[test]
    fn bad_arguments_and_empty_roots_are_reported_not_fatal() {
        let root = tempdir().unwrap();
        assert_eq!(run(root.path(), Some("../../x"), false, None), 2);
        assert_eq!(run(root.path(), None, false, None), 2);
        assert_eq!(run(root.path(), None, true, None), 0);
    }

    #[test]
    fn an_unreadable_settings_file_fails_loudly_without_losing_the_target() {
        let root = tempdir().unwrap();
        let dir = tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        setup(root.path(), &settings, r#"{"type":"command","command":"x"}"#);
        std::fs::write(&settings, "{ broken").unwrap();
        assert_eq!(run(root.path(), Some(ID), false, None), 1);
        // The target is still enabled, so a second attempt is possible.
        assert_eq!(target::load(root.path(), ID).unwrap().state, TargetState::Enabled);
    }
}
