use claude_switchboard_lib::auth::exchange::TokenExchange;
use mockito::Server;

#[tokio::test]
async fn successful_code_exchange() {
    let mut server = Server::new_async().await;
    let _m = server
        .mock("POST", "/")
        .match_header("content-type", "application/json")
        .match_body(mockito::Matcher::JsonString(
            serde_json::json!({
                "grant_type": "authorization_code",
                "code": "abc",
                "redirect_uri": "http://localhost:1234/callback",
                "client_id": claude_switchboard_lib::auth::oauth_paste_back::CLIENT_ID,
                "code_verifier": "verif",
                "state": "st1",
            })
            .to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"access_token":"acc","refresh_token":"ref","expires_in":3600,"token_type":"Bearer"}"#)
        .create_async()
        .await;

    let ex = TokenExchange::with_endpoint(server.url());
    let tok = ex
        .exchange_code("abc", "verif", "http://localhost:1234/callback", "st1", None)
        .await
        .unwrap();
    assert_eq!(tok.access_token, "acc");
    assert_eq!(tok.refresh_token.as_deref(), Some("ref"));
}

#[tokio::test]
async fn long_lived_exchange_includes_expires_in() {
    let mut server = Server::new_async().await;
    let _m = server
        .mock("POST", "/")
        .match_header("content-type", "application/json")
        .match_body(mockito::Matcher::JsonString(
            serde_json::json!({
                "grant_type": "authorization_code",
                "code": "abc",
                "redirect_uri": "http://localhost:1234/callback",
                "client_id": claude_switchboard_lib::auth::oauth_paste_back::CLIENT_ID,
                "code_verifier": "verif",
                "state": "st1",
                "expires_in": 31_536_000u64,
            })
            .to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"access_token":"acc","expires_in":31536000}"#)
        .create_async()
        .await;

    let ex = TokenExchange::with_endpoint(server.url());
    let tok = ex
        .exchange_code(
            "abc",
            "verif",
            "http://localhost:1234/callback",
            "st1",
            Some(365 * 24 * 60 * 60),
        )
        .await
        .unwrap();
    assert_eq!(tok.access_token, "acc");
}

#[tokio::test]
async fn exchange_error_surfaces_status_not_body() {
    // Body is redacted from the returned error (logged at debug level instead)
    // to prevent token data from leaking into frontend-bound error strings.
    let mut server = Server::new_async().await;
    let _m = server
        .mock("POST", "/")
        .with_status(400)
        .with_body("bad_code")
        .create_async()
        .await;
    let ex = TokenExchange::with_endpoint(server.url());
    let err = ex
        .exchange_code("abc", "verif", "http://localhost:1234/callback", "st1", None)
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("400"), "error should include HTTP status: {msg}");
    assert!(!msg.contains("bad_code"), "error must not include response body: {msg}");
}

#[tokio::test]
async fn refresh_preserves_refresh_token_when_not_returned() {
    let mut server = Server::new_async().await;
    let _m = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"access_token":"new","expires_in":3600}"#)
        .create_async()
        .await;
    let ex = TokenExchange::with_endpoint(server.url());
    let tok = ex.refresh("old-refresh").await.unwrap();
    assert_eq!(tok.refresh_token.as_deref(), Some("old-refresh"));
}
