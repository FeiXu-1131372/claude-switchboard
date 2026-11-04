-- v9 → v10: reconcile session costs with Claude Code's own totals.
--
-- Claude Code makes API calls it never writes to the transcript
-- (auto-compaction, away summaries, follow-up calls after a turn, memory
-- extraction, titles, WebSearch/WebFetch). Measured on 2026-11-04, the app's
-- transcript-only cost was 16.6% below `/cost` across 397 session/model pairs.
-- The `cost-state` record Claude Code appends on exit holds the exact totals.
--
-- Clearing jsonl_cursors forces a re-read from byte 0, which picks up every
-- `cost-state` record and the estimate markers. session_events is NOT
-- deleted: event_id is stable and UNIQUE, so the re-read only adds rows
-- (same reasoning as 0008 and 0009).
CREATE TABLE IF NOT EXISTS session_official_costs (
    source_file           TEXT NOT NULL,
    -- startTime of the Claude Code process that wrote the record (epoch ms).
    start_time            INTEGER NOT NULL DEFAULT 0,
    model                 TEXT NOT NULL,
    input_tokens          INTEGER NOT NULL DEFAULT 0,
    output_tokens         INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens     INTEGER NOT NULL DEFAULT 0,
    cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
    web_search_requests   INTEGER NOT NULL DEFAULT 0,
    cost_usd              REAL NOT NULL DEFAULT 0,
    source_line           INTEGER NOT NULL,
    PRIMARY KEY (source_file, start_time, model)
);
CREATE INDEX IF NOT EXISTS idx_events_source_file ON session_events(source_file);

DELETE FROM jsonl_cursors;
