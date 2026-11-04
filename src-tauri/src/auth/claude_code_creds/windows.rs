use super::super::StoredToken;
use anyhow::{anyhow, Context, Result};
use chrono::{TimeZone, Utc};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
struct RawCreds {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: OauthBlock,
}

#[derive(Deserialize)]
struct OauthBlock {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "refreshToken")]
    refresh_token: Option<String>,
    #[serde(rename = "expiresAt")]
    expires_at_ms: i64,
}

fn credentials_path() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE")?;
    Some(PathBuf::from(home).join(".claude").join(".credentials.json"))
}

pub fn load() -> Result<Option<StoredToken>> {
    let p = match credentials_path() {
        Some(p) => p,
        None => return Ok(None),
    };
    if !p.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&p).context("read .credentials.json")?;
    let raw: RawCreds = match serde_json::from_str(&text) {
        Ok(r) => r,
        Err(_) => return Ok(None),
    };
    let exp = Utc
        .timestamp_millis_opt(raw.claude_ai_oauth.expires_at_ms)
        .single()
        .ok_or_else(|| anyhow!("invalid expires_at_ms"))?;
    Ok(Some(StoredToken {
        access_token: raw.claude_ai_oauth.access_token,
        refresh_token: raw.claude_ai_oauth.refresh_token,
        expires_at: exp,
    }))
}

pub fn has_creds() -> bool {
    credentials_path().map(|p| p.exists()).unwrap_or(false)
}

pub fn load_full_blob() -> Result<Option<serde_json::Value>> {
    let p = match credentials_path() {
        Some(p) => p,
        None => return Ok(None),
    };
    if !p.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&p).context("read .credentials.json")?;
    let parsed: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    Ok(parsed.get("claudeAiOauth").filter(|value| value.is_object()).cloned())
}

/// Atomic write: temp file + rename. ACL inherited from parent dir
/// (`%USERPROFILE%\.claude`) which is per-user by default on Windows.
pub fn write_full_blob(blob: &serde_json::Value) -> Result<()> {
    let p = credentials_path().ok_or_else(|| anyhow!("USERPROFILE unset"))?;
    let dir = p.parent().ok_or_else(|| anyhow!("no parent dir"))?;
    std::fs::create_dir_all(dir).context("create .claude dir")?;

    let wrapped = serde_json::json!({ "claudeAiOauth": blob });
    let payload = serde_json::to_string_pretty(&wrapped)?;

    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, &payload).context("write temp .credentials.json")?;
    std::fs::rename(&tmp, &p).context("rename temp into place")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn parses_realistic_payload_from_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".credentials.json");
        fs::write(
            &path,
            r#"{"claudeAiOauth":{"accessToken":"a","refreshToken":"r","expiresAt":1840000000000}}"#,
        )
        .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        let raw: RawCreds = serde_json::from_str(&text).unwrap();
        assert_eq!(raw.claude_ai_oauth.access_token, "a");
    }

    #[test]
    fn full_blob_preserves_extra_fields_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".credentials.json");
        let original = r#"{"claudeAiOauth":{"accessToken":"a","refreshToken":"r","expiresAt":1840000000000,"scopes":["user:inference"],"subscriptionType":"max"}}"#;
        fs::write(&path, original).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        let blob = parsed.get("claudeAiOauth").unwrap();
        assert_eq!(blob["subscriptionType"], "max");
    }
}
