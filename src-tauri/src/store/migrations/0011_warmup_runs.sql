-- Shared by the GUI dispatcher and headless ticks. A lease prevents parallel
-- requests; completed_fire makes catch-up idempotent across process restarts.
CREATE TABLE IF NOT EXISTS warmup_runs (
    account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    claim_token TEXT,
    lease_until INTEGER NOT NULL DEFAULT 0,
    completed_fire INTEGER,
    last_attempt_at INTEGER,
    last_success_at INTEGER,
    last_result TEXT,
    last_error TEXT,
    next_retry_at INTEGER NOT NULL DEFAULT 0
);
