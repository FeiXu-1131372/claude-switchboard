use anyhow::{bail, Result};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use claude_switchboard_lib::auth::{
    account_identity::{IdentityFetcher, UserInfo},
    accounts::{store, token_import::validate_token, AccountManager, AddSource},
    exchange::TokenExchange,
    keychain_guardian::CredIO,
    orchestrator::{AuthOrchestrator, LiveClaudeCode},
    StoredToken,
};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tempfile::tempdir;

const TOKEN: &str = "sk-ant-oat01-synthetic_token_for_tests_only";
const OTHER_TOKEN: &str = "sk-ant-oat01-another_synthetic_test_token";

#[tokio::test]
async fn imports_inference_only_token_without_cli_or_invented_identity() {
    let dir = tempdir().unwrap();
    let manager = AccountManager::new(dir.path().to_owned());
    let mut server = mockito::Server::new_async().await;
    let request = server
        .mock("GET", "/profile")
        .match_header("authorization", format!("Bearer {TOKEN}").as_str())
        .with_status(403)
        .expect(1)
        .create_async()
        .await;
    let identity = IdentityFetcher::with_endpoint(format!("{}/profile", server.url()));
    let slot = manager
        .import_long_lived_token(TOKEN, None, None, &identity)
        .await
        .unwrap();
    let account = manager.get(slot).unwrap().unwrap();
    assert_eq!(account.source, AddSource::LongLivedToken);
    assert!(account.email.is_empty());
    assert!(account.account_uuid.starts_with("token:"));
    assert!(account.oauth_account_blob.is_null());
    assert!(account.token_expires_at.is_none());
    assert!(account.claude_code_oauth_blob.get("expiresAt").is_none());
    assert!(!account.supports_usage());
    assert!(!account.can_refresh());
    assert_eq!(account.display_name(), "Token account 1");
    let live = LiveClaudeCode::from_blobs(account.claude_code_oauth_blob.clone(), None).unwrap();
    assert!(account.matches_credentials(&live.claude_code_oauth_blob, &live.oauth_account_blob));
    assert!(manager
        .get(slot)
        .unwrap()
        .unwrap()
        .token_expires_at
        .is_none());
    request.assert_async().await;
}

#[tokio::test]
async fn imports_verified_profile_when_token_has_permission() {
    let dir = tempdir().unwrap();
    let manager = AccountManager::new(dir.path().to_owned());
    let mut server = mockito::Server::new_async().await;
    let request = server.mock("GET", "/profile").with_status(200)
        .with_body(r#"{"account":{"uuid":"verified-id","email":"verified@example.invalid","display_name":"Verified"}}"#)
        .create_async().await;
    let identity = IdentityFetcher::with_endpoint(format!("{}/profile", server.url()));
    let slot = manager
        .import_long_lived_token(TOKEN, Some(" Work "), None, &identity)
        .await
        .unwrap();
    let account = manager.get(slot).unwrap().unwrap();
    assert_eq!(account.account_uuid, "verified-id");
    assert_eq!(account.email, "verified@example.invalid");
    assert_eq!(account.display_name(), "Work");
    assert!(account.supports_usage());
    assert!(!account.can_refresh());
    request.assert_async().await;
}

#[tokio::test]
async fn invalid_or_revoked_tokens_leave_store_untouched_and_errors_redacted() {
    let dir = tempdir().unwrap();
    let manager = AccountManager::new(dir.path().to_owned());
    for invalid in [
        "",
        "sk-ant-api03-not-an-oauth-token",
        "claude setup-token",
        "sk-ant-oat01-short",
        "sk-ant-oat01-no whitespace inside",
    ] {
        assert!(validate_token(invalid).is_err());
    }
    let mut server = mockito::Server::new_async().await;
    let request = server
        .mock("GET", "/profile")
        .with_status(401)
        .with_body(format!("do not display {TOKEN}"))
        .create_async()
        .await;
    let identity = IdentityFetcher::with_endpoint(format!("{}/profile", server.url()));
    let error = manager
        .import_long_lived_token(TOKEN, None, None, &identity)
        .await
        .unwrap_err();
    assert!(!error.to_string().contains(TOKEN));
    assert!(!dir.path().join("accounts.json").exists());
    request.assert_async().await;
}

#[tokio::test]
async fn transient_profile_failure_does_not_silently_save_an_unchecked_token() {
    let dir = tempdir().unwrap();
    let manager = AccountManager::new(dir.path().to_owned());
    let mut server = mockito::Server::new_async().await;
    let request = server
        .mock("GET", "/profile")
        .with_status(500)
        .with_body(TOKEN)
        .create_async()
        .await;
    let identity = IdentityFetcher::with_endpoint(format!("{}/profile", server.url()));
    let error = manager
        .import_long_lived_token(TOKEN, None, None, &identity)
        .await
        .unwrap_err();
    assert!(!error.to_string().contains(TOKEN));
    assert!(!dir.path().join("accounts.json").exists());
    request.assert_async().await;
}

#[tokio::test]
async fn preserves_existing_oauth_credentials_and_rejects_duplicate_tokens() {
    let dir = tempdir().unwrap();
    let manager = AccountManager::new(dir.path().to_owned());
    let original_slot = manager
        .add_from_oauth(
            StoredToken {
                access_token: "ordinary-access-token".into(),
                refresh_token: Some("ordinary-refresh-token".into()),
                expires_at: Utc::now() + Duration::hours(8),
            },
            UserInfo {
                id: "ordinary-id".into(),
                email: "ordinary@example.invalid".into(),
                name: None,
                organization_uuid: None,
                organization_name: None,
            },
        )
        .await
        .unwrap();
    let original = serde_json::to_value(manager.get(original_slot).unwrap().unwrap()).unwrap();
    let mut server = mockito::Server::new_async().await;
    let request = server
        .mock("GET", "/profile")
        .with_status(403)
        .expect(2)
        .create_async()
        .await;
    let identity = IdentityFetcher::with_endpoint(format!("{}/profile", server.url()));
    let token_slot = manager
        .import_long_lived_token(TOKEN, Some("Shared"), None, &identity)
        .await
        .unwrap();
    let before_duplicate = std::fs::read(dir.path().join("accounts.json")).unwrap();
    assert!(manager
        .import_long_lived_token(TOKEN, None, None, &identity)
        .await
        .is_err());
    assert_eq!(
        std::fs::read(dir.path().join("accounts.json")).unwrap(),
        before_duplicate
    );
    assert_ne!(token_slot, original_slot);
    assert_eq!(
        serde_json::to_value(manager.get(original_slot).unwrap().unwrap()).unwrap(),
        original
    );
    request.assert_async().await;
}

#[tokio::test]
async fn replacing_an_imported_token_preserves_slot_label_and_local_identifier() {
    let dir = tempdir().unwrap();
    let manager = AccountManager::new(dir.path().to_owned());
    let mut server = mockito::Server::new_async().await;
    let request = server
        .mock("GET", "/profile")
        .with_status(403)
        .expect(2)
        .create_async()
        .await;
    let identity = IdentityFetcher::with_endpoint(format!("{}/profile", server.url()));
    let slot = manager
        .import_long_lived_token(TOKEN, Some("Shared"), None, &identity)
        .await
        .unwrap();
    let before = manager.get(slot).unwrap().unwrap();
    assert_eq!(
        manager
            .import_long_lived_token(OTHER_TOKEN, None, Some(slot), &identity)
            .await
            .unwrap(),
        slot
    );
    let after = manager.get(slot).unwrap().unwrap();
    assert_eq!(after.label, before.label);
    assert_eq!(after.account_uuid, before.account_uuid);
    assert_eq!(after.added_at, before.added_at);
    assert_eq!(after.claude_code_oauth_blob["accessToken"], OTHER_TOKEN);
    assert!(!after.matches_credentials(&before.claude_code_oauth_blob, &Value::Null));
    request.assert_async().await;
}

#[tokio::test]
async fn unknown_expiry_never_attempts_oauth_refresh() {
    let dir = tempdir().unwrap();
    let manager = AccountManager::new(dir.path().to_owned());
    let mut server = mockito::Server::new_async().await;
    let _profile = server
        .mock("GET", "/profile")
        .with_status(403)
        .create_async()
        .await;
    let refresh = server.mock("POST", "/token").expect(0).create_async().await;
    let identity = IdentityFetcher::with_endpoint(format!("{}/profile", server.url()));
    let slot = manager
        .import_long_lived_token(TOKEN, None, None, &identity)
        .await
        .unwrap();
    let auth = AuthOrchestrator::with_collaborators(
        dir.path().to_owned(),
        TokenExchange::with_endpoint(format!("{}/token", server.url())),
        identity,
    );
    assert_eq!(
        auth.token_for_slot(slot, None, &manager).await.unwrap(),
        TOKEN
    );
    refresh.assert_async().await;
}

#[derive(Default)]
struct MemoryCredentials {
    cc: Mutex<Option<Value>>,
    account: Mutex<Option<Value>>,
    fail_account_write: AtomicBool,
}

#[async_trait]
impl CredIO for MemoryCredentials {
    async fn load_cc(&self) -> Result<Option<Value>> {
        Ok(self.cc.lock().unwrap().clone())
    }
    async fn load_oauth_account(&self) -> Result<Option<Value>> {
        Ok(self.account.lock().unwrap().clone())
    }
    async fn write_cc(&self, value: &Value) -> Result<()> {
        *self.cc.lock().unwrap() = (!value.is_null()).then(|| value.clone());
        Ok(())
    }
    async fn write_oauth_account(&self, value: &Value) -> Result<()> {
        if self.fail_account_write.swap(false, Ordering::SeqCst) {
            bail!("simulated config write failure");
        }
        *self.account.lock().unwrap() = (!value.is_null()).then(|| value.clone());
        Ok(())
    }
}

#[tokio::test]
async fn switches_without_profile_and_restores_previous_account_on_failure() {
    let dir = tempdir().unwrap();
    let manager = AccountManager::new(dir.path().to_owned());
    let mut server = mockito::Server::new_async().await;
    let _profile = server
        .mock("GET", "/profile")
        .with_status(403)
        .create_async()
        .await;
    let identity = IdentityFetcher::with_endpoint(format!("{}/profile", server.url()));
    let slot = manager
        .import_long_lived_token(TOKEN, None, None, &identity)
        .await
        .unwrap();
    let cc = json!({"accessToken":"old-access","refreshToken":"old-refresh"});
    let account = json!({"accountUuid":"old-account","emailAddress":"old@example.invalid"});
    let io = MemoryCredentials {
        cc: Mutex::new(Some(cc.clone())),
        account: Mutex::new(Some(account.clone())),
        fail_account_write: AtomicBool::new(true),
    };
    assert!(manager.swap_to_with_io(slot, &io).await.is_err());
    assert_eq!(io.load_cc().await.unwrap(), Some(cc));
    assert_eq!(io.load_oauth_account().await.unwrap(), Some(account));
    manager.swap_to_with_io(slot, &io).await.unwrap();
    assert_eq!(io.load_cc().await.unwrap().unwrap()["accessToken"], TOKEN);
    assert!(io.load_oauth_account().await.unwrap().is_none());
}

#[tokio::test]
async fn failed_first_switch_does_not_leave_new_credentials_behind() {
    let dir = tempdir().unwrap();
    let manager = AccountManager::new(dir.path().to_owned());
    let mut server = mockito::Server::new_async().await;
    let _profile = server
        .mock("GET", "/profile")
        .with_status(403)
        .create_async()
        .await;
    let identity = IdentityFetcher::with_endpoint(format!("{}/profile", server.url()));
    let slot = manager
        .import_long_lived_token(TOKEN, None, None, &identity)
        .await
        .unwrap();
    let io = MemoryCredentials::default();
    io.fail_account_write.store(true, Ordering::SeqCst);
    assert!(manager.swap_to_with_io(slot, &io).await.is_err());
    assert!(io.load_cc().await.unwrap().is_none());
    assert!(io.load_oauth_account().await.unwrap().is_none());
}

#[test]
fn old_account_store_still_loads_and_labels_cannot_contain_a_token() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("accounts.json"),
        r#"{
      "schema_version":1,"accounts":{"1":{
        "slot":1,"email":"old@example.invalid","account_uuid":"old",
        "organization_uuid":null,"organization_name":null,"subscription_type":null,
        "source":"OAuth","claude_code_oauth_blob":{},"oauth_account_blob":{},
        "token_expires_at":"2026-10-27T00:00:00Z","added_at":"2026-10-27T00:00:00Z",
        "last_seen_active":null
      }}}"#,
    )
    .unwrap();
    let loaded = store::load(dir.path()).unwrap();
    assert!(loaded.accounts[&1].label.is_none());
    assert!(loaded.accounts[&1].token_expires_at.is_some());
    assert!(
        claude_switchboard_lib::auth::accounts::token_import::validate_label(Some(TOKEN)).is_err()
    );
    assert!(
        claude_switchboard_lib::auth::accounts::token_import::validate_label(Some(&"x".repeat(81)))
            .is_err()
    );
}
