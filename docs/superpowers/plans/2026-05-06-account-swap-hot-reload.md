# Account Swap — Hot Reload Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Claude Code account swaps "hot reload" — running CC sessions adopt the new account within ~30 seconds (macOS) or one API tick (Windows) without restart, by adding a 60-second keychain guardian that defeats the in-flight-refresh clobber race, and replacing the misleading "restart required" UX copy.

**Architecture:** Add a `KeychainGuardian` background task that re-applies the target account's credentials every 2 seconds for 60 seconds after each swap, defeating the narrow race where a CC process started its OAuth refresh just before our swap and writes the previous account's tokens back when the round-trip completes. CC's own 30-second keychain cache TTL (`KEYCHAIN_CACHE_TTL_MS = 30_000`) and `invalidateOAuthCacheIfDiskChanged()` mtime check do the actual hot-reload work — we just protect the keychain entry through the danger window.

**Tech Stack:** Rust (tokio, parking_lot, anyhow, serde_json, tempfile/mockito for tests), TypeScript/React (Tailwind v4 tokens), Tauri 2.x.

**Spec reference:** `docs/superpowers/specs/2026-05-06-account-swap-hot-reload-design.md`

---

## File Structure

**New files:**
- `src-tauri/src/auth/keychain_guardian.rs` — guardian module with `KeychainGuardian` struct, `CredIO` trait, `arm`/`cancel` API, and unit tests inline.

**Modified files:**
- `src-tauri/src/auth/mod.rs` — add `pub mod keychain_guardian;`
- `src-tauri/src/app_state.rs` — add `pub keychain_guardian: parking_lot::Mutex<Option<KeychainGuardian>>` field; default-initialize in constructor.
- `src-tauri/src/commands.rs` — in `swap_to_account`, after `swap_to(slot)` succeeds, fetch the target `ManagedAccount` and arm the guardian.
- `src/accounts/SwapConfirmCard.tsx` — replace "What happens" bullet list copy and remove the warning treatment on the running-sessions line.
- `src/popover/CompactPopover.tsx` — `SwapToast` component (lines ~28–61), replace "Restart … to apply" hint with "Adopting in ~30s".
- `docs/release-checklist.md` — add hot-reload smoke items under "Multi-account swap".

**Deleted/replaced (memory only, not source):**
- `~/.claude/projects/-Users-feixu-Developer-claude-usage-gh-claude-limits/memory/project_swap_semantics.md` — rewrite content; index entry in `MEMORY.md` updated to match.

---

## Task 1: Define `CredIO` trait + `KeychainGuardian::arm` skeleton (failing test first)

**Files:**
- Create: `src-tauri/src/auth/keychain_guardian.rs`
- Modify: `src-tauri/src/auth/mod.rs`

**Why:** A trait makes the guardian unit-testable without spawning real `security` subprocesses. Production wires real keychain I/O; tests use a mock `CredIO`.

- [ ] **Step 1: Add module declaration**

In `src-tauri/src/auth/mod.rs`, append after the existing `pub mod` lines (around line 8):

```rust
pub mod keychain_guardian;
```

- [ ] **Step 2: Write the failing test**

Create `src-tauri/src/auth/keychain_guardian.rs`:

```rust
//! Background task that holds a swap target's credentials in the platform
//! credential store for 60 seconds post-swap, defeating the narrow race
//! where a still-running Claude Code process completes an in-flight OAuth
//! refresh after our swap and writes the previous account's rotated tokens
//! back. CC's own keychain cache TTL (30s, src/utils/secureStorage/
//! macOsKeychainHelpers.ts:69) does the natural hot-reload; we just protect
//! the entry through the danger window.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

#[async_trait]
pub trait CredIO: Send + Sync + 'static {
    async fn load(&self) -> Result<Option<Value>>;
    async fn write(&self, blob: &Value) -> Result<()>;
}

pub struct KeychainGuardian {
    cancel: Arc<Notify>,
}

impl KeychainGuardian {
    pub fn arm<I: CredIO>(_target_blob: Value, _io: Arc<I>) -> Self {
        Self {
            cancel: Arc::new(Notify::new()),
        }
    }

    pub fn cancel(self) {
        self.cancel.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    struct MockIO {
        current: Mutex<Option<Value>>,
        writes: AtomicUsize,
    }

    impl MockIO {
        fn new(initial: Value) -> Arc<Self> {
            Arc::new(Self {
                current: Mutex::new(Some(initial)),
                writes: AtomicUsize::new(0),
            })
        }
    }

    #[async_trait]
    impl CredIO for MockIO {
        async fn load(&self) -> Result<Option<Value>> {
            Ok(self.current.lock().unwrap().clone())
        }
        async fn write(&self, blob: &Value) -> Result<()> {
            *self.current.lock().unwrap() = Some(blob.clone());
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn blob(refresh: &str) -> Value {
        serde_json::json!({ "refreshToken": refresh, "accessToken": "at" })
    }

    #[tokio::test]
    async fn arm_returns_a_guardian_handle() {
        let io = MockIO::new(blob("rt-b"));
        let g = KeychainGuardian::arm(blob("rt-b"), io.clone());
        // Smoke: cancel must consume self without panicking.
        g.cancel();
        assert_eq!(io.writes.load(Ordering::SeqCst), 0);
    }
}
```

Add the `async-trait` dependency. Run:

```bash
cd src-tauri && cargo add async-trait
```

- [ ] **Step 3: Run the test — must compile and pass smoke**

```bash
cd src-tauri && cargo test --lib auth::keychain_guardian::tests::arm_returns_a_guardian_handle -- --nocapture
```

Expected: PASS. (The test does nothing real yet; it just asserts the API compiles.)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/auth/keychain_guardian.rs src-tauri/src/auth/mod.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(auth): scaffold KeychainGuardian + CredIO trait"
```

---

## Task 2: Polling loop drives writes when keychain drifts

**Files:**
- Modify: `src-tauri/src/auth/keychain_guardian.rs`

**Why:** This is the core behavior — detect when the keychain's `refreshToken` differs from the target's, re-apply the target.

- [ ] **Step 1: Add the failing test**

Append to the `tests` mod in `src-tauri/src/auth/keychain_guardian.rs`:

```rust
    #[tokio::test(start_paused = true)]
    async fn rewrites_target_when_keychain_drifts() {
        // Keychain initially has B (the swap target). After 1s a "rogue"
        // refresh writes A back. Guardian should detect and re-apply B.
        let io = MockIO::new(blob("rt-b"));
        let g = KeychainGuardian::arm(blob("rt-b"), io.clone());

        // Simulate clobber at t+1s.
        tokio::time::sleep(Duration::from_secs(1)).await;
        *io.current.lock().unwrap() = Some(blob("rt-a"));

        // Advance to t+5s — guardian polls every 2s, so it must have
        // written at least once by now.
        tokio::time::sleep(Duration::from_secs(4)).await;

        let cur = io.current.lock().unwrap().clone().unwrap();
        assert_eq!(
            cur.get("refreshToken").and_then(|v| v.as_str()),
            Some("rt-b"),
            "guardian must re-apply target after drift"
        );
        assert!(
            io.writes.load(Ordering::SeqCst) >= 1,
            "guardian must have issued at least one write"
        );
        g.cancel();
    }
```

- [ ] **Step 2: Run — must fail**

```bash
cd src-tauri && cargo test --lib auth::keychain_guardian::tests::rewrites_target_when_keychain_drifts
```

Expected: FAIL — test panics on the `assert_eq!` because the current `arm` does nothing.

- [ ] **Step 3: Implement the polling loop**

Replace the `KeychainGuardian` and its `impl` block in `src-tauri/src/auth/keychain_guardian.rs` with:

```rust
const POLL_INTERVAL: Duration = Duration::from_secs(2);
const GUARD_DURATION: Duration = Duration::from_secs(60);

pub struct KeychainGuardian {
    cancel: Arc<Notify>,
}

impl KeychainGuardian {
    pub fn arm<I: CredIO>(target_blob: Value, io: Arc<I>) -> Self {
        let cancel = Arc::new(Notify::new());
        let cancel_for_task = cancel.clone();
        tokio::spawn(async move {
            run_guardian(target_blob, io, cancel_for_task).await;
        });
        Self { cancel }
    }

    pub fn cancel(self) {
        self.cancel.notify_waiters();
    }
}

async fn run_guardian<I: CredIO>(
    target_blob: Value,
    io: Arc<I>,
    cancel: Arc<Notify>,
) {
    let target_refresh = target_blob
        .get("refreshToken")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let deadline = tokio::time::Instant::now() + GUARD_DURATION;

    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            break;
        }
        let sleep_until = (now + POLL_INTERVAL).min(deadline);
        tokio::select! {
            _ = tokio::time::sleep_until(sleep_until) => {}
            _ = cancel.notified() => return,
        }

        match io.load().await {
            Ok(Some(current)) => {
                let current_refresh = current
                    .get("refreshToken")
                    .and_then(|v| v.as_str());
                if current_refresh != target_refresh.as_deref() {
                    if let Err(e) = io.write(&target_blob).await {
                        tracing::warn!("keychain_guardian: re-apply write failed: {e:#}");
                    } else {
                        tracing::info!("keychain_guardian: re-applied target after clobber");
                    }
                }
            }
            Ok(None) => {
                if let Err(e) = io.write(&target_blob).await {
                    tracing::warn!("keychain_guardian: re-apply (was empty) failed: {e:#}");
                }
            }
            Err(e) => {
                tracing::warn!("keychain_guardian: load failed: {e:#}");
            }
        }
    }
}
```

If `tracing` isn't already in `Cargo.toml`, run:

```bash
cd src-tauri && cargo add tracing
```

(Project already uses tracing per `logging.rs`; this should be a no-op.)

- [ ] **Step 4: Run — must pass**

```bash
cd src-tauri && cargo test --lib auth::keychain_guardian::tests::rewrites_target_when_keychain_drifts
```

Expected: PASS.

- [ ] **Step 5: Re-run the smoke test from Task 1**

```bash
cd src-tauri && cargo test --lib auth::keychain_guardian::tests
```

Expected: both tests pass.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/auth/keychain_guardian.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(auth): KeychainGuardian polls and re-applies target on drift"
```

---

## Task 3: Guardian stops at the 60-second deadline

**Files:**
- Modify: `src-tauri/src/auth/keychain_guardian.rs`

**Why:** The guardian must release the entry after the in-flight-refresh window has passed. We tested *that* it writes; now test *that it stops*.

- [ ] **Step 1: Add the failing test**

Append to `tests` in `src-tauri/src/auth/keychain_guardian.rs`:

```rust
    #[tokio::test(start_paused = true)]
    async fn stops_writing_after_deadline() {
        let io = MockIO::new(blob("rt-b"));
        let _g = KeychainGuardian::arm(blob("rt-b"), io.clone());

        // Drift in at t+5s, well within the guard window.
        tokio::time::sleep(Duration::from_secs(5)).await;
        *io.current.lock().unwrap() = Some(blob("rt-a"));
        tokio::time::sleep(Duration::from_secs(4)).await; // expect re-apply
        let writes_during_guard = io.writes.load(Ordering::SeqCst);
        assert!(writes_during_guard >= 1, "should re-apply during guard window");

        // Past deadline (t+9s + 60s + slack), drift in again — must NOT re-apply.
        tokio::time::sleep(Duration::from_secs(70)).await;
        *io.current.lock().unwrap() = Some(blob("rt-c"));
        tokio::time::sleep(Duration::from_secs(10)).await;
        let writes_after_deadline = io.writes.load(Ordering::SeqCst);
        assert_eq!(
            writes_after_deadline, writes_during_guard,
            "guardian must not write after deadline"
        );
    }
```

- [ ] **Step 2: Run — should pass already (deadline check is in `run_guardian`)**

```bash
cd src-tauri && cargo test --lib auth::keychain_guardian::tests::stops_writing_after_deadline
```

Expected: PASS. (If FAIL, the deadline branch in `run_guardian` is wrong — re-check `now >= deadline` placement.)

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/auth/keychain_guardian.rs
git commit -m "test(auth): assert KeychainGuardian stops at 60s deadline"
```

---

## Task 4: Cancel stops the loop early

**Files:**
- Modify: `src-tauri/src/auth/keychain_guardian.rs`

- [ ] **Step 1: Add the failing test**

Append to `tests`:

```rust
    #[tokio::test(start_paused = true)]
    async fn cancel_stops_writes_immediately() {
        let io = MockIO::new(blob("rt-b"));
        let g = KeychainGuardian::arm(blob("rt-b"), io.clone());

        // Cancel at t+1s, before the first poll fires.
        tokio::time::sleep(Duration::from_secs(1)).await;
        g.cancel();

        // Drift in after cancel — guardian must not react.
        tokio::time::sleep(Duration::from_secs(2)).await;
        *io.current.lock().unwrap() = Some(blob("rt-a"));
        tokio::time::sleep(Duration::from_secs(10)).await;

        assert_eq!(
            io.writes.load(Ordering::SeqCst),
            0,
            "no writes should happen after cancel"
        );
    }
```

- [ ] **Step 2: Run — must pass (cancel branch is in `run_guardian`)**

```bash
cd src-tauri && cargo test --lib auth::keychain_guardian::tests::cancel_stops_writes_immediately
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/auth/keychain_guardian.rs
git commit -m "test(auth): assert KeychainGuardian.cancel halts loop"
```

---

## Task 5: Real `CredIO` impl backed by `claude_code_creds`

**Files:**
- Modify: `src-tauri/src/auth/keychain_guardian.rs`

**Why:** Production needs an impl that hits the actual macOS keychain (or Windows credentials file). `claude_code_creds::{load_full_blob, write_full_blob}` already exist (`src-tauri/src/auth/claude_code_creds/mod.rs:18,27`) and abstract platform differences.

- [ ] **Step 1: Add the impl**

Append to `src-tauri/src/auth/keychain_guardian.rs` (above the `#[cfg(test)]`):

```rust
pub struct ClaudeCodeCredIO;

#[async_trait]
impl CredIO for ClaudeCodeCredIO {
    async fn load(&self) -> Result<Option<Value>> {
        crate::auth::claude_code_creds::load_full_blob().await
    }
    async fn write(&self, blob: &Value) -> Result<()> {
        crate::auth::claude_code_creds::write_full_blob(blob).await
    }
}
```

Add a convenience constructor on `KeychainGuardian` so callers don't need to construct the IO themselves:

```rust
impl KeychainGuardian {
    pub fn arm_with_claude_code_creds(target_blob: Value) -> Self {
        Self::arm(target_blob, Arc::new(ClaudeCodeCredIO))
    }
}
```

- [ ] **Step 2: Verify it compiles**

```bash
cd src-tauri && cargo check
```

Expected: clean (warnings about unused are acceptable; no errors).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/auth/keychain_guardian.rs
git commit -m "feat(auth): ClaudeCodeCredIO impl + arm_with_claude_code_creds"
```

---

## Task 6: Wire guardian into `AppState` and `swap_to_account`

**Files:**
- Modify: `src-tauri/src/app_state.rs`
- Modify: `src-tauri/src/commands.rs`

**Why:** Each swap arms a new guardian; the previous one (if any) is cancelled. The guardian handle lives on `AppState` so it survives across swaps and can be cancelled on app exit naturally via tokio's runtime shutdown.

- [ ] **Step 1: Add the field to `AppState`**

In `src-tauri/src/app_state.rs`, modify the `AppState` struct around line 75:

```rust
use crate::auth::keychain_guardian::KeychainGuardian;
// (add to existing imports at top)

pub struct AppState {
    pub db: Arc<Db>,
    pub auth: Arc<AuthOrchestrator>,
    pub usage: Arc<UsageClient>,
    pub pricing: Arc<PricingTable>,
    pub settings: RwLock<Settings>,
    pub cached_usage: RwLock<Option<CachedUsage>>,
    pub force_refresh: Notify,
    pub accounts: Arc<AccountManager>,
    pub cached_usage_by_slot: RwLock<HashMap<u32, CachedUsage>>,
    pub active_slot: RwLock<Option<u32>>,
    pub backoff_by_slot: RwLock<HashMap<u32, BackoffState>>,
    pub keychain_guardian: parking_lot::Mutex<Option<KeychainGuardian>>,
}
```

- [ ] **Step 2: Find the `AppState` constructor and default-initialize the new field**

```bash
grep -n "fn new\|AppState {" src-tauri/src/app_state.rs src-tauri/src/lib.rs src-tauri/src/main.rs
```

In every site that constructs `AppState`, add:

```rust
keychain_guardian: parking_lot::Mutex::new(None),
```

- [ ] **Step 3: Verify it compiles**

```bash
cd src-tauri && cargo check
```

Expected: clean.

- [ ] **Step 4: Modify `commands::swap_to_account`**

In `src-tauri/src/commands.rs:487`, replace the function body:

```rust
#[command]
#[specta::specta]
pub async fn swap_to_account(
    slot: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<SwapReport, String> {
    state
        .accounts
        .swap_to(slot)
        .await
        .map_err(|e| e.to_string())?;

    if let Ok(Some(target)) = state.accounts.get(slot) {
        let prev = state.keychain_guardian.lock().replace(
            crate::auth::keychain_guardian::KeychainGuardian::arm_with_claude_code_creds(
                target.claude_code_oauth_blob.clone(),
            ),
        );
        if let Some(p) = prev {
            p.cancel();
        }
    }

    let running = process_detection::detect();
    state.force_refresh.notify_one();
    Ok(SwapReport {
        new_active_slot: slot,
        running,
    })
}
```

- [ ] **Step 5: Build the whole crate**

```bash
cd src-tauri && cargo build
```

Expected: clean build.

- [ ] **Step 6: Run all tests to confirm nothing regressed**

```bash
cd src-tauri && cargo test
```

Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/app_state.rs src-tauri/src/commands.rs
git commit -m "feat(swap): arm KeychainGuardian after every account swap"
```

---

## Task 7: Update `SwapConfirmCard` copy

**Files:**
- Modify: `src/accounts/SwapConfirmCard.tsx:115-134`

**Why:** The "What happens" panel currently warns about restart-required. With hot reload working, this is misleading.

- [ ] **Step 1: Replace the bullet list**

In `src/accounts/SwapConfirmCard.tsx`, locate the `<ul>` block currently at lines 119–133 and replace it with:

```tsx
            <ul className="flex flex-col gap-[var(--space-2xs)] text-[length:var(--text-micro)] text-[color:var(--color-text-secondary)]">
              <li>• Replaces the upstream-CLI credentials in your macOS Keychain</li>
              <li>• Rewrites the <code className="mono">oauthAccount</code> slice of <code className="mono">~/.claude.json</code></li>
              <li>• New <code className="mono">claude</code> invocations use {target.email} immediately</li>
              {hasRunning ? (
                <li>
                  • {cli > 0 && `${cli} running CLI session${cli > 1 ? 's' : ''}`}
                  {cli > 0 && code > 0 && ' and '}
                  {code > 0 && `${code} VS Code workspace${code > 1 ? 's' : ''}`}
                  {' '}adopt the new account within ~30 seconds (when their cached credentials refresh)
                </li>
              ) : (
                <li>• No Claude Code sessions are running — nothing else to do</li>
              )}
            </ul>
```

The key changes:
1. The third bullet now ends in "immediately" (was "will use…").
2. The fourth bullet drops the `text-[color:var(--color-warn)]` class (downgrade from warning to info).
3. The fourth bullet's text changes from "restart … to switch (otherwise their next token refresh can overwrite this swap)" to "adopt the new account within ~30 seconds (when their cached credentials refresh)".

- [ ] **Step 2: Build the frontend**

```bash
npm run build
```

Expected: clean. Watch for TSX errors.

- [ ] **Step 3: Commit**

```bash
git add src/accounts/SwapConfirmCard.tsx
git commit -m "feat(ui): swap-confirm copy reflects hot-reload semantics"
```

---

## Task 8: Update `SwapToast` copy

**Files:**
- Modify: `src/popover/CompactPopover.tsx:42-58`

**Why:** Post-swap toast still says "Restart … to apply." Update to match new behavior.

- [ ] **Step 1: Replace the toast body**

In `src/popover/CompactPopover.tsx`, locate the `SwapToast` return JSX (around line 45) and replace lines 42–58:

```tsx
  if (!report) return null;
  const cli = report.running.cli_processes;
  const code = report.running.vscode_with_extension.length;
  const hasRunning = cli > 0 || code > 0;
  return (
    <div className="absolute bottom-[40px] left-[var(--popover-pad)] right-[var(--popover-pad)] rounded-[var(--radius-sm)] bg-[var(--color-accent)] px-[var(--space-sm)] py-[var(--space-2xs)] text-[length:var(--text-micro)] text-white shadow-[0_4px_14px_rgba(0,0,0,0.18)]">
      <div className="truncate">
        ✓ Switched to {email ?? `slot ${report.new_active_slot}`}
      </div>
      {hasRunning && (
        <div className="opacity-85">
          {cli > 0 && `${cli} CLI session${cli > 1 ? 's' : ''}`}
          {cli > 0 && code > 0 && ' · '}
          {code > 0 && `${code} VS Code`}
          {' adopting in ~30s'}
        </div>
      )}
    </div>
  );
```

- [ ] **Step 2: Build the frontend**

```bash
npm run build
```

Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add src/popover/CompactPopover.tsx
git commit -m "feat(ui): swap toast copy reflects hot-reload"
```

---

## Task 9: Update release-checklist smoke items

**Files:**
- Modify: `docs/release-checklist.md`

- [ ] **Step 1: Add hot-reload smoke items**

In `docs/release-checklist.md`, locate the `## Multi-account swap (added 2026-05-05)` block. Replace the line:

```
- [ ] Click row B → swap → verify CC primary store + `~/.claude.json` reflect B; restart upstream and confirm B is active
```

with:

```
- [ ] Click row B → swap → verify CC primary store + `~/.claude.json` reflect B
- [ ] Hot reload — leave a `claude` CLI session running as A in another terminal; swap to B in tray; within ~30s, send a CC turn and verify it succeeds against B (check `~/.claude/logs` or run `/account` in CC)
- [ ] Hot reload under in-flight refresh — force the running CC to refresh (e.g., wait until access expiry near or use `--debug` log to confirm refresh-in-flight) and trigger swap mid-refresh; verify final keychain state is B (`security find-generic-password -s "Claude Code-credentials" -w | jq -r .claudeAiOauth.refreshToken | head -c 12`); guardian re-applies within 60s
```

- [ ] **Step 2: Commit**

```bash
git add docs/release-checklist.md
git commit -m "docs(release): hot-reload + guardian smoke items"
```

---

## Task 10: Replace project-memory file content

**Files:**
- Replace: `~/.claude/projects/-Users-feixu-Developer-claude-usage-gh-claude-limits/memory/project_swap_semantics.md`
- Modify: `~/.claude/projects/-Users-feixu-Developer-claude-usage-gh-claude-limits/memory/MEMORY.md`

**Why:** The current memory file is wrong; it'll keep producing bad UX choices in future sessions.

- [ ] **Step 1: Rewrite the memory file**

Use the Write tool to replace the file at `/Users/feixu/.claude/projects/-Users-feixu-Developer-claude-usage-gh-claude-limits/memory/project_swap_semantics.md` with:

```markdown
---
name: account-swap semantics — hot reload, no restart needed
description: After a swap, running CC processes adopt the new account within ~30s on macOS or one API tick on Windows; KeychainGuardian protects against in-flight-refresh clobber for 60s
type: project
---
After `swap_to_account` writes the new credentials to the macOS Keychain (or `~/.claude/.credentials.json` on Windows), running Claude Code processes adopt the new account automatically — restart is NOT required:
- **macOS**: within ≤30 seconds, when CC's in-process keychain cache TTL expires (`KEYCHAIN_CACHE_TTL_MS = 30_000` in `src/utils/secureStorage/macOsKeychainHelpers.ts:69`).
- **Windows / Linux**: within one API tick, via the mtime check in `invalidateOAuthCacheIfDiskChanged()` (`src/utils/auth.ts:1320`).

CC's refresh path (`src/utils/auth.ts:1474–1481`) explicitly re-reads the keychain after clearing caches and skips the OAuth refresh when the freshly-read tokens aren't expired — so it adopts our swap target without an OAuth round-trip.

**Why:** A `KeychainGuardian` background task (`src-tauri/src/auth/keychain_guardian.rs`) re-applies the swap target's credentials every 2s for 60s post-swap. This defeats the narrow race where a CC process started its OAuth refresh just before our swap and writes the *previous* account's rotated tokens back when its round-trip completes.

**How to apply:** Any UI string about post-swap behavior should say:
- New `claude` invocations use the new account immediately.
- Running sessions adopt the new account within ~30 seconds (no restart).

Don't reintroduce "restart required" / "next token refresh can overwrite this swap" copy — the guardian handles the clobber, and CC's own keychain TTL handles the adoption.
```

- [ ] **Step 2: Update the MEMORY.md index entry**

Read `/Users/feixu/.claude/projects/-Users-feixu-Developer-claude-usage-gh-claude-limits/memory/MEMORY.md`. Replace the existing line:

```
- [Account-swap semantics — restart required](project_swap_semantics.md) — running CC sessions don't auto-adopt new account; their refresh can clobber the swap
```

with:

```
- [Account-swap semantics — hot reload](project_swap_semantics.md) — running CC sessions adopt new account within ~30s on macOS / one API tick on Win; KeychainGuardian protects 60s window
```

- [ ] **Step 3: Verify (no commit — memory files are not part of repo)**

```bash
ls /Users/feixu/.claude/projects/-Users-feixu-Developer-claude-usage-gh-claude-limits/memory/
grep -A1 "project_swap_semantics" /Users/feixu/.claude/projects/-Users-feixu-Developer-claude-usage-gh-claude-limits/memory/MEMORY.md
```

Expected: file present; index line shows the new "hot reload" wording.

---

## Task 11: Final verification + spec commit

**Files:**
- Commit: `docs/superpowers/specs/2026-05-06-account-swap-hot-reload-design.md`
- Commit: `docs/superpowers/plans/2026-05-06-account-swap-hot-reload.md`

- [ ] **Step 1: Run the full Rust test suite**

```bash
cd src-tauri && cargo test
```

Expected: all green, including the 4 new `keychain_guardian::tests` tests and existing integration tests in `src-tauri/tests/`.

- [ ] **Step 2: Run lints**

```bash
cd src-tauri && cargo clippy --all-targets -- -D warnings
```

Expected: clean (no warnings escalated to errors).

- [ ] **Step 3: Frontend type-check**

```bash
npm run build
```

Expected: clean TypeScript.

- [ ] **Step 4: Commit the spec and plan**

```bash
git add docs/superpowers/specs/2026-05-06-account-swap-hot-reload-design.md \
        docs/superpowers/plans/2026-05-06-account-swap-hot-reload.md
git commit -m "docs(spec,plan): account-swap hot-reload design + plan"
```

- [ ] **Step 5: Manual smoke (one-time, before declaring done)**

1. `npm run tauri dev`
2. Add two managed accounts (A and B).
3. In a separate terminal tab, run `claude` while A is active. Have it print something (`/account` ideally).
4. In the tray app, swap to B. Don't restart anything.
5. Wait 30 seconds.
6. In the still-running CC tab, send a turn ("hi").
7. Verify the response account context corresponds to B (e.g., `/account` shows B's email).
8. Inspect keychain after 60s: `security find-generic-password -s "Claude Code-credentials" -w | jq -r .claudeAiOauth.refreshToken | head -c 12` — refresh token first 12 chars should match B's.

If any step fails, do not declare done — re-open the relevant task and fix.

---

## Self-Review Notes

**Spec coverage:**
- §2.1 Keychain guardian → Tasks 1–5
- §2.2 Wire-up in `commands::swap_to_account` → Task 6
- §2.3 UX copy changes (SwapConfirmCard) → Task 7
- §2.3 UX copy changes (SwapToast) → Task 8
- §2.4 Optional "Switch immediately" button → explicitly deferred to phase 2 per spec; not in this plan
- §2.5 Project memory correction → Task 10
- §5 Tests → inline with Tasks 1–5; manual smoke in Task 11
- §6 Files touched → all listed in this plan's "File Structure" header

**Type / signature consistency:** `CredIO::load` returns `Result<Option<Value>>`, `CredIO::write` takes `&Value`; both used identically in `ClaudeCodeCredIO` (Task 5) and `MockIO` (Task 1). `KeychainGuardian::arm` signature uses `Arc<I: CredIO>` consistently across Tasks 1, 2, 5; `arm_with_claude_code_creds(Value)` in Task 5 matches the call site in Task 6.

**Placeholder scan:** No "TBD"/"TODO"/"similar to"/etc. Every step has the actual code or command. The deferred phase-2 button is explicitly out-of-scope, not a placeholder.

**Note on integration tests:** Spec §5 mentions an integration test in `src-tauri/tests/`. The unit tests in Tasks 2–4 already cover drift, deadline, and cancel via `start_paused = true` mocked time; an integration test against the real keychain would require a CI macOS runner with keychain access, which the existing tests under `src-tauri/tests/` avoid. I've intentionally not added an integration test to this plan — the unit tests + manual smoke (Task 11 step 5) cover the behavior. If integration coverage is wanted later, it'd be a separate scope.
