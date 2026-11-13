use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Canonical post-parsed event used by the walker and downstream code.
/// `cost_usd` starts at 0.0 and is computed by the walker via the pricing table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionEvent {
    pub ts: DateTime<Utc>,
    pub project: String,
    pub model: String,

    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub cache_creation_5m_tokens: u64,
    #[serde(default)]
    pub cache_creation_1h_tokens: u64,

    #[serde(default)]
    pub cost_usd: f64,

    /// Stable per-API-call dedup key. "{requestId}:{message.id}" when both
    /// fields are present (native Claude Code); `message.id` alone when
    /// `requestId` is absent (every third-party relay — GLM, k3, MiniMax,
    /// kimi — omits requestId). None only when `message.id` is absent too,
    /// in which case the walker substitutes a structural
    /// "{source_file}:{source_line}" fallback.
    #[serde(default)]
    pub event_id: Option<String>,

    /// The API response's `stop_reason`. `"end_turn"` marks the last call of
    /// a turn, which is when Claude Code fires its unlogged follow-up call
    /// (see `hidden_calls`).
    #[serde(default)]
    pub stop_reason: Option<String>,
    /// `isSidechain` on the record: true for subagent/side-thread calls.
    #[serde(default)]
    pub is_sidechain: bool,
    /// How Claude Code was started: `"cli"` for the interactive terminal,
    /// `"sdk-cli"` for `claude -p` / Agent SDK runs.
    #[serde(default)]
    pub entrypoint: Option<String>,

    #[serde(flatten, default)]
    pub unknown: HashMap<String, serde_json::Value>,
}

impl SessionEvent {
    /// Size of the prompt this call sent: every input-side bucket. This is
    /// the context an unlogged follow-up call re-reads from cache.
    pub fn context_tokens(&self) -> u64 {
        self.input_tokens
            + self.cache_read_tokens
            + self.cache_creation_5m_tokens
            + self.cache_creation_1h_tokens
    }
}

/// Raw shape of one JSONL line as Claude Code writes it. Many line types
/// (`user`, `permission-mode`, `attachment`, `system`, `last-prompt`, etc.)
/// share this envelope but only `assistant` lines carry the usage payload
/// we care about.
#[derive(Debug, Deserialize)]
struct ClaudeCodeRecord {
    #[serde(rename = "type")]
    record_type: String,
    timestamp: DateTime<Utc>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default, rename = "requestId")]
    request_id: Option<String>,
    #[serde(default, rename = "isSidechain")]
    is_sidechain: bool,
    #[serde(default)]
    entrypoint: Option<String>,
    message: Option<ClaudeMessage>,
}

#[derive(Debug, Deserialize)]
struct ClaudeMessage {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    usage: Option<ClaudeUsage>,
}

#[derive(Debug, Deserialize, Default)]
struct ClaudeUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_creation: Option<CacheCreationDetails>,
}

#[derive(Debug, Deserialize, Default)]
struct CacheCreationDetails {
    #[serde(default)]
    ephemeral_5m_input_tokens: u64,
    #[serde(default)]
    ephemeral_1h_input_tokens: u64,
}

/// Parses one JSONL line and returns a `SessionEvent` if the line is an
/// assistant message carrying token usage. All other line types and any
/// malformed records return `None`.
///
/// `fallback_project` is used when the record lacks `cwd` (rare); it should
/// be the JSONL file's parent directory name, which Claude Code derives from
/// the originating cwd anyway.
pub fn parse_event_line(line: &str, fallback_project: &str) -> Option<SessionEvent> {
    let rec: ClaudeCodeRecord = serde_json::from_str(line).ok()?;
    if rec.record_type != "assistant" {
        return None;
    }
    let msg = rec.message?;
    let model = msg.model.clone()?;
    // No usage block → not a usage-bearing message (could be a continuation
    // or partial). Skip silently.
    let usage = msg.usage?;

    // Build the dedup key from Claude's stable identifiers. Prefer the
    // "{requestId}:{message.id}" combo (what ccusage uses) when both are
    // present. When `requestId` is absent — true for every third-party relay
    // (GLM, k3, MiniMax, kimi) — fall back to `message.id` alone. Relays
    // write each response's usage to multiple JSONL lines (one per content
    // block); without this fallback every duplicate line gets a distinct
    // line-based key and is counted separately, inflating relay-model totals
    // 2-8x. `message.id` is the API's globally-unique response id, so keying
    // on it collapses those duplicates.
    let event_id = match (rec.request_id.as_deref(), msg.id.as_deref()) {
        (Some(req), Some(mid)) if !req.is_empty() && !mid.is_empty() => {
            Some(format!("{req}:{mid}"))
        }
        (_, Some(mid)) if !mid.is_empty() => Some(mid.to_string()),
        _ => None,
    };

    let project = rec
        .cwd
        .as_deref()
        .and_then(|c| {
            Path::new(c)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback_project.to_string());

    // Prefer the structured per-bucket split. When older records carry only the
    // flat cache_creation_input_tokens, attribute it to the 5m bucket — that's
    // Anthropic's default TTL, so it's the correct guess and avoids the 1.6×
    // over-billing that 1h pricing would impose.
    let (cache_5m, cache_1h) = match usage.cache_creation.as_ref() {
        Some(c) => (c.ephemeral_5m_input_tokens, c.ephemeral_1h_input_tokens),
        None => (usage.cache_creation_input_tokens, 0),
    };

    Some(SessionEvent {
        ts: rec.timestamp,
        project,
        model,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_read_tokens: usage.cache_read_input_tokens,
        cache_creation_5m_tokens: cache_5m,
        cache_creation_1h_tokens: cache_1h,
        cost_usd: 0.0,
        event_id,
        stop_reason: msg.stop_reason,
        is_sidechain: rec.is_sidechain,
        entrypoint: rec.entrypoint,
        unknown: HashMap::new(),
    })
}

/// Claude Code's own per-model usage for one model, from a `cost-state` record.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OfficialModelUsage {
    #[serde(default, rename = "inputTokens")]
    pub input_tokens: u64,
    #[serde(default, rename = "outputTokens")]
    pub output_tokens: u64,
    #[serde(default, rename = "cacheReadInputTokens")]
    pub cache_read_tokens: u64,
    #[serde(default, rename = "cacheCreationInputTokens")]
    pub cache_creation_tokens: u64,
    #[serde(default, rename = "webSearchRequests")]
    pub web_search_requests: u64,
    #[serde(default, rename = "costUSD")]
    pub cost_usd: f64,
}

/// A `cost-state` record: one Claude Code process's running per-model totals
/// for the session, the same numbers `/cost` prints.
///
/// Claude Code appends one when the process exits. The totals are cumulative
/// and survive `--resume` (the resumed process keeps the same `startTime`),
/// so the last record per `startTime` is that process's total. Two processes
/// can write to one transcript (seen in 2 of 251 sessions); each counts only
/// its own calls, so the session total is the sum per `startTime`.
///
/// `--resume` also starts a new transcript: a copy of the old history, ending
/// in a record with the old `startTime` and the old totals carried forward
/// (0fdfef68 resumed a992e056 on 2026-11-13). Transcripts that share a
/// `startTime` are one running total, so the larger record is the later
/// reading and the others are earlier readings of the same spend; the store
/// reconciles them together (`store::queries::lineage_of`).
///
/// The total counts calls that are
/// never written to the transcript: auto-compaction, away summaries, prompt
/// suggestions, memory extraction, session titles, WebSearch/WebFetch on
/// Haiku. Subagent usage is included.
#[derive(Debug, Clone, PartialEq)]
pub struct CostState {
    /// Start of the process that wrote it (epoch ms); 0 when absent.
    pub start_time: i64,
    /// Keyed by model id exactly as Claude Code wrote it (may carry `[1m]`).
    pub model_usage: Vec<(String, OfficialModelUsage)>,
}

#[derive(Debug, Deserialize)]
struct CostStateRecord {
    #[serde(rename = "type")]
    record_type: String,
    #[serde(default, rename = "startTime")]
    start_time: i64,
    #[serde(default, rename = "modelUsage")]
    model_usage: HashMap<String, OfficialModelUsage>,
}

/// Parse a `cost-state` record. Returns `None` for other lines, and for a
/// record with no model usage (a process that made no API calls), which
/// carries no information.
pub fn parse_cost_state_line(line: &str) -> Option<CostState> {
    // Cheap guard: the walker calls this for every non-assistant line.
    if !line.contains("cost-state") {
        return None;
    }
    let rec: CostStateRecord = serde_json::from_str(line).ok()?;
    if rec.record_type != "cost-state" || rec.model_usage.is_empty() {
        return None;
    }
    let mut model_usage: Vec<_> = rec.model_usage.into_iter().collect();
    model_usage.sort_by(|a, b| a.0.cmp(&b.0));
    Some(CostState { start_time: rec.start_time, model_usage })
}

/// An away summary ("while you were away…" recap): a `system` line Claude
/// Code writes after an unlogged API call that re-read the whole context.
#[derive(Debug, Clone, PartialEq)]
pub struct AwaySummary {
    pub ts: DateTime<Utc>,
    pub uuid: String,
}

#[derive(Debug, Deserialize)]
struct SystemRecord {
    #[serde(rename = "type")]
    record_type: String,
    #[serde(default)]
    subtype: Option<String>,
    timestamp: DateTime<Utc>,
    uuid: String,
}

pub fn parse_away_summary_line(line: &str) -> Option<AwaySummary> {
    if !line.contains("away_summary") {
        return None;
    }
    let rec: SystemRecord = serde_json::from_str(line).ok()?;
    if rec.record_type != "system" || rec.subtype.as_deref() != Some("away_summary") {
        return None;
    }
    Some(AwaySummary { ts: rec.timestamp, uuid: rec.uuid })
}

/// One compaction inside a session: the point where Claude Code summarised
/// the conversation and dropped the rest of the context.
#[derive(Debug, Clone, PartialEq)]
pub struct CompactionEvent {
    pub ts: DateTime<Utc>,
    /// The record's own uuid — the dedup key, mirroring `event_id`.
    pub uuid: String,
    /// "manual" when the user ran `/compact`, "auto" when the context filled up.
    pub trigger: String,
    pub pre_tokens: u64,
    pub post_tokens: u64,
}

#[derive(Debug, Deserialize)]
struct CompactionRecord {
    #[serde(rename = "type")]
    record_type: String,
    timestamp: DateTime<Utc>,
    uuid: String,
    #[serde(rename = "compactMetadata")]
    compact_metadata: CompactMetadata,
}

#[derive(Debug, Deserialize)]
struct CompactMetadata {
    #[serde(default)]
    trigger: Option<String>,
    #[serde(default, rename = "preTokens")]
    pre_tokens: u64,
    #[serde(default, rename = "postTokens")]
    post_tokens: u64,
}

/// Parse a compaction record. Returns `None` for every other line type.
///
/// Claude Code writes this as `type:"system"` **in the middle of the same
/// transcript** — compaction does not open a new session file, which is why
/// a compacted session otherwise looks like an unbroken run of turns.
///
/// The cheap `contains` guard matters: the walker calls this for every line
/// it reads, and without it every non-assistant line would be handed to serde
/// a second time just to be rejected.
pub fn parse_compaction_line(line: &str) -> Option<CompactionEvent> {
    if !line.contains("compactMetadata") {
        return None;
    }
    let rec: CompactionRecord = serde_json::from_str(line).ok()?;
    if rec.record_type != "system" {
        return None;
    }
    Some(CompactionEvent {
        ts: rec.timestamp,
        uuid: rec.uuid,
        // Absent trigger is possible on older writers; "auto" is the safer
        // guess than claiming the user asked for it.
        trigger: rec.compact_metadata.trigger.unwrap_or_else(|| "auto".into()),
        pre_tokens: rec.compact_metadata.pre_tokens,
        post_tokens: rec.compact_metadata.post_tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPACTION_LINE: &str = r#"{
      "type": "system",
      "subtype": "compact_boundary",
      "uuid": "22a1c0de-0000-4000-8000-000000000001",
      "timestamp": "2026-07-29T03:17:26.033Z",
      "compactMetadata": {
        "trigger": "manual",
        "preTokens": 495927,
        "postTokens": 16608,
        "cumulativeDroppedTokens": 479319
      }
    }"#;

    #[test]
    fn parses_compaction_record() {
        let c = parse_compaction_line(COMPACTION_LINE).expect("should parse");
        assert_eq!(c.trigger, "manual");
        assert_eq!(c.pre_tokens, 495_927);
        assert_eq!(c.post_tokens, 16_608);
        assert_eq!(c.uuid, "22a1c0de-0000-4000-8000-000000000001");
    }

    #[test]
    fn compaction_parser_ignores_ordinary_lines() {
        assert!(parse_compaction_line(ASSISTANT_LINE).is_none());
        assert!(parse_compaction_line(USER_LINE).is_none());
        assert!(parse_compaction_line("not json").is_none());
    }

    /// A `system` line without compactMetadata is a plain log entry, and an
    /// assistant line that merely mentions the word must not be mistaken for
    /// a boundary.
    #[test]
    fn compaction_parser_requires_the_metadata_object() {
        let plain_system = r#"{"type":"system","uuid":"u","timestamp":"2026-07-29T03:17:26.033Z"}"#;
        assert!(parse_compaction_line(plain_system).is_none());
        let mentions_it = r#"{"type":"user","uuid":"u","timestamp":"2026-07-29T03:17:26.033Z","message":{"content":"what is compactMetadata?"}}"#;
        assert!(parse_compaction_line(mentions_it).is_none());
    }

    #[test]
    fn compaction_trigger_defaults_to_auto_when_absent() {
        let line = r#"{"type":"system","uuid":"u","timestamp":"2026-07-29T03:17:26.033Z","compactMetadata":{"preTokens":10,"postTokens":2}}"#;
        assert_eq!(parse_compaction_line(line).unwrap().trigger, "auto");
    }

    const ASSISTANT_LINE: &str = r#"{
      "parentUuid": "abc",
      "isSidechain": false,
      "type": "assistant",
      "timestamp": "2026-04-26T03:59:37.845Z",
      "cwd": "/Users/feixu/Developer/my-project",
      "sessionId": "abc-123",
      "message": {
        "model": "claude-opus-4-7",
        "role": "assistant",
        "usage": {
          "input_tokens": 6,
          "output_tokens": 280,
          "cache_read_input_tokens": 19006,
          "cache_creation_input_tokens": 19452,
          "cache_creation": {
            "ephemeral_5m_input_tokens": 0,
            "ephemeral_1h_input_tokens": 19452
          }
        }
      }
    }"#;

    const USER_LINE: &str = r#"{
      "type": "user",
      "timestamp": "2026-04-26T03:59:00.000Z",
      "message": {"role": "user"}
    }"#;

    const PERMISSION_LINE: &str = r#"{
      "type": "permission-mode",
      "timestamp": "2026-04-26T03:59:00.000Z"
    }"#;

    #[test]
    fn parses_assistant_line() {
        let ev = parse_event_line(ASSISTANT_LINE, "fallback").expect("should parse");
        assert_eq!(ev.model, "claude-opus-4-7");
        assert_eq!(ev.project, "my-project");
        assert_eq!(ev.input_tokens, 6);
        assert_eq!(ev.output_tokens, 280);
        assert_eq!(ev.cache_read_tokens, 19006);
        assert_eq!(ev.cache_creation_5m_tokens, 0);
        assert_eq!(ev.cache_creation_1h_tokens, 19452);
        assert_eq!(ev.cost_usd, 0.0);
    }

    #[test]
    fn skips_non_assistant_types() {
        assert!(parse_event_line(USER_LINE, "fallback").is_none());
        assert!(parse_event_line(PERMISSION_LINE, "fallback").is_none());
    }

    #[test]
    fn skips_malformed_json() {
        assert!(parse_event_line("not json", "fallback").is_none());
        assert!(parse_event_line("{}", "fallback").is_none());
    }

    #[test]
    fn falls_back_when_cwd_absent() {
        let line = r#"{
          "type": "assistant",
          "timestamp": "2026-04-26T03:59:37.845Z",
          "message": {
            "model": "claude-haiku-4-5",
            "usage": {"input_tokens": 1, "output_tokens": 1}
          }
        }"#;
        let ev = parse_event_line(line, "-Users-feixu").expect("should parse");
        assert_eq!(ev.project, "-Users-feixu");
    }

    #[test]
    fn event_id_uses_request_id_and_message_id_when_both_present() {
        let line = r#"{
          "type": "assistant",
          "timestamp": "2026-04-26T03:59:37.845Z",
          "cwd": "/x/y",
          "requestId": "req_abc",
          "message": {
            "id": "msg_xyz",
            "model": "claude-sonnet-4-6",
            "usage": {"input_tokens": 1, "output_tokens": 1}
          }
        }"#;
        let ev = parse_event_line(line, "fb").expect("should parse");
        assert_eq!(ev.event_id.as_deref(), Some("req_abc:msg_xyz"));
    }

    #[test]
    fn event_id_uses_message_id_when_request_id_absent() {
        // This is the third-party relay case (GLM / k3 / MiniMax / kimi):
        // requestId is never written, so we must dedupe on message.id alone.
        // Without this, the same response written to multiple JSONL lines
        // would be counted once per line.
        let relay_line = r#"{
          "type": "assistant",
          "timestamp": "2026-04-26T03:59:37.845Z",
          "cwd": "/x/y",
          "message": {
            "id": "msg_xyz",
            "model": "claude-sonnet-4-6",
            "usage": {"input_tokens": 1, "output_tokens": 1}
          }
        }"#;
        assert_eq!(
            parse_event_line(relay_line, "fb").unwrap().event_id.as_deref(),
            Some("msg_xyz")
        );
    }

    #[test]
    fn event_id_is_none_when_message_id_absent() {
        // Only message.id is absent → no content-stable key is possible, so
        // the walker falls back to the structural {source_file}:{line} key.
        let no_message_id = r#"{
          "type": "assistant",
          "timestamp": "2026-04-26T03:59:37.845Z",
          "cwd": "/x/y",
          "requestId": "req_abc",
          "message": {
            "model": "claude-sonnet-4-6",
            "usage": {"input_tokens": 1, "output_tokens": 1}
          }
        }"#;
        assert!(parse_event_line(no_message_id, "fb").unwrap().event_id.is_none());
    }

    #[test]
    fn captures_turn_end_metadata() {
        let line = r#"{"type":"assistant","timestamp":"2026-11-02T18:10:20Z","cwd":"/x/y",
          "isSidechain":false,"entrypoint":"cli","requestId":"r","message":{"id":"m",
          "model":"claude-opus-5-5","stop_reason":"end_turn",
          "usage":{"input_tokens":2,"output_tokens":5,"cache_read_input_tokens":1000,
                   "cache_creation_input_tokens":30}}}"#;
        let ev = parse_event_line(line, "fb").unwrap();
        assert_eq!(ev.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(ev.entrypoint.as_deref(), Some("cli"));
        assert!(!ev.is_sidechain);
        assert_eq!(ev.context_tokens(), 1032);
    }

    /// Shape copied from a real Claude Code 2.1.316 transcript.
    const COST_STATE_LINE: &str = r#"{"type":"cost-state","sessionId":"fcfb095a",
      "totalCostUSD":0.2854113,"startTime":1792262748949,
      "modelUsage":{
        "claude-haiku-5-5":{"inputTokens":1181,"outputTokens":22,"thinkingTokens":0,
          "cacheReadInputTokens":0,"cacheCreationInputTokens":0,"webSearchRequests":0,
          "costUSD":0.0001291},
        "claude-sonnet-5-5":{"inputTokens":1034,"outputTokens":7625,"thinkingTokens":3971,
          "cacheReadInputTokens":690922,"cacheCreationInputTokens":34468,
          "webSearchRequests":0,"costUSD":0.2852822}},
      "hasUnknownModelCost":false}"#;

    #[test]
    fn parses_cost_state_record() {
        let cs = parse_cost_state_line(COST_STATE_LINE).expect("should parse");
        assert_eq!(cs.start_time, 1_792_262_748_949);
        assert_eq!(cs.model_usage.len(), 2);
        let (model, u) = &cs.model_usage[1];
        assert_eq!(model, "claude-sonnet-5-5");
        assert_eq!(u.cache_read_tokens, 690_922);
        assert_eq!(u.cache_creation_tokens, 34_468);
        assert!((u.cost_usd - 0.2852822).abs() < 1e-12);
    }

    #[test]
    fn cost_state_without_usage_or_other_types_is_ignored() {
        let empty = r#"{"type":"cost-state","totalCostUSD":0,"modelUsage":{}}"#;
        assert!(parse_cost_state_line(empty).is_none());
        assert!(parse_cost_state_line(ASSISTANT_LINE).is_none());
        let mentions = r#"{"type":"user","message":{"content":"what is cost-state?"}}"#;
        assert!(parse_cost_state_line(mentions).is_none());
    }

    #[test]
    fn parses_away_summary_record() {
        let line = r#"{"type":"system","subtype":"away_summary","content":"Recap",
          "timestamp":"2026-11-02T19:24:35.132Z","uuid":"06bc1b4d"}"#;
        let a = parse_away_summary_line(line).unwrap();
        assert_eq!(a.uuid, "06bc1b4d");
        assert!(parse_away_summary_line(COMPACTION_LINE).is_none());
    }

    #[test]
    fn flat_cache_creation_field_used_when_no_split() {
        let line = r#"{
          "type": "assistant",
          "timestamp": "2026-04-26T03:59:37.845Z",
          "cwd": "/x/y",
          "message": {
            "model": "claude-sonnet-4-6",
            "usage": {
              "input_tokens": 10,
              "output_tokens": 20,
              "cache_creation_input_tokens": 500
            }
          }
        }"#;
        let ev = parse_event_line(line, "fb").expect("should parse");
        assert_eq!(ev.cache_creation_5m_tokens, 500);
        assert_eq!(ev.cache_creation_1h_tokens, 0);
    }
}
