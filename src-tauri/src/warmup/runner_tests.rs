use super::*;
use crate::auth::accounts::AddSource;
use crate::scheduler::HhMm;
use std::sync::atomic::{AtomicUsize, Ordering};

struct FakeBackend {
    calls: AtomicUsize,
    outcome: WarmupOutcome,
    endpoint: Option<String>,
}

fn account() -> ManagedAccount {
    ManagedAccount {
        slot: 1,
        email: "fixture@example.invalid".into(),
        label: None,
        account_uuid: "fixture".into(),
        organization_uuid: None,
        organization_name: None,
        subscription_type: None,
        source: AddSource::LongLivedToken,
        claude_code_oauth_blob: serde_json::json!({"accessToken":"fixture-token"}),
        oauth_account_blob: serde_json::Value::Null,
        token_expires_at: None,
        added_at: Utc::now(),
        last_seen_active: None,
    }
}

#[async_trait]
impl Backend for FakeBackend {
    fn accounts(&self) -> Result<Vec<ManagedAccount>> {
        Ok(vec![account()])
    }
    async fn issue(&self, _: &ManagedAccount) -> Result<WarmupOutcome> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::task::yield_now().await;
        if let Some(endpoint) = &self.endpoint {
            let response = crate::warmup::api_call::issue_to(
                &reqwest::Client::new(),
                "fixture-token",
                endpoint,
            )
            .await?;
            Ok(crate::warmup::errors::classify_status(
                response.status().as_u16(),
            ))
        } else {
            Ok(self.outcome.clone())
        }
    }
}

fn fixture() -> (tempfile::TempDir, Db, FakeBackend, DateTime<Local>) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(dir.path()).unwrap();
    let now = Local::now();
    let missed = now - chrono::Duration::minutes(2);
    use chrono::Timelike;
    let schedule = Schedule::Custom {
        times: vec![HhMm::new(missed.hour() as u8, missed.minute() as u8)],
    };
    db.conn().execute("INSERT INTO accounts(id,email,last_seen_at,warmup_enabled,schedule) VALUES ('fixture','fixture@example.invalid',0,1,?1)", [serde_json::to_string(&schedule).unwrap()]).unwrap();
    db.conn()
        .execute(
            "UPDATE settings SET value='1' WHERE key='warmup_consent_granted'",
            [],
        )
        .unwrap();
    (
        dir,
        db,
        FakeBackend {
            calls: AtomicUsize::new(0),
            outcome: WarmupOutcome::Success,
            endpoint: None,
        },
        now,
    )
}

#[tokio::test]
async fn missed_fire_catches_up_once_across_processes_and_posts_real_payload() {
    let (dir, db, mut backend, now) = fixture();
    let mut server = mockito::Server::new_async().await;
    let request = server
        .mock("POST", "/")
        .match_header("authorization", "Bearer fixture-token")
        .match_header("anthropic-beta", "oauth-2025-04-20")
        .match_body(mockito::Matcher::PartialJson(
            serde_json::json!({"max_tokens":1}),
        ))
        .with_status(200)
        .expect(1)
        .create_async()
        .await;
    backend.endpoint = Some(server.url());
    let other_process = Db::open_for_tick(dir.path()).unwrap();
    let (gui, tick) = tokio::join!(
        scheduled(&db, &backend, now),
        scheduled(&other_process, &backend, now)
    );
    assert_eq!(gui.unwrap().attempted + tick.unwrap().attempted, 1);
    assert_eq!(
        scheduled(&other_process, &backend, now + chrono::Duration::minutes(3))
            .await
            .unwrap()
            .attempted,
        0
    );
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    let state = claim::run_state(&db.conn(), "fixture").unwrap();
    assert_eq!(state.last_result.as_deref(), Some("success"));
    assert!(state.last_success_at.is_some());
    request.assert_async().await;
}

#[tokio::test]
async fn persisted_active_window_skips_http_and_credentials() {
    let (_dir, db, backend, now) = fixture();
    let snapshot = serde_json::json!({"five_hour":{"utilization":20,"resets_at":Utc::now()+chrono::Duration::hours(2)},"fetched_at":Utc::now()});
    db.insert_snapshot("fixture", Utc::now(), &snapshot.to_string())
        .unwrap();
    assert_eq!(scheduled(&db, &backend, now).await.unwrap().attempted, 1);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
    let state = claim::run_state(&db.conn(), "fixture").unwrap();
    assert_eq!(state.last_result.as_deref(), Some("already_active"));
    assert!(state.last_success_at.is_none());
}

#[tokio::test]
async fn consent_disabled_accounts_and_off_schedules_never_fire() {
    let (_dir, db, backend, now) = fixture();
    db.conn()
        .execute(
            "UPDATE settings SET value='0' WHERE key='warmup_consent_granted'",
            [],
        )
        .unwrap();
    assert_eq!(scheduled(&db, &backend, now).await.unwrap().attempted, 0);
    assert!(manual(&db, &backend, "fixture", None).await.is_err());
    db.conn()
        .execute(
            "UPDATE settings SET value='1' WHERE key='warmup_consent_granted'",
            [],
        )
        .unwrap();
    db.conn()
        .execute("UPDATE accounts SET warmup_enabled=0", [])
        .unwrap();
    assert_eq!(scheduled(&db, &backend, now).await.unwrap().attempted, 0);
    db.conn()
        .execute(
            "UPDATE accounts SET warmup_enabled=1,schedule=?1",
            [r#"{"type":"Off"}"#],
        )
        .unwrap();
    assert_eq!(scheduled(&db, &backend, now).await.unwrap().attempted, 0);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn failures_are_visible_and_retry_backoff_survives_a_new_process() {
    let (dir, db, mut backend, now) = fixture();
    backend.outcome = WarmupOutcome::NetworkError;
    assert_eq!(scheduled(&db, &backend, now).await.unwrap().failed, 1);
    let other = Db::open_for_tick(dir.path()).unwrap();
    assert_eq!(
        scheduled(&other, &backend, now + chrono::Duration::seconds(90))
            .await
            .unwrap()
            .attempted,
        0
    );
    let state = claim::run_state(&other.conn(), "fixture").unwrap();
    assert_eq!(state.last_result.as_deref(), Some("network_error"));
    assert!(state.last_error.unwrap().contains("Network"));
    assert!(state.last_success_at.is_none());
    backend.outcome = WarmupOutcome::Success;
    assert_eq!(
        scheduled(&other, &backend, now + chrono::Duration::seconds(301))
            .await
            .unwrap()
            .failed,
        0
    );
    assert_eq!(backend.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn expired_crash_lease_is_recovered_and_old_owner_cannot_overwrite_result() {
    let (_dir, db, backend, now) = fixture();
    let schedule: Schedule = serde_json::from_str(
        &db.conn()
            .query_row("SELECT schedule FROM accounts", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
    )
    .unwrap();
    let fire = catchup::most_recent_expected_fire(&schedule, now)
        .unwrap()
        .timestamp();
    let old = claim::claim_run(&db.conn(), "fixture", Some(fire), now.timestamp())
        .unwrap()
        .unwrap();
    assert_eq!(
        scheduled(&db, &backend, now + chrono::Duration::seconds(90))
            .await
            .unwrap()
            .attempted,
        0
    );
    assert_eq!(
        scheduled(&db, &backend, now + chrono::Duration::seconds(121))
            .await
            .unwrap()
            .attempted,
        1
    );
    claim::finish_run(
        &db.conn(),
        &old,
        &Ok(WarmupOutcome::NetworkError),
        now.timestamp() + 122,
    )
    .unwrap();
    assert_eq!(
        claim::run_state(&db.conn(), "fixture")
            .unwrap()
            .last_result
            .as_deref(),
        Some("success")
    );
}

/// Ages the account's last claim so the 60-second double-click guard is out of
/// the way and a test can isolate the rule it is about.
fn age_last_claim(db: &Db, seconds: i64) {
    db.conn()
        .execute("UPDATE accounts SET last_warmup_at = last_warmup_at - ?1", [seconds])
        .unwrap();
}

/// A rate-limited warm-up sets a 30-minute automatic retry back-off. That
/// back-off stops the *scheduler* hammering a limited account. A person who
/// clicks "Try again" asked on purpose, and must not be refused by it.
#[tokio::test]
async fn a_manual_retry_is_not_blocked_by_the_automatic_backoff() {
    let (_dir, db, mut backend, now) = fixture();
    backend.outcome = WarmupOutcome::AtRateLimit;
    assert_eq!(scheduled(&db, &backend, now).await.unwrap().failed, 1);
    let state = claim::run_state(&db.conn(), "fixture").unwrap();
    assert_eq!(state.last_result.as_deref(), Some("rate_limited"));

    // Two minutes on: past the double-click guard, far inside the 30-minute back-off.
    age_last_claim(&db, 120);
    backend.outcome = WarmupOutcome::Success;
    let outcome = manual(&db, &backend, "fixture", None).await.expect("a deliberate retry must run");
    assert_eq!(outcome, WarmupOutcome::Success);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 2);
    let state = claim::run_state(&db.conn(), "fixture").unwrap();
    assert_eq!(state.last_result.as_deref(), Some("success"));
}

/// The scheduler, on the other hand, still waits the back-off out.
#[tokio::test]
async fn the_scheduler_still_waits_out_a_rate_limit_backoff_after_a_manual_failure() {
    let (_dir, db, mut backend, now) = fixture();
    backend.outcome = WarmupOutcome::AtRateLimit;
    manual(&db, &backend, "fixture", None).await.unwrap();
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        scheduled(&db, &backend, now + chrono::Duration::minutes(10)).await.unwrap().attempted,
        0,
        "a failed manual run still starts the automatic back-off"
    );
}

/// Two runs for one account must never overlap: a run in flight holds a lease.
#[tokio::test]
async fn a_manual_click_is_still_refused_while_a_run_is_in_flight() {
    let (_dir, db, backend, _now) = fixture();
    let _in_flight = claim::claim_run(&db.conn(), "fixture", None, Utc::now().timestamp())
        .unwrap()
        .expect("the first claim wins");
    age_last_claim(&db, 120);
    let err = manual(&db, &backend, "fixture", None).await.unwrap_err().to_string();
    assert!(err.contains("already running"), "{err}");
    assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
}

/// A double click, or the scheduler and a click landing together.
#[tokio::test]
async fn a_second_manual_click_within_a_minute_is_refused_with_an_accurate_message() {
    let (_dir, db, backend, _now) = fixture();
    manual(&db, &backend, "fixture", None).await.unwrap();
    let err = manual(&db, &backend, "fixture", None).await.unwrap_err().to_string();
    assert!(err.contains("less than a minute ago"), "{err}");
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
}

/// A refusal that is not a usage limit is recorded as such, with its reason in
/// the message a person sees, and the scheduler backs off like it does for a
/// limit (it must not hammer an account that is refusing it).
#[tokio::test]
async fn a_refusal_is_recorded_with_its_reason_and_the_scheduler_backs_off() {
    let (_dir, db, mut backend, now) = fixture();
    backend.outcome = WarmupOutcome::Refused {
        detail: "HTTP 429, not a usage limit; request req_abc".into(),
    };
    assert_eq!(scheduled(&db, &backend, now).await.unwrap().failed, 1);
    let state = claim::run_state(&db.conn(), "fixture").unwrap();
    assert_eq!(state.last_result.as_deref(), Some("refused"));
    let message = state.last_error.unwrap();
    assert!(message.contains("refused"), "{message}");
    assert!(message.contains("req_abc"), "{message}");
    assert!(!message.contains("rate limited"), "it must not claim a limit: {message}");
    assert_eq!(
        scheduled(&db, &backend, now + chrono::Duration::minutes(10)).await.unwrap().attempted,
        0,
        "30-minute back-off"
    );
}
