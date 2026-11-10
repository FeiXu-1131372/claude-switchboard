//! Tauri adapters for the shared, headless-capable warm-up runner.
use crate::app_state::AppState;
use crate::warmup::{
    runner::{self, AccountBackend},
    WarmupOutcome,
};
use anyhow::Result;
use std::sync::Arc;

pub async fn manual_warmup(state: &Arc<AppState>, account_id: &str) -> Result<WarmupOutcome> {
    let _account_change = state.account_change_lock.lock().await;
    let snapshot = {
        let cache = state.cached_usage_by_slot.read();
        cache
            .values()
            .find(|c| c.account_id == account_id)
            .map(|c| c.snapshot.clone())
    };
    runner::manual(
        &state.db,
        &AccountBackend {
            db: &state.db,
            accounts: &state.accounts,
            auth: &state.auth,
            http: &state.http_client,
        },
        account_id,
        snapshot.as_ref(),
    )
    .await
}

pub async fn walk_due_accounts(state: &Arc<AppState>) -> Result<()> {
    let _account_change = state.account_change_lock.lock().await;
    let result = runner::scheduled(
        &state.db,
        &AccountBackend {
            db: &state.db,
            accounts: &state.accounts,
            auth: &state.auth,
            http: &state.http_client,
        },
        chrono::Local::now(),
    )
    .await?;
    if result.failed > 0 {
        tracing::warn!(target: "switchboard.warmup", "{} scheduled warm-up(s) failed; see saved account results", result.failed);
    }
    Ok(())
}
