use super::super::StoredToken;
use anyhow::{anyhow, Context, Result};
use chrono::{TimeZone, Utc};
use serde::Deserialize;
use std::io;
use std::process::Command;

const SERVICE_PREFIX: &str = "Claude Code-credentials";

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

pub async fn load() -> Result<Option<StoredToken>> {
    let services = discover_services().await?;
    let mut candidates = Vec::new();
    for svc in services {
        if let Ok(Some(tok)) = read_one(svc).await {
            candidates.push(tok);
        }
    }
    candidates.sort_by_key(|t| t.expires_at);
    Ok(candidates.pop())
}

async fn discover_services() -> Result<Vec<String>> {
    let output = tokio::task::spawn_blocking(|| {
        Command::new("security").arg("dump-keychain").output()
    })
    .await
    .map_err(io::Error::other)?;

    let stdout = match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
        Err(_) => return Ok(vec![SERVICE_PREFIX.to_string()]),
    };
    let mut services = Vec::new();
    for line in stdout.lines() {
        if let Some(idx) = line.find("\"svce\"<blob>=\"") {
            let rest = &line[idx + 14..];
            if let Some(end) = rest.find('"') {
                let name = &rest[..end];
                if name.starts_with(SERVICE_PREFIX) && !services.contains(&name.to_string()) {
                    services.push(name.to_string());
                }
            }
        }
    }
    if services.is_empty() {
        services.push(SERVICE_PREFIX.to_string());
    }
    Ok(services)
}

async fn read_one(service: String) -> Result<Option<StoredToken>> {
    let out = tokio::task::spawn_blocking(move || {
        Command::new("security")
            .args(["find-generic-password", "-s", &service, "-w"])
            .output()
    })
    .await
    .map_err(io::Error::other)?
    .context("spawn security find-generic-password")?;

    if !out.status.success() {
        return Ok(None);
    }

    let mut bytes = out.stdout;
    if let Some(&last) = bytes.last() {
        if last == b'\n' {
            bytes.pop();
        }
    }

    if !bytes.is_empty() && bytes[0] > 0x7F {
        bytes.remove(0);
    }

    let text = String::from_utf8(bytes).context("keychain payload not utf-8")?;
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

pub async fn has_creds() -> bool {
    tokio::task::spawn_blocking(|| {
        Command::new("security")
            .args(["find-generic-password", "-s", SERVICE_PREFIX])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false)
}

/// Read the full `claudeAiOauth` JSON value (preserving every field) for the
/// canonical service. Falls back to enumeration only when canonical is absent.
pub async fn load_full_blob() -> Result<Option<serde_json::Value>> {
    // Try canonical service first — this is what claude-code itself reads.
    if let Some(blob) = read_one_blob(SERVICE_PREFIX.to_string()).await? {
        return Ok(Some(blob));
    }
    // Fall back to enumeration for installs with non-canonical service names.
    let services = discover_services().await?;
    for svc in services {
        if svc == SERVICE_PREFIX {
            continue;
        }
        if let Some(blob) = read_one_blob(svc).await? {
            return Ok(Some(blob));
        }
    }
    Ok(None)
}

async fn read_one_blob(service: String) -> Result<Option<serde_json::Value>> {
    let out = tokio::task::spawn_blocking(move || {
        Command::new("security")
            .args(["find-generic-password", "-s", &service, "-w"])
            .output()
    })
    .await
    .map_err(io::Error::other)?
    .context("spawn security find-generic-password")?;

    if !out.status.success() {
        return Ok(None);
    }

    let mut bytes = out.stdout;
    if let Some(&last) = bytes.last() {
        if last == b'\n' {
            bytes.pop();
        }
    }
    if !bytes.is_empty() && bytes[0] > 0x7F {
        bytes.remove(0);
    }

    let text = String::from_utf8(bytes).context("keychain payload not utf-8")?;
    let parsed: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    Ok(parsed.get("claudeAiOauth").cloned())
}

/// Write the full `claudeAiOauth` blob to the canonical Keychain service.
/// Always writes to `"Claude Code-credentials"` (no per-account variant).
///
/// The blob is passed as the `-w <password>` argument. macOS `security` has
/// no stdin-mode for passwords (omitting `-w` reads from `/dev/tty`, not
/// stdin), so the JSON briefly appears in this process's command line —
/// readable by other processes of the same user via `ps`. Acceptable for
/// a desktop tool where the same user already owns the keychain item
/// itself; reach for the `security-framework` crate if that ever changes.
pub async fn write_full_blob(blob: &serde_json::Value) -> Result<()> {
    let wrapped = serde_json::json!({ "claudeAiOauth": blob });
    let payload = serde_json::to_string(&wrapped)?;
    let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());

    tokio::task::spawn_blocking(move || -> Result<()> {
        let out = Command::new("security")
            .args([
                "add-generic-password", "-U",
                "-s", SERVICE_PREFIX,
                "-a", &user,
                "-w", &payload,
            ])
            .output()
            .context("spawn security add-generic-password")?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            anyhow::bail!("security add-generic-password failed: {stderr}");
        }
        Ok(())
    })
    .await
    .map_err(io::Error::other)??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn parse_sample_payload() {
        let sample = r#"{"claudeAiOauth":{"accessToken":"a","refreshToken":"r","expiresAt":1840000000000}}"#;
        let raw: RawCreds = serde_json::from_str(sample).unwrap();
        assert_eq!(raw.claude_ai_oauth.access_token, "a");
        assert_eq!(
            raw.claude_ai_oauth.refresh_token.as_deref(),
            Some("r")
        );
        let expected = Utc
            .timestamp_millis_opt(1_840_000_000_000)
            .single()
            .unwrap();
        assert!(expected > Utc::now() - Duration::days(365 * 100));
    }

    #[test]
    fn parse_full_blob_preserves_unknown_fields() {
        let sample = r#"{"claudeAiOauth":{"accessToken":"a","refreshToken":"r","expiresAt":1840000000000,"scopes":["user:inference"],"subscriptionType":"max","rateLimitTier":"default_claude_max_5x"}}"#;
        let raw: serde_json::Value = serde_json::from_str(sample).unwrap();
        let blob = raw.get("claudeAiOauth").unwrap();
        assert_eq!(blob["subscriptionType"], "max");
        assert_eq!(blob["rateLimitTier"], "default_claude_max_5x");
        assert_eq!(blob["scopes"][0], "user:inference");
    }
}
