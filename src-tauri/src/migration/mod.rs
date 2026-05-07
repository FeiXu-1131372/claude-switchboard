//! Migration module — currently a no-op since there are no v0.3.x users
//! to migrate from. The `settings.migration_completed` flag from migration
//! 0004 still exists as a startup gate marker; if a future migration
//! becomes necessary, it lands here.

use anyhow::Result;
use rusqlite::Connection;

/// Mark first-launch migration as complete. Called once during startup
/// to flip `settings.migration_completed` from '0' to '1'. Idempotent.
pub fn mark_complete(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('migration_completed', '1') \
         ON CONFLICT (key) DO UPDATE SET value = '1'",
        [],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_fresh_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL); \
             INSERT INTO settings (key, value) VALUES ('migration_completed', '0');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn mark_complete_flips_flag() {
        let conn = open_fresh_conn();
        mark_complete(&conn).unwrap();
        let v: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'migration_completed'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v, "1");
    }

    #[test]
    fn mark_complete_is_idempotent() {
        let conn = open_fresh_conn();
        mark_complete(&conn).unwrap();
        mark_complete(&conn).unwrap();
        let v: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'migration_completed'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v, "1");
    }
}
