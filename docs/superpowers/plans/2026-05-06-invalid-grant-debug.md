# Bug: inactive slot refresh returns `invalid_grant` after ~1 hour

## Symptom

Three managed accounts visible in the UI. After a period of normal operation, one account (slot 1) begins logging:

```
WARN claude_limits_lib::auth::exchange: token refresh error body:
    {"error": "invalid_grant", "error_description": "Refresh token not found or invalid"}
WARN claude_limits_lib::poll_loop: token_for_slot(1) failed: refresh failed: 400 Bad Request
```

This repeats every poll cycle (~5 min). Eventually the access token expires and the slot falls through to `401 Unauthorized` on the usage fetch. The account shows as errored in the UI and cannot self-recover.

---

## Codebase orientation — relevant files

| File | Role |
|------|------|
| `src-tauri/src/auth/accounts/manager.rs` | `refresh_inactive()` — reads RT from blob, calls exchange, persists new token |
| `src-tauri/src/auth/orchestrator.rs` | `token_for_slot()` — decides active vs inactive, calls `refresh_inactive` |
| `src-tauri/src/poll_loop.rs` | `poll_all()` — drives `token_for_slot` per slot each poll cycle |
| `src-tauri/src/auth/exchange.rs` | `refresh()` — actual HTTP call to `platform.claude.com/v1/oauth/token` |
| `src-tauri/src/auth/accounts/store.rs` | `ManagedAccount`, `AddSource`, `accounts.json` format |
| `src-tauri/src/auth/accounts/manager.rs` (`synthesize_blobs`) | How OAuth-added account blobs are constructed |

---

## Key code paths

### Token decision (`orchestrator.rs:token_for_slot`)

```rust
// Active slot → read live from CC's credential store, never refresh.
if Some(slot) == active_slot {
    let live = self.read_live_claude_code().await?...;
    return live.claude_code_oauth_blob["accessToken"];
}

// Inactive slot → refresh if AT expires within 2 min.
let needs_refresh = acc.token_expires_at <= Utc::now() + Duration::minutes(2);
if needs_refresh {
    accounts.refresh_inactive(slot, &self.exchange).await?;
}
```

### Active-slot detection (`poll_loop.rs:poll_all`)

```rust
let live = state.auth.read_live_claude_code().await.ok().flatten();
let active_slot = live.as_ref().and_then(|l| {
    accounts.iter()
        .find(|a| a.account_uuid == l.account_uuid)
        .map(|a| a.slot)
});
```

### Refresh call (`exchange.rs`)

```rust
let params = [
    ("grant_type", "refresh_token"),
    ("refresh_token", refresh_token),
    ("client_id", CLIENT_ID),
];
// POST platform.claude.com/v1/oauth/token
```

### Token persistence after refresh (`manager.rs:refresh_inactive`)

```rust
if let Some(rt) = new_token.refresh_token.as_ref() {
    blob.insert("refreshToken", rt.clone());  // rotating RT is persisted
}
blob.insert("expiresAt", new_token.expires_at.timestamp_millis());
store::save(&self.data_dir, &store, &lock)?;
```

---

## Hypotheses (ranked by likelihood)

### H1 — Rotating RT not captured before swap (most likely)

**Scenario:**
1. Slot 1 was added via `ImportedFromClaudeCode` (or was previously the active CC account).
2. While slot 1 was active, the upstream CLI refreshed the access token in the background, rotating the refresh token from `RT-old` to `RT-new`.
3. Our `accounts.json` still holds `RT-old` (captured at import time or last swap-away).
4. ~1 hour later slot 1's AT expires. We call `refresh_inactive(1)` with `RT-old`.
5. Platform returns `invalid_grant` because `RT-old` was already consumed.

**Why the 2-hour delay**: AT validity is ~1 hour. If the slot was active for 30 min before swap, the AT in `accounts.json` is still valid for another 30 min after swap. The first refresh attempt happens when it expires.

**Check**: Inspect `accounts.json` (see path below). Compare `expiresAt` in the slot 1 blob to the timestamp of the first `invalid_grant` log line. They should be within 2 minutes of each other.

### H2 — Active-slot detection fails → active slot gets refreshed

**Scenario:**
1. `read_live_claude_code()` returns `Err` or `None` (CC not running, file locked, etc.).
2. `active_slot` becomes `None`.
3. Even if slot 1 IS the live CC account, it's now treated as inactive.
4. Both our `refresh_inactive` and the CC CLI attempt to refresh simultaneously.
5. One wins; the other gets `invalid_grant`.

**Check**: Look for the pattern `WARN poll_loop: token_for_slot(1) failed` immediately after app start or after a CC restart. If yes, `read_live_claude_code` may be returning `None` unreliably.

### H3 — `synthesize_blobs` stores null refresh_token

**Scenario:**
1. The OAuth exchange did not return a refresh token (server omits it for certain grant types).
2. `synthesize_blobs` stores `"refreshToken": null`.
3. `refresh_inactive` extracts `null`, `.as_str()` returns `None` → early error "slot N has no refresh token".

**Status**: This would cause a DIFFERENT error message, not `invalid_grant`. Logs show the request reaches the server, so the RT is present. H3 is **unlikely** to be the root cause but worth confirming.

**Check**: Inspect `accounts.json` slot 1 blob. If `refreshToken` is `null`, this is the cause and the fix is different (surface auth_required immediately rather than attempting refresh).

### H4 — Multiple concurrent refresh calls for same slot

**Scenario:**
1. `poll_all` is called from two concurrent tasks (e.g. force-refresh + timer).
2. Both read the same expired AT, both call `refresh_inactive(1)`.
3. First call succeeds, persists `RT-new`. Second call sends `RT-old` again → `invalid_grant`.

**Check**: Look for duplicate `WARN token_for_slot(1)` lines within the same second. The `force_refresh` notify path can overlap with the 5-min timer.

---

## Where to look first

### 1. Find `accounts.json` on disk

```
Windows: %APPDATA%\com.claude-limits.app\accounts.json
macOS:   ~/Library/Application Support/com.claude-limits.app/accounts.json
```

Inspect slot 1:
- `source`: is it `"OAuth"` or `"ImportedFromClaudeCode"`?
- `token_expires_at`: what time is stored?
- `claude_code_oauth_blob.refreshToken`: present and non-null?
- `claude_code_oauth_blob.expiresAt`: compare to `token_expires_at` — they should match.

### 2. Correlate log timestamps

First `invalid_grant` timestamp vs `token_expires_at` in slot 1's blob. A match within 2 minutes confirms H1 or H2.

### 3. Check active-slot detection reliability

Add a temporary `tracing::info!("active_slot = {active_slot:?}")` at the top of `poll_all` and watch whether it ever flips to `None` when CC is running.

---

## Likely fix

### For H1 (stale RT from pre-swap rotation):

In `manager.rs::refresh_inactive`, on `invalid_grant` for an `ImportedFromClaudeCode` account, attempt to re-read from CC's live credential store if the UUID matches:

```rust
// pseudo-code
match exchange.refresh(&refresh_token).await {
    Err(e) if e.to_string().contains("invalid_grant") && acc.source == AddSource::ImportedFromClaudeCode => {
        // Try to re-sync from live CC store
        if let Ok(Some(live)) = read_live_claude_code().await {
            if live.account_uuid == acc.account_uuid {
                // re-capture fresh tokens from live store
                // ... update blob and save
                return Ok(());
            }
        }
        // Account is inactive and RT is dead → surface auth_required
        return Err(e);
    }
    other => other?,
}
```

For accounts that are genuinely inactive (CC no longer holds their tokens), the only recovery is re-authentication. Emit `auth_required_for_slot` (already wired in poll_loop.rs) so the UI prompts the user.

### For H1 (proactive fix at swap time):

In `manager.rs::swap_to`, before writing the new account's blobs to CC, re-read the CURRENT live blobs and update the outgoing slot's entry in `accounts.json` with the freshest token. This ensures the RT captured in `accounts.json` is the one CC most recently issued, not the one from initial import.

```rust
// In swap_to, step a (snapshot current):
// Also: update the current active slot's stored blob with the live snapshot
// so our accounts.json has the freshest RT for that slot.
let active_slot = find_active_slot(&store, &live_cc_uuid);
if let Some(s) = active_slot {
    if let Some(acc) = store.accounts.get_mut(&s) {
        acc.claude_code_oauth_blob = prev_cc.clone().unwrap_or(acc.claude_code_oauth_blob.clone());
    }
}
```

### For H2 (active-slot misidentification):

In `token_for_slot`, if `active_slot` is `None` but the slot's `source` is `ImportedFromClaudeCode`, try `read_live_claude_code()` as a fallback before attempting OAuth refresh:

```rust
// If we couldn't determine the active slot, check directly
if active_slot.is_none() && acc.source == AddSource::ImportedFromClaudeCode {
    if let Ok(Some(live)) = self.read_live_claude_code().await {
        if live.account_uuid == acc.account_uuid {
            // This IS the active CC account; use live token
            return live.claude_code_oauth_blob["accessToken"]...;
        }
    }
}
```

### For H4 (concurrent refresh):

Add a per-slot async Mutex or AtomicBool in `AppState` to guard `refresh_inactive` calls. Only one refresh per slot at a time.

---

## Verification

After the fix:
1. Set polling interval to 1 minute (settings).
2. Add 3 accounts, let them run for 2+ hours.
3. Confirm no `invalid_grant` in logs.
4. Swap between accounts, confirm no `invalid_grant` after the first poll post-swap.
5. Restart the app with CC not running, confirm no `invalid_grant` (H2 case).
