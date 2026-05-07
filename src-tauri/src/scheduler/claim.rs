//! Transactional claim — the only cross-process synchronization point
//! between the GUI tokio scheduler and the headless `--tick` invocation.
//! Per spec §7.

use anyhow::Result;
use rusqlite::Connection;

const CLAIM_WINDOW_SECS: i64 = 60;

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

    fn insert_account(conn: &Connection, id: &str, warmup_enabled: i64, last_warmup_at: Option<i64>) {
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
