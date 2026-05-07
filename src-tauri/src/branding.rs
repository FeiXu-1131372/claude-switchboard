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
}
