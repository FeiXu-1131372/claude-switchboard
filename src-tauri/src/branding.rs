//! Single source of truth for product naming and platform identifiers.
//! All hard-coded "Claude Switchboard" / `com.claude-switchboard.app` /
//! `claude-switchboard` references should read from this module so a
//! future rename touches one file.

pub const PRODUCT_NAME: &str = "Claude Switchboard";
pub const TAURI_BUNDLE_ID: &str = "com.claude-switchboard.app";

pub const PROJECT_DIRS_QUALIFIER: &str = "com";
pub const PROJECT_DIRS_ORG: &str = "claude-switchboard";
pub const PROJECT_DIRS_APP: &str = "ClaudeSwitchboard";

pub const USER_AGENT_PREFIX: &str = "claude-switchboard";
pub const GITHUB_REPO_PATH: &str = "FeiXu-1131372/claude-switchboard";
pub const DB_LOCKFILE_NAME: &str = "claude-switchboard.lock";

// === Legacy values, used by migration to detect/clean v0.3.x install ===

pub const LEGACY_PRODUCT_NAME: &str = "Claude Limits";
pub const LEGACY_TAURI_BUNDLE_ID: &str = "com.claude-limits.app";
pub const LEGACY_PROJECT_DIRS_QUALIFIER: &str = "com";
pub const LEGACY_PROJECT_DIRS_ORG: &str = "claude-limits";
pub const LEGACY_PROJECT_DIRS_APP: &str = "ClaudeLimits";
pub const LEGACY_DB_LOCKFILE_NAME: &str = "claude-monitor.lock";
pub const LEGACY_AUTOSTART_PLIST_FILENAME: &str = "com.claude-limits.app.plist";
pub const LEGACY_WINDOWS_AUTOSTART_REGKEY_NAME: &str = "Claude Limits";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_constants_match_spec() {
        assert_eq!(PRODUCT_NAME, "Claude Switchboard");
        assert_eq!(TAURI_BUNDLE_ID, "com.claude-switchboard.app");
        assert_eq!(USER_AGENT_PREFIX, "claude-switchboard");
        assert_eq!(GITHUB_REPO_PATH, "FeiXu-1131372/claude-switchboard");
    }

    #[test]
    fn legacy_constants_match_v03x_install() {
        assert_eq!(LEGACY_PRODUCT_NAME, "Claude Limits");
        assert_eq!(LEGACY_TAURI_BUNDLE_ID, "com.claude-limits.app");
        assert_eq!(LEGACY_PROJECT_DIRS_ORG, "claude-limits");
        assert_eq!(LEGACY_PROJECT_DIRS_APP, "ClaudeLimits");
        assert_eq!(LEGACY_DB_LOCKFILE_NAME, "claude-monitor.lock");
    }
}
