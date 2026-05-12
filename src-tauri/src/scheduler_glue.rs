//! Glue between Tauri-side state (AppState handles, AccountManager, HTTP
//! client) and the pure scheduler/warmup modules. Centralises the
//! "load schedule + load token + load snapshot + call tick_for_account"
//! plumbing so both the manual trigger (commands.rs) and the periodic
//! tokio dispatcher (lib.rs) share one code path.
//!
//! # Identifier mapping
//!
//! Two account stores coexist:
//! - SQLite `accounts` table: primary key is `account_uuid` (TEXT), which
//!   carries `warmup_enabled` and `schedule`.
//! - `accounts.json` (via `AccountManager`): indexed by `slot` (u32), where
//!   `ManagedAccount.account_uuid` is the cross-reference key.
//!
//! `walk_due_accounts` joins them: it reads `account_uuid`s from SQLite,
//! looks up the matching slot from the JSON store, then delegates to the
//! scheduler/warmup modules via the slot-based token loader and the
//! slot-keyed snapshot cache.
//!
//! # `MutexGuard<Connection>` and `Send` bounds
//!
//! `scheduler::tick_for_account` takes `&Connection` and is `async`, which
//! means it would hold the `MutexGuard<Connection>` across an await point.
//! `rusqlite::Connection` is not `Sync`, so `&Connection` is not `Send`, and
//! `tauri::async_runtime::spawn` requires `Send` futures.
//!
//! The glue resolves this by **separating sync and async phases**:
//! - Phase A (sync, no await): schedule check + transactional claim via
//!   `scheduler::is_due` + `scheduler::claim::try_claim`. Drop the guard.
//! - Phase B (async): OAuth token load + HTTP warm-up call via
//!   `warmup::warmup_account`. No conn held.

use anyhow::{Context, Result};
use chrono::{DateTime, Local, Utc};
use std::sync::Arc;

use crate::app_state::AppState;
use crate::scheduler::{self, claim, presets::Schedule};
use crate::warmup::{self, errors::WarmupOutcome};

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Look up the schedule JSON for an account from the SQLite `accounts` table.
fn load_schedule(state: &AppState, account_uuid: &str) -> Result<Schedule> {
    let conn = state.db.conn();
    let json: String = conn
        .query_row(
            "SELECT schedule FROM accounts WHERE id = ?1",
            [account_uuid],
            |r| r.get(0),
        )
        .with_context(|| format!("read schedule for {account_uuid}"))?;
    serde_json::from_str(&json).with_context(|| format!("parse schedule for {account_uuid}"))
}

/// Resolve `account_uuid` to a slot number by scanning `accounts.json`.
/// Returns `None` when the account is not found in the JSON store (e.g. it
/// was in the DB but removed from accounts.json — shouldn't happen in normal
/// operation but handled gracefully).
fn slot_for_account_uuid(state: &AppState, account_uuid: &str) -> Option<u32> {
    let accounts = match state.accounts.list() {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!("scheduler_glue: could not list accounts: {e:#}");
            return None;
        }
    };
    accounts
        .into_iter()
        .find(|a| a.account_uuid == account_uuid)
        .map(|a| a.slot)
}

/// Look up the most recent `five_hour.resets_at` from the per-slot snapshot
/// cache. Returns `None` when the slot has no cached snapshot, when
/// `five_hour` is absent, or when `resets_at` is null — all of which mean
/// "window inactive" per spec §6.
fn load_five_hour_resets_at(state: &AppState, slot: u32) -> Option<DateTime<Utc>> {
    let cache = state.cached_usage_by_slot.read();
    let cached = cache.get(&slot)?;
    cached.snapshot.five_hour.as_ref()?.resets_at
}

/// Sync phase: check schedule eligibility and atomically claim the warm-up
/// slot. Returns `Ok(true)` if this caller should proceed with the HTTP call;
/// `Ok(false)` if not due or already claimed; `Err` on DB failure.
///
/// The `MutexGuard<Connection>` is acquired and dropped entirely within this
/// function — no conn escapes into async context.
fn sync_check_and_claim(
    state: &AppState,
    account_uuid: &str,
    schedule: &Schedule,
    is_due_override: bool,
) -> Result<bool> {
    if !is_due_override && !scheduler::is_due(schedule, Local::now()) {
        return Ok(false);
    }
    let now_secs = Utc::now().timestamp();
    let conn = state.db.conn();
    claim::try_claim(&conn, account_uuid, now_secs)
        .with_context(|| format!("try_claim for {account_uuid}"))
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Manual warm-up entry point (called by Tauri command `warmup_account_now`
/// wired in Plan B T16).
///
/// Bypasses the schedule eligibility check (`is_due_override = true`) so the
/// warm-up fires immediately regardless of the configured schedule.
pub async fn manual_warmup(
    state: &Arc<AppState>,
    account_uuid: &str,
) -> Result<WarmupOutcome> {
    tracing::info!(target: "switchboard.warmup", "manual warm-up requested for {account_uuid}");
    let schedule = load_schedule(state, account_uuid)?;

    let slot = slot_for_account_uuid(state, account_uuid)
        .with_context(|| format!("slot not found for account {account_uuid}"))?;

    let resets_at = load_five_hour_resets_at(state, slot);
    tracing::debug!(
        target: "switchboard.warmup",
        "manual warm-up slot={slot} resets_at={resets_at:?} schedule={schedule:?}"
    );

    // Phase A: sync check + claim (guard dropped before any await).
    let claimed = sync_check_and_claim(state, account_uuid, &schedule, true)?;
    if !claimed {
        tracing::info!(
            target: "switchboard.warmup",
            "manual warm-up for slot {slot} skipped (not eligible or already claimed within 60s)"
        );
        return Ok(WarmupOutcome::SkippedAlreadyActive);
    }

    // Phase B: token load + HTTP warm-up (no conn held).
    let active_slot = *state.active_slot.read();
    let token = state
        .auth
        .token_for_slot(slot, active_slot, &state.accounts)
        .await
        .with_context(|| format!("load OAuth token for slot {slot}"))?;

    let outcome = warmup::warmup_account(resets_at, move || Ok(token), &state.http_client)
        .await?;
    tracing::info!(
        target: "switchboard.warmup",
        "manual warm-up slot={slot} outcome={outcome:?}"
    );
    Ok(outcome)
}

/// Periodic dispatcher: walk all accounts with `warmup_enabled = 1`, evaluate
/// their schedules, and tick the due ones.
///
/// Called every 30 seconds by the in-app tokio task in `lib.rs`, and (once
/// wired) once per `--tick` CLI invocation in `cli.rs`.
pub async fn walk_due_accounts(state: &Arc<AppState>) -> Result<()> {
    // Read all warmup-eligible account UUIDs from SQLite.
    // Guard is acquired and dropped synchronously — no await while held.
    let account_uuids: Vec<String> = {
        let conn = state.db.conn();
        let mut stmt = conn
            .prepare("SELECT id FROM accounts WHERE warmup_enabled = 1")
            .context("prepare warmup-eligible accounts query")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<_>>()
            .context("collect warmup-eligible account ids")?
    };

    tracing::debug!(
        target: "switchboard.warmup",
        "dispatcher tick: {} warm-up-enabled account(s)",
        account_uuids.len()
    );

    for account_uuid in account_uuids {
        // --- Load schedule (sync, conn dropped inside helper) --------------
        let schedule = match load_schedule(state, &account_uuid) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("scheduler: skip {account_uuid}: {e:#}");
                continue;
            }
        };

        // --- Resolve slot --------------------------------------------------
        let slot = match slot_for_account_uuid(state, &account_uuid) {
            Some(s) => s,
            None => {
                tracing::warn!(
                    "scheduler: skip {account_uuid}: no matching slot in accounts.json"
                );
                continue;
            }
        };

        // --- five_hour.resets_at from cache --------------------------------
        let resets_at = load_five_hour_resets_at(state, slot);

        // --- Phase A: sync schedule check + claim (no async) ---------------
        let claimed = match sync_check_and_claim(state, &account_uuid, &schedule, false) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("scheduler: {account_uuid} claim error: {e:#}");
                continue;
            }
        };
        if !claimed {
            // Not due or already claimed by another tick — quiet.
            continue;
        }

        // --- Phase B: token + HTTP (async, no conn held) -------------------
        let active_slot = *state.active_slot.read();
        let token = match state
            .auth
            .token_for_slot(slot, active_slot, &state.accounts)
            .await
        {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("scheduler: {account_uuid} token error: {e:#}");
                continue;
            }
        };

        match warmup::warmup_account(resets_at, move || Ok(token), &state.http_client).await {
            Ok(outcome) => {
                tracing::info!(
                    target: "switchboard.warmup",
                    "scheduled warm-up slot={slot} account={account_uuid} → {outcome:?}"
                );
            }
            Err(e) => {
                tracing::warn!(
                    target: "switchboard.warmup",
                    "scheduled warm-up slot={slot} account={account_uuid} failed: {e:#}"
                );
            }
        }
    }
    Ok(())
}
