# Account Swap — Hot Reload Design Specification

**Date:** 2026-05-06
**Status:** Design pending user review
**Builds on:** `docs/superpowers/specs/2026-05-05-multi-account-swap-design.md`
**Source-validated against:** Claude Code v2.x — `src/utils/auth.ts`, `src/utils/secureStorage/macOsKeychainHelpers.ts`, `src/utils/secureStorage/macOsKeychainStorage.ts` at `/Users/feixu/Developer/claude code source/claude-code/src/`

---

## 1. Overview

Make the account swap "hot reload" — running Claude Code (CC) sessions automatically adopt the new account within ~30 seconds (macOS) or one API tick (Linux/Windows), with no kill or restart required. Replace the current "restart required" UX with copy that reflects what actually happens, and add a small background "keychain guardian" to defeat a narrow clobber race.

This supersedes the project memory at `~/.claude/projects/.../memory/project_swap_semantics.md`, which incorrectly claimed restart was required and that running CC processes would clobber the swap on their next refresh.

### What CC actually does (source-validated)

1. **`KEYCHAIN_CACHE_TTL_MS = 30_000`** (`macOsKeychainHelpers.ts:69`). Each CC process caches keychain reads for 30 seconds. After our swap, that's the worst-case time before any in-process read sees the new credentials.

2. **`invalidateOAuthCacheIfDiskChanged()`** (`auth.ts:1320`) runs at the top of every refresh check. On Linux/Windows it's an mtime check on `~/.claude/.credentials.json` — when we update the file, mtime advances, all in-memory caches clear, next read returns the new account. On macOS the function always clears the in-process memoize, so the read falls through to the 30-second keychain cache.

3. **`checkAndRefreshOAuthTokenIfNeededImpl()`** (`auth.ts:1447`) re-reads keychain *after* clearing caches before deciding to refresh:
   ```typescript
   getClaudeAIOAuthTokens.cache?.clear?.()
   clearKeychainCache()
   const freshTokens = await getClaudeAIOAuthTokensAsync()
   if (!freshTokens?.refreshToken || !isOAuthTokenExpired(freshTokens.expiresAt)) {
     return false   // sees our newly-swapped account, not expired → no refresh
   }
   ```
   So when CC's old access token expires, it re-reads keychain, finds our new account's non-expired tokens, and adopts them — no OAuth refresh, no write back, no restart.

4. **`handleOAuth401ErrorImpl()`** (`auth.ts:1373`) wires explicit cross-tab adoption: on a 401, CC clears caches, re-reads keychain, and if it finds a different access token than the one that failed, immediately uses it (logged as `tengu_oauth_401_recovered_from_keychain`).

### Goals

| Goal | Decision |
|---|---|
| Running CC sessions adopt the new account without restart | Yes — relies on CC's existing 30s keychain TTL + mtime invalidation |
| Reliably defeat the narrow in-flight-refresh clobber race | Yes — keychain guardian re-applies target creds for 60s post-swap |
| UX copy reflects actual behavior | Yes — drop "restart required" warning |
| Optional escape hatch for users who don't want to wait 30s | Yes — `SIGTERM` + `claude --continue` toast |
| Update incorrect project memory | Yes — replace with source-validated fact |

### Non-goals

- In-process token swap that bypasses the 30-second window on macOS (would require process injection or a wrapper; out of scope)
- Wrapper-based swap mechanism like `cux` (would change app surface area significantly; not warranted given hot reload already works)
- VS Code extension SIGTERM (different process tree; user restarts the extension manually if they want immediate adoption there)
- `CLAUDE_CONFIG_DIR`-based per-account redesign

---

## 2. Architecture

### 2.1 Keychain guardian

A new module at `src-tauri/src/auth/keychain_guardian.rs`. Single public surface:

```rust
pub struct KeychainGuardian { /* opaque */ }

impl KeychainGuardian {
    pub fn arm(target: ManagedAccount) -> Self;
    pub fn cancel(self);
}
```

**Responsibility.** After a successful swap, hold the keychain entry on the target account for 60 seconds, defeating any in-flight CC refresh that completes after our swap and would otherwise write the previous account's rotated tokens back.

**Mechanism.**

1. On `arm`, capture the target's `claude_code_oauth_blob`.
2. Spawn a tokio task that loops every ~2 seconds for up to 60 seconds:
   - Read the canonical keychain entry via `claude_code_creds::load_full_blob()`.
   - Compare the stored `refreshToken` against the target's `refreshToken`.
   - If different → re-write the target's blob via `claude_code_creds::write_full_blob()`. Log a guardian-clobber-defeated event.
3. Stop on:
   - 60-second deadline.
   - `cancel()` (called on app exit or a new swap superseding this one).

**Concurrency / cancellation.** Single `Arc<Notify>` for cancellation; a new `arm` cancels the previous guardian before spawning. Held in `AppState` as `Mutex<Option<KeychainGuardian>>`.

**Platform surface.**
- macOS: identical, since the keychain backend is what CC reads.
- Windows: read/compare/write `~/.claude/.credentials.json` (the file CC consults). Existing `claude_code_creds::windows::*` module covers the I/O.
- Linux: not supported by this app (per existing scope), but the code path is identical to Windows since CC's mtime check on the same file naturally drives hot reload.

**Why 60 seconds?** CC's worst-case in-flight refresh round-trip is bounded by the OAuth endpoint timeout (typically <30s). 60s gives generous headroom against slow networks. After 60s, any subsequent CC refresh re-reads keychain fresh and uses the target's tokens directly (per `auth.ts:1474–1481`).

### 2.2 Wire-up in `commands::swap_to_account`

```rust
pub async fn swap_to_account(slot, state) -> Result<SwapReport, String> {
    state.accounts.swap_to(slot).await.map_err(|e| e.to_string())?;
    let target = state.accounts.get(slot)?.ok_or("…")?;
    state.keychain_guardian.replace(KeychainGuardian::arm(target));
    let running = process_detection::detect();
    state.force_refresh.notify_one();
    Ok(SwapReport { new_active_slot: slot, running })
}
```

`AppState` gains one field:
```rust
pub keychain_guardian: parking_lot::Mutex<Option<KeychainGuardian>>,
```

Cancellation on app exit happens naturally — the tokio runtime drops the task. Explicit cancel is only needed when a *new* swap supersedes the old guardian.

### 2.3 UX copy changes

`src/accounts/SwapConfirmCard.tsx` — replace the "What happens" block:

**Before:**
> • Replaces the upstream-CLI credentials in your macOS Keychain
> • Rewrites the `oauthAccount` slice of `~/.claude.json`
> • New `claude` invocations will use {target.email}
> • {N} running CLI sessions cache {previous} in memory — restart them to switch (otherwise their next token refresh can overwrite this swap)

**After:**
> • Replaces the upstream-CLI credentials in your macOS Keychain
> • Rewrites the `oauthAccount` slice of `~/.claude.json`
> • New `claude` invocations use {target.email} immediately
> • Running Claude Code sessions adopt the new account within ~30 seconds (when their cached credentials refresh)

The warning color treatment is removed — the running-session line is now informational, not a hazard.

`SwapToast.tsx` (popover toast after swap) — same direction: replace "restart required" hint with "running sessions adopt within 30s".

### 2.4 Optional "Switch immediately" button (deferred to phase 2)

A button on `SwapConfirmCard` labeled "Switch all running sessions immediately." Behavior:

1. Sends `SIGTERM` to detected `claude` CLI PIDs (CC handles SIGTERM gracefully and persists session state to `~/.claude/projects/.../<session-id>.jsonl`).
2. Shows a follow-up toast: "Run `claude --continue` to resume on the new account." (Per `code.claude.com/docs/en/agent-sdk/sessions`, `--continue` restores full conversation context.)
3. VS Code extension processes are listed but not killed — user restarts manually.

This is opt-in and not in the v1 scope of this spec — the default 30-second adoption is the primary UX. We include the design here so a follow-up PR can land it without re-design.

### 2.5 Project memory correction

Replace `~/.claude/projects/-Users-feixu-Developer-claude-usage-gh-claude-limits/memory/project_swap_semantics.md` with text that matches the source-validated reality:

> **Account-swap semantics — hot reload, no restart needed**
>
> After `swap_to_account` writes the new credentials to the macOS Keychain (or `~/.claude/.credentials.json` on Windows), running Claude Code processes adopt the new account automatically:
> - **macOS:** within ≤30 seconds, when the in-process keychain cache (`KEYCHAIN_CACHE_TTL_MS = 30_000`) expires.
> - **Windows/Linux:** within one API tick, via mtime invalidation in `invalidateOAuthCacheIfDiskChanged()`.
>
> Source references: `auth.ts:1320, 1447, 1474–1481, 1373`; `macOsKeychainHelpers.ts:69`; `macOsKeychainStorage.ts:30,69`.
>
> A keychain guardian (`src-tauri/src/auth/keychain_guardian.rs`) re-applies target creds for 60s post-swap to defeat the narrow race where a CC process started its refresh just before our swap and writes the previous account's rotated tokens back when its OAuth round-trip completes.
>
> UX copy must NOT say "restart required." Say "running sessions adopt within ~30 seconds."

---

## 3. Data flow

```
User clicks "Switch to B" in popover
  ↓
commands::swap_to_account(slot=B)
  ├─ AccountManager::swap_to(B)
  │    ├─ snapshot prev keychain + ~/.claude.json
  │    ├─ write B's blob to keychain
  │    └─ splice B's oauthAccount into ~/.claude.json
  ├─ KeychainGuardian::arm(B) — replaces any prior guardian
  │    └─ tokio task: re-apply B every 2s for 60s if drifted
  └─ force_refresh.notify_one()

Meanwhile, in any running CC process:
  ↓
  Next API call triggers checkAndRefreshOAuthTokenIfNeeded()
  ↓
  invalidateOAuthCacheIfDiskChanged() clears in-process memoize
  ↓
  getClaudeAIOAuthTokensAsync() reads keychain
    - macOS: cache hit if <30s old (returns previous A) OR
             cache miss → fresh `security` spawn (returns B)
    - Win/Linux: mtime advanced → fresh read (returns B)
  ↓
  freshTokens (B) not expired → return false; no refresh
  ↓
  CC's next API call uses B's access token — hot reload complete
```

The clobber race (with guardian):
```
T0:    CC process X starts refresh check (still has A in keychain)
T0+0:  X reads A from keychain, A is expired
T0+0:  X acquires lockfile, calls refreshOAuthToken(A.refreshToken) — in flight
T0+1:  User clicks swap → we write B to keychain
T0+1:  KeychainGuardian armed
T0+2:  X's refresh returns new A tokens; X writes new A to keychain (CLOBBER)
T0+3:  Guardian polls, sees A's refreshToken in keychain, re-writes B
T0+5+: All future X reads return B; X adopts B on next refresh check
```

---

## 4. Error handling

- **Guardian write fails.** Log; continue retrying every 2s. After 60s, give up (the user will see the issue if running tabs don't adopt — they can manually re-swap).
- **Guardian read fails.** Log; treat as "no drift detected" and continue polling.
- **Keychain blob is malformed when guardian reads it.** Treat as drift → re-write target. (Defensive; shouldn't happen.)
- **Swap itself fails.** Existing rollback path in `AccountManager::swap_to` is unchanged; guardian is not armed.

No new failure modes are introduced for the swap path. The guardian is best-effort over and above an already-correct swap.

---

## 5. Testing

### Unit tests (Rust)

`src-tauri/src/auth/keychain_guardian.rs` — module-level tests with a mock secure-storage trait:

- **arm-then-cancel** within deadline → no writes attempted after cancel.
- **drift detected → re-write** — set up keychain to return account A after first poll; assert guardian writes B back; assert next poll sees B and stops re-writing.
- **deadline reached → task exits** — tokio `time::pause()` to fast-forward; assert task joins within 60s of arm.
- **arm twice cancels first** — assert first task no longer writes after second arm.

### Integration test

Add to `src-tauri/tests/integration_*.rs` (existing pattern):

- Swap A → B with no running CC simulator → guardian observes no drift → 0 re-writes.
- Swap A → B with simulator that writes A back at T+1s → guardian detects at T+2-3s → re-applies B; final keychain state is B.

### Manual smoke (release-checklist addition)

Add to `docs/release-checklist.md` under "Multi-account swap":

```
- [ ] Hot reload — start a `claude` CLI session as A, swap to B in the tray app,
      wait 30s, send a CC message, verify it succeeds against B (check logs
      or `/account` slash command output)
- [ ] Hot reload under in-flight refresh — same as above but trigger the swap
      while CC is mid-refresh (force token expiry via clock skew or wait until
      ~5min in); verify final keychain state is B and CC adopts B within 60s
```

---

## 6. Files touched

**New:**
- `src-tauri/src/auth/keychain_guardian.rs`
- (tests inline in module)

**Modified:**
- `src-tauri/src/auth/mod.rs` — re-export `keychain_guardian`
- `src-tauri/src/app_state.rs` — add `keychain_guardian: Mutex<Option<KeychainGuardian>>`
- `src-tauri/src/commands.rs` — arm guardian after `swap_to`
- `src/accounts/SwapConfirmCard.tsx` — copy update, drop warning treatment
- `src/popover/CompactPopover.tsx` — `SwapToast` component (inline, line ~28) — copy update
- `docs/release-checklist.md` — add hot-reload smoke items
- `~/.claude/projects/.../memory/project_swap_semantics.md` — replace with corrected text

**Deferred (phase 2, separate PR):**
- "Switch immediately" button + SIGTERM path in `SwapConfirmCard.tsx` and a new `commands::terminate_running_claude_code`

---

## 7. Rollout

Single PR. No feature flag — the guardian is unconditionally armed on swap, and the copy change is a pure UX correction. The behavior degrades gracefully: if the guardian is removed in a future change, swaps still work (CC's natural hot-reload still fires), they're just slightly more vulnerable to the in-flight-refresh clobber.

No migration concerns — no schema changes, no on-disk format changes.

---

## 8. Open questions

None. Source behavior is verified; design is bounded to the keychain guardian + UX copy.
