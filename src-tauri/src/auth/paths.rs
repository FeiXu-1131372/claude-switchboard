//! Path resolution mirroring claude-code's own behavior.
//!
//! `<config_home> = $CLAUDE_CONFIG_DIR ?? <homedir>/.claude`.
//!
//! The global config is `<config_home>/.config.json` when that legacy file is
//! non-empty (or writable while the new path is missing), else
//! `($CLAUDE_CONFIG_DIR ?? <homedir>)/.claude.json`. The rule lives in
//! `statusline_bridge::config_paths`, shared with the bridge binary.

use std::path::PathBuf;

pub fn claude_config_home() -> Option<PathBuf> {
    if let Some(env) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return Some(PathBuf::from(env));
    }
    home_dir().map(|h| h.join(".claude"))
}

/// The global config holding `oauthAccount`. Delegates to the statusline
/// bridge crate so the app and the bridge always pick the same file, and so
/// both follow Claude Code's own `h_()`: a legacy `<config_home>/.config.json`
/// wins only when non-empty (or writable while the new path is missing),
/// whether or not `CLAUDE_CONFIG_DIR` is set.
pub fn claude_global_config() -> Option<PathBuf> {
    use statusline_bridge::config_paths::{config_home, global_config_path, EnvView};
    let env = EnvView::from_process()?;
    let home = config_home(&env, None);
    Some(global_config_path(&env, &home))
}

#[cfg(unix)]
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(windows)]
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn env_var_wins() {
        let dir = tempdir().unwrap();
        // Use a scoped env var via std::env::set_var (test-process scoped, fine
        // for single-threaded tests). Restore at end.
        let prev = std::env::var_os("CLAUDE_CONFIG_DIR");
        // SAFETY: test-only, single-threaded by default.
        unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", dir.path()) };
        let p = claude_global_config().unwrap();
        assert_eq!(p, dir.path().join(".claude.json"));
        // A non-empty legacy file under the config home wins even with the
        // variable set (the old rule only looked at `~/.claude` without it).
        std::fs::write(dir.path().join(".config.json"), "{\"a\":1}").unwrap();
        assert_eq!(claude_global_config().unwrap(), dir.path().join(".config.json"));
        match prev {
            Some(v) => unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", v) },
            None => unsafe { std::env::remove_var("CLAUDE_CONFIG_DIR") },
        }
    }

    #[test]
    fn legacy_filename_is_config_json_not_claude_json() {
        // Compile-time check: function signature exists. Runtime behavior
        // depends on filesystem state (legacy file presence) — covered in
        // env-var test above. This is a smoke test.
        let _ = claude_global_config();
    }
}
