//! Clean up the legacy launch-agent plist (macOS) / Run-key entry (Windows)
//! left behind by a v0.3.x install with autostart enabled.
//!
//! Without this, after rebrand the OS still launches the old binary at every
//! login (per `tauri-plugin-autostart` in LaunchAgent mode).

use anyhow::Result;
use std::path::{Path, PathBuf};

/// Where the legacy macOS LaunchAgents plist lives, relative to a home dir.
pub fn legacy_plist_path(home: &Path) -> PathBuf {
    home.join("Library")
        .join("LaunchAgents")
        .join(crate::branding::LEGACY_AUTOSTART_PLIST_FILENAME)
}

/// Returns true if the legacy plist exists on disk.
pub fn legacy_plist_exists(home: &Path) -> bool {
    legacy_plist_path(home).exists()
}

/// Remove the legacy plist. On macOS this also runs `launchctl unload` first
/// so the in-memory job is dropped. Best-effort: any failure of the unload
/// call is logged and ignored — what matters is that the file is gone.
#[cfg(target_os = "macos")]
pub fn remove_legacy_plist(home: &Path) -> Result<()> {
    let path = legacy_plist_path(home);
    if !path.exists() {
        return Ok(());
    }
    let _ = std::process::Command::new("launchctl")
        .arg("unload")
        .arg(&path)
        .status();
    std::fs::remove_file(&path)?;
    Ok(())
}

/// On non-macOS the file does not exist; this is a no-op.
#[cfg(not(target_os = "macos"))]
pub fn remove_legacy_plist(_home: &Path) -> Result<()> {
    Ok(())
}

/// Remove the legacy Run-key entry from `HKCU\…\Run\Claude Limits`.
/// Returns Ok(()) on Windows and a no-op on other platforms.
#[cfg(target_os = "windows")]
pub fn remove_legacy_run_key() -> Result<()> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run = hkcu.open_subkey_with_flags(
        r"Software\Microsoft\Windows\CurrentVersion\Run",
        winreg::enums::KEY_SET_VALUE,
    );
    if let Ok(run) = run {
        let _ = run.delete_value(crate::branding::LEGACY_WINDOWS_AUTOSTART_REGKEY_NAME);
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn remove_legacy_run_key() -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn legacy_plist_path_uses_branding() {
        let home = tempdir().unwrap();
        let path = legacy_plist_path(home.path());
        let s = path.to_string_lossy();
        assert!(s.ends_with("LaunchAgents/com.claude-limits.app.plist"));
    }

    #[test]
    fn legacy_plist_exists_returns_false_when_absent() {
        let home = tempdir().unwrap();
        assert!(!legacy_plist_exists(home.path()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn remove_legacy_plist_no_op_on_missing_file() {
        let home = tempdir().unwrap();
        let res = remove_legacy_plist(home.path());
        assert!(res.is_ok());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn remove_legacy_plist_deletes_existing_file() {
        let home = tempdir().unwrap();
        let agents = home.path().join("Library").join("LaunchAgents");
        std::fs::create_dir_all(&agents).unwrap();
        let plist = agents.join("com.claude-limits.app.plist");
        std::fs::write(&plist, "<?xml version=\"1.0\"?><plist/>").unwrap();
        assert!(plist.exists());

        remove_legacy_plist(home.path()).unwrap();
        assert!(!plist.exists());
    }
}
