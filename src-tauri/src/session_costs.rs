//! What each conversation cost on each day, for the Cost tab.
//!
//! The webview used to receive every API call in the window and sum them
//! itself: tens of thousands of rows across the bridge to draw a few hundred.
//! The sums are made here and only the rows that are drawn cross over.

use crate::jsonl_parser::hidden_calls::session_file_of;
use crate::jsonl_parser::pricing::PricingTable;
use crate::store::StoredSessionEvent;
use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub use crate::jsonl_parser::pricing::CostBreakdown;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct TokenBreakdown {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
}

/// A subagent's transcript, listed under the conversation that started it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct SubagentCost {
    /// The subagent transcript's path.
    pub id: String,
    pub label: String,
    #[specta(type = String)]
    pub latest_ts: DateTime<Utc>,
    pub turn_count: u32,
    pub headline_tokens: u64,
    pub total_cost_usd: f64,
    pub dominant_model: String,
}

/// One conversation on one local day.
///
/// The day is part of the identity, not a label: a conversation resumed after
/// a few days is several rows, each holding what was spent on its own date, so
/// the rows under a day add up to that day's bar in Trends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct SessionCostRow {
    /// `<parent transcript path>#<YYYY-MM-DD>`.
    pub id: String,
    pub project: String,
    pub day: String,
    #[specta(type = String)]
    pub latest_ts: DateTime<Utc>,
    /// API calls that appear in the transcript. Unlogged-call rows add cost
    /// and tokens but are not turns.
    pub turn_count: u32,
    /// Input plus output, the parent and its subagents together. Cache tokens
    /// are far larger and mostly reuse, so they are left to the breakdown.
    pub headline_tokens: u64,
    pub total_cost_usd: f64,
    /// The model that wrote the most output; the first one seen on a tie.
    pub dominant_model: String,
    pub breakdown: TokenBreakdown,
    pub cost_breakdown: CostBreakdown,
    /// Oldest first, so the order they were started in reads top to bottom.
    pub subagents: Vec<SubagentCost>,
}

/// Subagent transcripts are `<session>/subagents/agent-<hex>.jsonl`.
const SUBAGENT_SEGMENT: &str = "/subagents/agent-";

/// Rows the backend adds for calls that never reach the transcript (see
/// `jsonl_parser::hidden_calls`): real cost and tokens, but not turns.
fn is_unlogged_call(event_id: &str) -> bool {
    event_id.starts_with("adj:") || event_id.starts_with("est:")
}

/// `agent 0a1b2c3d` from the first `agent-<hex>` in the path.
fn subagent_label(source_file: &str) -> String {
    let lower = source_file.to_ascii_lowercase();
    for (at, marker) in lower.match_indices("agent-") {
        let hex: String = source_file[at + marker.len()..]
            .chars()
            .take_while(char::is_ascii_hexdigit)
            .take(8)
            .collect();
        if !hex.is_empty() {
            return format!("agent {hex}");
        }
    }
    "subagent".to_string()
}

/// Output tokens per model, in the order the models were first seen.
#[derive(Default)]
struct ModelOutput(Vec<(String, u64)>);

impl ModelOutput {
    fn add(&mut self, model: &str, output: u64) {
        match self.0.iter_mut().find(|(m, _)| m == model) {
            Some((_, total)) => *total += output,
            None => self.0.push((model.to_string(), output)),
        }
    }

    fn dominant(&self) -> String {
        let mut best: Option<&(String, u64)> = None;
        for entry in &self.0 {
            if best.is_none_or(|b| entry.1 > b.1) {
                best = Some(entry);
            }
        }
        best.map(|(model, _)| model.clone()).unwrap_or_default()
    }
}

struct SubAgg {
    row: SubagentCost,
    models: ModelOutput,
}

struct ParentAgg {
    row: SessionCostRow,
    models: ModelOutput,
    subs: Vec<SubAgg>,
    sub_index: HashMap<String, usize>,
}

/// Group API calls into one row per conversation per day in `tz`.
///
/// Rows come back newest first. Category costs are worked out call by call,
/// because a pricing tier applies to a call by that call's own context size;
/// a model with no price contributes nothing to them.
pub fn aggregate<Tz: TimeZone>(
    events: &[StoredSessionEvent],
    pricing: &PricingTable,
    tz: &Tz,
) -> Vec<SessionCostRow>
where
    Tz::Offset: std::fmt::Display,
{
    let mut parents: Vec<ParentAgg> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();

    for e in events {
        let day = e.ts.with_timezone(tz).format("%Y-%m-%d").to_string();
        let id = format!("{}#{}", session_file_of(&e.source_file), day);
        let at = *index.entry(id.clone()).or_insert_with(|| {
            parents.push(ParentAgg {
                row: SessionCostRow {
                    id,
                    project: e.project.clone(),
                    day,
                    latest_ts: e.ts,
                    turn_count: 0,
                    headline_tokens: 0,
                    total_cost_usd: 0.0,
                    dominant_model: String::new(),
                    breakdown: TokenBreakdown::default(),
                    cost_breakdown: CostBreakdown::default(),
                    subagents: Vec::new(),
                },
                models: ModelOutput::default(),
                subs: Vec::new(),
                sub_index: HashMap::new(),
            });
            parents.len() - 1
        });
        let p = &mut parents[at];
        let is_turn = !is_unlogged_call(&e.event_id);

        // The row is the whole conversation, so a subagent's usage counts
        // toward it as well as being listed underneath.
        if is_turn {
            p.row.turn_count += 1;
        }
        p.row.headline_tokens += e.input_tokens + e.output_tokens;
        p.row.breakdown.input += e.input_tokens;
        p.row.breakdown.output += e.output_tokens;
        p.row.breakdown.cache_read += e.cache_read_tokens;
        p.row.breakdown.cache_write_5m += e.cache_creation_5m_tokens;
        p.row.breakdown.cache_write_1h += e.cache_creation_1h_tokens;
        p.row.total_cost_usd += e.cost_usd;
        p.row.latest_ts = p.row.latest_ts.max(e.ts);
        p.models.add(&e.model, e.output_tokens);

        if let Some(c) = pricing.cost_by_category(
            &e.model,
            e.input_tokens,
            e.output_tokens,
            e.cache_read_tokens,
            e.cache_creation_5m_tokens,
            e.cache_creation_1h_tokens,
        ) {
            p.row.cost_breakdown.input += c.input;
            p.row.cost_breakdown.output += c.output;
            p.row.cost_breakdown.cache_read += c.cache_read;
            p.row.cost_breakdown.cache_write_5m += c.cache_write_5m;
            p.row.cost_breakdown.cache_write_1h += c.cache_write_1h;
        }

        if e.source_file.contains(SUBAGENT_SEGMENT) {
            let sub_at = match p.sub_index.get(&e.source_file) {
                Some(&i) => i,
                None => {
                    p.subs.push(SubAgg {
                        row: SubagentCost {
                            id: e.source_file.clone(),
                            label: subagent_label(&e.source_file),
                            latest_ts: e.ts,
                            turn_count: 0,
                            headline_tokens: 0,
                            total_cost_usd: 0.0,
                            dominant_model: String::new(),
                        },
                        models: ModelOutput::default(),
                    });
                    p.sub_index.insert(e.source_file.clone(), p.subs.len() - 1);
                    p.subs.len() - 1
                }
            };
            let s = &mut p.subs[sub_at];
            if is_turn {
                s.row.turn_count += 1;
            }
            s.row.headline_tokens += e.input_tokens + e.output_tokens;
            s.row.total_cost_usd += e.cost_usd;
            s.row.latest_ts = s.row.latest_ts.max(e.ts);
            s.models.add(&e.model, e.output_tokens);
        }
    }

    let mut rows: Vec<SessionCostRow> = parents
        .into_iter()
        .map(|p| {
            let mut row = p.row;
            row.dominant_model = p.models.dominant();
            row.subagents = p
                .subs
                .into_iter()
                .map(|s| SubagentCost { dominant_model: s.models.dominant(), ..s.row })
                .collect();
            row.subagents
                .sort_by(|a, b| a.latest_ts.cmp(&b.latest_ts).then_with(|| a.id.cmp(&b.id)));
            row
        })
        .collect();
    rows.sort_by(|a, b| b.latest_ts.cmp(&a.latest_ts).then_with(|| a.id.cmp(&b.id)));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn table() -> PricingTable {
        PricingTable::bundled().unwrap()
    }

    fn fixture() -> Vec<StoredSessionEvent> {
        serde_json::from_str(include_str!("../tests/fixtures/session_costs/events.json")).unwrap()
    }

    fn nz() -> FixedOffset {
        FixedOffset::east_opt(13 * 3600).unwrap()
    }

    fn close(a: f64, b: f64, tol: f64, what: &str, id: &str) {
        assert!((a - b).abs() <= tol, "{id}: {what} is {a}, expected {b}");
    }

    /// Every field equal; floats within `tol`.
    fn assert_rows_close(got: &[SessionCostRow], expected: &[SessionCostRow], tol: f64) {
        assert_eq!(
            got.iter().map(|r| &r.id).collect::<Vec<_>>(),
            expected.iter().map(|r| &r.id).collect::<Vec<_>>()
        );
        for (g, e) in got.iter().zip(expected) {
            let id = g.id.as_str();
            assert_eq!(g.project, e.project, "{id}: project");
            assert_eq!(g.day, e.day, "{id}: day");
            assert_eq!(g.latest_ts, e.latest_ts, "{id}: latest_ts");
            assert_eq!(g.turn_count, e.turn_count, "{id}: turn_count");
            assert_eq!(g.headline_tokens, e.headline_tokens, "{id}: headline_tokens");
            assert_eq!(g.dominant_model, e.dominant_model, "{id}: dominant_model");
            assert_eq!(g.breakdown, e.breakdown, "{id}: breakdown");
            close(g.total_cost_usd, e.total_cost_usd, tol, "total_cost_usd", id);
            let (gc, ec) = (&g.cost_breakdown, &e.cost_breakdown);
            close(gc.input, ec.input, tol, "cost input", id);
            close(gc.output, ec.output, tol, "cost output", id);
            close(gc.cache_read, ec.cache_read, tol, "cost cache_read", id);
            close(gc.cache_write_5m, ec.cache_write_5m, tol, "cost cache_write_5m", id);
            close(gc.cache_write_1h, ec.cache_write_1h, tol, "cost cache_write_1h", id);
            assert_eq!(
                g.subagents.iter().map(|s| &s.id).collect::<Vec<_>>(),
                e.subagents.iter().map(|s| &s.id).collect::<Vec<_>>(),
                "{id}: subagent order"
            );
            for (gs, es) in g.subagents.iter().zip(&e.subagents) {
                assert_eq!(gs.label, es.label, "{id}: subagent label");
                assert_eq!(gs.latest_ts, es.latest_ts, "{id}: subagent latest_ts");
                assert_eq!(gs.turn_count, es.turn_count, "{id}: subagent turn_count");
                assert_eq!(gs.headline_tokens, es.headline_tokens, "{id}: subagent tokens");
                assert_eq!(gs.dominant_model, es.dominant_model, "{id}: subagent model");
                close(gs.total_cost_usd, es.total_cost_usd, tol, "subagent cost", id);
            }
        }
    }

    #[test]
    fn matches_the_typescript_aggregator_on_the_golden_fixture() {
        let expected: Vec<SessionCostRow> =
            serde_json::from_str(include_str!("../tests/fixtures/session_costs/expected.json"))
                .unwrap();
        let mut got = aggregate(&fixture(), &table(), &nz());
        got.sort_by(|a, b| a.id.cmp(&b.id));
        assert_rows_close(&got, &expected, 1e-9);
    }

    #[test]
    fn total_cost_equals_the_sum_of_the_events() {
        let events = fixture();
        let rows = aggregate(&events, &table(), &nz());
        let spent: f64 = events.iter().map(|e| e.cost_usd).sum();
        let shown: f64 = rows.iter().map(|r| r.total_cost_usd).sum();
        assert!((spent - shown).abs() < 1e-9, "events {spent}, rows {shown}");
        // A subagent's cost is inside its parent's total, never on top of it.
        for r in &rows {
            let subs: f64 = r.subagents.iter().map(|s| s.total_cost_usd).sum();
            assert!(subs <= r.total_cost_usd + 1e-12, "{}", r.id);
        }
    }

    #[test]
    fn an_empty_window_gives_no_rows() {
        assert!(aggregate(&[], &table(), &Utc).is_empty());
    }

    #[test]
    fn rows_come_back_newest_first() {
        let rows = aggregate(&fixture(), &table(), &nz());
        let stamps: Vec<_> = rows.iter().map(|r| r.latest_ts).collect();
        let mut sorted = stamps.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        assert_eq!(stamps, sorted);
    }

    #[test]
    fn the_day_is_taken_in_the_callers_time_zone() {
        // 10:50 UTC and 11:10 UTC on the 10th are either side of midnight at
        // UTC+13, and the same day in UTC.
        let harbor = |rows: &[SessionCostRow]| {
            rows.iter().filter(|r| r.project == "harbor").map(|r| r.day.clone()).collect::<Vec<_>>()
        };
        let mut local = harbor(&aggregate(&fixture(), &table(), &nz()));
        local.sort();
        assert_eq!(local, ["2026-11-10", "2026-11-11"]);
        assert_eq!(harbor(&aggregate(&fixture(), &table(), &Utc)), ["2026-11-10"]);
    }

    #[test]
    fn a_subagent_whose_session_made_no_calls_that_day_still_gets_a_row() {
        // Only the subagent was ingested: it must appear under its parent's
        // key, as one row with one subagent, rather than vanish.
        let mut only_sub = fixture();
        only_sub.retain(|e| e.event_id == "req_a7:msg_a7");
        let rows = aggregate(&only_sub, &table(), &nz());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "atlas/aaaa1111.jsonl#2026-11-11");
        assert_eq!(rows[0].turn_count, 1);
        assert_eq!(rows[0].subagents.len(), 1);
        assert_eq!(rows[0].subagents[0].label, "agent 0a1b2c3d");
    }

    #[test]
    fn unlogged_calls_add_cost_and_tokens_but_are_not_turns() {
        let rows = aggregate(&fixture(), &table(), &nz());
        let harbor = rows.iter().find(|r| r.id == "harbor/bbbb2222.jsonl#2026-11-11").unwrap();
        assert_eq!(harbor.turn_count, 1, "the est: row is not a turn");
        assert!((harbor.total_cost_usd - 0.324).abs() < 1e-9, "but its cost counts");
        assert_eq!(harbor.breakdown.output, 3350);
    }

    #[test]
    fn a_subagent_is_named_from_its_transcript() {
        assert_eq!(subagent_label("p/s/subagents/agent-0a1b2c3d4e5f.jsonl"), "agent 0a1b2c3d");
        // The first `agent-` that is followed by hex is the one that counts.
        assert_eq!(subagent_label("agent-zeta/s/subagents/agent-ab.jsonl"), "agent ab");
        assert_eq!(subagent_label("p/s/subagents/other.jsonl"), "subagent");
    }
}
