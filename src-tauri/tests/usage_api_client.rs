use claude_switchboard_lib::usage_api::{FetchOutcome, UsageClient};
use mockito::Server;

#[tokio::test]
async fn handles_200_response() {
    let mut server = Server::new_async().await;
    let body = include_str!("fixtures/api_responses/standard_account.json");
    let _m = server
        .mock("GET", "/")
        .match_header("authorization", "Bearer tok")
        .match_header("anthropic-beta", "oauth-2025-04-20")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create_async()
        .await;

    let c = UsageClient::with_base_url(server.url(), "0.0.0-test".into()).unwrap();
    match c.fetch("tok").await {
        FetchOutcome::Ok(snap) => assert!(snap.five_hour.is_some()),
        other => panic!("expected Ok, got {other:?}"),
    }
}

#[tokio::test]
async fn handles_401() {
    let mut server = Server::new_async().await;
    let _m = server.mock("GET", "/").with_status(401).create_async().await;
    let c = UsageClient::with_base_url(server.url(), "0.0.0-test".into()).unwrap();
    assert!(matches!(c.fetch("tok").await, FetchOutcome::Unauthorized));
}

#[tokio::test]
async fn handles_429() {
    let mut server = Server::new_async().await;
    let _m = server.mock("GET", "/").with_status(429).create_async().await;
    let c = UsageClient::with_base_url(server.url(), "0.0.0-test".into()).unwrap();
    assert!(matches!(
        c.fetch("tok").await,
        FetchOutcome::RateLimited(_)
    ));
}

#[tokio::test]
async fn handles_5xx_as_transient() {
    let mut server = Server::new_async().await;
    let _m = server.mock("GET", "/").with_status(503).create_async().await;
    let c = UsageClient::with_base_url(server.url(), "0.0.0-test".into()).unwrap();
    assert!(matches!(c.fetch("tok").await, FetchOutcome::Transient(_)));
}
