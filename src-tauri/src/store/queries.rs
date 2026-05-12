use super::Db;
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

use crate::app_state::Settings;

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct StoredAccount {
    pub id: String,
    pub email: String,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct StoredSessionEvent {
    #[specta(type = String)]
    pub ts: DateTime<Utc>,
    pub project: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_5m_tokens: u64,
    pub cache_creation_1h_tokens: u64,
    pub cost_usd: f64,
    pub source_file: String,
    pub source_line: i64,
    /// Stable per-API-call key used for dedup. Format: "{requestId}:{message.id}"
    /// when both are present in the JSONL line, else "{source_file}:{source_line}"
    /// as a structural fallback for older / pre-requestId schemas.
    pub event_id: String,
}

fn insert_events_in_tx(tx: &Transaction<'_>, events: &[StoredSessionEvent]) -> Result<usize> {
    if events.is_empty() {
        return Ok(0);
    }
    let mut stmt = tx.prepare(
        "INSERT OR IGNORE INTO session_events
        (ts, project, model, input_tokens, output_tokens, cache_read_tokens,
         cache_creation_5m_tokens, cache_creation_1h_tokens, cost_usd,
         source_file, source_line, event_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    )?;
    let mut inserted = 0;
    for e in events {
        inserted += stmt.execute(params![
            e.ts.timestamp(),
            e.project,
            e.model,
            e.input_tokens as i64,
            e.output_tokens as i64,
            e.cache_read_tokens as i64,
            e.cache_creation_5m_tokens as i64,
            e.cache_creation_1h_tokens as i64,
            e.cost_usd,
            e.source_file,
            e.source_line,
            e.event_id,
        ])?;
    }
    Ok(inserted)
}

impl Db {
    pub fn upsert_account(&self, acc: &StoredAccount) -> Result<()> {
        let now = Utc::now().timestamp();
        self.conn().execute(
            "INSERT INTO accounts (id, email, display_name, last_seen_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET email=excluded.email,
                                            display_name=excluded.display_name,
                                            last_seen_at=excluded.last_seen_at",
            params![acc.id, acc.email, acc.display_name, now],
        )?;
        Ok(())
    }

    /// Remove the SQLite row for `account_uuid`. Idempotent (no error when
    /// the row is already absent). Paired with AccountManager::remove so the
    /// warmup state for a removed account does not linger.
    pub fn delete_account(&self, account_uuid: &str) -> Result<()> {
        self.conn().execute(
            "DELETE FROM accounts WHERE id = ?1",
            params![account_uuid],
        )?;
        Ok(())
    }

    pub fn insert_snapshot(
        &self,
        account_id: &str,
        fetched_at: DateTime<Utc>,
        payload_json: &str,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT INTO api_snapshots (account_id, fetched_at, payload_json) VALUES (?1, ?2, ?3)",
            params![account_id, fetched_at.timestamp(), payload_json],
        )?;
        Ok(())
    }

    pub fn latest_snapshot(
        &self,
        account_id: &str,
    ) -> Result<Option<(DateTime<Utc>, String)>> {
        let conn = self.conn();
        let row = conn
            .query_row(
                "SELECT fetched_at, payload_json FROM api_snapshots
                 WHERE account_id = ?1 ORDER BY fetched_at DESC LIMIT 1",
                params![account_id],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?;
        Ok(row.map(|(ts, p)| (DateTime::from_timestamp(ts, 0).unwrap(), p)))
    }

    pub fn insert_events(&self, events: &[StoredSessionEvent]) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let inserted = insert_events_in_tx(&tx, events)?;
        tx.commit()?;
        Ok(inserted)
    }

    pub fn events_between(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<StoredSessionEvent>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT ts, project, model, input_tokens, output_tokens, cache_read_tokens,
                    cache_creation_5m_tokens, cache_creation_1h_tokens, cost_usd,
                    source_file, source_line, event_id
             FROM session_events WHERE ts BETWEEN ?1 AND ?2 ORDER BY ts DESC",
        )?;
        let rows = stmt.query_map(params![from.timestamp(), to.timestamp()], |r| {
            Ok(StoredSessionEvent {
                ts: DateTime::from_timestamp(r.get(0)?, 0).unwrap(),
                project: r.get(1)?,
                model: r.get(2)?,
                input_tokens: r.get::<_, i64>(3)? as u64,
                output_tokens: r.get::<_, i64>(4)? as u64,
                cache_read_tokens: r.get::<_, i64>(5)? as u64,
                cache_creation_5m_tokens: r.get::<_, i64>(6)? as u64,
                cache_creation_1h_tokens: r.get::<_, i64>(7)? as u64,
                cost_usd: r.get(8)?,
                source_file: r.get(9)?,
                source_line: r.get(10)?,
                event_id: r.get(11)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn prune_events_older_than(&self, cutoff: DateTime<Utc>) -> Result<usize> {
        let rows = self
            .conn()
            .execute("DELETE FROM session_events WHERE ts < ?1", params![cutoff.timestamp()])?;
        Ok(rows)
    }

    pub fn get_cursor(&self, file: &str) -> Result<Option<(i64, i64)>> {
        let conn = self.conn();
        let row = conn
            .query_row(
                "SELECT last_mtime_ns, byte_offset FROM jsonl_cursors WHERE file_path = ?1",
                params![file],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
            )
            .optional()?;
        Ok(row)
    }

    pub fn set_cursor(&self, file: &str, mtime_ns: i64, offset: i64) -> Result<()> {
        self.conn().execute(
            "INSERT INTO jsonl_cursors (file_path, last_mtime_ns, byte_offset) VALUES (?1, ?2, ?3)
             ON CONFLICT(file_path) DO UPDATE SET
               last_mtime_ns = MAX(excluded.last_mtime_ns, jsonl_cursors.last_mtime_ns),
               byte_offset   = MAX(excluded.byte_offset,   jsonl_cursors.byte_offset)",
            params![file, mtime_ns, offset],
        )?;
        Ok(())
    }

    /// Insert events and advance the JSONL cursor in a single SQLite
    /// transaction. This prevents the race where two concurrent callers
    /// (backfill task + watcher) split the two writes across transactions,
    /// letting the slower caller's cursor write regress the faster caller's
    /// progress and causing repeated re-ingestion of the same lines.
    /// Insert events and advance the JSONL cursor in a single SQLite
    /// transaction. This prevents the race where two concurrent callers
    /// (backfill task + watcher) split the two writes across transactions,
    /// letting the slower caller's cursor write regress the faster caller's
    /// progress and causing repeated re-ingestion of the same lines.
    pub fn ingest_atomic(
        &self,
        file: &str,
        events: &[StoredSessionEvent],
        mtime_ns: i64,
        byte_offset: i64,
    ) -> Result<usize> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let inserted = insert_events_in_tx(&tx, events)?;
        tx.execute(
            "INSERT INTO jsonl_cursors (file_path, last_mtime_ns, byte_offset) VALUES (?1, ?2, ?3)
             ON CONFLICT(file_path) DO UPDATE SET
               last_mtime_ns = MAX(excluded.last_mtime_ns, jsonl_cursors.last_mtime_ns),
               byte_offset   = MAX(excluded.byte_offset,   jsonl_cursors.byte_offset)",
            params![file, mtime_ns, byte_offset],
        )?;
        tx.commit()?;
        Ok(inserted)
    }

    pub fn notification_last_fired(
        &self,
        account_id: &str,
        bucket: &str,
        threshold: i64,
    ) -> Result<Option<DateTime<Utc>>> {
        let conn = self.conn();
        let row = conn
            .query_row(
                "SELECT last_fired_at FROM notification_state
                 WHERE account_id = ?1 AND bucket = ?2 AND threshold = ?3",
                params![account_id, bucket, threshold],
                |r| r.get::<_, i64>(0),
            )
            .optional()?;
        Ok(row.map(|ts| DateTime::from_timestamp(ts, 0).unwrap()))
    }

    pub fn record_notification_fired(
        &self,
        account_id: &str,
        bucket: &str,
        threshold: i64,
        at: DateTime<Utc>,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT INTO notification_state (account_id, bucket, threshold, last_fired_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(account_id, bucket, threshold) DO UPDATE SET last_fired_at=excluded.last_fired_at",
            params![account_id, bucket, threshold, at.timestamp()],
        )?;
        Ok(())
    }

    /// Persist user settings to the `settings` table as a single JSON blob
    /// keyed on `"settings"`. Subsequent calls overwrite the previous value.
    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        let json = serde_json::to_string(settings)?;
        self.conn().execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('settings', ?1)",
            params![json],
        )?;
        Ok(())
    }

    /// Load user settings from the `settings` table. Returns `None` when no
    /// row has been written yet (first launch), so the caller can fall back to
    /// `Settings::default()`.
    pub fn load_settings(&self) -> Result<Option<Settings>> {
        let conn = self.conn();
        let row = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'settings'",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        match row {
            None => Ok(None),
            Some(json) => {
                let settings = serde_json::from_str(&json)?;
                Ok(Some(settings))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Db;
    use tempfile::tempdir;

    fn fresh_db() -> (tempfile::TempDir, Db) {
        let dir = tempdir().unwrap();
        let db = Db::open(dir.path()).unwrap();
        db.upsert_account(&StoredAccount {
            id: "acc1".into(),
            email: "a@example.com".into(),
            display_name: None,
        })
        .unwrap();
        (dir, db)
    }

    #[test]
    fn snapshot_roundtrip() {
        let (_dir, db) = fresh_db();
        let now = Utc::now();
        db.insert_snapshot("acc1", now, r#"{"five_hour":null}"#)
            .unwrap();
        let (ts, payload) = db.latest_snapshot("acc1").unwrap().expect("snapshot");
        assert_eq!(ts.timestamp(), now.timestamp());
        assert!(payload.contains("five_hour"));
    }

    #[test]
    fn events_insert_and_dedupe() {
        let (_dir, db) = fresh_db();
        let e = StoredSessionEvent {
            ts: Utc::now(),
            project: "p".into(),
            model: "sonnet-4-6".into(),
            input_tokens: 10,
            output_tokens: 5,
            cache_read_tokens: 0,
            cache_creation_5m_tokens: 0,
            cache_creation_1h_tokens: 0,
            cost_usd: 0.001,
            source_file: "f.jsonl".into(),
            source_line: 1,
            event_id: "req_1:msg_1".into(),
        };
        assert_eq!(db.insert_events(std::slice::from_ref(&e)).unwrap(), 1);
        assert_eq!(
            db.insert_events(std::slice::from_ref(&e)).unwrap(),
            0,
            "same event_id is rejected"
        );
    }

    /// The exact regression that v1 missed: Claude Code can write the same
    /// `message.usage` block to multiple offsets in the same file (retries,
    /// partial rewinds). Different (source_file, source_line) but identical
    /// event_id — must dedupe.
    #[test]
    fn dedupe_catches_same_event_at_different_offsets() {
        let (_dir, db) = fresh_db();
        let mk = |line: i64| StoredSessionEvent {
            ts: Utc::now(),
            project: "p".into(),
            model: "opus-4-7".into(),
            input_tokens: 6,
            output_tokens: 332,
            cache_read_tokens: 19099,
            cache_creation_5m_tokens: 0,
            cache_creation_1h_tokens: 396681,
            cost_usd: 11.95,
            source_file: "/Users/me/.claude/projects/p/abc.jsonl".into(),
            source_line: line,
            event_id: "req_abc:msg_xyz".into(),
        };
        // Same event written at offsets 100, 1000, 2000 — only the first lands.
        assert_eq!(db.insert_events(&[mk(100), mk(1000), mk(2000)]).unwrap(), 1);
    }

    #[test]
    fn cursor_roundtrip() {
        let (_dir, db) = fresh_db();
        assert!(db.get_cursor("f.jsonl").unwrap().is_none());
        db.set_cursor("f.jsonl", 123, 456).unwrap();
        assert_eq!(db.get_cursor("f.jsonl").unwrap(), Some((123, 456)));
    }

    #[test]
    fn notification_state_roundtrip() {
        let (_dir, db) = fresh_db();
        assert!(db
            .notification_last_fired("acc1", "five_hour", 75)
            .unwrap()
            .is_none());
        let now = Utc::now();
        db.record_notification_fired("acc1", "five_hour", 75, now)
            .unwrap();
        let got = db
            .notification_last_fired("acc1", "five_hour", 75)
            .unwrap()
            .unwrap();
        assert_eq!(got.timestamp(), now.timestamp());
    }

    #[test]
    fn settings_roundtrip() {
        let (_dir, db) = fresh_db();

        // No row written yet — should return None.
        assert!(db.load_settings().unwrap().is_none());

        // Write non-default settings.
        let s = Settings {
            polling_interval_secs: 60,
            stagger_gap_secs: 45,
            thresholds: vec![50, 80, 95],
            theme: "dark".into(),
            launch_at_login: true,
            crash_reports: true,
            preferred_auth_source: None,
        };
        db.save_settings(&s).unwrap();

        // Read back and assert every field survived the round-trip.
        let loaded = db.load_settings().unwrap().expect("settings row");
        assert_eq!(loaded.polling_interval_secs, s.polling_interval_secs);
        assert_eq!(loaded.stagger_gap_secs, s.stagger_gap_secs);
        assert_eq!(loaded.thresholds, s.thresholds);
        assert_eq!(loaded.theme, s.theme);
        assert_eq!(loaded.launch_at_login, s.launch_at_login);
        assert_eq!(loaded.crash_reports, s.crash_reports);

        // Overwrite and confirm the latest value wins (UPSERT).
        let s2 = Settings { polling_interval_secs: 120, ..Settings::default() };
        db.save_settings(&s2).unwrap();
        let loaded2 = db.load_settings().unwrap().expect("updated settings row");
        assert_eq!(loaded2.polling_interval_secs, 120);
        assert_eq!(loaded2.theme, Settings::default().theme);
    }

    #[test]
    fn prune_removes_old_events() {
        let (_dir, db) = fresh_db();
        let old = Utc::now() - chrono::Duration::days(200);
        let recent = Utc::now();
        let mk = |ts, line: i64| StoredSessionEvent {
            ts,
            project: "p".into(),
            model: "sonnet-4-6".into(),
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_5m_tokens: 0,
            cache_creation_1h_tokens: 0,
            cost_usd: 0.0,
            source_file: "f.jsonl".into(),
            source_line: line,
            event_id: format!("ev_{line}"),
        };
        db.insert_events(&[mk(old, 1), mk(recent, 2)]).unwrap();
        let cutoff = Utc::now() - chrono::Duration::days(90);
        assert_eq!(db.prune_events_older_than(cutoff).unwrap(), 1);
    }
}
