use super::oauth_paste_back::{CLIENT_ID, TOKEN_URL};
use super::StoredToken;
use anyhow::{anyhow, Result};
use chrono::{Duration, Utc};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: i64,
    #[allow(dead_code)]
    token_type: Option<String>,
}

pub struct TokenExchange {
    endpoint: String,
    client: Arc<reqwest::Client>,
}

impl TokenExchange {
    pub fn new(client: Arc<reqwest::Client>) -> Self {
        Self {
            endpoint: TOKEN_URL.to_string(),
            client,
        }
    }

    /// Test-only constructor: builds a fresh client pointed at a mock endpoint.
    pub fn with_endpoint(endpoint: String) -> Self {
        Self {
            endpoint,
            client: Arc::new(reqwest::Client::new()),
        }
    }

    pub async fn exchange_code(
        &self,
        code: &str,
        pkce_verifier: &str,
        redirect_uri: &str,
        state: &str,
        expires_in: Option<u64>,
    ) -> Result<StoredToken, anyhow::Error> {
        // platform.claude.com/v1/oauth/token takes a JSON body — matches
        // claude-code's services/oauth/client.ts:exchangeCodeForTokens. State
        // is echoed back through the token request. `expires_in` requests a
        // long-lived token (only honored for inference-only scope).
        let mut body = serde_json::json!({
            "grant_type": "authorization_code",
            "code": code,
            "redirect_uri": redirect_uri,
            "client_id": CLIENT_ID,
            "code_verifier": pkce_verifier,
            "state": state,
        });
        if let Some(n) = expires_in {
            body["expires_in"] = serde_json::json!(n);
        }
        tracing::debug!(target: "switchboard.auth", "POST {} (exchange_code) starting", self.endpoint);
        let start = std::time::Instant::now();
        let resp = self.client.post(&self.endpoint).json(&body).send().await?;
        let status = resp.status();
        let elapsed_ms = start.elapsed().as_millis() as u64;
        if !status.is_success() {
            // Server error body is safe to log — it carries no token. Token
            // exchange success body, however, contains a refresh_token and
            // MUST never be logged. We only call resp.text() on the error
            // path, and we read the success body straight into TokenResponse.
            let text = resp.text().await.unwrap_or_default();
            tracing::warn!(
                target: "switchboard.auth",
                status = status.as_u16(),
                elapsed_ms,
                "token exchange failed: {status}; body: {text}"
            );
            return Err(anyhow!("token exchange failed: {status}"));
        }
        let tr: TokenResponse = resp.json().await?;
        tracing::info!(
            target: "switchboard.auth",
            status = status.as_u16(),
            elapsed_ms,
            "token exchange ok (expires_in={}s, refresh_token={})",
            tr.expires_in,
            if tr.refresh_token.is_some() { "present" } else { "absent" }
        );
        Ok(StoredToken {
            access_token: tr.access_token,
            refresh_token: tr.refresh_token,
            expires_at: Utc::now() + Duration::seconds(tr.expires_in),
        })
    }

    pub async fn refresh(&self, refresh_token: &str) -> Result<StoredToken, anyhow::Error> {
        let params = [
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", CLIENT_ID),
        ];
        tracing::debug!(target: "switchboard.auth", "POST {} (refresh) starting", self.endpoint);
        let start = std::time::Instant::now();
        let resp = self.client.post(&self.endpoint).form(&params).send().await?;
        let status = resp.status();
        let elapsed_ms = start.elapsed().as_millis() as u64;
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            tracing::warn!(
                target: "switchboard.auth",
                status = status.as_u16(),
                elapsed_ms,
                "token refresh failed: {status}; body: {text}"
            );
            return Err(anyhow!("refresh failed: {status}"));
        }
        let tr: TokenResponse = resp.json().await?;
        tracing::info!(
            target: "switchboard.auth",
            status = status.as_u16(),
            elapsed_ms,
            "token refresh ok (expires_in={}s, rotated_rt={})",
            tr.expires_in,
            tr.refresh_token.is_some()
        );
        Ok(StoredToken {
            access_token: tr.access_token,
            refresh_token: tr.refresh_token.or_else(|| Some(refresh_token.to_string())),
            expires_at: Utc::now() + Duration::seconds(tr.expires_in),
        })
    }
}
