//! Statusline observations as a local usage source (spec §5).
//!
//! The statusline bridge records Claude Code's `rate_limits` into one file per
//! CLI process under `obs/`. This module decides which observation, if any,
//! belongs to a managed account right now, and turns it into a
//! [`UsageSnapshot`] the poll loop can adopt like any other local source.
//!
//! Everything that decides is a pure function of an [`Observation`] and a
//! [`SlotContext`]; only [`freshest_for_slot`] touches the filesystem.

use crate::app_state::{AppState, CachedUsage, SnapshotOrigin};
use crate::auth::accounts::{AddSource, ManagedAccount};
use crate::poll_loop::{has_usage_data, is_within_age};
use crate::usage_api::{UsageSnapshot, Utilization};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use statusline_bridge::format::{obs_dir, bridge_root, Identity, Observation, RateLimits, Window, OBS_VERSION};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

/// After this long, a statusline adoption no longer satisfies an
/// HTTP-capable slot's poll: the Sonnet/Opus/extra-usage buckets it carries
/// over come only from `/usage`, so the slot makes one real call.
pub const CARRIED_BUCKET_MAX_AGE: Duration = Duration::from_secs(15 * 60);

/// How far the observation's reset times may differ from the slot's own
/// `/usage` snapshot before the observation is judged to belong to another
/// account.
const RESET_TOLERANCE_SECS: i64 = 60;

/// Settling time after the live account changes, matching the CLI's token
/// cache lifetime: a response served under the old token can finish after
/// the swap.
const ACTIVE_SINCE_GRACE_SECS: i64 = 30;

/// Everything about one slot the rules need.
pub struct SlotContext<'a> {
    pub acc: &'a ManagedAccount,
    /// Whether this slot is the live Claude Code account.
    pub is_active: bool,
    pub active_since: Option<DateTime<Utc>>,
    /// The config home this app itself reads (`CLAUDE_CONFIG_DIR` or `~/.claude`).
    pub app_config_home: Option<&'a Path>,
    /// What the slot shows right now: the source of the cross-check and of
    /// the slower buckets carried into the merged snapshot.
    pub shown: Option<&'a CachedUsage>,
    pub now: DateTime<Utc>,
    pub max_age: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    NotThisAccount,
    UnknownVersion,
    Unconfirmed,
    NoBuckets,
    OutOfWindow,
    BeforeActiveSince,
    /// The observation's reset time disagrees with the slot's own snapshot.
    IdentityMismatch {
        window: &'static str,
        observed: i64,
        known: i64,
    },
}

/// The slot's last snapshot from a source that asked Anthropic about *this*
/// account: the reference for the reset-time cross-check. A statusline
/// snapshot is never its own reference, or one misattributed observation
/// would vouch for the next.
pub fn prior_of(shown: Option<&CachedUsage>) -> Option<&UsageSnapshot> {
    shown
        .filter(|c| c.origin != SnapshotOrigin::Statusline && has_usage_data(&c.snapshot))
        .map(|c| &c.snapshot)
}

/// The snapshot whose Sonnet/Opus/extra buckets a statusline snapshot
/// carries, and when those buckets were actually fetched.
pub fn carried_of(shown: Option<&CachedUsage>) -> Option<(&UsageSnapshot, DateTime<Utc>)> {
    let c = shown.filter(|c| has_usage_data(&c.snapshot))?;
    match c.origin {
        // Already a merge: its buckets keep the age they were carried with.
        SnapshotOrigin::Statusline => c.carried_fetched_at.map(|at| (&c.snapshot, at)),
        _ => Some((&c.snapshot, c.snapshot.fetched_at)),
    }
}

fn same_dir(a: &Path, b: &Path) -> bool {
    a == b
        || matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

fn in_app_config_home(obs: &Observation, ctx: &SlotContext) -> bool {
    ctx.app_config_home
        .is_some_and(|home| same_dir(Path::new(&obs.config_home), home))
}

/// Spec §5.2: does this observation describe this slot's account?
pub fn matches_account(obs: &Observation, ctx: &SlotContext) -> bool {
    match &obs.identity {
        Identity::Oauth {
            account_uuid,
            organization_uuid,
            ..
        } => {
            if ctx.acc.account_uuid != *account_uuid {
                return false;
            }
            // One person in a personal and a Team organization has separate
            // limits under one account uuid: never credit the wrong ones.
            match &ctx.acc.organization_uuid {
                Some(slot_org) => organization_uuid.as_deref() == Some(slot_org.as_str()),
                None => true,
            }
        }
        // No identity at all. The only account that has none is a disk-stored
        // long-lived token, and only the live one, in the config home the
        // app itself reads.
        Identity::None { reason: None } => {
            ctx.is_active
                && ctx.acc.source == AddSource::LongLivedToken
                && ctx.acc.oauth_account_blob.is_null()
                && in_app_config_home(obs, ctx)
        }
        // Routed or otherwise unattributable auth: never matches.
        Identity::None { reason: Some(_) } => false,
    }
}

/// Spec §5.4: is this observation a usable reading for this slot? Returns the
/// still-live buckets and when the data arrived.
pub fn candidate(
    obs: &Observation,
    ctx: &SlotContext,
) -> Result<(RateLimits, DateTime<Utc>), Rejection> {
    if !matches_account(obs, ctx) {
        return Err(Rejection::NotThisAccount);
    }
    if obs.v != OBS_VERSION {
        return Err(Rejection::UnknownVersion);
    }
    if !obs.confirmed {
        return Err(Rejection::Unconfirmed);
    }
    let limits = obs
        .rate_limits
        .as_ref()
        .filter(|l| !l.is_empty())
        .ok_or(Rejection::NoBuckets)?;
    let data_at =
        DateTime::from_timestamp_millis(obs.data_at_ms).ok_or(Rejection::OutOfWindow)?;
    if !is_within_age(data_at, ctx.max_age, ctx.now) {
        return Err(Rejection::OutOfWindow);
    }
    // A swap the bridge never saw (A -> B -> A) leaves old-account numbers in
    // a running session; data older than the swap, plus settling time, is
    // not trusted.
    if in_app_config_home(obs, ctx) {
        if let Some(since) = ctx.active_since {
            if data_at < since + ChronoDuration::seconds(ACTIVE_SINCE_GRACE_SECS) {
                return Err(Rejection::BeforeActiveSince);
            }
        }
    }
    // A window that has already reset must not be shown at its old percentage.
    let now_s = ctx.now.timestamp();
    let live = RateLimits {
        five_hour: limits.five_hour.clone().filter(|w| w.resets_at > now_s),
        seven_day: limits.seven_day.clone().filter(|w| w.resets_at > now_s),
    };
    if live.is_empty() {
        return Err(Rejection::NoBuckets);
    }
    // Two accounts' windows almost never end at the same second, so a reset
    // time that disagrees with the slot's own snapshot means a different
    // account (an env-token session, a proxy, a missed swap). Accounts with
    // no `/usage` snapshot, such as disk-stored token accounts, skip this.
    if let Some(prior) = prior_of(ctx.shown) {
        let pairs = [
            ("five_hour", live.five_hour.as_ref(), prior.five_hour.as_ref()),
            ("seven_day", live.seven_day.as_ref(), prior.seven_day.as_ref()),
        ];
        for (window, observed, known) in pairs {
            let (Some(observed), Some(known)) =
                (observed, known.and_then(|u| u.resets_at))
            else {
                continue;
            };
            // The slot's window has ended: a newer window is expected.
            if known <= ctx.now {
                continue;
            }
            if (observed.resets_at - known.timestamp()).abs() > RESET_TOLERANCE_SECS {
                return Err(Rejection::IdentityMismatch {
                    window,
                    observed: observed.resets_at,
                    known: known.timestamp(),
                });
            }
        }
    }
    Ok((live, data_at))
}

/// Spec §5.5: turn an observation into a snapshot. The 5h/7d buckets come
/// from the status line; Sonnet, Opus and extra usage are carried over from
/// the slot's last full snapshot (the headers don't have them). Returns the
/// time the carried buckets were fetched, if any were carried.
pub fn merge_into_snapshot(
    limits: &RateLimits,
    data_at: DateTime<Utc>,
    carried: Option<(&UsageSnapshot, DateTime<Utc>)>,
) -> (UsageSnapshot, Option<DateTime<Utc>>) {
    let util = |w: &Window| Utilization {
        utilization: w.used_percentage,
        resets_at: DateTime::from_timestamp(w.resets_at, 0),
    };
    let snapshot = UsageSnapshot {
        five_hour: limits.five_hour.as_ref().map(util),
        seven_day: limits.seven_day.as_ref().map(util),
        seven_day_sonnet: carried.and_then(|(c, _)| c.seven_day_sonnet.clone()),
        seven_day_opus: carried.and_then(|(c, _)| c.seven_day_opus.clone()),
        seven_day_fable: carried.and_then(|(c, _)| c.seven_day_fable.clone()),
        extra_usage: carried.and_then(|(c, _)| c.extra_usage.clone()),
        fetched_at: data_at,
        unknown: Default::default(),
    };
    (snapshot, carried.map(|(_, at)| at))
}

/// Whether a statusline adoption satisfies an HTTP-capable slot's poll, or
/// the slot must still make its one call per [`CARRIED_BUCKET_MAX_AGE`].
pub fn satisfies_poll(carried_fetched_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    let Ok(max) = ChronoDuration::from_std(CARRIED_BUCKET_MAX_AGE) else {
        return false;
    };
    carried_fetched_at.is_some_and(|at| now - at < max)
}

/// Whether a statusline snapshot is worth a history row: at most one a
/// minute per account, and only when a headline value actually changed.
pub fn should_persist(
    last_persisted: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    changed: bool,
) -> bool {
    changed && last_persisted.is_none_or(|last| now - last >= ChronoDuration::seconds(60))
}

/// Whether two snapshots differ in a headline value.
pub fn headline_changed(before: Option<&UsageSnapshot>, after: &UsageSnapshot) -> bool {
    let pct = |s: &UsageSnapshot| {
        (
            s.five_hour.as_ref().map(|u| u.utilization.to_bits()),
            s.seven_day.as_ref().map(|u| u.utilization.to_bits()),
            s.seven_day_fable
                .as_ref()
                .map(|u| u.utilization.to_bits()),
        )
    };
    before.is_none_or(|b| pct(b) != pct(after))
}

type ObsCache = HashMap<PathBuf, (SystemTime, u64, Option<Observation>)>;

fn cache() -> &'static Mutex<ObsCache> {
    static CACHE: OnceLock<Mutex<ObsCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Every parseable observation under `dir`, re-parsing a file only when its
/// `(mtime, len)` changed.
pub fn load_observations(dir: &Path) -> Vec<(PathBuf, Observation)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        // `*.tmp` files are the bridge's half-written temporaries.
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(mtime) = meta.modified() else { continue };
        let len = meta.len();
        seen.insert(path.clone());
        let fresh = matches!(cache.get(&path), Some((m, l, _)) if *m == mtime && *l == len);
        if !fresh {
            let parsed = std::fs::read_to_string(&path)
                .ok()
                .and_then(|text| serde_json::from_str::<Observation>(&text).ok());
            cache.insert(path.clone(), (mtime, len, parsed));
        }
        if let Some((_, _, Some(obs))) = cache.get(&path) {
            out.push((path, obs.clone()));
        }
    }
    cache.retain(|path, _| !path.starts_with(dir) || seen.contains(path));
    out
}

/// Log a rejected observation once per (file, data time), not once per tick.
fn log_mismatch(path: &Path, obs: &Observation, why: &Rejection) {
    static SEEN: OnceLock<Mutex<HashSet<(PathBuf, i64)>>> = OnceLock::new();
    let mut seen = SEEN
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if seen.len() > 1024 {
        seen.clear();
    }
    if seen.insert((path.to_path_buf(), obs.data_at_ms)) {
        tracing::warn!(
            target: "switchboard.poll",
            "statusline observation {} rejected: identity mismatch ({why:?})",
            path.display()
        );
    }
}

/// A rejection's name, for the log.
fn reason_name(why: &Rejection) -> &'static str {
    match why {
        Rejection::NotThisAccount => "NotThisAccount",
        Rejection::UnknownVersion => "UnknownVersion",
        Rejection::Unconfirmed => "Unconfirmed",
        Rejection::NoBuckets => "NoBuckets",
        Rejection::OutOfWindow => "OutOfWindow",
        Rejection::BeforeActiveSince => "BeforeActiveSince",
        Rejection::IdentityMismatch { .. } => "IdentityMismatch",
    }
}

/// What the app decided about an account's statusline readings, in one line:
/// which session's reading it uses, and how many files it could not use and
/// why (counted by reason, alphabetically, so the line is stable).
fn summarize_outcome(winner: Option<(u32, DateTime<Utc>)>, files: usize, rejected: &[&str]) -> String {
    let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for reason in rejected {
        *counts.entry(reason).or_default() += 1;
    }
    let reasons = counts
        .iter()
        .map(|(reason, n)| format!("{reason}×{n}"))
        .collect::<Vec<_>>()
        .join(", ");
    match winner {
        Some((pid, data_at)) => {
            let used = format!(
                "using the reading from Claude Code pid {pid} (data {} UTC)",
                data_at.format("%H:%M:%S")
            );
            if reasons.is_empty() {
                used
            } else {
                format!("{used}; not usable: {reasons}")
            }
        }
        None if files == 0 => "no observation files yet".to_string(),
        None => format!("no usable reading among {files} file(s): {reasons}"),
    }
}

/// A changed outcome is logged, but not more than once per account per this
/// long: sessions confirming one by one would otherwise fill the log.
const OUTCOME_LOG_MIN_GAP: Duration = Duration::from_secs(30);

fn should_log_outcome(last: Option<(&str, Instant)>, summary: &str, now: Instant) -> bool {
    match last {
        None => true,
        Some((previous, at)) => previous != summary && now.duration_since(at) >= OUTCOME_LOG_MIN_GAP,
    }
}

/// Log `summary` for `slot` when it is new (see [`should_log_outcome`]).
fn log_outcome(slot: u32, summary: String) {
    static LAST: OnceLock<Mutex<HashMap<u32, (String, Instant)>>> = OnceLock::new();
    let mut last = LAST
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    let previous = last.get(&slot).map(|(s, at)| (s.as_str(), *at));
    if should_log_outcome(previous, &summary, now) {
        tracing::info!(target: "switchboard.poll", "slot {slot}: statusline: {summary}");
        last.insert(slot, (summary, now));
    }
}

/// The freshest usable observation for one slot, merged into a snapshot.
pub fn freshest_for_slot(
    dir: &Path,
    ctx: &SlotContext,
) -> Option<(UsageSnapshot, Option<DateTime<Utc>>)> {
    let mut best: Option<(RateLimits, DateTime<Utc>, u32)> = None;
    let mut files = 0usize;
    let mut rejected: Vec<&'static str> = Vec::new();
    for (path, obs) in load_observations(dir) {
        files += 1;
        match candidate(&obs, ctx) {
            Ok((limits, data_at)) => {
                if best.as_ref().is_none_or(|(_, at, _)| data_at > *at) {
                    best = Some((limits, data_at, obs.claude_pid));
                }
            }
            Err(why) => {
                rejected.push(reason_name(&why));
                if matches!(why, Rejection::IdentityMismatch { .. }) {
                    log_mismatch(&path, &obs, &why);
                }
            }
        }
    }
    log_outcome(
        ctx.acc.slot,
        summarize_outcome(best.as_ref().map(|(_, at, pid)| (*pid, *at)), files, &rejected),
    );
    let (limits, data_at, _) = best?;
    Some(merge_into_snapshot(&limits, data_at, carried_of(ctx.shown)))
}

/// The statusline snapshot for `acc`, as the poll loop's waterfall wants it.
pub fn snapshot_for(
    state: &AppState,
    acc: &ManagedAccount,
    active_slot: Option<u32>,
    max_age: Duration,
) -> Option<UsageSnapshot> {
    let shown = state.cached_usage_by_slot.read().get(&acc.slot).cloned();
    let app_home = crate::auth::paths::claude_config_home();
    let ctx = SlotContext {
        acc,
        is_active: Some(acc.slot) == active_slot,
        active_since: *state.active_since.read(),
        app_config_home: app_home.as_deref(),
        shown: shown.as_ref(),
        now: Utc::now(),
        max_age,
    };
    freshest_for_slot(&obs_dir(&bridge_root()), &ctx).map(|(snap, _)| snap)
}

/// How often the watcher checks that the directory it watches is still the
/// one it started on.
const WATCH_RECHECK: Duration = Duration::from_secs(10);

/// Who a directory is, so a deleted-and-recreated one reads as a different
/// directory even though the path is the same. `None` when it doesn't exist.
fn dir_identity(dir: &Path) -> Option<(u64, Option<SystemTime>)> {
    let meta = std::fs::metadata(dir).ok().filter(|m| m.is_dir())?;
    #[cfg(unix)]
    let id = {
        use std::os::unix::fs::MetadataExt;
        meta.ino()
    };
    #[cfg(not(unix))]
    let id = 0;
    Some((id, meta.created().ok()))
}

/// Keep the observation directory watched for as long as the app runs, and
/// wake the poll loop whenever something in it changes. The directory only
/// exists once the bridge has been enabled, so this waits for it to appear,
/// and starts over if it is deleted and created again (an OS watch follows
/// the old directory, not the path).
pub fn start_watcher(state: Arc<AppState>) {
    use notify::RecursiveMode;
    use notify_debouncer_full::new_debouncer;
    tauri::async_runtime::spawn(async move {
        let dir = obs_dir(&bridge_root());
        loop {
            if let Some(watched) = dir_identity(&dir) {
                let notify_state = state.clone();
                let debouncer = new_debouncer(
                    Duration::from_secs(1),
                    None,
                    move |res: notify_debouncer_full::DebounceEventResult| {
                        if res.is_ok() {
                            notify_state.local_source_changed.notify_one();
                        }
                    },
                );
                match debouncer {
                    Ok(mut d) => {
                        if d.watch(&dir, RecursiveMode::NonRecursive).is_ok() {
                            // Hold the watch until the directory is replaced
                            // or removed, then wake the poll loop once: what
                            // was written meanwhile was missed.
                            while dir_identity(&dir) == Some(watched) {
                                tokio::time::sleep(WATCH_RECHECK).await;
                            }
                            state.local_source_changed.notify_one();
                            continue;
                        }
                    }
                    Err(e) => tracing::warn!("statusline watcher failed to start: {e}"),
                }
            }
            tokio::time::sleep(WATCH_RECHECK).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_recreated_directory_is_a_different_directory() {
        let root = tempdir().unwrap();
        let dir = root.path().join("obs");
        assert_eq!(dir_identity(&dir), None, "missing");
        std::fs::create_dir(&dir).unwrap();
        let first = dir_identity(&dir).expect("exists");
        assert_eq!(dir_identity(&dir), Some(first), "stable while it stays");
        std::fs::remove_dir(&dir).unwrap();
        assert_eq!(dir_identity(&dir), None, "removed");
        // Created again, possibly at the same inode: the creation time still
        // tells them apart on filesystems that report one.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::create_dir(&dir).unwrap();
        let second = dir_identity(&dir).expect("exists again");
        if first.1.is_some() {
            assert_ne!(first, second);
        }
    }

    #[test]
    fn a_file_where_the_directory_should_be_is_not_a_directory() {
        let root = tempdir().unwrap();
        let path = root.path().join("obs");
        std::fs::write(&path, b"x").unwrap();
        assert_eq!(dir_identity(&path), None);
    }

    use crate::auth::AuthSource;
    use serde_json::json;
    use tempfile::tempdir;

    const HOME: &str = "/home/u/.claude";

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap()
    }

    fn acc(source: AddSource, uuid: &str, org: Option<&str>, oauth_blob: serde_json::Value) -> ManagedAccount {
        ManagedAccount {
            slot: 4,
            email: String::new(),
            label: None,
            account_uuid: uuid.into(),
            organization_uuid: org.map(str::to_string),
            organization_name: None,
            subscription_type: None,
            source,
            claude_code_oauth_blob: json!({}),
            oauth_account_blob: oauth_blob,
            token_expires_at: None,
            added_at: now(),
            last_seen_active: None,
        }
    }

    fn token_acc() -> ManagedAccount {
        acc(AddSource::LongLivedToken, "token:abc", None, serde_json::Value::Null)
    }

    fn oauth_acc(org: Option<&str>) -> ManagedAccount {
        acc(AddSource::OAuth, "u-1", org, json!({"accountUuid": "u-1"}))
    }

    fn window(pct: f64, resets_in_s: i64) -> Window {
        Window {
            used_percentage: pct,
            resets_at: now().timestamp() + resets_in_s,
        }
    }

    fn obs(identity: Identity) -> Observation {
        Observation {
            v: OBS_VERSION,
            session_id: "s".into(),
            claude_pid: 1,
            config_home: HOME.into(),
            identity,
            observed_at_ms: now().timestamp_millis(),
            api_ms: 0,
            requests: 0,
            data_at_ms: now().timestamp_millis() - 5_000,
            no_data_reason: None,
            rate_limits: Some(RateLimits {
                five_hour: Some(window(33.0, 3600)),
                seven_day: Some(window(16.0, 86_400)),
            }),
            confirmed: true,
            identity_since_ms: 0,
            baseline_api_ms: None,
            baseline_requests: None,
            growths_since_baseline: 0,
            pre_switch_rate_limits: None,
            cc_version: None,
        }
    }

    fn none_id() -> Identity {
        Identity::None { reason: None }
    }

    fn oauth_id(org: Option<&str>) -> Identity {
        Identity::Oauth {
            account_uuid: "u-1".into(),
            email: "a@b.c".into(),
            organization_uuid: org.map(str::to_string),
        }
    }

    fn ctx<'a>(acc: &'a ManagedAccount, shown: Option<&'a CachedUsage>) -> SlotContext<'a> {
        SlotContext {
            acc,
            is_active: true,
            active_since: None,
            app_config_home: Some(Path::new(HOME)),
            shown,
            now: now(),
            max_age: Duration::from_secs(300),
        }
    }

    fn cached(snapshot: UsageSnapshot, origin: SnapshotOrigin, carried: Option<DateTime<Utc>>) -> CachedUsage {
        CachedUsage {
            snapshot,
            account_id: "u-1".into(),
            account_email: String::new(),
            last_error: None,
            burn_rate: None,
            auth_source: AuthSource::OAuth,
            origin,
            carried_fetched_at: carried,
        }
    }

    fn http_snapshot(five_resets_in: i64, fetched_ago_s: i64) -> UsageSnapshot {
        UsageSnapshot {
            five_hour: Some(Utilization {
                utilization: 10.0,
                resets_at: DateTime::from_timestamp(now().timestamp() + five_resets_in, 0),
            }),
            seven_day: Some(Utilization {
                utilization: 5.0,
                resets_at: DateTime::from_timestamp(now().timestamp() + 86_400, 0),
            }),
            seven_day_sonnet: Some(Utilization { utilization: 7.0, resets_at: None }),
            seven_day_opus: Some(Utilization { utilization: 8.0, resets_at: None }),
            seven_day_fable: Some(Utilization { utilization: 9.0, resets_at: None }),
            extra_usage: None,
            fetched_at: now() - ChronoDuration::seconds(fetched_ago_s),
            unknown: Default::default(),
        }
    }

    // ---- matching (§5.2) ----

    #[test]
    fn oauth_matches_by_account_uuid() {
        let a = oauth_acc(None);
        assert!(matches_account(&obs(oauth_id(None)), &ctx(&a, None)));
        let mut other = obs(oauth_id(None));
        other.identity = Identity::Oauth {
            account_uuid: "u-2".into(),
            email: "x@y.z".into(),
            organization_uuid: None,
        };
        assert!(!matches_account(&other, &ctx(&a, None)));
    }

    #[test]
    fn oauth_with_slot_org_requires_equal_org() {
        let a = oauth_acc(Some("org-1"));
        assert!(matches_account(&obs(oauth_id(Some("org-1"))), &ctx(&a, None)));
        assert!(!matches_account(&obs(oauth_id(Some("org-2"))), &ctx(&a, None)));
        assert!(!matches_account(&obs(oauth_id(None)), &ctx(&a, None)));
    }

    #[test]
    fn none_matches_only_the_active_long_lived_token_in_the_app_config_home() {
        let t = token_acc();
        assert!(matches_account(&obs(none_id()), &ctx(&t, None)));

        let mut inactive = ctx(&t, None);
        inactive.is_active = false;
        assert!(!matches_account(&obs(none_id()), &inactive));

        let mut elsewhere = obs(none_id());
        elsewhere.config_home = "/somewhere/else/.claude".into();
        assert!(!matches_account(&elsewhere, &ctx(&t, None)));

        let oauth = oauth_acc(None);
        assert!(!matches_account(&obs(none_id()), &ctx(&oauth, None)));
    }

    #[test]
    fn none_with_reason_never_matches() {
        let t = token_acc();
        let routed = obs(Identity::None { reason: Some("routed".into()) });
        assert!(!matches_account(&routed, &ctx(&t, None)));
    }

    // ---- freshness rules (§5.4) ----

    #[test]
    fn a_confirmed_fresh_observation_is_a_candidate() {
        let t = token_acc();
        let (limits, at) = candidate(&obs(none_id()), &ctx(&t, None)).unwrap();
        assert_eq!(limits.five_hour.unwrap().used_percentage, 33.0);
        assert_eq!(at.timestamp_millis(), now().timestamp_millis() - 5_000);
    }

    #[test]
    fn unconfirmed_observation_is_rejected() {
        let t = token_acc();
        let mut o = obs(none_id());
        o.confirmed = false;
        assert_eq!(candidate(&o, &ctx(&t, None)), Err(Rejection::Unconfirmed));
    }

    #[test]
    fn unknown_version_is_rejected() {
        let t = token_acc();
        let mut o = obs(none_id());
        o.v = 2;
        assert_eq!(candidate(&o, &ctx(&t, None)), Err(Rejection::UnknownVersion));
    }

    #[test]
    fn data_older_than_max_age_is_rejected() {
        let t = token_acc();
        let mut o = obs(none_id());
        o.data_at_ms = now().timestamp_millis() - 301_000;
        assert_eq!(candidate(&o, &ctx(&t, None)), Err(Rejection::OutOfWindow));
    }

    #[test]
    fn data_more_than_30s_in_the_future_is_rejected() {
        let t = token_acc();
        let mut o = obs(none_id());
        o.data_at_ms = now().timestamp_millis() + 31_000;
        assert_eq!(candidate(&o, &ctx(&t, None)), Err(Rejection::OutOfWindow));
    }

    #[test]
    fn data_before_active_since_plus_30s_is_rejected_for_the_app_config_home_only() {
        let t = token_acc();
        let mut c = ctx(&t, None);
        // Swapped 20 s before the observation's data arrived: not settled.
        c.active_since = Some(now() - ChronoDuration::seconds(25));
        assert_eq!(candidate(&obs(none_id()), &c), Err(Rejection::BeforeActiveSince));

        // Swapped 60 s before: settled.
        c.active_since = Some(now() - ChronoDuration::seconds(65));
        assert!(candidate(&obs(none_id()), &c).is_ok());

        // The same stale data from another config home is judged by identity
        // alone: the swap that moved the app's home says nothing about it.
        let a = oauth_acc(None);
        let mut other_home = obs(oauth_id(None));
        other_home.config_home = "/other/.claude".into();
        let mut c2 = ctx(&a, None);
        c2.active_since = Some(now() - ChronoDuration::seconds(25));
        assert!(candidate(&other_home, &c2).is_ok());
    }

    #[test]
    fn expired_bucket_is_dropped_not_shown() {
        let t = token_acc();
        let mut o = obs(none_id());
        o.rate_limits = Some(RateLimits {
            five_hour: Some(window(99.0, -10)),
            seven_day: Some(window(16.0, 86_400)),
        });
        let (limits, _) = candidate(&o, &ctx(&t, None)).unwrap();
        assert!(limits.five_hour.is_none());
        assert_eq!(limits.seven_day.unwrap().used_percentage, 16.0);
    }

    #[test]
    fn observation_with_no_buckets_is_rejected() {
        let t = token_acc();
        let mut o = obs(none_id());
        o.rate_limits = None;
        assert_eq!(candidate(&o, &ctx(&t, None)), Err(Rejection::NoBuckets));
        o.rate_limits = Some(RateLimits::default());
        assert_eq!(candidate(&o, &ctx(&t, None)), Err(Rejection::NoBuckets));
        o.rate_limits = Some(RateLimits { five_hour: Some(window(1.0, -1)), seven_day: None });
        assert_eq!(candidate(&o, &ctx(&t, None)), Err(Rejection::NoBuckets));
    }

    // ---- reset-time cross-check (§5.4 rule 5) ----

    #[test]
    fn reset_mismatch_beyond_60s_is_rejected() {
        let a = oauth_acc(None);
        // The slot's own 5h window ends in 7200 s; the observation says 3600 s.
        let shown = cached(http_snapshot(7200, 30), SnapshotOrigin::Http, None);
        let err = candidate(&obs(oauth_id(None)), &ctx(&a, Some(&shown))).unwrap_err();
        assert!(matches!(err, Rejection::IdentityMismatch { window: "five_hour", .. }));
    }

    #[test]
    fn reset_within_60s_is_accepted() {
        let a = oauth_acc(None);
        let shown = cached(http_snapshot(3600 + 45, 30), SnapshotOrigin::Http, None);
        assert!(candidate(&obs(oauth_id(None)), &ctx(&a, Some(&shown))).is_ok());
    }

    #[test]
    fn a_newer_window_than_the_prior_snapshot_is_accepted() {
        let a = oauth_acc(None);
        // The slot's snapshot describes a window that ended 100 s ago.
        let mut old = http_snapshot(-100, 600);
        old.seven_day = None;
        let shown = cached(old, SnapshotOrigin::Http, None);
        assert!(candidate(&obs(oauth_id(None)), &ctx(&a, Some(&shown))).is_ok());
    }

    #[test]
    fn no_prior_snapshot_skips_the_cross_check() {
        // The token-account path: there is no /usage snapshot to compare to.
        let t = token_acc();
        assert!(candidate(&obs(none_id()), &ctx(&t, None)).is_ok());
    }

    #[test]
    fn a_statusline_snapshot_is_never_its_own_cross_check_reference() {
        let a = oauth_acc(None);
        let shown = cached(http_snapshot(7200, 30), SnapshotOrigin::Statusline, Some(now()));
        assert!(candidate(&obs(oauth_id(None)), &ctx(&a, Some(&shown))).is_ok());
    }

    // ---- merge (§5.5) ----

    #[test]
    fn merge_sets_utilization_from_used_percentage_and_converts_resets_at() {
        let limits = RateLimits {
            five_hour: Some(Window { used_percentage: 33.0, resets_at: 1_800_003_600 }),
            seven_day: Some(Window { used_percentage: 16.0, resets_at: 1_800_086_400 }),
        };
        let (snap, carried) = merge_into_snapshot(&limits, now(), None);
        let five = snap.five_hour.unwrap();
        assert_eq!(five.utilization, 33.0);
        assert_eq!(five.resets_at.unwrap().timestamp(), 1_800_003_600);
        assert_eq!(snap.seven_day.unwrap().utilization, 16.0);
        assert_eq!(snap.fetched_at, now());
        assert!(
            snap.seven_day_sonnet.is_none()
                && snap.seven_day_fable.is_none()
                && snap.extra_usage.is_none()
        );
        assert!(carried.is_none());
    }

    #[test]
    fn merge_carries_sonnet_opus_and_extra_and_records_carried_fetched_at() {
        let full = http_snapshot(3600, 120);
        let fetched = full.fetched_at;
        let limits = RateLimits { five_hour: Some(window(33.0, 3600)), seven_day: None };
        let (snap, carried) = merge_into_snapshot(&limits, now(), Some((&full, fetched)));
        assert_eq!(snap.seven_day_sonnet.unwrap().utilization, 7.0);
        assert_eq!(snap.seven_day_opus.unwrap().utilization, 8.0);
        assert_eq!(snap.seven_day_fable.unwrap().utilization, 9.0);
        assert_eq!(carried, Some(fetched));
    }

    #[test]
    fn carried_of_an_earlier_statusline_merge_keeps_the_original_fetch_time() {
        let full = http_snapshot(3600, 600);
        let original = full.fetched_at;
        let shown = cached(full, SnapshotOrigin::Statusline, Some(original));
        let (_, at) = carried_of(Some(&shown)).unwrap();
        assert_eq!(at, original, "a statusline snapshot must not look freshly fetched");
    }

    // ---- HTTP floor and history throttle ----

    #[test]
    fn carried_buckets_older_than_15_minutes_force_an_http_fetch() {
        let n = now();
        assert!(satisfies_poll(Some(n - ChronoDuration::minutes(14)), n));
        assert!(!satisfies_poll(Some(n - ChronoDuration::minutes(15)), n));
        // Nothing carried means the slow buckets were never fetched.
        assert!(!satisfies_poll(None, n));
    }

    #[test]
    fn statusline_history_rows_are_throttled_to_one_per_60s_and_only_on_change() {
        let n = now();
        assert!(should_persist(None, n, true));
        assert!(!should_persist(None, n, false));
        assert!(!should_persist(Some(n - ChronoDuration::seconds(59)), n, true));
        assert!(should_persist(Some(n - ChronoDuration::seconds(60)), n, true));
        assert!(!should_persist(Some(n - ChronoDuration::seconds(600)), n, false));
    }

    #[test]
    fn headline_changed_compares_five_hour_seven_day_and_fable() {
        let a = http_snapshot(3600, 0);
        let mut b = a.clone();
        assert!(!headline_changed(Some(&a), &b));
        b.five_hour.as_mut().unwrap().utilization = 11.0;
        assert!(headline_changed(Some(&a), &b));

        let mut c = a.clone();
        c.seven_day_fable.as_mut().unwrap().utilization = 10.0;
        assert!(headline_changed(Some(&a), &c));
        assert!(headline_changed(None, &b));
    }

    // ---- reading the directory ----

    fn write_obs(dir: &Path, name: &str, o: &Observation) {
        std::fs::write(dir.join(name), serde_json::to_string(o).unwrap()).unwrap();
    }

    #[test]
    fn freshest_confirmed_file_wins_across_processes() {
        let dir = tempdir().unwrap();
        let t = token_acc();

        let mut older = obs(none_id());
        older.data_at_ms = now().timestamp_millis() - 60_000;
        older.rate_limits.as_mut().unwrap().five_hour = Some(window(10.0, 3600));
        write_obs(dir.path(), "a.1.json", &older);

        let mut newest = obs(none_id());
        newest.data_at_ms = now().timestamp_millis() - 2_000;
        newest.rate_limits.as_mut().unwrap().five_hour = Some(window(40.0, 3600));
        write_obs(dir.path(), "b.2.json", &newest);

        // A fresher but unconfirmed file must not win.
        let mut unconfirmed = obs(none_id());
        unconfirmed.data_at_ms = now().timestamp_millis() - 1_000;
        unconfirmed.confirmed = false;
        unconfirmed.rate_limits.as_mut().unwrap().five_hour = Some(window(99.0, 3600));
        write_obs(dir.path(), "c.3.json", &unconfirmed);

        let (snap, _) = freshest_for_slot(dir.path(), &ctx(&t, None)).unwrap();
        assert_eq!(snap.five_hour.unwrap().utilization, 40.0);
    }

    #[test]
    fn files_with_unknown_version_and_temp_files_are_ignored() {
        let dir = tempdir().unwrap();
        let t = token_acc();
        let mut future = obs(none_id());
        future.v = 9;
        write_obs(dir.path(), "a.1.json", &future);
        std::fs::write(dir.path().join("b.2.json.123.tmp"), "{ half written").unwrap();
        std::fs::write(dir.path().join("c.3.json"), "not json").unwrap();
        assert!(freshest_for_slot(dir.path(), &ctx(&t, None)).is_none());
    }

    #[test]
    fn a_missing_directory_yields_nothing() {
        let t = token_acc();
        assert!(freshest_for_slot(Path::new("/definitely/not/here"), &ctx(&t, None)).is_none());
    }

    #[test]
    fn a_rewritten_file_is_re_parsed() {
        let dir = tempdir().unwrap();
        let t = token_acc();
        let mut o = obs(none_id());
        write_obs(dir.path(), "a.1.json", &o);
        let first = freshest_for_slot(dir.path(), &ctx(&t, None)).unwrap().0;
        assert_eq!(first.five_hour.unwrap().utilization, 33.0);
        let first_len = std::fs::metadata(dir.path().join("a.1.json")).unwrap().len();

        // Longer content, so (mtime, len) changes even on a coarse clock.
        o.rate_limits.as_mut().unwrap().five_hour = Some(window(45.55, 3600));
        write_obs(dir.path(), "a.1.json", &o);
        assert_ne!(
            std::fs::metadata(dir.path().join("a.1.json")).unwrap().len(),
            first_len,
            "the rewrite must invalidate the cache even when mtime is unchanged"
        );
        let second = freshest_for_slot(dir.path(), &ctx(&t, None)).unwrap().0;
        assert_eq!(second.five_hour.unwrap().utilization, 45.55);
    }
}

// Review focus: the bridge writes the config home the way Claude Code's
// environment spelled it, and the app builds its own from `USERPROFILE`.
// Slash style, drive-letter case and a trailing separator must not stop an
// observation from matching the app's own config home.
#[cfg(all(test, windows))]
mod windows_path_tests {
    use super::*;

    #[test]
    fn same_dir_ignores_slash_style_drive_case_and_a_trailing_separator() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join(".claude");
        std::fs::create_dir_all(&home).unwrap();
        let canonical = home.canonicalize().unwrap();
        // Strip the verbatim prefix (backslash backslash ? backslash) from the
        // canonical form, then respell it.
        let plain = canonical.to_string_lossy().trim_start_matches(r"\\?\").to_string();
        let swapped_case = {
            let mut chars = plain.chars();
            let drive = chars.next().unwrap();
            let flipped = if drive.is_ascii_uppercase() { drive.to_ascii_lowercase() } else { drive.to_ascii_uppercase() };
            format!("{flipped}{}", chars.as_str())
        };
        let spellings = [
            plain.clone(),
            plain.replace('\\', "/"),
            swapped_case,
            format!("{plain}\\"),
        ];
        for spelling in spellings {
            assert!(same_dir(Path::new(&spelling), &home), "{spelling} should name {}", home.display());
        }
        assert!(!same_dir(Path::new(&format!("{plain}-other")), &home));
    }
}

// The line the app logs for an account's statusline readings, so a reading that
// is not being used can be explained from the log instead of guessed at.
#[cfg(test)]
mod outcome_log_tests {
    use super::*;
    use chrono::TimeZone;

    fn at(h: u32, m: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 12, h, m, s).unwrap()
    }

    #[test]
    fn every_rejection_has_a_name() {
        assert_eq!(reason_name(&Rejection::NotThisAccount), "NotThisAccount");
        assert_eq!(reason_name(&Rejection::UnknownVersion), "UnknownVersion");
        assert_eq!(reason_name(&Rejection::Unconfirmed), "Unconfirmed");
        assert_eq!(reason_name(&Rejection::NoBuckets), "NoBuckets");
        assert_eq!(reason_name(&Rejection::OutOfWindow), "OutOfWindow");
        assert_eq!(reason_name(&Rejection::BeforeActiveSince), "BeforeActiveSince");
        let mismatch = Rejection::IdentityMismatch { window: "five_hour", observed: 1, known: 2 };
        assert_eq!(reason_name(&mismatch), "IdentityMismatch");
    }

    #[test]
    fn a_used_reading_is_named_with_its_session_and_data_time() {
        let said = summarize_outcome(Some((9404, at(2, 52, 15))), 1, &[]);
        assert_eq!(said, "using the reading from Claude Code pid 9404 (data 02:52:15 UTC)");
    }

    #[test]
    fn what_was_not_used_is_counted_by_reason_in_a_stable_order() {
        let said = summarize_outcome(
            Some((9404, at(2, 52, 15))),
            4,
            &["Unconfirmed", "NotThisAccount", "Unconfirmed", "OutOfWindow"],
        );
        assert_eq!(
            said,
            "using the reading from Claude Code pid 9404 (data 02:52:15 UTC); \
             not usable: NotThisAccount×1, OutOfWindow×1, Unconfirmed×2"
        );
    }

    #[test]
    fn with_nothing_usable_the_reasons_say_why() {
        let said = summarize_outcome(None, 7, &["Unconfirmed"; 6].iter().chain(&["OutOfWindow"]).copied().collect::<Vec<_>>());
        assert_eq!(said, "no usable reading among 7 file(s): OutOfWindow×1, Unconfirmed×6");
    }

    #[test]
    fn with_no_files_at_all_it_says_so() {
        assert_eq!(summarize_outcome(None, 0, &[]), "no observation files yet");
    }

    #[test]
    fn the_log_line_appears_on_change_and_at_most_every_30_seconds() {
        let t0 = Instant::now();
        let later = |s: u64| t0 + Duration::from_secs(s);
        // The first outcome for an account is always worth a line.
        assert!(should_log_outcome(None, "a", t0));
        // Unchanged: never again, however long it has been.
        assert!(!should_log_outcome(Some(("a", t0)), "a", later(600)));
        // Changed, but a moment after the last line: wait.
        assert!(!should_log_outcome(Some(("a", t0)), "b", later(10)));
        // Changed and settled: log it.
        assert!(should_log_outcome(Some(("a", t0)), "b", later(31)));
    }
}
