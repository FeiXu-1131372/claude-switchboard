//! Where Claude Code keeps its config, resolved exactly as Claude Code does
//! (spec §3.1). The bridge and the Switchboard app both call this, so they
//! always agree on which global config names the live account.

use std::path::{Path, PathBuf};

/// The slice of the environment the resolution depends on, injectable so
/// every rule is testable without touching the process environment.
#[derive(Debug, Clone, Default)]
pub struct EnvView {
    pub claude_config_dir: Option<PathBuf>,
    pub home: PathBuf,
    pub anthropic_base_url: Option<String>,
    pub use_bedrock: bool,
    pub use_vertex: bool,
    pub use_foundry: bool,
}

fn truthy(name: &str) -> bool {
    std::env::var(name)
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            !v.is_empty() && v != "0" && v != "false"
        })
        .unwrap_or(false)
}

impl EnvView {
    /// `None` when neither a home directory nor `CLAUDE_CONFIG_DIR` exists.
    pub fn from_process() -> Option<EnvView> {
        let claude_config_dir = std::env::var_os("CLAUDE_CONFIG_DIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        let home = crate::format::home_dir();
        if home.is_none() && claude_config_dir.is_none() {
            return None;
        }
        Some(EnvView {
            claude_config_dir,
            home: home.unwrap_or_default(),
            anthropic_base_url: std::env::var("ANTHROPIC_BASE_URL").ok(),
            use_bedrock: truthy("CLAUDE_CODE_USE_BEDROCK"),
            use_vertex: truthy("CLAUDE_CODE_USE_VERTEX"),
            use_foundry: truthy("CLAUDE_CODE_USE_FOUNDRY"),
        })
    }

    /// Auth that bypasses the config dir's login: a non-default base URL, or
    /// a third-party provider flag. Never attributable to an account.
    pub fn is_routed(&self) -> bool {
        const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
        let custom_url = self
            .anthropic_base_url
            .as_deref()
            .map(|u| u.trim().trim_end_matches('/'))
            .is_some_and(|u| !u.is_empty() && u != DEFAULT_BASE_URL);
        custom_url || self.use_bedrock || self.use_vertex || self.use_foundry
    }
}

/// `CLAUDE_CONFIG_DIR`, else the `…/projects/…` ancestor of `transcript_path`
/// (a session started with the variable set in a shell the bridge cannot
/// see), else `~/.claude`.
pub fn config_home(env: &EnvView, transcript_path: Option<&str>) -> PathBuf {
    if let Some(dir) = &env.claude_config_dir {
        return dir.clone();
    }
    if let Some(transcript) = transcript_path.filter(|t| !t.is_empty()) {
        let found = Path::new(transcript).ancestors().find(|a| {
            a.file_name().is_some_and(|n| n == "projects") && a.parent().is_some()
        });
        if let Some(projects) = found {
            if let Some(home) = projects.parent() {
                return home.to_path_buf();
            }
        }
    }
    env.home.join(".claude")
}

/// The global config file holding `oauthAccount`, chosen as Claude Code's
/// `h_()` does: the legacy `<config_home>/.config.json` when it is a
/// non-empty file, or a writable one while the new path is missing; otherwise
/// `(CLAUDE_CONFIG_DIR || home)/.claude.json`. (Production builds use an
/// empty OAuth file suffix.)
pub fn global_config_path(env: &EnvView, config_home: &Path) -> PathBuf {
    let legacy = config_home.join(".config.json");
    let new = env
        .claude_config_dir
        .clone()
        .unwrap_or_else(|| env.home.clone())
        .join(".claude.json");
    let legacy_wins = match std::fs::metadata(&legacy) {
        Ok(meta) if meta.is_file() => {
            meta.len() > 0 || (!new.exists() && !meta.permissions().readonly())
        }
        _ => false,
    };
    if legacy_wins {
        legacy
    } else {
        new
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn env(home: &Path, config_dir: Option<&Path>) -> EnvView {
        EnvView {
            claude_config_dir: config_dir.map(Path::to_path_buf),
            home: home.to_path_buf(),
            ..Default::default()
        }
    }

    #[test]
    fn config_home_prefers_env_then_transcript_ancestor_then_default() {
        let home = Path::new("/home/u");
        let transcript = "/data/cc/projects/-Users-u-app/abc.jsonl";

        let with_env = env(home, Some(Path::new("/custom/dir")));
        assert_eq!(config_home(&with_env, Some(transcript)), PathBuf::from("/custom/dir"));

        let no_env = env(home, None);
        assert_eq!(config_home(&no_env, Some(transcript)), PathBuf::from("/data/cc"));
        assert_eq!(config_home(&no_env, Some("")), PathBuf::from("/home/u/.claude"));
        assert_eq!(config_home(&no_env, Some("/no/such/shape.jsonl")), PathBuf::from("/home/u/.claude"));
        assert_eq!(config_home(&no_env, None), PathBuf::from("/home/u/.claude"));
    }

    #[test]
    fn global_config_defaults_to_dot_claude_json_in_home() {
        let home = tempdir().unwrap();
        let e = env(home.path(), None);
        let ch = home.path().join(".claude");
        assert_eq!(global_config_path(&e, &ch), home.path().join(".claude.json"));
    }

    #[test]
    fn legacy_config_json_is_used_only_when_non_empty_or_writable_while_new_path_missing() {
        let home = tempdir().unwrap();
        let ch = home.path().join(".claude");
        std::fs::create_dir_all(&ch).unwrap();
        let legacy = ch.join(".config.json");
        let new = home.path().join(".claude.json");
        let e = env(home.path(), None);

        // Non-empty legacy file wins even when the new path exists.
        std::fs::write(&legacy, "{\"a\":1}").unwrap();
        std::fs::write(&new, "{}").unwrap();
        assert_eq!(global_config_path(&e, &ch), legacy);

        // Empty legacy file with the new path present: the new path wins.
        std::fs::write(&legacy, "").unwrap();
        assert_eq!(global_config_path(&e, &ch), new);

        // Empty, writable legacy file while the new path is missing: legacy.
        std::fs::remove_file(&new).unwrap();
        assert_eq!(global_config_path(&e, &ch), legacy);

        // Empty read-only legacy file while the new path is missing: new.
        let mut perms = std::fs::metadata(&legacy).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&legacy, perms).unwrap();
        assert_eq!(global_config_path(&e, &ch), new);
    }

    #[test]
    fn config_dir_env_with_a_legacy_file_present() {
        let home = tempdir().unwrap();
        let dir = tempdir().unwrap();
        let e = env(home.path(), Some(dir.path()));
        std::fs::write(dir.path().join(".config.json"), "{\"a\":1}").unwrap();
        assert_eq!(global_config_path(&e, dir.path()), dir.path().join(".config.json"));

        std::fs::remove_file(dir.path().join(".config.json")).unwrap();
        assert_eq!(global_config_path(&e, dir.path()), dir.path().join(".claude.json"));
    }

    #[test]
    fn routed_detection() {
        let mut e = env(Path::new("/h"), None);
        assert!(!e.is_routed());
        e.anthropic_base_url = Some("https://api.anthropic.com/".into());
        assert!(!e.is_routed(), "the default URL, with a trailing slash, is not routed");
        e.anthropic_base_url = Some("https://proxy.example.com".into());
        assert!(e.is_routed());
        e.anthropic_base_url = Some(String::new());
        assert!(!e.is_routed());

        for flag in 0..3 {
            let mut e = env(Path::new("/h"), None);
            match flag {
                0 => e.use_bedrock = true,
                1 => e.use_vertex = true,
                _ => e.use_foundry = true,
            }
            assert!(e.is_routed());
        }
    }
}
