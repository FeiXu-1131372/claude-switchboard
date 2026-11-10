//! Transactional claim — the only cross-process synchronization point
//! between the GUI tokio scheduler and the headless `--tick` invocation.
//! Per spec §7.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};

const CLAIM_WINDOW_SECS: i64 = 60;

#[derive(Debug)]
pub struct RunClaim {
    pub account_id: String,
    token: String,
    scheduled_fire: Option<i64>,
}

#[derive(Debug, Default)]
pub struct RunState {
    pub last_attempt_at: Option<i64>,
    pub last_success_at: Option<i64>,
    pub last_result: Option<String>,
    pub last_error: Option<String>,
}

/// One atomic, cross-process lease. Completed scheduled occurrences are never
/// replayed; failed ones can retry after their persisted backoff expires.
pub fn claim_run(
    conn: &Connection,
    account_id: &str,
    scheduled_fire: Option<i64>,
    now: i64,
) -> Result<Option<RunClaim>> {
    let token = format!("{:016x}", rand::random::<u64>());
    let won = conn.execute(
        "INSERT INTO warmup_runs (account_id, claim_token, lease_until, last_attempt_at, last_result)
         SELECT ?1, ?2, ?3, ?4, 'running'
         WHERE EXISTS (SELECT 1 FROM accounts WHERE id = ?1 AND warmup_enabled = 1
           AND (last_warmup_at IS NULL OR last_warmup_at <= ?6))
           AND EXISTS (SELECT 1 FROM settings WHERE key = 'warmup_consent_granted' AND value = '1')
         ON CONFLICT(account_id) DO UPDATE SET claim_token = ?2, lease_until = ?3,
           last_attempt_at = ?4, last_result = 'running', last_error = NULL
         WHERE warmup_runs.lease_until <= ?4 AND warmup_runs.next_retry_at <= ?4
           AND (?5 IS NULL OR warmup_runs.completed_fire IS NULL OR warmup_runs.completed_fire < ?5)",
        rusqlite::params![account_id, token, now + 120, now, scheduled_fire, now - CLAIM_WINDOW_SECS],
    )?;
    if won == 0 {
        return Ok(None);
    }
    conn.execute(
        "UPDATE accounts SET last_warmup_at = ?1 WHERE id = ?2",
        rusqlite::params![now, account_id],
    )?;
    Ok(Some(RunClaim {
        account_id: account_id.into(),
        token,
        scheduled_fire,
    }))
}

pub fn finish_run(
    conn: &Connection,
    claim: &RunClaim,
    result: &Result<crate::warmup::WarmupOutcome>,
    now: i64,
) -> Result<()> {
    use crate::warmup::WarmupOutcome::*;
    let (status, error, retry, complete, success) = match result {
        Ok(Success) => ("success", None, 60, true, true),
        Ok(SkippedAlreadyActive) => ("already_active", None, 60, true, false),
        Ok(NeedsReauth) => (
            "needs_reauth",
            Some("Sign in to this account again.".into()),
            1800,
            false,
            false,
        ),
        Ok(AtRateLimit) => (
            "rate_limited",
            Some("Account is rate limited; retrying later.".into()),
            1800,
            false,
            false,
        ),
        Ok(NetworkError) => (
            "network_error",
            Some("Network error; retrying in five minutes.".into()),
            300,
            false,
            false,
        ),
        Ok(AnthropicServerError) => (
            "server_error",
            Some("Anthropic server error; retrying in five minutes.".into()),
            300,
            false,
            false,
        ),
        Ok(OtherFailure { status }) => (
            "error",
            Some(format!("Warm-up failed (HTTP {status}).")),
            300,
            false,
            false,
        ),
        Err(e) => ("error", Some(format!("{e:#}")), 300, false, false),
    };
    conn.execute(
        "UPDATE warmup_runs SET claim_token = NULL, lease_until = 0,
           completed_fire = CASE WHEN ?3 THEN COALESCE(?4, completed_fire) ELSE completed_fire END,
           last_success_at = CASE WHEN ?5 THEN ?6 ELSE last_success_at END,
           last_result = ?7, last_error = ?8, next_retry_at = ?9
         WHERE account_id = ?1 AND claim_token = ?2",
        rusqlite::params![
            claim.account_id,
            claim.token,
            complete,
            claim.scheduled_fire,
            success,
            now,
            status,
            error,
            now + retry
        ],
    )?;
    Ok(())
}

pub fn run_state(conn: &Connection, account_id: &str) -> Result<RunState> {
    Ok(conn.query_row(
        "SELECT last_attempt_at, last_success_at, last_result, last_error FROM warmup_runs WHERE account_id = ?1",
        [account_id], |r| Ok(RunState { last_attempt_at: r.get(0)?, last_success_at: r.get(1)?, last_result: r.get(2)?, last_error: r.get(3)? }),
    ).optional()?.unwrap_or_default())
}

/// Try to claim the right to fire a warm-up for `account_id` at `now`
/// (unix epoch seconds). Returns true if this caller won the claim and
/// should proceed; false if another process / tick already claimed it
/// within the last 60 seconds, or if `warmup_enabled = 0`.
pub fn try_claim(conn: &Connection, account_id: &str, now: i64) -> Result<bool> {
    let rows = conn.execute(
        "UPDATE accounts \
         SET last_warmup_at = ?1 \
         WHERE id = ?2 \
           AND warmup_enabled = 1 \
           AND (last_warmup_at IS NULL OR last_warmup_at < ?3)",
        rusqlite::params![now, account_id, now - CLAIM_WINDOW_SECS],
    )?;
    Ok(rows == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE accounts ( \
               id TEXT PRIMARY KEY, \
               email TEXT NOT NULL, \
               last_seen_at INTEGER NOT NULL, \
               warmup_enabled INTEGER NOT NULL DEFAULT 0, \
               schedule TEXT NOT NULL DEFAULT '{\"type\":\"Off\"}', \
               last_warmup_at INTEGER \
             );",
        )
        .unwrap();
        conn
    }

    fn insert_account(
        conn: &Connection,
        id: &str,
        warmup_enabled: i64,
        last_warmup_at: Option<i64>,
    ) {
        conn.execute(
            "INSERT INTO accounts (id, email, last_seen_at, warmup_enabled, last_warmup_at) \
             VALUES (?1, 'x@y.z', 0, ?2, ?3)",
            rusqlite::params![id, warmup_enabled, last_warmup_at],
        )
        .unwrap();
    }

    #[test]
    fn claim_succeeds_when_warmup_enabled_and_never_fired() {
        let conn = fixture_conn();
        insert_account(&conn, "a", 1, None);
        assert!(try_claim(&conn, "a", 1000).unwrap());

        let stored: i64 = conn
            .query_row(
                "SELECT last_warmup_at FROM accounts WHERE id = 'a'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, 1000);
    }

    #[test]
    fn claim_fails_when_warmup_disabled() {
        let conn = fixture_conn();
        insert_account(&conn, "a", 0, None);
        assert!(!try_claim(&conn, "a", 1000).unwrap());
    }

    #[test]
    fn claim_fails_within_60_second_window() {
        let conn = fixture_conn();
        insert_account(&conn, "a", 1, Some(1000));
        // 1030 is 30s after the last claim — inside the dedup window.
        assert!(!try_claim(&conn, "a", 1030).unwrap());
    }

    #[test]
    fn claim_succeeds_after_60_second_window() {
        let conn = fixture_conn();
        insert_account(&conn, "a", 1, Some(1000));
        // 1061 is 61s later — outside the dedup window.
        assert!(try_claim(&conn, "a", 1061).unwrap());
    }

    #[test]
    fn second_concurrent_claim_in_same_tick_loses() {
        let conn = fixture_conn();
        insert_account(&conn, "a", 1, None);
        // First claim wins.
        assert!(try_claim(&conn, "a", 1000).unwrap());
        // Second simulated concurrent claim at the same `now` loses because
        // last_warmup_at is now 1000 and 1000 < (1000 - 60) is false.
        assert!(!try_claim(&conn, "a", 1000).unwrap());
    }
}
