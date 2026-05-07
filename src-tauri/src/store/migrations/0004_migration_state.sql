-- src-tauri/src/store/migrations/0004_migration_state.sql

-- Adds a flag the new Switchboard app uses to gate first-launch migration.
-- Idempotent: ON CONFLICT DO NOTHING so re-runs don't fail.
INSERT INTO settings (key, value) VALUES ('migration_completed', '0')
  ON CONFLICT (key) DO NOTHING;
