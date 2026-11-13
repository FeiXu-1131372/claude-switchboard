use super::hidden_calls::{self, MainContext};
use super::record::{
    parse_away_summary_line, parse_compaction_line, parse_cost_state_line, parse_event_line,
};
use super::pricing::PricingTable;
use crate::store::{CostStateAt, Db, HiddenCallBatch, StoredCompaction, StoredSessionEvent};
use anyhow::{Context, Result, anyhow};
use std::fs::{self, File};
use std::io::{BufRead, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Sanity cap for the startup backfill. Reading is incremental and streamed
/// line by line, so this only guards against pathological files. It used to
/// be 100 MiB, which skipped the largest (and most expensive) sessions:
/// transcripts of 291 MiB and 382 MiB were seen on 2026-11-04, and their
/// end-of-session `cost-state` record was never read.
const MAX_FILE_BYTES: u64 = 1024 * 1024 * 1024;

pub fn claude_projects_root() -> Option<PathBuf> {
    directories::UserDirs::new()
        .map(|u| u.home_dir().join(".claude").join("projects"))
}

pub fn discover_jsonl_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !root.exists() {
        return Ok(files);
    }
    for entry in fs::read_dir(root).context("read projects dir")? {
        let entry = entry?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            continue;
        }
        if !meta.is_dir() {
            continue;
        }
        let project_dir = path;
        for f in fs::read_dir(&project_dir)? {
            let f = f?;
            let fpath = f.path();
            let fmeta = fs::symlink_metadata(&fpath)?;
            if fmeta.file_type().is_symlink() {
                continue;
            }
            // A session directory (`<sessionId>/`) may hold subagent
            // transcripts at `<sessionId>/subagents/agent-*.jsonl`. The
            // watcher already picks these up live (RecursiveMode::Recursive),
            // so skipping them here made the backfill disagree with the
            // watcher — 125 of 138 files were never ingested, and their API
            // calls are real spend that the Cost tab is built to display.
            if fmeta.is_dir() {
                let subagents = fpath.join("subagents");
                let Ok(entries) = fs::read_dir(&subagents) else {
                    continue;
                };
                for s in entries {
                    let s = s?;
                    let spath = s.path();
                    let smeta = fs::symlink_metadata(&spath)?;
                    if smeta.file_type().is_symlink() || !smeta.is_file() {
                        continue;
                    }
                    if spath.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                        continue;
                    }
                    if smeta.len() > MAX_FILE_BYTES {
                        tracing::warn!("skipping oversized file (>1GiB): {}", spath.display());
                        continue;
                    }
                    files.push(spath);
                }
                continue;
            }
            if !fmeta.is_file() {
                continue;
            }
            if fpath.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if fmeta.len() > MAX_FILE_BYTES {
                tracing::warn!("skipping oversized file (>1GiB): {}", fpath.display());
                continue;
            }
            files.push(fpath);
        }
    }
    Ok(files)
}

pub fn ingest_file(
    db: &Db,
    pricing: &PricingTable,
    path: &Path,
    projects_root: &Path,
) -> Result<usize> {
    let meta = fs::metadata(path)?;
    let file_len = meta.len() as i64;
    let mtime_ns = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos().min(i64::MAX as u128) as i64)
        .unwrap_or(0);

    // P1-20: use to_str() so non-UTF-8 paths surface an error rather than
    // producing a lossy key that silently never matches the cursor store.
    let key = path
        .to_str()
        .ok_or_else(|| anyhow!("non-UTF-8 path: {:?}", path))?
        .to_owned();
    let (prev_mtime, mut offset) = db.get_cursor(&key)?.unwrap_or((0, 0));

    if file_len < offset {
        tracing::info!("truncation detected, resetting cursor for {}", key);
        offset = 0;
    } else if prev_mtime == mtime_ns && file_len == offset {
        return Ok(0);
    }

    let mut f = File::open(path)?;
    f.seek(SeekFrom::Start(offset as u64))?;

    let project = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    // P1-7: store relative path from the Claude projects root so that
    // source_file values are not machine-specific absolute paths. Persist
    // forward slashes on Windows too: subagent grouping and reconciliation
    // use '/' to associate these calls with the parent's official total.
    let source_file_path = path
        .strip_prefix(projects_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/");

    // Unlogged calls are tracked on parent transcripts only: a subagent's
    // usage is already inside its parent's `cost-state`, and subagents don't
    // compact or get away summaries.
    let is_main = !source_file_path.contains("/subagents/");
    let mut hidden = HiddenCallBatch {
        session_file: hidden_calls::session_file_of(&source_file_path),
        ..Default::default()
    };
    // Latest main-thread call, which compactions and away summaries re-read.
    // Looked up from the DB only if a marker appears before any call in this
    // chunk.
    let mut last_main: Option<MainContext> = None;

    let mut reader = std::io::BufReader::new(f);
    let mut buf = Vec::new();
    let mut stored = Vec::<StoredSessionEvent>::new();
    let mut compactions = Vec::<StoredCompaction>::new();
    let mut consumed: i64 = offset;

    loop {
        buf.clear();
        let line_start = consumed;
        let n = reader.read_until(b'\n', &mut buf)?;
        if n == 0 {
            break;
        }
        if *buf.last().unwrap() != b'\n' {
            break;
        }
        consumed += n as i64;
        let text = match std::str::from_utf8(&buf) {
            Ok(t) => t.trim(),
            Err(_) => continue,
        };
        if text.is_empty() {
            continue;
        }
        // Claude Code JSONLs interleave many non-usage record types
        // (`user`, `permission-mode`, `attachment`, `system`, `last-prompt`, …);
        // only `assistant` lines carry a `message.usage` payload. parse_event_line
        // returns None for everything else, so silent skip is correct here.
        if let Some(ev) = parse_event_line(text, &project) {
            let cost = pricing.cost_for(
                &ev.model,
                ev.input_tokens,
                ev.output_tokens,
                ev.cache_read_tokens,
                ev.cache_creation_5m_tokens,
                ev.cache_creation_1h_tokens,
            );
            // Structural fallback only when the JSONL line lacked the
            // canonical Claude identifiers (older Claude Code versions or
            // hand-written test fixtures). For modern Claude Code, this
            // path isn't taken — the parser produces a real
            // "{requestId}:{message.id}" key that survives Claude Code
            // re-writing the same usage block to multiple offsets.
            let event_id = ev
                .event_id
                .clone()
                .unwrap_or_else(|| format!("{}:{}", key, line_start));
            if is_main && !ev.is_sidechain {
                if let Some(est) = hidden_calls::turn_end_estimate(
                    pricing,
                    &ev,
                    &event_id,
                    &source_file_path,
                    line_start,
                ) {
                    hidden.estimates.push(est);
                }
                last_main = Some(MainContext {
                    model: ev.model.clone(),
                    project: ev.project.clone(),
                    context_tokens: ev.context_tokens(),
                });
            }
            stored.push(StoredSessionEvent {
                ts: ev.ts,
                project: ev.project,
                model: ev.model,
                input_tokens: ev.input_tokens,
                output_tokens: ev.output_tokens,
                cache_read_tokens: ev.cache_read_tokens,
                cache_creation_5m_tokens: ev.cache_creation_5m_tokens,
                cache_creation_1h_tokens: ev.cache_creation_1h_tokens,
                cost_usd: cost,
                source_file: source_file_path.clone(),
                source_line: line_start,
                event_id,
            });
        } else if let Some(c) = parse_compaction_line(text) {
            // `else if`: a compaction is a `system` line, so it can never also
            // be a usage-bearing assistant line — no need to test both.
            if is_main {
                if let Some(main) = main_context(db, &mut last_main, &source_file_path) {
                    hidden.estimates.push(hidden_calls::compaction_estimate(
                        pricing,
                        c.ts,
                        &c.uuid,
                        c.pre_tokens,
                        &main,
                        &source_file_path,
                        line_start,
                    ));
                }
                // What a recap right after this would re-read is the
                // compacted context, not the old one.
                if let Some(m) = last_main.as_mut() {
                    m.context_tokens = c.post_tokens;
                }
            }
            compactions.push(StoredCompaction {
                ts: c.ts,
                source_file: source_file_path.clone(),
                trigger: c.trigger,
                pre_tokens: c.pre_tokens,
                post_tokens: c.post_tokens,
                uuid: c.uuid,
            });
        } else if !is_main {
            continue;
        } else if let Some(a) = parse_away_summary_line(text) {
            if let Some(main) = main_context(db, &mut last_main, &source_file_path) {
                hidden.estimates.push(hidden_calls::away_summary_estimate(
                    pricing,
                    a.ts,
                    &a.uuid,
                    &main,
                    &source_file_path,
                    line_start,
                ));
            }
        } else if let Some(cs) = parse_cost_state_line(text) {
            // Exact totals up to here: estimates before this line are moot.
            hidden.estimates.retain(|e| e.source_line > line_start);
            hidden.cost_states.retain(|c| c.state.start_time != cs.start_time);
            hidden.cost_states.push(CostStateAt { line: line_start, state: cs });
        }
    }
    let inserted = db.ingest_atomic(
        &key,
        &stored,
        &compactions,
        &hidden,
        pricing,
        mtime_ns,
        consumed,
    )?;
    Ok(inserted)
}

/// The chunk's latest main-thread call, else the latest one already stored.
fn main_context(
    db: &Db,
    last_main: &mut Option<MainContext>,
    source_file: &str,
) -> Option<MainContext> {
    if last_main.is_none() {
        *last_main = db.last_main_context(source_file).ok().flatten();
    }
    last_main.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Db;
    use chrono::Utc;
    use std::io::Write;
    use std::sync::Arc;
    use tempfile::tempdir;

    /// Build a minimal assistant JSONL line that parse_event_line will accept.
    fn assistant_line(req_id: &str, msg_id: &str) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{}","cwd":"/tmp/project","requestId":"{}","message":{{"id":"{}","model":"claude-sonnet-4-6","usage":{{"input_tokens":1,"output_tokens":1}}}}}}"#,
            Utc::now().to_rfc3339(),
            req_id,
            msg_id,
        )
    }

    /// Write `lines` to a temp file inside a project sub-directory so the
    /// walker can derive a project name from the parent directory.
    fn write_jsonl(dir: &tempfile::TempDir, lines: &[String]) -> PathBuf {
        let project_dir = dir.path().join("my-project");
        std::fs::create_dir_all(&project_dir).unwrap();
        let path = project_dir.join("session.jsonl");
        let mut f = std::fs::File::create(&path).unwrap();
        for line in lines {
            writeln!(f, "{}", line).unwrap();
        }
        path
    }

    /// Verify that two concurrent `ingest_file` calls on the same file produce
    /// no duplicate rows and leave the cursor at the file's end. This is the
    /// regression test for the race where two separate `insert_events` +
    /// `set_cursor` calls could let the slower caller regress the cursor.
    #[test]
    fn concurrent_ingest_no_duplicates_and_correct_cursor() {
        let dir = tempdir().unwrap();
        // Use a separate directory for the DB so it doesn't collide with the
        // JSONL project sub-directory.
        let db_dir = dir.path().join("db");
        std::fs::create_dir_all(&db_dir).unwrap();
        let db = Arc::new(Db::open(&db_dir).unwrap());
        let pricing = Arc::new(PricingTable::bundled().unwrap());

        let lines: Vec<String> = (0..10)
            .map(|i| assistant_line(&format!("req_{i}"), &format!("msg_{i}")))
            .collect();
        let path = write_jsonl(&dir, &lines);
        let file_len = std::fs::metadata(&path).unwrap().len() as i64;

        // The projects root is the temp dir itself (the JSONL lives in
        // <dir>/my-project/session.jsonl, so dir.path() is the root).
        let projects_root = dir.path().to_path_buf();

        // Spawn two threads; both race to ingest the same file.
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let db = Arc::clone(&db);
                let pricing = Arc::clone(&pricing);
                let path = path.clone();
                let root = projects_root.clone();
                std::thread::spawn(move || ingest_file(&db, &pricing, &path, &root))
            })
            .collect();

        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        // Both threads must succeed (the Mutex ensures they serialize).
        for r in &results {
            assert!(r.is_ok(), "ingest_file failed: {:?}", r);
        }
        // Total inserted across both threads must equal exactly 10 — no dups.
        let total_inserted: usize = results.iter().map(|r| r.as_ref().unwrap()).sum();
        assert_eq!(total_inserted, 10, "expected exactly 10 unique events");

        // Cursor byte_offset must be at the file end (not regressed by the
        // slower thread). The cursor key is the absolute path string.
        let key = path.to_str().expect("test path must be UTF-8").to_owned();
        let (_mtime, offset) = db.get_cursor(&key).unwrap().expect("cursor missing");
        assert_eq!(offset, file_len, "cursor must be at end-of-file");
    }

    fn turn_line(req: &str, out: u64, cache_read: u64, stop: &str) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"2026-11-02T20:00:00Z","cwd":"/tmp/proj","isSidechain":false,"entrypoint":"cli","requestId":"{req}","message":{{"id":"m-{req}","model":"claude-opus-5-5","stop_reason":"{stop}","usage":{{"input_tokens":0,"output_tokens":{out},"cache_read_input_tokens":{cache_read}}}}}}}"#
        )
    }

    fn cost_state_line(opus_cost: f64, haiku_cost: f64) -> String {
        format!(
            r#"{{"type":"cost-state","sessionId":"s","modelUsage":{{"claude-opus-5-5":{{"inputTokens":0,"outputTokens":3000000,"cacheReadInputTokens":20000000,"cacheCreationInputTokens":0,"costUSD":{opus_cost}}},"claude-haiku-5-5":{{"inputTokens":1500000,"outputTokens":122500,"cacheReadInputTokens":0,"cacheCreationInputTokens":0,"webSearchRequests":37,"costUSD":{haiku_cost}}}}}}}"#
        )
    }

    const COMPACT: &str = r#"{"type":"system","subtype":"compact_boundary","uuid":"c-1","timestamp":"2026-11-02T20:05:00Z","compactMetadata":{"trigger":"auto","preTokens":400000,"postTokens":30000}}"#;
    const AWAY: &str = r#"{"type":"system","subtype":"away_summary","uuid":"a-1","timestamp":"2026-11-02T20:06:00Z","content":"Recap"}"#;

    struct Fixture {
        dir: tempfile::TempDir,
        db: Db,
        pricing: PricingTable,
        main: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempdir().unwrap();
            let db_dir = dir.path().join("db");
            std::fs::create_dir_all(&db_dir).unwrap();
            let db = Db::open(&db_dir).unwrap();
            let main = dir.path().join("proj").join("s.jsonl");
            std::fs::create_dir_all(main.parent().unwrap()).unwrap();
            std::fs::write(&main, "").unwrap();
            Fixture { dir, db, pricing: PricingTable::bundled().unwrap(), main }
        }
        fn append(&self, path: &Path, lines: &[String]) {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
            for l in lines {
                writeln!(f, "{l}").unwrap();
            }
        }
        fn ingest(&self, path: &Path) {
            ingest_file(&self.db, &self.pricing, path, self.dir.path()).unwrap();
        }
        /// (total cost, number of rows) for rows whose event_id matches `like`.
        fn sum(&self, like: &str) -> (f64, i64) {
            self.db
                .conn()
                .query_row(
                    "SELECT COALESCE(SUM(cost_usd), 0), COUNT(*) FROM session_events WHERE event_id LIKE ?1",
                    [like],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap()
        }
    }

    /// Live session: estimates appear for the turn end, the compaction and
    /// the away summary. When the session closes and Claude Code writes its
    /// cost-state, the estimates are replaced and the session total equals
    /// Claude Code's own total exactly.
    #[test]
    fn live_estimates_give_way_to_claude_codes_exact_total() {
        let fx = Fixture::new();
        fx.append(&fx.main, &[
            turn_line("r1", 1_000_000, 10_000_000, "tool_use"),
            turn_line("r2", 1_000_000, 5_000_000, "end_turn"),
            COMPACT.to_string(),
            AWAY.to_string(),
        ]);
        fx.ingest(&fx.main);

        let (est, n_est) = fx.sum("est:%");
        assert_eq!(n_est, 3, "turn end + compaction + away summary");
        assert!(est > 0.0);

        // Process exits: Claude Code appends its totals. Opus $70 official vs
        // $40 + $2 + $1 observed from the transcript ($43): $27 unlogged.
        fx.append(&fx.main, &[cost_state_line(70.0, 0.58)]);
        fx.ingest(&fx.main);

        assert_eq!(fx.sum("est:%").1, 0, "estimates replaced by exact totals");
        let (total, _) = fx.sum("%");
        assert!((total - 70.58).abs() < 1e-9, "session total must equal /cost, got {total}");
        let haiku: f64 = fx
            .db
            .conn()
            .query_row(
                "SELECT SUM(cost_usd) FROM session_events WHERE model = 'claude-haiku-5-5'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!((haiku - 0.58).abs() < 1e-9, "never-logged Haiku calls are added");
    }

    /// The re-read after migration 0010 can ingest the parent before its
    /// subagent transcripts. The subagent's calls must shrink the adjustment,
    /// not stack on top of it.
    #[test]
    fn subagent_ingested_after_parent_does_not_double_count() {
        let fx = Fixture::new();
        fx.append(&fx.main, &[
            turn_line("r1", 1_000_000, 0, "end_turn"),
            cost_state_line(50.0, 0.0),
        ]);
        fx.ingest(&fx.main);
        assert!((fx.sum("%").0 - 50.0).abs() < 1e-9);

        let sub = fx.dir.path().join("proj").join("s").join("subagents").join("agent-a.jsonl");
        fx.append(&sub, &[turn_line("r9", 1_000_000, 0, "end_turn")]);
        fx.ingest(&sub);
        let (total, _) = fx.sum("%");
        assert!((total - 50.0).abs() < 1e-9, "still exactly /cost, got {total}");
        assert_eq!(fx.sum("est:%").1, 0, "subagent turns are never estimated");
    }

    /// A resumed session: new turns after the cost-state get estimates until
    /// the next cost-state, which then sets the total exactly again.
    #[test]
    fn resumed_session_estimates_only_after_the_last_cost_state() {
        let fx = Fixture::new();
        fx.append(&fx.main, &[turn_line("r1", 1_000_000, 0, "end_turn"), cost_state_line(30.0, 0.0)]);
        fx.ingest(&fx.main);
        fx.append(&fx.main, &[turn_line("r2", 100_000, 1_000_000, "end_turn")]);
        fx.ingest(&fx.main);
        assert_eq!(fx.sum("est:%").1, 1, "the resumed turn is estimated");

        fx.append(&fx.main, &[cost_state_line(45.0, 0.0)]);
        fx.ingest(&fx.main);
        assert_eq!(fx.sum("est:%").1, 0);
        assert!((fx.sum("%").0 - 45.0).abs() < 1e-9);
    }

    /// Two Claude Code processes wrote to one transcript (real case:
    /// 81495532, $144.87 + $139.37). Each record counts only its own process,
    /// so the session total is their sum, and a later record from the same
    /// process replaces its earlier one.
    #[test]
    fn totals_from_separate_processes_add_up() {
        let fx = Fixture::new();
        let with_start = |cost: f64, start: i64| {
            cost_state_line(cost, 0.0).replacen(
                r#""sessionId":"s","#,
                &format!(r#""sessionId":"s","startTime":{start},"#),
                1,
            )
        };
        fx.append(&fx.main, &[
            turn_line("r1", 1_000_000, 0, "end_turn"),
            with_start(30.0, 111),
            with_start(25.0, 222),
        ]);
        fx.ingest(&fx.main);
        assert!((fx.sum("%").0 - 55.0).abs() < 1e-9);

        fx.append(&fx.main, &[with_start(40.0, 111)]);
        fx.ingest(&fx.main);
        assert!((fx.sum("%").0 - 65.0).abs() < 1e-9, "process 111 now at $40, 222 still $25");
    }

    /// An opus call of `out` output tokens ($20 per million) at noon UTC on
    /// `day`. `tool_use` keeps it from earning a turn-end estimate.
    fn call_on(req: &str, out: u64, day: &str) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{day}T12:00:00Z","cwd":"/tmp/proj","isSidechain":false,"entrypoint":"cli","requestId":"{req}","message":{{"id":"m-{req}","model":"claude-opus-5-5","stop_reason":"tool_use","usage":{{"input_tokens":0,"output_tokens":{out}}}}}}}"#
        )
    }

    /// The `cost-state` of the Claude Code process that started at `start`.
    fn process_total(start: i64, out_tokens: u64, opus_cost: f64) -> String {
        format!(
            r#"{{"type":"cost-state","sessionId":"s","startTime":{start},"modelUsage":{{"claude-opus-5-5":{{"inputTokens":0,"outputTokens":{out_tokens},"cacheReadInputTokens":0,"cacheCreationInputTokens":0,"costUSD":{opus_cost}}}}}}}"#
        )
    }

    /// `claude --resume` writes a new transcript. It starts with a copy of the
    /// old history and ends with a `cost-state` that keeps the old process's
    /// `startTime` and carries on from its totals (real case on 2026-11-13:
    /// 0fdfef68 resumed a992e056, $5,884 against $5,875).
    ///
    /// Here the original made two calls ($40) and Claude Code says it cost
    /// $50, so $10 went unlogged. The resumed file repeats those two calls and
    /// adds one more ($20); its cumulative total is $72: the same $10, plus
    /// $2 unlogged since. Everything together cost $72, whichever way the two
    /// files are read.
    fn resumed_pair(fx: &Fixture) -> (PathBuf, PathBuf) {
        let parent = fx.main.clone();
        let child = fx.dir.path().join("proj").join("s2.jsonl");
        let history = [
            call_on("r1", 1_000_000, "2026-11-02"),
            call_on("r2", 1_000_000, "2026-11-02"),
        ];
        fx.append(&parent, &[history[0].clone(), history[1].clone(), process_total(111, 2_000_000, 50.0)]);
        fx.append(&child, &[
            history[0].clone(),
            history[1].clone(),
            call_on("r3", 1_000_000, "2026-11-13"),
            process_total(111, 3_000_000, 72.0),
        ]);
        (parent, child)
    }

    /// Break caught: reconciling each file against its own `cost-state`. The
    /// resumed file's total already contains the original's, so counting both
    /// charges the same spend twice ($122 here, and $11,759 on the real data).
    #[test]
    fn a_resumed_session_does_not_count_its_inherited_cost_twice() {
        let fx = Fixture::new();
        let (parent, child) = resumed_pair(&fx);
        fx.ingest(&parent);
        fx.ingest(&child);
        let (total, _) = fx.sum("%");
        assert!((total - 72.0).abs() < 1e-9, "the pair cost $72 in all, got {total}");
    }

    /// Break caught: a fix that only works for one read order. The resumed
    /// file's repeated calls are kept by whichever file is read first, so the
    /// original can arrive holding none of its own calls.
    #[test]
    fn the_total_does_not_depend_on_which_transcript_is_read_first() {
        let fx = Fixture::new();
        let (parent, child) = resumed_pair(&fx);
        fx.ingest(&child);
        fx.ingest(&parent);
        let (total, _) = fx.sum("%");
        assert!((total - 72.0).abs() < 1e-9, "the pair cost $72 in all, got {total}");
    }

    /// Break caught: pricing the gap against the resumed file's one new call.
    /// The gap is mostly the original's, from 11-02, but it was all placed on
    /// 11-13, which is how a single resume put $5,881 on "today" in Trends.
    /// Observed cost splits 40:20 by day, so the $12 gap splits $8:$4.
    #[test]
    fn the_unlogged_gap_is_spread_over_the_days_the_whole_session_ran() {
        let fx = Fixture::new();
        let (parent, child) = resumed_pair(&fx);
        fx.ingest(&parent);
        fx.ingest(&child);
        let on_or_after = |day: &str| -> f64 {
            fx.db
                .conn()
                .query_row(
                    "SELECT COALESCE(SUM(cost_usd), 0) FROM session_events
                     WHERE ts >= CAST(strftime('%s', ?1) AS INTEGER)",
                    [day],
                    |r| r.get(0),
                )
                .unwrap()
        };
        let later = on_or_after("2026-11-10");
        let earlier = on_or_after("2026-11-01") - later;
        assert!((earlier - 48.0).abs() < 1e-9, "11-02: $40 of calls + $8 of gap, got {earlier}");
        assert!((later - 24.0).abs() < 1e-9, "11-13: $20 of calls + $4 of gap, got {later}");
    }

    /// Break caught: fixing only new ingests. A database that has already
    /// read a resumed session still holds the row the old reconciliation
    /// wrote, the resumed file's whole cumulative total less its one new call
    /// ($72 - $20 = $52), and nothing would ever revisit it.
    #[test]
    fn rebuilding_repairs_the_phantom_row_an_earlier_version_wrote() {
        let fx = Fixture::new();
        let (parent, child) = resumed_pair(&fx);
        fx.ingest(&parent);
        fx.ingest(&child);
        fx.db
            .conn()
            .execute(
                "INSERT INTO session_events
                 (ts, project, model, input_tokens, output_tokens, cache_read_tokens,
                  cache_creation_5m_tokens, cache_creation_1h_tokens, cost_usd,
                  source_file, source_line, event_id)
                 VALUES (CAST(strftime('%s', '2026-11-13T12:00:00') AS INTEGER), 'proj',
                         'claude-opus-5-5', 0, 2000000, 0, 0, 0, 52.0,
                         'proj/s2.jsonl', 1, 'adj:proj/s2.jsonl:claude-opus-5-5:2026-11-13')",
                [],
            )
            .unwrap();
        assert!((fx.sum("%").0 - 124.0).abs() < 1e-9, "setup: the double count is in place");

        assert!(fx.db.repair_adjustments_once(&fx.pricing).unwrap(), "the first run does the work");
        let (total, _) = fx.sum("%");
        assert!((total - 72.0).abs() < 1e-9, "the pair cost $72 in all, got {total}");
    }

    /// Break caught: redoing every session's reconciliation on each launch.
    #[test]
    fn the_repair_runs_once_per_database() {
        let fx = Fixture::new();
        let (parent, child) = resumed_pair(&fx);
        fx.ingest(&parent);
        fx.ingest(&child);
        assert!(fx.db.repair_adjustments_once(&fx.pricing).unwrap());
        assert!(!fx.db.repair_adjustments_once(&fx.pricing).unwrap(), "already done");
    }

    /// Re-reading a transcript from byte 0 (what migration 0010 forces) is
    /// idempotent: same rows, same total.
    #[test]
    fn rereading_a_closed_session_is_idempotent() {
        let fx = Fixture::new();
        fx.append(&fx.main, &[
            turn_line("r1", 1_000_000, 0, "end_turn"),
            COMPACT.to_string(),
            cost_state_line(50.0, 0.58),
        ]);
        fx.ingest(&fx.main);
        let before = fx.sum("%");
        fx.db.conn().execute("DELETE FROM jsonl_cursors", []).unwrap();
        fx.ingest(&fx.main);
        let after = fx.sum("%");
        assert_eq!(before.1, after.1);
        assert!((before.0 - after.0).abs() < 1e-9);
        assert!((after.0 - 50.58).abs() < 1e-9);
    }

    /// Repricing must not touch adjustment rows directly; it recomputes them,
    /// so the session still totals Claude Code's figure afterwards.
    #[test]
    fn repricing_keeps_closed_sessions_at_the_official_total() {
        let fx = Fixture::new();
        fx.append(&fx.main, &[turn_line("r1", 1_000_000, 0, "end_turn"), cost_state_line(50.0, 0.0)]);
        fx.ingest(&fx.main);
        // Simulate a stale cost on the real row, as a pricing fix would leave.
        fx.db
            .conn()
            .execute("UPDATE session_events SET cost_usd = 1.0 WHERE event_id NOT LIKE 'adj:%'", [])
            .unwrap();
        fx.db.reprice_outdated_events(&fx.pricing).unwrap();
        assert!((fx.sum("%").0 - 50.0).abs() < 1e-9);
    }

    #[test]
    fn discovers_subagent_transcripts_one_level_deeper() {
        let root = tempdir().unwrap();
        let project = root.path().join("-Users-me-proj");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("sess-1.jsonl"), "{}\n").unwrap();

        let subagents = project.join("sess-1").join("subagents");
        std::fs::create_dir_all(&subagents).unwrap();
        std::fs::write(subagents.join("agent-aaa.jsonl"), "{}\n").unwrap();
        std::fs::write(subagents.join("agent-bbb.jsonl"), "{}\n").unwrap();

        let found = discover_jsonl_files(root.path()).unwrap();
        assert_eq!(found.len(), 3, "top-level transcript plus both subagents");
        assert!(
            found.iter().any(|p| p.ends_with("agent-aaa.jsonl")),
            "subagent transcripts must be backfilled — their API calls cost money"
        );
    }

    #[test]
    fn ignores_non_subagent_subdirectories() {
        let root = tempdir().unwrap();
        let project = root.path().join("-Users-me-proj");
        std::fs::create_dir_all(project.join("sess-1").join("something-else")).unwrap();
        std::fs::write(
            project.join("sess-1").join("something-else").join("x.jsonl"),
            "{}\n",
        )
        .unwrap();
        let found = discover_jsonl_files(root.path()).unwrap();
        assert!(found.is_empty(), "only subagents/ is descended into");
    }
}
