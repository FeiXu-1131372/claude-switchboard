//! Shared warm-up runner. No Tauri, window, tray, or in-memory app state is
//! needed: the GUI and `--tick` use the same persisted schedules and leases.
#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;
use super::WarmupOutcome;
use crate::auth::{
    accounts::{AccountManager, ManagedAccount},
    AuthOrchestrator,
};
use crate::scheduler::{catchup, claim, Schedule};
use crate::store::Db;
use crate::usage_api::UsageSnapshot;
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Local, Utc};

// async-trait adds must_use to methods returning futures that newer Rust
// toolchains already consider must-use.
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait Backend: Send + Sync {
    fn accounts(&self) -> Result<Vec<ManagedAccount>>;
    async fn issue(&self, account: &ManagedAccount) -> Result<WarmupOutcome>;
}

pub struct AccountBackend<'a> {
    pub db: &'a Db,
    pub accounts: &'a AccountManager,
    pub auth: &'a AuthOrchestrator,
    pub http: &'a reqwest::Client,
}

#[async_trait]
impl Backend for AccountBackend<'_> {
    fn accounts(&self) -> Result<Vec<ManagedAccount>> {
        self.accounts.list()
    }

    async fn issue(&self, account: &ManagedAccount) -> Result<WarmupOutcome> {
        // Resolve the live identity on every attempt. Never refresh an active
        // account behind Claude Code's back, including in a headless process.
        let live = self.auth.read_live_claude_code().await?;
        let token = match live.filter(|live| {
            account.matches_credentials(&live.claude_code_oauth_blob, &live.oauth_account_blob)
        }) {
            // Use the same credential snapshot whose identity we checked.
            // Reading the live store twice could warm the wrong account if
            // another process signs in between the two reads.
            Some(live) => live.claude_code_oauth_blob["accessToken"]
                .as_str()
                .context("Live account has no access token")?
                .to_owned(),
            None => self
                .auth
                .token_for_slot(account.slot, None, self.accounts)
                .await
                .context("load warm-up credentials")?,
        };
        anyhow::ensure!(
            self.accounts
                .get(account.slot)?
                .is_some_and(|a| a.account_uuid == account.account_uuid),
            "Warm-up account was removed or replaced"
        );
        // Consent may have changed while a token refresh was in flight.
        anyhow::ensure!(
            allowed(self.db, &account.account_uuid)?,
            "Warm-up has been disabled or consent revoked"
        );
        // Through Claude Code when it is installed: the API sometimes refuses
        // the direct request that the CLI's own request gets through.
        let claude = crate::providers::launcher::resolve_claude_binary().ok();
        super::claude_cli::warm_up_or_fall_back(claude.as_deref(), &token, || {
            super::warmup_account(None, || Ok(token.clone()), self.http)
        })
        .await
    }
}

#[derive(Debug, Default)]
pub struct TickSummary {
    pub attempted: usize,
    pub failed: usize,
}

fn allowed(db: &Db, account_id: &str) -> Result<bool> {
    Ok(db.conn().query_row(
        "SELECT EXISTS(SELECT 1 FROM accounts WHERE id = ?1 AND warmup_enabled = 1)
         AND EXISTS(SELECT 1 FROM settings WHERE key = 'warmup_consent_granted' AND value = '1')",
        [account_id],
        |r| r.get(0),
    )?)
}

async fn perform(
    db: &Db,
    backend: &impl Backend,
    account: &ManagedAccount,
    hot_snapshot: Option<&UsageSnapshot>,
) -> Result<WarmupOutcome> {
    // Persisted readings survive app exit. A future reset means the existing
    // window is still active even when the reading itself is old. Prefer a
    // newer in-app reading when available; never assume an empty headless cache.
    let persisted = db
        .latest_snapshot(&account.account_uuid)?
        .map(|(_, json)| serde_json::from_str::<UsageSnapshot>(&json))
        .transpose()?;
    let snapshot = match (hot_snapshot, persisted.as_ref()) {
        (Some(hot), Some(saved)) if saved.fetched_at > hot.fetched_at => Some(saved),
        (Some(hot), _) => Some(hot),
        (_, saved) => saved,
    };
    if snapshot
        .and_then(|s| s.five_hour.as_ref())
        .and_then(|b| b.resets_at)
        .is_some_and(|at| at > Utc::now())
    {
        return Ok(WarmupOutcome::SkippedAlreadyActive);
    }
    anyhow::ensure!(
        allowed(db, &account.account_uuid)?,
        "Warm-up has been disabled or consent revoked"
    );
    backend.issue(account).await
}

pub async fn scheduled(
    db: &Db,
    backend: &impl Backend,
    now: DateTime<Local>,
) -> Result<TickSummary> {
    let schedules: Vec<(String, String)> = {
        let conn = db.conn();
        let mut stmt = conn.prepare("SELECT id, schedule FROM accounts WHERE warmup_enabled = 1
            AND EXISTS(SELECT 1 FROM settings WHERE key = 'warmup_consent_granted' AND value = '1')")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let mut summary = TickSummary::default();
    if schedules.is_empty() {
        return Ok(summary);
    }
    let accounts = backend.accounts()?;
    for (id, json) in schedules {
        let schedule: Schedule = match serde_json::from_str(&json) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("invalid warm-up schedule: {e}");
                summary.failed += 1;
                continue;
            }
        };
        let Some(fire) = catchup::most_recent_expected_fire(&schedule, now) else {
            continue;
        };
        let Some(account) = accounts.iter().find(|a| a.account_uuid == id) else {
            continue;
        };
        let Some(lease) =
            claim::claim_run(&db.conn(), &id, Some(fire.timestamp()), now.timestamp())?
        else {
            continue;
        };
        let outcome = perform(db, backend, account, None).await;
        claim::finish_run(&db.conn(), &lease, &outcome, Utc::now().timestamp())?;
        summary.attempted += 1;
        if !matches!(
            outcome,
            Ok(WarmupOutcome::Success | WarmupOutcome::SkippedAlreadyActive)
        ) {
            summary.failed += 1;
        }
        tracing::info!(target: "switchboard.warmup", slot = account.slot, scheduled_fire = %fire,
            "scheduled warm-up result: {outcome:?}");
    }
    Ok(summary)
}

pub async fn manual(
    db: &Db,
    backend: &impl Backend,
    id: &str,
    hot_snapshot: Option<&UsageSnapshot>,
) -> Result<WarmupOutcome> {
    anyhow::ensure!(
        allowed(db, id)?,
        "Enable warm-up and grant consent for this account first"
    );
    let account = backend
        .accounts()?
        .into_iter()
        .find(|a| a.account_uuid == id)
        .context("Warm-up account was removed")?;
    let Some(lease) = claim::claim_run(&db.conn(), id, None, Utc::now().timestamp())? else {
        // Not the automatic retry back-off: a manual run ignores that. Only a
        // run in flight, or one started a moment ago, lands here.
        anyhow::bail!(
            "A warm-up for this account is already running or was started less than a minute ago."
        );
    };
    let outcome = perform(db, backend, &account, hot_snapshot).await;
    claim::finish_run(&db.conn(), &lease, &outcome, Utc::now().timestamp())?;
    outcome
}
