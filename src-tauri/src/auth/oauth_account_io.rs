//! Read and write the `oauthAccount` slice of `~/.claude.json` (or the
//! resolved location per `auth::paths::claude_global_config`).
//!
//! Atomic write: temp + rename. Preserves unknown top-level keys
//! (e.g. `hasCompletedOnboarding`, `lastOnboardingVersion`, MCP config) by
//! parsing the whole file, replacing only `.oauthAccount`, and writing back.

use anyhow::{anyhow, Context, Result};
use std::path::Path;

pub fn read_oauth_account(path: &Path) -> Result<Option<serde_json::Value>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path).with_context(|| format!("read {path:?}"))?;
    let parsed: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("parse {path:?} as json"))?;
    Ok(parsed.get("oauthAccount").filter(|value| !value.is_null()).cloned())
}

pub fn write_oauth_account(path: &Path, slice: &serde_json::Value) -> Result<()> {
    let mut existing: serde_json::Value = if path.exists() {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read {path:?} for splice"))?;
        serde_json::from_str(&text)
            .with_context(|| format!("parse {path:?} for splice"))?
    } else {
        serde_json::json!({})
    };

    let obj = existing
        .as_object_mut()
        .ok_or_else(|| anyhow!("{path:?} is not a JSON object"))?;
    if slice.is_null() {
        obj.remove("oauthAccount");
    } else {
        obj.insert("oauthAccount".to_string(), slice.clone());
    }

    let payload = serde_json::to_string_pretty(&existing)?;
    let tmp = path.with_extension("json.tmp");
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).context("create config dir")?;
    }
    std::fs::write(&tmp, &payload).context("write temp config")?;
    std::fs::rename(&tmp, path).context("rename temp into place")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn clearing_unknown_identity_preserves_other_config() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".claude.json");
        std::fs::write(&path, r#"{"hasCompletedOnboarding":true,"oauthAccount":{"accountUuid":"old"}}"#).unwrap();
        write_oauth_account(&path, &serde_json::Value::Null).unwrap();
        let config: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(config["hasCompletedOnboarding"], true);
        assert!(config.get("oauthAccount").is_none());
        assert!(read_oauth_account(&path).unwrap().is_none());
    }

    #[test]
    fn read_returns_none_when_file_absent() {
        let dir = tempdir().unwrap();
        let p = dir.path().join(".claude.json");
        let r = read_oauth_account(&p).unwrap();
        assert!(r.is_none());
    }

    #[test]
    fn write_preserves_other_keys() {
        let dir = tempdir().unwrap();
        let p = dir.path().join(".claude.json");
        std::fs::write(
            &p,
            r#"{"hasCompletedOnboarding":true,"lastOnboardingVersion":"2.1.29","oauthAccount":{"emailAddress":"old@x.com"}}"#,
        )
        .unwrap();

        let new_slice = serde_json::json!({
            "accountUuid": "uuid-new",
            "emailAddress": "new@x.com",
            "organizationUuid": "org-1",
            "organizationName": "Acme"
        });
        write_oauth_account(&p, &new_slice).unwrap();

        let text = std::fs::read_to_string(&p).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["hasCompletedOnboarding"], true);
        assert_eq!(parsed["lastOnboardingVersion"], "2.1.29");
        assert_eq!(parsed["oauthAccount"]["emailAddress"], "new@x.com");
        assert_eq!(parsed["oauthAccount"]["organizationName"], "Acme");
    }

    #[test]
    fn write_creates_file_when_absent() {
        let dir = tempdir().unwrap();
        let p = dir.path().join(".claude.json");
        let slice = serde_json::json!({ "emailAddress": "a@b.c" });
        write_oauth_account(&p, &slice).unwrap();
        let parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(parsed["oauthAccount"]["emailAddress"], "a@b.c");
    }
}
