-- src-tauri/src/store/migrations/0005_warmup.sql

-- Adds the warm-up & scheduling pillar columns.
-- Per spec §13, last_warmup_at uses unix epoch SECONDS to match
-- accounts.last_seen_at (schema.sql) for unit consistency.
ALTER TABLE accounts ADD COLUMN warmup_enabled INTEGER NOT NULL DEFAULT 0;
ALTER TABLE accounts ADD COLUMN schedule        TEXT    NOT NULL DEFAULT '{"type":"Off"}';
ALTER TABLE accounts ADD COLUMN last_warmup_at  INTEGER;

-- Global consent gate (key/value, not a column on settings).
INSERT INTO settings (key, value) VALUES ('warmup_consent_granted', '0')
  ON CONFLICT (key) DO NOTHING;
