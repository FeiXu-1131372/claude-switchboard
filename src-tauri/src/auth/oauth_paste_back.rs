use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::Url;
use zeroize::ZeroizeOnDrop;

pub const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
// Claude.ai-account login. Anthropic migrated off claude.ai/oauth/authorize
// and console.anthropic.com/v1/oauth/token to the claude.com / platform.claude.com
// hosts; the old URLs now return a generic "Invalid request format" page.
pub const AUTHORIZE_URL: &str = "https://claude.com/cai/oauth/authorize";
pub const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
pub const SCOPES: &str =
    "org:create_api_key user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";
// Anthropic only issues long-lived (>1h) tokens for inference-only scope —
// matches `mode === 'setup-token'` in claude-code's ConsoleOAuthFlow.
pub const INFERENCE_ONLY_SCOPES: &str = "user:inference";
pub const LONG_LIVED_EXPIRES_IN_SECS: u64 = 365 * 24 * 60 * 60;

#[derive(Debug, Clone, ZeroizeOnDrop)]
pub struct PkcePair {
    pub verifier: String,
    pub challenge: String,
    pub state: String,
}

pub fn generate_pkce() -> PkcePair {
    let mut verifier_bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut verifier_bytes);
    let verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);

    let challenge_bytes = Sha256::digest(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(challenge_bytes);

    // 32 bytes (43-char base64url) — claude.com's authorize endpoint
    // rejects shorter state values with "Invalid request format". Matches
    // claude-code's services/oauth/crypto.ts:generateState (randomBytes(32)).
    let mut state_bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut state_bytes);
    let state = URL_SAFE_NO_PAD.encode(state_bytes);

    PkcePair { verifier, challenge, state }
}

pub fn build_authorize_url(
    pkce: &PkcePair,
    redirect_uri: &str,
    inference_only: bool,
) -> Result<String> {
    let scope = if inference_only { INFERENCE_ONLY_SCOPES } else { SCOPES };
    let mut url = Url::parse(AUTHORIZE_URL)?;
    url.query_pairs_mut()
        .append_pair("code", "true")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", scope)
        .append_pair("code_challenge", &pkce.challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &pkce.state);
    Ok(url.into())
}

// Chrome refuses to load `http://localhost:PORT/...` URLs for ports on this
// list with ERR_UNSAFE_PORT, breaking the OAuth redirect even though the
// authorize server completes successfully. Mirrors Chromium's
// `net/base/port_util.cc` kRestrictedPorts. The OS-assigned ephemeral port
// occasionally lands here (we hit 6697 in the wild) — rebind if so.
const CHROME_UNSAFE_PORTS: &[u16] = &[
    1, 7, 9, 11, 13, 15, 17, 19, 20, 21, 22, 23, 25, 37, 42, 43, 53, 69, 77, 79, 87, 95, 101, 102,
    103, 104, 109, 110, 111, 113, 115, 117, 119, 123, 135, 137, 139, 143, 161, 179, 389, 427, 465,
    512, 513, 514, 515, 526, 530, 531, 532, 540, 548, 554, 556, 563, 587, 601, 636, 989, 990, 993,
    995, 1719, 1720, 1723, 2049, 3659, 4045, 4190, 5060, 5061, 6000, 6566, 6665, 6666, 6667, 6668,
    6669, 6697, 10080,
];

// Browser-facing callback pages. Self-contained: inline CSS, no external
// assets. Colors and radii mirror `src/styles/tokens.css` so the page reads
// as the same product the user just came from. The success variant uses the
// `--color-safe` (warm mint) accent, the failure variant uses `--color-danger`
// (warm coral).
const CALLBACK_SUCCESS_HTML: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Authorization complete · Claude Switchboard</title>
<style>
:root{--bg:oklch(20% 0.012 65);--surface:oklch(28% 0.018 65 / 0.95);--border:oklch(95% 0.02 65 / 0.18);--rule:oklch(95% 0.02 65 / 0.08);--text:oklch(96% 0.01 65 / 0.96);--muted:oklch(78% 0.025 65 / 0.62);--safe:oklch(76% 0.14 162);--safe-dim:oklch(76% 0.14 162 / 0.16);}
*{box-sizing:border-box;margin:0;padding:0}
html,body{height:100%}
body{font-family:-apple-system,BlinkMacSystemFont,'SF Pro Text','Segoe UI',Inter,system-ui,sans-serif;color:var(--text);background:radial-gradient(120% 80% at 0% 0%,oklch(72% 0.10 55 / 0.10),transparent 55%),radial-gradient(120% 80% at 100% 100%,oklch(67% 0.135 38 / 0.08),transparent 55%),var(--bg);min-height:100vh;display:flex;align-items:center;justify-content:center;padding:24px;-webkit-font-smoothing:antialiased;-moz-osx-font-smoothing:grayscale;text-rendering:optimizeLegibility}
.card{width:100%;max-width:360px;background:var(--surface);border:1px solid var(--border);border-radius:14px;padding:28px 24px 22px;text-align:center}
.icon{width:48px;height:48px;border-radius:12px;margin:0 auto 18px;display:flex;align-items:center;justify-content:center;background:var(--safe-dim);color:var(--safe)}
h1{font-size:16px;font-weight:600;letter-spacing:-0.01em;margin-bottom:6px;line-height:1.3}
p{font-size:13px;color:var(--muted);line-height:1.5}
.brand{margin-top:22px;padding-top:14px;border-top:1px solid var(--rule);font-size:10.5px;color:var(--muted);letter-spacing:0.08em;text-transform:uppercase}
</style></head><body><div class="card"><div class="icon"><svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="20 6 9 17 4 12"/></svg></div><h1>Authorization complete</h1><p>You can close this tab and return to Claude Switchboard.</p><div class="brand">Claude Switchboard</div></div></body></html>"#;

const CALLBACK_FAILURE_HTML: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Authorization failed · Claude Switchboard</title>
<style>
:root{--bg:oklch(20% 0.012 65);--surface:oklch(28% 0.018 65 / 0.95);--border:oklch(95% 0.02 65 / 0.18);--rule:oklch(95% 0.02 65 / 0.08);--text:oklch(96% 0.01 65 / 0.96);--muted:oklch(78% 0.025 65 / 0.62);--danger:oklch(66% 0.20 25);--danger-dim:oklch(66% 0.20 25 / 0.16);}
*{box-sizing:border-box;margin:0;padding:0}
html,body{height:100%}
body{font-family:-apple-system,BlinkMacSystemFont,'SF Pro Text','Segoe UI',Inter,system-ui,sans-serif;color:var(--text);background:radial-gradient(120% 80% at 0% 0%,oklch(72% 0.10 55 / 0.10),transparent 55%),radial-gradient(120% 80% at 100% 100%,oklch(67% 0.135 38 / 0.08),transparent 55%),var(--bg);min-height:100vh;display:flex;align-items:center;justify-content:center;padding:24px;-webkit-font-smoothing:antialiased;-moz-osx-font-smoothing:grayscale;text-rendering:optimizeLegibility}
.card{width:100%;max-width:360px;background:var(--surface);border:1px solid var(--border);border-radius:14px;padding:28px 24px 22px;text-align:center}
.icon{width:48px;height:48px;border-radius:12px;margin:0 auto 18px;display:flex;align-items:center;justify-content:center;background:var(--danger-dim);color:var(--danger)}
h1{font-size:16px;font-weight:600;letter-spacing:-0.01em;margin-bottom:6px;line-height:1.3}
p{font-size:13px;color:var(--muted);line-height:1.5}
.brand{margin-top:22px;padding-top:14px;border-top:1px solid var(--rule);font-size:10.5px;color:var(--muted);letter-spacing:0.08em;text-transform:uppercase}
</style></head><body><div class="card"><div class="icon"><svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg></div><h1>Authorization failed</h1><p>Something went wrong handling the redirect. Return to Claude Switchboard and try again.</p><div class="brand">Claude Switchboard</div></div></body></html>"#;

/// Binds an ephemeral HTTP server on a random loopback port. Returns the port
/// and a receiver that resolves to `(code, state)` when the browser hits
/// `/callback`. Mirrors how Claude Code handles the OAuth redirect.
pub async fn start_local_callback_server(
) -> Result<(u16, tokio::sync::oneshot::Receiver<Result<(String, String)>>)> {
    let mut listener_opt = None;
    for _ in 0..20 {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let p = l.local_addr()?.port();
        if !CHROME_UNSAFE_PORTS.contains(&p) {
            listener_opt = Some((l, p));
            break;
        }
        // Drop `l` to release the port, then loop.
    }
    let (listener, port) = listener_opt
        .ok_or_else(|| anyhow!("could not bind a Chrome-safe loopback port after 20 attempts"))?;
    let (tx, rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = vec![0u8; 8192];
            let n = stream.read(&mut buf).await.unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]);
            let result = parse_callback_request(&request);

            let (status_line, body) = if result.is_ok() {
                ("200 OK", CALLBACK_SUCCESS_HTML)
            } else {
                ("400 Bad Request", CALLBACK_FAILURE_HTML)
            };
            let response = format!(
                "HTTP/1.1 {status_line}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = tx.send(result);
        }
    });

    Ok((port, rx))
}

fn parse_callback_request(request: &str) -> Result<(String, String)> {
    // "GET /callback?code=X&state=Y HTTP/1.1"
    let first_line = request.lines().next().unwrap_or("");
    let path = first_line.split_whitespace().nth(1).unwrap_or("");
    let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");

    let mut code = None;
    let mut state = None;
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            match k {
                "code" => code = Some(v.to_string()),
                "state" => state = Some(v.to_string()),
                _ => {}
            }
        }
    }

    Ok((
        code.ok_or_else(|| anyhow!("Missing code in callback"))?,
        state.ok_or_else(|| anyhow!("Missing state in callback"))?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_verifier_and_challenge_are_distinct() {
        let p = generate_pkce();
        assert_ne!(p.verifier, p.challenge);
        assert!(p.state.len() >= 16);
    }

    #[test]
    fn authorize_url_contains_expected_params() {
        let p = generate_pkce();
        let redirect = "http://127.0.0.1:12345/callback";
        let url = build_authorize_url(&p, redirect, false).unwrap();
        assert!(url.contains("code=true"));
        assert!(url.contains("client_id=9d1c250a-e61b-44d9-88ed-5944d1962f5e"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains(&format!("state={}", p.state)));
        assert!(url.contains("127.0.0.1"));
        assert!(url.contains("user%3Aprofile"));
    }

    #[test]
    fn authorize_url_inference_only_uses_narrow_scope() {
        let p = generate_pkce();
        let url = build_authorize_url(&p, "http://127.0.0.1:1/callback", true).unwrap();
        // Long-lived tokens must request `user:inference` only — full-scope
        // long-lived tokens are rejected by Anthropic.
        assert!(url.contains("scope=user%3Ainference"));
        assert!(!url.contains("user%3Aprofile"));
        assert!(!url.contains("org%3Acreate_api_key"));
    }

    #[test]
    fn parse_callback_extracts_code_and_state() {
        let req = "GET /callback?code=abc123&state=xyz HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        let (code, state) = parse_callback_request(req).unwrap();
        assert_eq!(code, "abc123");
        assert_eq!(state, "xyz");
    }

    #[test]
    fn parse_callback_rejects_missing_code() {
        let req = "GET /callback?state=xyz HTTP/1.1\r\n\r\n";
        assert!(parse_callback_request(req).is_err());
    }
}
