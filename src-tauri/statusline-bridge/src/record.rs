//! What the bridge records about one Claude Code process (spec §5.1, §5.3).
//!
//! [`next_record`] is a pure function of the previous record and what this
//! status-line run saw, so every confirmation rule is testable without a
//! filesystem. [`write_record`] and [`prune`] are the only parts that touch
//! disk.

use crate::format::{obs_file_name, Identity, Observation, RateLimits, Window, OBS_VERSION};
use crate::identity::Resolution;
use serde_json::Value;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::time::{Duration, SystemTime};

/// Only this much of stdin is parsed; the original command still receives all
/// of it.
pub const MAX_PARSE_BYTES: usize = 1024 * 1024;

/// Settling time after the identity changes, matching the CLI's own token
/// cache lifetime (spec §2.4).
const GRACE_MS: i64 = 30_000;

/// A write whose `observed_at_ms` is older than the file's by up to this much
/// is an overlapping older run and is skipped; a file further in the future
/// than this means the clock jumped back, and the write goes ahead.
const CLOCK_JUMP_MS: i64 = 5 * 60 * 1000;

/// What this run's stdin told us. Everything is optional on the wire; the
/// shape drifts between CLI versions, so only the fields we need are read.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StdinFacts {
    pub session_id: String,
    pub transcript_path: Option<String>,
    pub rate_limits: Option<RateLimits>,
    /// `rate_limits` was present but not in a shape we understand.
    pub rate_limits_malformed: bool,
    /// `cost.total_api_duration_ms`.
    pub api_ms: i64,
    /// `prompt_cache.requests`, 0 when absent.
    pub requests: i64,
    pub cc_version: Option<String>,
}

#[derive(Debug)]
pub enum ParseError {
    Json(serde_json::Error),
    /// Without a session id there is no file to name.
    NoSessionId,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Json(e) => write!(f, "stdin is not JSON: {e}"),
            ParseError::NoSessionId => write!(f, "stdin has no session_id"),
        }
    }
}

fn as_i64(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))
}

fn parse_window(v: Option<&Value>) -> Option<Window> {
    let v = v?;
    Some(Window {
        used_percentage: v.get("used_percentage")?.as_f64()?,
        resets_at: as_i64(v.get("resets_at")?)?,
    })
}

fn parse_rate_limits(v: Option<&Value>) -> (Option<RateLimits>, bool) {
    match v {
        None | Some(Value::Null) => (None, false),
        Some(Value::Object(o)) => {
            let limits = RateLimits {
                five_hour: parse_window(o.get("five_hour")),
                seven_day: parse_window(o.get("seven_day")),
            };
            if limits.is_empty() {
                // `{}` or only `spend_limit` is fine; a window we can't read is not.
                let unreadable = o.contains_key("five_hour") || o.contains_key("seven_day");
                (None, unreadable)
            } else {
                (Some(limits), false)
            }
        }
        Some(_) => (None, true),
    }
}

impl StdinFacts {
    pub fn parse(bytes: &[u8]) -> Result<StdinFacts, ParseError> {
        let head = &bytes[..bytes.len().min(MAX_PARSE_BYTES)];
        let v: Value = serde_json::from_slice(head).map_err(ParseError::Json)?;
        let session_id = v
            .get("session_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or(ParseError::NoSessionId)?
            .to_string();
        let (rate_limits, rate_limits_malformed) = parse_rate_limits(v.get("rate_limits"));
        Ok(StdinFacts {
            session_id,
            transcript_path: v
                .get("transcript_path")
                .and_then(Value::as_str)
                .map(str::to_string),
            rate_limits,
            rate_limits_malformed,
            api_ms: v
                .pointer("/cost/total_api_duration_ms")
                .and_then(as_i64)
                .unwrap_or(0),
            requests: v
                .pointer("/prompt_cache/requests")
                .and_then(as_i64)
                .unwrap_or(0),
            cc_version: v.get("version").and_then(Value::as_str).map(str::to_string),
        })
    }
}

/// An identity recorded only because the very first config read failed.
fn is_read_error_placeholder(identity: &Identity) -> bool {
    matches!(identity, Identity::None { reason: Some(r) } if r == "read_error")
}

/// The record that follows `prev` for the same `(session_id, claude_pid)`.
pub fn next_record(
    prev: Option<&Observation>,
    facts: &StdinFacts,
    ident: Resolution,
    claude_pid: u32,
    config_home: &str,
    now_ms: i64,
) -> Observation {
    let identity = match ident {
        // A torn read says nothing about who is signed in: keep the previous
        // identity and everything derived from it, record only a heartbeat.
        Resolution::ReadError => {
            return match prev {
                Some(p) => {
                    let mut r = p.clone();
                    r.observed_at_ms = now_ms;
                    r.no_data_reason = Some("read_error".into());
                    r
                }
                None => Observation {
                    v: OBS_VERSION,
                    session_id: facts.session_id.clone(),
                    claude_pid,
                    config_home: config_home.to_string(),
                    identity: Identity::None {
                        reason: Some("read_error".into()),
                    },
                    observed_at_ms: now_ms,
                    api_ms: facts.api_ms,
                    requests: facts.requests,
                    data_at_ms: now_ms,
                    no_data_reason: Some("read_error".into()),
                    rate_limits: None,
                    confirmed: false,
                    identity_since_ms: now_ms,
                    baseline_api_ms: None,
                    baseline_requests: None,
                    growths_since_baseline: 0,
                    pre_switch_rate_limits: None,
                    cc_version: facts.cc_version.clone(),
                },
            };
        }
        Resolution::Identity(i) => i,
    };

    // A placeholder from a failed first read is not a real previous record.
    let prev = prev.filter(|p| !is_read_error_placeholder(&p.identity));
    let rl = facts.rate_limits.clone();

    let mut rec = Observation {
        v: OBS_VERSION,
        session_id: facts.session_id.clone(),
        claude_pid,
        config_home: config_home.to_string(),
        identity: identity.clone(),
        observed_at_ms: now_ms,
        api_ms: facts.api_ms,
        requests: facts.requests,
        data_at_ms: now_ms,
        no_data_reason: None,
        rate_limits: rl.clone(),
        confirmed: false,
        identity_since_ms: now_ms,
        baseline_api_ms: None,
        baseline_requests: None,
        growths_since_baseline: 0,
        pre_switch_rate_limits: None,
        cc_version: facts.cc_version.clone(),
    };

    match prev {
        // First record of this process. Without `rate_limits` (the usual case:
        // the CLI runs the status line at mount, before any API call) it
        // starts with empty state under its current token, so it is
        // confirmed. One that already carries `rate_limits` is treated as a
        // fresh identity change: it covers a bridge enabled mid-session after
        // a swap it never saw.
        None => rec.confirmed = rl.is_none(),
        Some(p) => {
            if p.identity != identity {
                rec.pre_switch_rate_limits = p.rate_limits.clone();
            } else {
                rec.confirmed = p.confirmed;
                rec.identity_since_ms = p.identity_since_ms;
                rec.baseline_api_ms = p.baseline_api_ms;
                rec.baseline_requests = p.baseline_requests;
                rec.growths_since_baseline = p.growths_since_baseline;
                rec.pre_switch_rate_limits = p.pre_switch_rate_limits.clone();
                // A counter that went down is a reset (a resumed process
                // starts its totals again): baseline afresh, keep `confirmed`.
                if facts.api_ms < p.api_ms || facts.requests < p.requests {
                    rec.baseline_api_ms = None;
                    rec.baseline_requests = None;
                    rec.growths_since_baseline = 0;
                }
            }
            // `data_at` records when data arrived, not when the status line
            // last ran: a line refreshing every minute with no API traffic
            // must not make old numbers look fresh.
            let grew = facts.api_ms > p.api_ms || facts.requests > p.requests;
            let changed = rl != p.rate_limits;
            rec.data_at_ms = if grew || changed { now_ms } else { p.data_at_ms };
        }
    }

    if !rec.confirmed {
        match rec.baseline_api_ms {
            None => {
                if now_ms >= rec.identity_since_ms + GRACE_MS {
                    rec.baseline_api_ms = Some(facts.api_ms);
                    rec.baseline_requests = Some(facts.requests);
                }
            }
            Some(base_api) => {
                let base_req = rec.baseline_requests.unwrap_or(0);
                if facts.api_ms > base_api || facts.requests > base_req {
                    rec.growths_since_baseline += 1;
                    rec.baseline_api_ms = Some(base_api.max(facts.api_ms));
                    rec.baseline_requests = Some(base_req.max(facts.requests));
                    // The first response after the baseline is the new
                    // account's own data if its numbers differ from before the
                    // switch; identical numbers could be an idle or capped
                    // account either way, so a second response must follow.
                    let differs = rl.is_some() && rl != rec.pre_switch_rate_limits;
                    if (rec.growths_since_baseline == 1 && differs)
                        || rec.growths_since_baseline >= 2
                    {
                        rec.confirmed = true;
                    }
                }
            }
        }
    }

    rec.no_data_reason = match &identity {
        Identity::None { reason: Some(reason) } => Some(reason.clone()),
        _ if facts.rate_limits_malformed => Some("parse_error".into()),
        _ if rl.is_none() => Some("no_rate_limits".into()),
        _ => None,
    };
    rec
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    Written,
    SkippedOlder,
}

fn create_private_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(dir)
    }
}

fn create_private_file(path: &Path) -> io::Result<fs::File> {
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)
}

/// Write the record to `dir` atomically (a temp file in the same directory,
/// then a rename), unless an overlapping older run would overwrite a newer
/// one. Files hold an email address, so they are 0600 in a 0700 directory.
pub fn write_record(dir: &Path, rec: &Observation) -> io::Result<WriteOutcome> {
    create_private_dir(dir)?;
    let name = obs_file_name(&rec.session_id, Some(rec.claude_pid));
    let target = dir.join(&name);

    if let Ok(text) = fs::read_to_string(&target) {
        if let Ok(existing) = serde_json::from_str::<Observation>(&text) {
            let ahead = existing.observed_at_ms - rec.observed_at_ms;
            if ahead > 0 && ahead <= CLOCK_JUMP_MS {
                return Ok(WriteOutcome::SkippedOlder);
            }
        }
    }

    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let tmp = dir.join(format!("{name}.{}.{nanos:x}.tmp", std::process::id()));
    let result = (|| {
        let mut file = create_private_file(&tmp)?;
        file.write_all(serde_json::to_string(rec)?.as_bytes())?;
        file.flush()?;
        drop(file);
        fs::rename(&tmp, &target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.map(|()| WriteOutcome::Written)
}

/// Delete observation files older than 24 h and temp files older than 5 min,
/// at most once an hour (a marker file records the last run). Returns how
/// many files were removed.
pub fn prune(dir: &Path, now: SystemTime) -> io::Result<usize> {
    const OBS_MAX_AGE: Duration = Duration::from_secs(24 * 3600);
    const TMP_MAX_AGE: Duration = Duration::from_secs(5 * 60);
    const EVERY: Duration = Duration::from_secs(3600);

    let marker = dir.join(".pruned");
    if let Ok(modified) = fs::metadata(&marker).and_then(|m| m.modified()) {
        if now.duration_since(modified).is_ok_and(|age| age < EVERY) {
            return Ok(0);
        }
    }
    let mut removed = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let max_age = match path.extension().and_then(|e| e.to_str()) {
            Some("json") => OBS_MAX_AGE,
            Some("tmp") => TMP_MAX_AGE,
            _ => continue,
        };
        let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        if now.duration_since(modified).is_ok_and(|age| age > max_age) && fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    // Touch the marker.
    fs::write(&marker, b"")?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use filetime::{set_file_mtime, FileTime};
    use tempfile::tempdir;

    const T0: i64 = 1_800_000_000_000;

    fn oauth(uuid: &str) -> Resolution {
        Resolution::Identity(Identity::Oauth {
            account_uuid: uuid.into(),
            email: "a@b.c".into(),
            organization_uuid: None,
        })
    }

    fn none() -> Resolution {
        Resolution::Identity(Identity::None { reason: None })
    }

    fn limits(five: f64, seven: f64) -> RateLimits {
        RateLimits {
            five_hour: Some(Window { used_percentage: five, resets_at: 1_800_003_600 }),
            seven_day: Some(Window { used_percentage: seven, resets_at: 1_800_086_400 }),
        }
    }

    fn facts(api_ms: i64, requests: i64, rl: Option<RateLimits>) -> StdinFacts {
        StdinFacts {
            session_id: "sess".into(),
            transcript_path: None,
            rate_limits: rl,
            rate_limits_malformed: false,
            api_ms,
            requests,
            cc_version: Some("2.1.318".into()),
        }
    }

    fn step(prev: Option<&Observation>, f: &StdinFacts, ident: Resolution, at: i64) -> Observation {
        next_record(prev, f, ident, 7, "/home/u/.claude", at)
    }

    // ---- stdin parsing ----

    #[test]
    fn parses_the_2_1_318_statusline_snapshot_fixture() {
        let bytes = include_bytes!("../tests/fixtures/stdin_2_1_318.json");
        let f = StdinFacts::parse(bytes).unwrap();
        assert_eq!(f.session_id, "4b1e7c2a-0000-4000-8000-000000000001");
        assert_eq!(f.api_ms, 849_637);
        assert_eq!(f.requests, 64);
        assert_eq!(f.cc_version.as_deref(), Some("2.1.318"));
        let rl = f.rate_limits.unwrap();
        assert_eq!(rl.five_hour.unwrap(), Window { used_percentage: 62.0, resets_at: 1_793_776_800 });
        assert_eq!(rl.seven_day.unwrap().used_percentage, 21.0);
        assert!(!f.rate_limits_malformed);
    }

    #[test]
    fn missing_fields_default_and_unknown_shapes_are_tolerated() {
        let f = StdinFacts::parse(br#"{"session_id":"s"}"#).unwrap();
        assert_eq!((f.api_ms, f.requests), (0, 0));
        assert!(f.rate_limits.is_none() && !f.rate_limits_malformed);

        let only_spend = StdinFacts::parse(br#"{"session_id":"s","rate_limits":{"spend_limit":{"x":1}}}"#).unwrap();
        assert!(only_spend.rate_limits.is_none() && !only_spend.rate_limits_malformed);

        let bad = StdinFacts::parse(br#"{"session_id":"s","rate_limits":{"five_hour":{"used_percentage":"lots"}}}"#).unwrap();
        assert!(bad.rate_limits.is_none() && bad.rate_limits_malformed);

        let not_obj = StdinFacts::parse(br#"{"session_id":"s","rate_limits":7}"#).unwrap();
        assert!(not_obj.rate_limits_malformed);

        let floats = StdinFacts::parse(br#"{"session_id":"s","rate_limits":{"five_hour":{"used_percentage":41.3,"resets_at":1800003600.0}}}"#).unwrap();
        assert_eq!(floats.rate_limits.unwrap().five_hour.unwrap().resets_at, 1_800_003_600);
    }

    #[test]
    fn stdin_without_a_session_id_or_json_is_a_parse_error() {
        assert!(matches!(StdinFacts::parse(b"{}"), Err(ParseError::NoSessionId)));
        assert!(matches!(StdinFacts::parse(b"not json"), Err(ParseError::Json(_))));
    }

    // ---- confirmation (§5.3) ----

    #[test]
    fn first_record_without_rate_limits_is_confirmed() {
        let r = step(None, &facts(0, 0, None), none(), T0);
        assert!(r.confirmed);
        assert_eq!(r.no_data_reason.as_deref(), Some("no_rate_limits"));
    }

    #[test]
    fn first_record_with_rate_limits_is_unconfirmed() {
        let r = step(None, &facts(5000, 3, Some(limits(10.0, 5.0))), none(), T0);
        assert!(!r.confirmed);
        assert_eq!(r.identity_since_ms, T0);
        assert!(r.no_data_reason.is_none());
    }

    #[test]
    fn identity_change_resets_confirmation_and_stores_pre_switch_limits() {
        let a = step(None, &facts(0, 0, None), oauth("A"), T0);
        let a2 = step(Some(&a), &facts(5000, 2, Some(limits(10.0, 5.0))), oauth("A"), T0 + 1000);
        assert!(a2.confirmed);

        let b = step(Some(&a2), &facts(5000, 2, Some(limits(10.0, 5.0))), oauth("B"), T0 + 2000);
        assert!(!b.confirmed);
        assert_eq!(b.identity_since_ms, T0 + 2000);
        assert_eq!(b.pre_switch_rate_limits, Some(limits(10.0, 5.0)));
        assert_eq!(b.baseline_api_ms, None);
        assert_eq!(b.growths_since_baseline, 0);
    }

    #[test]
    fn baseline_freezes_after_30s_grace() {
        let prev = step(None, &facts(5000, 3, Some(limits(10.0, 5.0))), none(), T0);
        let early = step(Some(&prev), &facts(5000, 3, Some(limits(10.0, 5.0))), none(), T0 + 29_000);
        assert_eq!(early.baseline_api_ms, None);
        let frozen = step(Some(&early), &facts(5200, 3, Some(limits(10.0, 5.0))), none(), T0 + 30_000);
        assert_eq!(frozen.baseline_api_ms, Some(5200));
        assert_eq!(frozen.baseline_requests, Some(3));
        assert_eq!(frozen.growths_since_baseline, 0);
        assert!(!frozen.confirmed);
    }

    fn frozen_pre_switch() -> Observation {
        // Mid-session start: unconfirmed, pre-switch limits = none, baseline frozen.
        let first = step(None, &facts(5000, 3, Some(limits(10.0, 5.0))), none(), T0);
        let mut frozen = step(Some(&first), &facts(5000, 3, Some(limits(10.0, 5.0))), none(), T0 + 31_000);
        // Pretend the previous account had these exact numbers.
        frozen.pre_switch_rate_limits = Some(limits(10.0, 5.0));
        frozen
    }

    #[test]
    fn growth_with_different_values_confirms() {
        let frozen = frozen_pre_switch();
        let r = step(Some(&frozen), &facts(6000, 4, Some(limits(12.0, 5.0))), none(), T0 + 40_000);
        assert_eq!(r.growths_since_baseline, 1);
        assert!(r.confirmed);
    }

    #[test]
    fn growth_with_identical_values_does_not_confirm_but_a_second_growth_does() {
        let frozen = frozen_pre_switch();
        let one = step(Some(&frozen), &facts(6000, 4, Some(limits(10.0, 5.0))), none(), T0 + 40_000);
        assert_eq!(one.growths_since_baseline, 1);
        assert!(!one.confirmed, "identical numbers could still be the old account's");
        let two = step(Some(&one), &facts(7000, 5, Some(limits(10.0, 5.0))), none(), T0 + 50_000);
        assert_eq!(two.growths_since_baseline, 2);
        assert!(two.confirmed);
    }

    #[test]
    fn growth_without_rate_limits_does_not_confirm_on_the_first_growth() {
        let frozen = frozen_pre_switch();
        let r = step(Some(&frozen), &facts(6000, 4, None), none(), T0 + 40_000);
        assert_eq!(r.growths_since_baseline, 1);
        assert!(!r.confirmed);
    }

    #[test]
    fn transient_read_error_keeps_identity_and_confirmation() {
        let a = step(None, &facts(0, 0, None), oauth("A"), T0);
        let a2 = step(Some(&a), &facts(5000, 2, Some(limits(10.0, 5.0))), oauth("A"), T0 + 1000);
        let torn = step(Some(&a2), &facts(5100, 2, Some(limits(11.0, 5.0))), Resolution::ReadError, T0 + 2000);
        assert_eq!(torn.identity, a2.identity);
        assert!(torn.confirmed);
        assert_eq!(torn.rate_limits, a2.rate_limits, "a torn read must not touch the data");
        assert_eq!(torn.no_data_reason.as_deref(), Some("read_error"));
        assert_eq!(torn.observed_at_ms, T0 + 2000);
        // And the next good read is not an identity change.
        let after = step(Some(&torn), &facts(5200, 3, Some(limits(12.0, 5.0))), oauth("A"), T0 + 3000);
        assert!(after.confirmed);
        assert_eq!(after.identity_since_ms, a2.identity_since_ms);
    }

    #[test]
    fn a_failed_first_read_is_not_a_previous_identity() {
        let torn = step(None, &facts(0, 0, None), Resolution::ReadError, T0);
        assert!(!torn.confirmed);
        let good = step(Some(&torn), &facts(0, 0, None), oauth("A"), T0 + 1000);
        assert!(good.confirmed, "treated as the first real record of the process");
        assert!(good.pre_switch_rate_limits.is_none());
    }

    #[test]
    fn api_ms_going_down_rebaselines_without_clearing_confirmed() {
        // Confirmed record: a counter reset keeps it confirmed and clears the
        // (unused) baseline state.
        let a = step(None, &facts(0, 0, None), oauth("A"), T0);
        let a2 = step(Some(&a), &facts(900_000, 50, Some(limits(10.0, 5.0))), oauth("A"), T0 + 60_000);
        assert!(a2.confirmed);
        let reset = step(Some(&a2), &facts(100, 1, Some(limits(10.0, 5.0))), oauth("A"), T0 + 120_000);
        assert!(reset.confirmed);
        assert_eq!((reset.baseline_api_ms, reset.baseline_requests), (None, None));
        assert_eq!(reset.growths_since_baseline, 0);

        // Unconfirmed record past its grace period: the baseline step re-runs
        // at once, freezing at the new, lower totals.
        let frozen = frozen_pre_switch();
        assert_eq!(frozen.baseline_api_ms, Some(5000));
        let reset = step(Some(&frozen), &facts(100, 1, Some(limits(10.0, 5.0))), none(), T0 + 60_000);
        assert!(!reset.confirmed);
        assert_eq!((reset.baseline_api_ms, reset.baseline_requests), (Some(100), Some(1)));
        assert_eq!(reset.growths_since_baseline, 0);
    }

    #[test]
    fn request_growth_alone_counts_as_growth_and_new_data() {
        let frozen = frozen_pre_switch();
        let r = step(Some(&frozen), &facts(5000, 4, Some(limits(10.0, 5.0))), none(), T0 + 40_000);
        assert_eq!(r.growths_since_baseline, 1);
        assert_eq!(r.data_at_ms, T0 + 40_000);
    }

    #[test]
    fn new_claude_pid_gets_its_own_file_and_baseline() {
        let dir = tempdir().unwrap();
        let f = facts(5000, 3, Some(limits(10.0, 5.0)));
        let one = next_record(None, &f, none(), 100, "/h", T0);
        let two = next_record(None, &f, none(), 200, "/h", T0 + 1);
        write_record(dir.path(), &one).unwrap();
        write_record(dir.path(), &two).unwrap();
        let mut names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, vec!["sess.100.json", "sess.200.json"]);
        assert_eq!(one.baseline_api_ms, None);
        assert_eq!(two.baseline_api_ms, None);
    }

    // ---- data_at ----

    #[test]
    fn data_at_advances_only_on_new_data() {
        let a = step(None, &facts(5000, 3, Some(limits(10.0, 5.0))), none(), T0);
        // Nothing changed: a minute later the heartbeat moves, the data does not.
        let quiet = step(Some(&a), &facts(5000, 3, Some(limits(10.0, 5.0))), none(), T0 + 60_000);
        assert_eq!(quiet.observed_at_ms, T0 + 60_000);
        assert_eq!(quiet.data_at_ms, T0);
        // A request completed: new data.
        let grew = step(Some(&quiet), &facts(5100, 4, Some(limits(10.0, 5.0))), none(), T0 + 90_000);
        assert_eq!(grew.data_at_ms, T0 + 90_000);
    }

    #[test]
    fn changed_rate_limits_without_counter_growth_still_counts() {
        // A header reading from a 429 error grows neither counter.
        let a = step(None, &facts(5000, 3, Some(limits(10.0, 5.0))), none(), T0);
        let r = step(Some(&a), &facts(5000, 3, Some(limits(100.0, 5.0))), none(), T0 + 5000);
        assert_eq!(r.data_at_ms, T0 + 5000);
    }

    #[test]
    fn heartbeat_records_no_data_reason() {
        let none_rl = step(None, &facts(0, 0, None), none(), T0);
        assert_eq!(none_rl.no_data_reason.as_deref(), Some("no_rate_limits"));

        let routed = step(
            None,
            &facts(0, 0, None),
            Resolution::Identity(Identity::None { reason: Some("routed".into()) }),
            T0,
        );
        assert_eq!(routed.no_data_reason.as_deref(), Some("routed"));

        let mut malformed = facts(0, 0, None);
        malformed.rate_limits_malformed = true;
        assert_eq!(step(None, &malformed, none(), T0).no_data_reason.as_deref(), Some("parse_error"));

        let torn = step(None, &facts(0, 0, None), Resolution::ReadError, T0);
        assert_eq!(torn.no_data_reason.as_deref(), Some("read_error"));
    }

    // ---- writing ----

    #[test]
    fn older_observed_at_write_is_skipped() {
        let dir = tempdir().unwrap();
        let newer = step(None, &facts(5000, 3, Some(limits(40.0, 5.0))), none(), T0 + 2000);
        let older = step(None, &facts(4000, 2, Some(limits(30.0, 5.0))), none(), T0);
        assert_eq!(write_record(dir.path(), &newer).unwrap(), WriteOutcome::Written);
        assert_eq!(write_record(dir.path(), &older).unwrap(), WriteOutcome::SkippedOlder);
        let kept: Observation = serde_json::from_str(
            &fs::read_to_string(dir.path().join("sess.7.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(kept.rate_limits.unwrap().five_hour.unwrap().used_percentage, 40.0);
    }

    #[test]
    fn a_record_6_minutes_in_the_future_is_overwritten() {
        let dir = tempdir().unwrap();
        let future = step(None, &facts(5000, 3, Some(limits(40.0, 5.0))), none(), T0 + 6 * 60_000);
        let now = step(None, &facts(4000, 2, Some(limits(30.0, 5.0))), none(), T0);
        write_record(dir.path(), &future).unwrap();
        assert_eq!(write_record(dir.path(), &now).unwrap(), WriteOutcome::Written);
    }

    #[cfg(unix)]
    #[test]
    fn write_is_atomic_and_0600_in_a_0700_dir() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempdir().unwrap();
        let dir = root.path().join("obs");
        let rec = step(None, &facts(5000, 3, Some(limits(40.0, 5.0))), none(), T0);
        write_record(&dir, &rec).unwrap();
        let file_mode = fs::metadata(dir.join("sess.7.json")).unwrap().permissions().mode() & 0o777;
        let dir_mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(file_mode, 0o600);
        assert_eq!(dir_mode, 0o700);
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "tmp"))
            .collect();
        assert!(leftovers.is_empty(), "no temp file may outlive the rename");
    }

    // ---- pruning ----

    fn age(path: &Path, secs: u64) {
        let t = SystemTime::now() - Duration::from_secs(secs);
        set_file_mtime(path, FileTime::from_system_time(t)).unwrap();
    }

    #[test]
    fn prune_removes_old_obs_and_stale_tmp_only_and_is_hourly() {
        let dir = tempdir().unwrap();
        let p = dir.path();
        for name in ["old.1.json", "fresh.2.json", "stale.tmp", "recent.tmp", "other.txt"] {
            fs::write(p.join(name), "{}").unwrap();
        }
        age(&p.join("old.1.json"), 25 * 3600);
        age(&p.join("stale.tmp"), 6 * 60);
        age(&p.join("other.txt"), 48 * 3600);

        let removed = prune(p, SystemTime::now()).unwrap();
        assert_eq!(removed, 2);
        assert!(!p.join("old.1.json").exists() && !p.join("stale.tmp").exists());
        assert!(p.join("fresh.2.json").exists() && p.join("recent.tmp").exists());
        assert!(p.join("other.txt").exists(), "unknown files are never ours to delete");

        // Within the hour, nothing runs even though a file has since expired.
        age(&p.join("fresh.2.json"), 30 * 3600);
        assert_eq!(prune(p, SystemTime::now()).unwrap(), 0);
        assert!(p.join("fresh.2.json").exists());

        // An hour later it runs again.
        age(&p.join(".pruned"), 3601);
        assert_eq!(prune(p, SystemTime::now()).unwrap(), 1);
    }
}
