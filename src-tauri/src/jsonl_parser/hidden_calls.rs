//! Claude Code API calls that never reach the transcript.
//!
//! Claude Code makes calls it does not write to the session JSONL: the
//! auto-compaction summary, away-summary recaps, a follow-up call after many
//! turns (it forks the conversation and re-reads the whole context from
//! cache), memory extraction, session titles, and WebSearch/WebFetch on
//! Haiku. `/cost` counts them, so a transcript-only total runs low: 16.6%
//! low across 397 session/model pairs measured on 2026-11-04, and 35–55% on
//! heavy subagent sessions.
//!
//! Two kinds of synthetic `session_events` row close that gap:
//!
//! * **Adjustment** rows (`adj:` event ids), exact. When a session's process
//!   exits, Claude Code appends a `cost-state` record with its own per-model
//!   totals. The difference between those and what the transcripts show is
//!   written as adjustment rows, spread over the days the session ran, so a
//!   closed session matches `/cost` to the cent.
//! * **Estimate** rows (`est:` event ids), approximate. A session that is
//!   still running has no `cost-state` yet, so each transcript marker of an
//!   unlogged call gets an estimate. They are deleted when the `cost-state`
//!   arrives.

use super::pricing::PricingTable;
use super::record::{OfficialModelUsage, SessionEvent};
use crate::store::StoredSessionEvent;
use chrono::{DateTime, Local, Utc};
use std::collections::BTreeMap;

pub const ESTIMATE_PREFIX: &str = "est:";
pub const ADJUSTMENT_PREFIX: &str = "adj:";

/// True for rows this module wrote, which are not real API calls.
pub fn is_synthetic(event_id: &str) -> bool {
    event_id.starts_with(ESTIMATE_PREFIX) || event_id.starts_with(ADJUSTMENT_PREFIX)
}

// Estimate sizes, calibrated on 2026-11-04 against `cost-state` records of
// 197 closed sessions without subagents (Claude Code 2.1.281–2.1.316).
//
// After a turn ends in an interactive session, Claude Code sometimes forks
// the conversation for a follow-up call that re-reads the full context from
// cache. Across `cli` sessions the unlogged cache reads came to 0–100% of
// the summed turn-end contexts, about a third on average. It never happens
// in `sdk-cli` (`claude -p`) sessions.
const TURN_END_SHARE: f64 = 0.35;
const FOLLOWUP_INPUT_TOKENS: u64 = 600;
const FOLLOWUP_OUTPUT_TOKENS: u64 = 150;
// An away summary re-reads the context once and writes a short recap. The
// unlogged cache reads matched the last context before each recap to within
// 10 tokens.
const AWAY_OUTPUT_TOKENS: u64 = 120;
// A compaction re-reads the pre-compaction context and writes the summary,
// plus thinking. Measured output: 5k–9k tokens per compaction at default
// effort.
const COMPACTION_OUTPUT_TOKENS: u64 = 8_000;

/// The parent transcript of `source_file`: subagent transcripts
/// (`<session>/subagents/agent-*.jsonl`) belong to `<session>.jsonl`.
pub fn session_file_of(source_file: &str) -> String {
    match source_file.find("/subagents/") {
        Some(i) => format!("{}.jsonl", &source_file[..i]),
        None => source_file.to_string(),
    }
}

/// `[from, to)` bounds of every subagent transcript path under `session_file`,
/// for an index range scan instead of a `LIKE` pattern.
pub fn subagent_range_of(session_file: &str) -> (String, String) {
    let stem = session_file.strip_suffix(".jsonl").unwrap_or(session_file);
    // '0' is the byte after '/', so every "<stem>/subagents/..." sorts below it.
    (format!("{stem}/subagents/"), format!("{stem}/subagents0"))
}

/// Model key with the `[1m]` context marker removed: Claude Code writes
/// `claude-opus-4-7[1m]` in `cost-state` and `claude-opus-4-7` in the
/// transcript for the same model.
fn model_key(model: &str) -> &str {
    model.strip_suffix("[1m]").unwrap_or(model)
}

fn scaled(tokens: u64, share: f64) -> u64 {
    (tokens as f64 * share).round() as u64
}

#[allow(clippy::too_many_arguments)]
fn estimate_row(
    pricing: &PricingTable,
    ts: DateTime<Utc>,
    project: &str,
    model: &str,
    input: u64,
    output: u64,
    cache_read: u64,
    source_file: &str,
    source_line: i64,
    event_id: String,
) -> StoredSessionEvent {
    StoredSessionEvent {
        ts,
        project: project.to_string(),
        model: model.to_string(),
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_creation_5m_tokens: 0,
        cache_creation_1h_tokens: 0,
        cost_usd: pricing.cost_for(model, input, output, cache_read, 0, 0),
        source_file: source_file.to_string(),
        source_line,
        event_id,
    }
}

/// Estimate for the follow-up call after a turn ends. `None` unless `ev` is
/// the last call of a main-thread turn in an interactive session.
pub fn turn_end_estimate(
    pricing: &PricingTable,
    ev: &SessionEvent,
    event_key: &str,
    source_file: &str,
    source_line: i64,
) -> Option<StoredSessionEvent> {
    if ev.stop_reason.as_deref() != Some("end_turn")
        || ev.is_sidechain
        || ev.entrypoint.as_deref() != Some("cli")
    {
        return None;
    }
    Some(estimate_row(
        pricing,
        ev.ts,
        &ev.project,
        &ev.model,
        scaled(FOLLOWUP_INPUT_TOKENS, TURN_END_SHARE),
        scaled(FOLLOWUP_OUTPUT_TOKENS, TURN_END_SHARE),
        scaled(ev.context_tokens(), TURN_END_SHARE),
        source_file,
        source_line,
        format!("{ESTIMATE_PREFIX}turn:{event_key}"),
    ))
}

/// The main thread's most recent call, used to price calls that re-read it.
#[derive(Debug, Clone, PartialEq)]
pub struct MainContext {
    pub model: String,
    pub project: String,
    pub context_tokens: u64,
}

#[allow(clippy::too_many_arguments)]
pub fn compaction_estimate(
    pricing: &PricingTable,
    ts: DateTime<Utc>,
    uuid: &str,
    pre_tokens: u64,
    main: &MainContext,
    source_file: &str,
    source_line: i64,
) -> StoredSessionEvent {
    estimate_row(
        pricing,
        ts,
        &main.project,
        &main.model,
        0,
        COMPACTION_OUTPUT_TOKENS,
        pre_tokens,
        source_file,
        source_line,
        format!("{ESTIMATE_PREFIX}compact:{uuid}"),
    )
}

pub fn away_summary_estimate(
    pricing: &PricingTable,
    ts: DateTime<Utc>,
    uuid: &str,
    main: &MainContext,
    source_file: &str,
    source_line: i64,
) -> StoredSessionEvent {
    estimate_row(
        pricing,
        ts,
        &main.project,
        &main.model,
        0,
        AWAY_OUTPUT_TOKENS,
        main.context_tokens,
        source_file,
        source_line,
        format!("{ESTIMATE_PREFIX}away:{uuid}"),
    )
}

/// A real (transcript-backed) event of the session, as the reconciler sees it.
#[derive(Debug, Clone)]
pub struct ObservedCall {
    pub ts: DateTime<Utc>,
    pub project: String,
    pub model: String,
    pub cost_usd: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
}

#[derive(Default)]
struct DayBucket {
    weight: f64,
    first_ts: Option<DateTime<Utc>>,
    project: String,
}

/// Group `calls` by local calendar day, weighting each day by `weight`.
fn day_buckets<'a>(
    calls: impl Iterator<Item = &'a ObservedCall>,
    weight: impl Fn(&ObservedCall) -> f64,
) -> BTreeMap<String, DayBucket> {
    let mut days: BTreeMap<String, DayBucket> = BTreeMap::new();
    for c in calls {
        let day = c.ts.with_timezone(&Local).format("%Y-%m-%d").to_string();
        let b = days.entry(day).or_default();
        b.weight += weight(c).max(0.0);
        if b.first_ts.is_none_or(|t| c.ts < t) {
            b.first_ts = Some(c.ts);
            b.project = c.project.clone();
        }
    }
    days
}

/// Split `total` across `weights` so the parts sum to `total` exactly.
fn split_tokens(total: u64, shares: &[f64]) -> Vec<u64> {
    let mut out = Vec::with_capacity(shares.len());
    let mut given = 0u64;
    for (i, s) in shares.iter().enumerate() {
        let part = if i + 1 == shares.len() {
            total - given
        } else {
            ((total as f64 * s).round() as u64).min(total - given)
        };
        given += part;
        out.push(part);
    }
    out
}

/// Adjustment rows that lift `observed` (every real call of one session,
/// subagents included) up to Claude Code's own `official` totals.
///
/// Per model: for Anthropic models the cost gap is Claude Code's `costUSD`
/// minus the observed cost. For third-party relay models Claude Code only
/// knows a placeholder rate, so the missing *tokens* are priced at this
/// table's vendor rate instead. Models where the app already sees as much
/// as Claude Code (or more) get no row: nothing is ever subtracted.
///
/// The gap is spread over the session's days in proportion to that model's
/// observed cost per day, so daily totals stay right for sessions that span
/// several days. A model with no observed calls (Haiku helper calls are
/// never logged) follows the session's overall daily spend.
pub fn reconcile(
    pricing: &PricingTable,
    session_file: &str,
    source_line: i64,
    official: &[(String, OfficialModelUsage)],
    observed: &[ObservedCall],
) -> Vec<StoredSessionEvent> {
    // Merge official entries that differ only by the [1m] marker.
    let mut merged: BTreeMap<&str, OfficialModelUsage> = BTreeMap::new();
    for (model, u) in official {
        let slot = merged.entry(model_key(model)).or_insert(OfficialModelUsage {
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            web_search_requests: 0,
            cost_usd: 0.0,
        });
        slot.input_tokens += u.input_tokens;
        slot.output_tokens += u.output_tokens;
        slot.cache_read_tokens += u.cache_read_tokens;
        slot.cache_creation_tokens += u.cache_creation_tokens;
        slot.web_search_requests += u.web_search_requests;
        slot.cost_usd += u.cost_usd;
    }

    let mut rows = Vec::new();
    for (key, off) in merged {
        let mine: Vec<&ObservedCall> =
            observed.iter().filter(|c| model_key(&c.model) == key).collect();
        let sum = |f: fn(&ObservedCall) -> u64| -> u64 { mine.iter().map(|c| f(c)).sum() };
        let d_in = off.input_tokens.saturating_sub(sum(|c| c.input_tokens));
        let d_out = off.output_tokens.saturating_sub(sum(|c| c.output_tokens));
        let d_cr = off.cache_read_tokens.saturating_sub(sum(|c| c.cache_read_tokens));
        let d_cw = off
            .cache_creation_tokens
            .saturating_sub(sum(|c| c.cache_creation_tokens));
        let observed_cost: f64 = mine.iter().map(|c| c.cost_usd).sum();

        let entry = pricing.lookup(key);
        let rate = |tokens: u64, per_mtok: f64| tokens as f64 / 1_000_000.0 * per_mtok;
        let anthropic = PricingTable::is_anthropic(key);
        let delta_cost = if anthropic {
            off.cost_usd - observed_cost
        } else {
            // Base rates, not cost_for: these are many calls summed, so the
            // per-call context tier must not be applied to the total.
            entry.map_or(0.0, |e| {
                rate(d_in, e.input_per_mtok)
                    + rate(d_out, e.output_per_mtok)
                    + rate(d_cr, e.cache_read_per_mtok)
                    + rate(d_cw, e.cache_5m_per_mtok)
            })
        };
        if delta_cost <= 1e-9 {
            continue;
        }

        // `cost-state` doesn't split cache writes by TTL. Pick the 5m/1h split
        // that makes the rows' tokens price back to `delta_cost`, so the
        // per-category cost breakdown computed from tokens agrees with it.
        let cw_5m = match entry {
            Some(e) if anthropic && e.cache_1h_per_mtok > e.cache_5m_per_mtok => {
                let rest = delta_cost
                    - rate(d_in, e.input_per_mtok)
                    - rate(d_out, e.output_per_mtok)
                    - rate(d_cr, e.cache_read_per_mtok);
                let x = (rate(d_cw, e.cache_1h_per_mtok) - rest)
                    / ((e.cache_1h_per_mtok - e.cache_5m_per_mtok) / 1_000_000.0);
                x.clamp(0.0, d_cw as f64).round() as u64
            }
            _ => d_cw,
        };
        let cw_1h = d_cw - cw_5m;

        let mut days = day_buckets(mine.iter().copied(), |c| c.cost_usd);
        if days.values().all(|b| b.weight <= 0.0) {
            days = day_buckets(mine.iter().copied(), |c| (c.input_tokens + c.output_tokens) as f64 + 1.0);
        }
        if days.is_empty() {
            days = day_buckets(observed.iter(), |c| c.cost_usd);
            if days.values().all(|b| b.weight <= 0.0) {
                days = day_buckets(observed.iter(), |_| 1.0);
            }
        }
        let total_weight: f64 = days.values().map(|b| b.weight).sum();
        if days.is_empty() || total_weight <= 0.0 {
            continue;
        }
        let shares: Vec<f64> = days.values().map(|b| b.weight / total_weight).collect();
        let ins = split_tokens(d_in, &shares);
        let outs = split_tokens(d_out, &shares);
        let crs = split_tokens(d_cr, &shares);
        let c5s = split_tokens(cw_5m, &shares);
        let c1s = split_tokens(cw_1h, &shares);
        let model = mine.first().map_or(key, |c| c.model.as_str());

        for (i, (day, b)) in days.iter().enumerate() {
            rows.push(StoredSessionEvent {
                // Earliest call of the day, so an adjustment never becomes the
                // session's "latest" event (the active-model badge reads that).
                ts: b.first_ts.expect("bucket has at least one call"),
                project: b.project.clone(),
                model: model.to_string(),
                input_tokens: ins[i],
                output_tokens: outs[i],
                cache_read_tokens: crs[i],
                cache_creation_5m_tokens: c5s[i],
                cache_creation_1h_tokens: c1s[i],
                cost_usd: delta_cost * shares[i],
                source_file: session_file.to_string(),
                source_line,
                event_id: format!("{ADJUSTMENT_PREFIX}{session_file}:{key}:{day}"),
            });
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> PricingTable {
        PricingTable::bundled().unwrap()
    }

    fn usage(input: u64, output: u64, cr: u64, cw: u64, cost: f64) -> OfficialModelUsage {
        OfficialModelUsage {
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cr,
            cache_creation_tokens: cw,
            web_search_requests: 0,
            cost_usd: cost,
        }
    }

    fn call(ts: &str, model: &str, cost: f64, output: u64, cr: u64) -> ObservedCall {
        ObservedCall {
            ts: ts.parse().unwrap(),
            project: "proj".into(),
            model: model.into(),
            cost_usd: cost,
            input_tokens: 0,
            output_tokens: output,
            cache_read_tokens: cr,
            cache_creation_tokens: 0,
        }
    }

    fn total(rows: &[StoredSessionEvent]) -> f64 {
        rows.iter().map(|r| r.cost_usd).sum()
    }

    #[test]
    fn session_and_subagent_paths() {
        assert_eq!(session_file_of("p/s.jsonl"), "p/s.jsonl");
        assert_eq!(session_file_of("p/s/subagents/agent-a.jsonl"), "p/s.jsonl");
        let (lo, hi) = subagent_range_of("p/s.jsonl");
        assert!("p/s/subagents/agent-a.jsonl" >= lo.as_str());
        assert!("p/s/subagents/agent-a.jsonl" < hi.as_str());
        assert!("p/s.jsonl" < lo.as_str() || "p/s.jsonl" >= hi.as_str());
        assert!(is_synthetic("adj:p/s.jsonl:m:2026-11-04"));
        assert!(is_synthetic("est:turn:r:m"));
        assert!(!is_synthetic("req_1:msg_1"));
    }

    /// The case from the bug report, reduced: Claude Code says $136.81 for
    /// Opus, the transcript shows $127.54. The adjustment makes up exactly
    /// the difference, and Haiku (never logged) is added in full.
    #[test]
    fn closes_the_gap_to_claude_codes_cost_exactly() {
        let pricing = table();
        let observed = vec![call("2026-11-02T20:00:00Z", "claude-opus-5-5", 127.54, 2_373_593, 152_110_045)];
        let official = vec![
            ("claude-opus-5-5".to_string(), usage(95_300, 2_700_000, 160_300_000, 7_600_000, 136.81)),
            ("claude-haiku-5-5".to_string(), usage(1_500_000, 122_500, 0, 0, 0.58)),
        ];
        let rows = reconcile(&pricing, "p/s.jsonl", 99, &official, &observed);
        assert_eq!(rows.len(), 2);
        let opus = rows.iter().find(|r| r.model == "claude-opus-5-5").unwrap();
        assert!((opus.cost_usd - 9.27).abs() < 1e-9, "got {}", opus.cost_usd);
        assert_eq!(opus.output_tokens, 2_700_000 - 2_373_593);
        assert_eq!(opus.cache_read_tokens, 160_300_000 - 152_110_045);
        let haiku = rows.iter().find(|r| r.model == "claude-haiku-5-5").unwrap();
        assert!((haiku.cost_usd - 0.58).abs() < 1e-9);
        assert!((127.54 + total(&rows) - 137.39).abs() < 1e-9);
        assert!(rows.iter().all(|r| r.source_file == "p/s.jsonl" && r.source_line == 99));
    }

    #[test]
    fn spreads_the_gap_over_days_by_observed_spend() {
        let pricing = table();
        let observed = vec![
            call("2026-10-01T12:00:00Z", "claude-sonnet-5-5", 3.0, 100, 0),
            call("2026-10-05T12:00:00Z", "claude-sonnet-5-5", 1.0, 100, 0),
        ];
        let official = vec![("claude-sonnet-5-5".to_string(), usage(0, 1_000, 0, 0, 6.0))];
        let rows = reconcile(&pricing, "p/s.jsonl", 1, &official, &observed);
        assert_eq!(rows.len(), 2);
        assert!((rows[0].cost_usd - 1.5).abs() < 1e-9, "3/4 of the $2 gap on day one");
        assert!((rows[1].cost_usd - 0.5).abs() < 1e-9);
        assert_eq!(rows[0].output_tokens + rows[1].output_tokens, 800, "token split is exact");
        assert_eq!(rows[0].ts, observed[0].ts, "pinned to the day's first call");
    }

    #[test]
    fn never_subtracts_when_the_app_already_sees_more() {
        let pricing = table();
        let observed = vec![call("2026-10-01T12:00:00Z", "claude-sonnet-5-5", 9.0, 100, 0)];
        let official = vec![("claude-sonnet-5-5".to_string(), usage(0, 50, 0, 0, 2.0))];
        assert!(reconcile(&pricing, "p/s.jsonl", 1, &official, &observed).is_empty());
    }

    #[test]
    fn one_m_marker_variants_merge_onto_the_transcript_model() {
        let pricing = table();
        let observed = vec![call("2026-10-01T12:00:00Z", "claude-opus-4-7", 1.0, 100, 0)];
        let official = vec![
            ("claude-opus-4-7".to_string(), usage(0, 100, 0, 0, 0.5)),
            ("claude-opus-4-7[1m]".to_string(), usage(0, 100, 0, 0, 1.5)),
        ];
        let rows = reconcile(&pricing, "p/s.jsonl", 1, &official, &observed);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].model, "claude-opus-4-7");
        assert!((rows[0].cost_usd - 1.0).abs() < 1e-9);
    }

    /// Claude Code prices relay models at a placeholder rate ($22.28 for this
    /// DeepSeek session, ~100× DeepSeek's real price). The adjustment prices
    /// only the missing tokens, at DeepSeek's own rate.
    #[test]
    fn relay_models_price_missing_tokens_at_vendor_rates() {
        let pricing = table();
        let observed = vec![call("2026-10-01T12:00:00Z", "deepseek-v4.1-flash-1017", 0.5, 100_000, 1_000_000)];
        let official = vec![(
            "deepseek-v4.1-flash-1017".to_string(),
            usage(0, 100_000, 2_000_000, 0, 22.28),
        )];
        let rows = reconcile(&pricing, "p/s.jsonl", 1, &official, &observed);
        assert_eq!(rows.len(), 1);
        // 1M missing cache reads × $0.006/MTok.
        assert!((rows[0].cost_usd - 0.006).abs() < 1e-9, "got {}", rows[0].cost_usd);
    }

    #[test]
    fn cache_write_ttl_split_prices_back_to_the_gap() {
        let pricing = table();
        let observed = vec![call("2026-10-01T12:00:00Z", "claude-sonnet-5-5", 0.0, 0, 0)];
        // 1M cache writes: all 5m would be $2.50, all 1h $4.00; $3.25 means half.
        let official = vec![("claude-sonnet-5-5".to_string(), usage(0, 0, 0, 1_000_000, 3.25))];
        let rows = reconcile(&pricing, "p/s.jsonl", 1, &official, &observed);
        let r = &rows[0];
        assert_eq!(r.cache_creation_5m_tokens, 500_000);
        assert_eq!(r.cache_creation_1h_tokens, 500_000);
        let repriced = pricing.cost_for(&r.model, r.input_tokens, r.output_tokens,
            r.cache_read_tokens, r.cache_creation_5m_tokens, r.cache_creation_1h_tokens);
        assert!((repriced - r.cost_usd).abs() < 1e-6);
    }

    fn end_turn(entrypoint: &str, sidechain: bool) -> SessionEvent {
        let line = format!(
            r#"{{"type":"assistant","timestamp":"2026-11-02T18:10:20Z","cwd":"/x/proj",
              "isSidechain":{sidechain},"entrypoint":"{entrypoint}","requestId":"r","message":{{
              "id":"m","model":"claude-opus-5-5","stop_reason":"end_turn",
              "usage":{{"input_tokens":0,"output_tokens":10,"cache_read_input_tokens":1000000}}}}}}"#
        );
        super::super::record::parse_event_line(&line, "fb").unwrap()
    }

    #[test]
    fn turn_end_estimate_only_for_interactive_main_thread() {
        let pricing = table();
        let e = turn_end_estimate(&pricing, &end_turn("cli", false), "r:m", "p/s.jsonl", 7).unwrap();
        assert_eq!(e.event_id, "est:turn:r:m");
        assert_eq!(e.cache_read_tokens, 350_000);
        assert!(e.cost_usd > 0.0);
        assert!(turn_end_estimate(&pricing, &end_turn("sdk-cli", false), "r:m", "p/s.jsonl", 7).is_none());
        assert!(turn_end_estimate(&pricing, &end_turn("cli", true), "r:m", "p/s.jsonl", 7).is_none());
    }

    #[test]
    fn compaction_and_away_estimates_reread_the_context() {
        let pricing = table();
        let main = MainContext {
            model: "claude-opus-5-5".into(),
            project: "proj".into(),
            context_tokens: 360_000,
        };
        let ts: DateTime<Utc> = "2026-11-02T18:46:55Z".parse().unwrap();
        let c = compaction_estimate(&pricing, ts, "u1", 367_637, &main, "p/s.jsonl", 3);
        assert_eq!(c.cache_read_tokens, 367_637);
        assert_eq!(c.output_tokens, 8_000);
        assert_eq!(c.event_id, "est:compact:u1");
        let a = away_summary_estimate(&pricing, ts, "u2", &main, "p/s.jsonl", 4);
        assert_eq!(a.cache_read_tokens, 360_000);
        assert_eq!(a.event_id, "est:away:u2");
    }
}
