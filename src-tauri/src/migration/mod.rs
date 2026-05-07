//! First-launch migration from claude-limits v0.3.x to Claude Switchboard.
//!
//! Two phases (per architectural revision in Plan A T11):
//!
//! 1. `run_phase1_file_copy(new_data_dir)` — pure file operations. Must run
//!    BEFORE `Db::open` on the new data dir, so the SQLite database file
//!    can be copied across before the new app creates a fresh empty one.
//!
//! 2. `run_phase2(new_data_dir, conn)` — DB-aware cleanup. Runs AFTER
//!    `Db::open`. Quits the legacy v0.3.x process, removes legacy autostart
//!    entries, and sets settings.migration_completed = '1' (idempotent gate).

pub mod autostart;
pub mod data_dir_copy;
pub mod legacy_process;

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

use crate::branding::{
    LEGACY_PROJECT_DIRS_APP, LEGACY_PROJECT_DIRS_ORG, LEGACY_PROJECT_DIRS_QUALIFIER,
};

/// What the migration steps found and did. Surfaces to UI for the
/// "Welcome to Switchboard" dialog (T13).
#[derive(Debug, Clone, Default, serde::Serialize, specta::Type)]
pub struct MigrationOutcome {
    pub legacy_data_dir_found: bool,
    pub files_copied: usize,
    pub legacy_process_quit: bool,
    pub legacy_autostart_removed: bool,
}

/// Resolve the legacy v0.3.x data directory. Symmetric with `store::default_dir()`
/// but using the legacy ProjectDirs strings.
pub fn legacy_data_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from(
        LEGACY_PROJECT_DIRS_QUALIFIER,
        LEGACY_PROJECT_DIRS_ORG,
        LEGACY_PROJECT_DIRS_APP,
    )
    .map(|p| p.data_local_dir().to_path_buf())
}

/// Phase 1: file-level copy of the legacy data dir into the new data dir.
/// Called BEFORE `Db::open(new_data_dir)`.
///
/// Uses presence of `<new_data_dir>/data.db` as the file-level marker:
/// if data.db already exists in the new dir, treat the new dir as
/// already-initialized and skip the copy. Otherwise, if the legacy dir
/// exists, copy its contents.
///
/// Returns the number of files copied (0 if a copy was not needed).
pub fn run_phase1_file_copy(new_data_dir: &Path) -> Result<usize> {
    let new_db = new_data_dir.join("data.db");
    if new_db.exists() {
        return Ok(0); // new dir already initialized; don't touch
    }
    let legacy_dir = match legacy_data_dir() {
        Some(p) if p.exists() => p,
        _ => return Ok(0), // fresh install path
    };
    data_dir_copy::copy_data_dir_contents(&legacy_dir, new_data_dir)
        .context("phase1 data dir copy")
}

/// True if Phase 2 has already run (settings.migration_completed = '1').
fn phase2_already_completed(conn: &Connection) -> Result<bool> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'migration_completed'",
            [],
            |r| r.get(0),
        )
        .ok();
    Ok(matches!(value.as_deref(), Some("1")))
}

fn mark_phase2_completed(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('migration_completed', '1') \
         ON CONFLICT (key) DO UPDATE SET value = '1'",
        [],
    )?;
    Ok(())
}

/// Phase 2: DB-aware cleanup. Called AFTER `Db::open(new_data_dir)`.
/// Idempotent — gated by settings.migration_completed.
///
/// Returns a MigrationOutcome describing what was found / done.
pub fn run_phase2(conn: &Connection, files_copied_in_phase1: usize) -> Result<MigrationOutcome> {
    if phase2_already_completed(conn)? {
        return Ok(MigrationOutcome::default());
    }

    let legacy_dir_present = legacy_data_dir()
        .map(|p| p.exists())
        .unwrap_or(false);

    if !legacy_dir_present && files_copied_in_phase1 == 0 {
        // Fresh install — no v0.3.x legacy state. Mark completed so we
        // never re-check on subsequent launches.
        mark_phase2_completed(conn)?;
        return Ok(MigrationOutcome::default());
    }

    // Step 1: quit any running v0.3.x process.
    let legacy_process_quit = match legacy_process::quit_legacy_processes(5) {
        Ok(()) => true,
        Err(e) => {
            tracing::error!("Failed to quit legacy process: {e:#}");
            return Err(e).context(
                "Couldn't quit Claude Limits automatically. \
                 Quit it manually and re-launch Switchboard to continue.",
            );
        }
    };

    // Step 2: clean up legacy autostart entries.
    let mut legacy_autostart_removed = false;
    if let Some(home) = directories::UserDirs::new().map(|u| u.home_dir().to_path_buf()) {
        if autostart::legacy_plist_exists(&home) {
            autostart::remove_legacy_plist(&home).ok();
            legacy_autostart_removed = true;
        }
    }
    autostart::remove_legacy_run_key().ok();

    // Step 3: mark complete.
    mark_phase2_completed(conn)?;

    Ok(MigrationOutcome {
        legacy_data_dir_found: true,
        files_copied: files_copied_in_phase1,
        legacy_process_quit,
        legacy_autostart_removed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn open_fresh_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL); \
             INSERT INTO settings (key, value) VALUES ('migration_completed', '0');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn phase2_marks_completed_on_fresh_install() {
        // No legacy dir on the test system (we can't easily fake ProjectDirs);
        // expect: outcome is empty, flag flipped to '1'.
        let conn = open_fresh_conn();
        if legacy_data_dir().map(|p| p.exists()).unwrap_or(false) {
            eprintln!("legacy dir present on this system; skipping fresh-install test");
            return;
        }

        let out = run_phase2(&conn, 0).unwrap();
        assert!(!out.legacy_data_dir_found);
        assert_eq!(out.files_copied, 0);

        let value: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'migration_completed'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(value, "1");
    }

    #[test]
    fn phase2_is_no_op_when_already_completed() {
        let conn = open_fresh_conn();
        conn.execute(
            "UPDATE settings SET value = '1' WHERE key = 'migration_completed'",
            [],
        )
        .unwrap();

        let out = run_phase2(&conn, 0).unwrap();
        assert!(!out.legacy_data_dir_found);
        assert_eq!(out.files_copied, 0);
    }

    #[test]
    fn phase1_skips_when_new_db_already_exists() {
        let new = tempdir().unwrap();
        std::fs::write(new.path().join("data.db"), "fake").unwrap();

        let n = run_phase1_file_copy(new.path()).unwrap();
        assert_eq!(n, 0, "should skip when new data.db exists");
    }

    #[test]
    fn phase1_returns_zero_when_no_legacy_dir() {
        let new = tempdir().unwrap();
        // No data.db in new dir, but also no legacy dir on this CI system.
        if legacy_data_dir().map(|p| p.exists()).unwrap_or(false) {
            eprintln!("legacy dir present on this system; skipping no-legacy test");
            return;
        }
        let n = run_phase1_file_copy(new.path()).unwrap();
        assert_eq!(n, 0);
    }
}
