//! Copy the contents of a v0.3.x data directory into the new Switchboard
//! data directory. Skips lockfiles and any temp/in-flight files.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// File names that should NOT be copied across (lockfiles, temp).
const SKIP_FILENAMES: &[&str] = &[
    "claude-monitor.lock",       // legacy DB lock
    ".accounts.lock",            // accounts.json lock (auth/accounts/store.rs)
    "claude-switchboard.lock",   // new DB lock, in case it somehow exists
];

/// Copy every regular file in `from_dir` into `to_dir`, creating `to_dir`
/// if needed. Existing files in `to_dir` are NOT overwritten — first-launch
/// migration runs against an empty new dir, and re-runs are gated by
/// settings.migration_completed (so this code path only fires once).
///
/// Returns the number of files copied.
pub fn copy_data_dir_contents(from_dir: &Path, to_dir: &Path) -> Result<usize> {
    if !from_dir.exists() {
        return Ok(0);
    }
    fs::create_dir_all(to_dir).context("create new data dir")?;

    let mut copied = 0;
    for entry in fs::read_dir(from_dir).context("read old data dir")? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(s) => s,
            None => continue,
        };
        if SKIP_FILENAMES.contains(&name) {
            continue;
        }

        let dst = to_dir.join(name);
        if dst.exists() {
            continue;
        }
        fs::copy(&path, &dst)
            .with_context(|| format!("copy {name} into new data dir"))?;
        copied += 1;
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn touch(path: &Path, body: &str) {
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn copies_data_files_skipping_lockfiles() {
        let from = tempdir().unwrap();
        let to = tempdir().unwrap();
        touch(&from.path().join("data.db"), "fake-sqlite");
        touch(&from.path().join("accounts.json"), "{}");
        touch(&from.path().join("updater.json"), "{}");
        touch(&from.path().join("claude-monitor.lock"), "");
        touch(&from.path().join(".accounts.lock"), "");

        let n = copy_data_dir_contents(from.path(), to.path()).unwrap();

        assert_eq!(n, 3, "should copy 3 data files");
        assert!(to.path().join("data.db").exists());
        assert!(to.path().join("accounts.json").exists());
        assert!(to.path().join("updater.json").exists());
        assert!(!to.path().join("claude-monitor.lock").exists());
        assert!(!to.path().join(".accounts.lock").exists());
    }

    #[test]
    fn no_op_when_source_does_not_exist() {
        let to = tempdir().unwrap();
        let n =
            copy_data_dir_contents(Path::new("/no/such/path/here"), to.path())
                .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn does_not_overwrite_existing_files_in_destination() {
        let from = tempdir().unwrap();
        let to = tempdir().unwrap();
        touch(&from.path().join("data.db"), "old-content");
        touch(&to.path().join("data.db"), "new-content");

        let n = copy_data_dir_contents(from.path(), to.path()).unwrap();
        assert_eq!(n, 0, "destination data.db already exists, skip");

        let kept =
            std::fs::read_to_string(to.path().join("data.db")).unwrap();
        assert_eq!(kept, "new-content");
    }
}
