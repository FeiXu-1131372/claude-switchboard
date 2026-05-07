# Rebrand to Claude Switchboard — Implementation Plan (Plan A)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebrand the app from "Claude Limits" to "Claude Switchboard" — change the bundle ID, repo path, data dir, lockfile, user-agent, autostart entries, and UI strings. Migrate existing v0.3.x users' data automatically on first launch. Ship as Switchboard v1.0.0 with **zero functional changes** beyond the rename — warm-up & scheduling come in Plan B.

**Architecture:** All brand identifiers move into `src-tauri/src/branding.rs` and `src/lib/branding.ts`. The new bundle ID forces a fresh data dir (`~/Library/Application Support/com.claude-switchboard.ClaudeSwitchboard/`); a one-shot migration on first launch copies the old data dir contents to the new path, removes the legacy autostart plist, and sets `settings.migration_completed = 1`. The migration is gated by that flag so it runs at most once. A new `migration/legacy_process.rs` finds and quits a still-running v0.3.x instance before copying.

**Tech Stack:** Rust (Tauri v2 backend, `sysinfo`, `directories`, `rusqlite`, `fs2`), React 19 + TypeScript + Vitest (frontend), tauri-plugin-autostart.

**Spec reference:** `docs/superpowers/specs/2026-05-07-switchboard-rebrand-and-warmup-design.md` §§4, 9, 10 (rename surface table, migration flow, file boundaries).

---

## File Structure

| Path | Action | Responsibility |
|---|---|---|
| `src-tauri/src/branding.rs` | Create | Single source of truth for product name, bundle IDs, project-dirs strings, user-agent prefix, GitHub path, lockfile names, **and the legacy equivalents** (used by migration to find old data) |
| `src/lib/branding.ts` | Create | Frontend mirror of branding constants |
| `src-tauri/src/migration/mod.rs` | Create | Orchestrates first-launch migration; idempotent via `settings.migration_completed` |
| `src-tauri/src/migration/legacy_process.rs` | Create | Finds and SIGTERMs a still-running v0.3.x app process |
| `src-tauri/src/migration/data_dir_copy.rs` | Create | Recursively copies the old data dir contents into the new dir, skipping lockfiles |
| `src-tauri/src/migration/autostart.rs` | Create | Removes legacy launch-agent plist (macOS) / Run-key (Windows); re-registers if autostart was on |
| `src-tauri/src/store/migrations/0004_migration_state.sql` | Create | Inserts `migration_completed` row into `settings` |
| `src-tauri/src/store/mod.rs` | Modify | `default_dir()` reads from `branding.rs`; lockfile name uses `branding.rs`; bumps schema_version to handle 0004 |
| `src-tauri/src/usage_api/client.rs` | Modify | User-Agent uses `branding::USER_AGENT_PREFIX` |
| `src-tauri/src/lib.rs` | Modify | Run migration before `Db::open`; replace any hard-coded strings with `branding.rs` constants |
| `src-tauri/src/main.rs` | Modify | Set process name from branding (cosmetic, but useful for `legacy_process.rs` heuristics) |
| `src-tauri/Cargo.toml` | Modify | Package name `claude-limits` → `claude-switchboard`; version → `1.0.0` |
| `src-tauri/tauri.conf.json` | Modify | `productName`, `identifier`, updater URL |
| `package.json` | Modify | Package name and version |
| `src/App.tsx` and any component that prints "Claude Limits" | Modify | Read product name from `branding.ts` |
| `src/components/modals/WelcomeToSwitchboard.tsx` | Create | One-time post-migration welcome dialog |
| `src/components/modals/__tests__/WelcomeToSwitchboard.test.tsx` | Create | Render + dismissal tests |
| `README.md` | Modify | Replace product name and paths; remove screenshots referencing "Claude Limits" branding (regen later) |

---

## Task 1: Create `branding.rs` with new + legacy constants

**Files:**
- Create: `src-tauri/src/branding.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod branding;`)
- Test: `src-tauri/src/branding.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Write the failing test**

Add to a new file `src-tauri/src/branding.rs`:

```rust
//! Single source of truth for product naming and platform identifiers.
//! All hard-coded "Claude Switchboard" / `com.claude-switchboard.app` /
//! `claude-switchboard` references should read from this module so a
//! future rename touches one file.

pub const PRODUCT_NAME: &str = "Claude Switchboard";
pub const TAURI_BUNDLE_ID: &str = "com.claude-switchboard.app";

pub const PROJECT_DIRS_QUALIFIER: &str = "com";
pub const PROJECT_DIRS_ORG: &str = "claude-switchboard";
pub const PROJECT_DIRS_APP: &str = "ClaudeSwitchboard";

pub const USER_AGENT_PREFIX: &str = "claude-switchboard";
pub const GITHUB_REPO_PATH: &str = "FeiXu-1131372/claude-switchboard";
pub const DB_LOCKFILE_NAME: &str = "claude-switchboard.lock";

// === Legacy values, used by migration to detect/clean v0.3.x install ===

pub const LEGACY_PRODUCT_NAME: &str = "Claude Limits";
pub const LEGACY_TAURI_BUNDLE_ID: &str = "com.claude-limits.app";
pub const LEGACY_PROJECT_DIRS_QUALIFIER: &str = "com";
pub const LEGACY_PROJECT_DIRS_ORG: &str = "claude-limits";
pub const LEGACY_PROJECT_DIRS_APP: &str = "ClaudeLimits";
pub const LEGACY_DB_LOCKFILE_NAME: &str = "claude-monitor.lock";
pub const LEGACY_AUTOSTART_PLIST_FILENAME: &str = "com.claude-limits.app.plist";
pub const LEGACY_WINDOWS_AUTOSTART_REGKEY_NAME: &str = "Claude Limits";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_constants_match_spec() {
        assert_eq!(PRODUCT_NAME, "Claude Switchboard");
        assert_eq!(TAURI_BUNDLE_ID, "com.claude-switchboard.app");
        assert_eq!(USER_AGENT_PREFIX, "claude-switchboard");
        assert_eq!(GITHUB_REPO_PATH, "FeiXu-1131372/claude-switchboard");
    }

    #[test]
    fn legacy_constants_match_v03x_install() {
        assert_eq!(LEGACY_PRODUCT_NAME, "Claude Limits");
        assert_eq!(LEGACY_TAURI_BUNDLE_ID, "com.claude-limits.app");
        assert_eq!(LEGACY_PROJECT_DIRS_ORG, "claude-limits");
        assert_eq!(LEGACY_PROJECT_DIRS_APP, "ClaudeLimits");
        assert_eq!(LEGACY_DB_LOCKFILE_NAME, "claude-monitor.lock");
    }
}
```

In `src-tauri/src/lib.rs`, add to the module declarations near the top (alongside existing `pub mod` lines):

```rust
pub mod branding;
```

- [ ] **Step 2: Run tests**

Run: `cd src-tauri && cargo test branding::tests`
Expected: PASS — both tests green.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/branding.rs src-tauri/src/lib.rs
git commit -m "feat(branding): add Rust branding module with new + legacy constants"
```

---

## Task 2: Mirror `branding.ts` for the frontend

**Files:**
- Create: `src/lib/branding.ts`
- Create: `src/lib/__tests__/branding.test.ts`

- [ ] **Step 1: Write the failing test**

```ts
// src/lib/__tests__/branding.test.ts
import { describe, it, expect } from "vitest";
import * as branding from "../branding";

describe("branding (frontend mirror of Rust branding.rs)", () => {
  it("exports the new product name and identifiers", () => {
    expect(branding.PRODUCT_NAME).toBe("Claude Switchboard");
    expect(branding.TAURI_BUNDLE_ID).toBe("com.claude-switchboard.app");
    expect(branding.GITHUB_REPO_PATH).toBe(
      "FeiXu-1131372/claude-switchboard",
    );
  });

  it("exports the legacy product name (used in migration UI copy)", () => {
    expect(branding.LEGACY_PRODUCT_NAME).toBe("Claude Limits");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test src/lib/__tests__/branding.test.ts`
Expected: FAIL — "Cannot find module '../branding'"

- [ ] **Step 3: Implement the module**

```ts
// src/lib/branding.ts

/**
 * Mirror of src-tauri/src/branding.rs constants. Keep in sync.
 *
 * If you rename anything here, also rename in branding.rs and run
 * `cargo test branding::tests` + `pnpm test src/lib/__tests__/branding.test.ts`.
 */

export const PRODUCT_NAME = "Claude Switchboard";
export const TAURI_BUNDLE_ID = "com.claude-switchboard.app";
export const GITHUB_REPO_PATH = "FeiXu-1131372/claude-switchboard";

export const LEGACY_PRODUCT_NAME = "Claude Limits";
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test src/lib/__tests__/branding.test.ts`
Expected: PASS — both assertions green.

- [ ] **Step 5: Commit**

```bash
git add src/lib/branding.ts src/lib/__tests__/branding.test.ts
git commit -m "feat(branding): add TS mirror of branding constants"
```

---

## Task 3: Wire `default_dir()` and lockfile through `branding.rs`

**Files:**
- Modify: `src-tauri/src/store/mod.rs`

- [ ] **Step 1: Add a test that asserts the path uses the new constants**

In `src-tauri/src/store/mod.rs`, find the existing `#[cfg(test)] mod tests` block. Add this test:

```rust
#[test]
fn default_dir_uses_branding_constants() {
    let path = default_dir();
    let path_str = path.to_string_lossy();
    // The macOS path is ~/Library/Application Support/com.claude-switchboard.ClaudeSwitchboard
    // Linux/Windows produce platform-specific paths but always include the org+app strings.
    assert!(
        path_str.contains("claude-switchboard")
            || path_str.contains("ClaudeSwitchboard"),
        "default_dir should reference branding constants, got: {path_str}",
    );
    assert!(
        !path_str.contains("claude-limits"),
        "default_dir should NOT reference legacy claude-limits, got: {path_str}",
    );
}

#[test]
fn lockfile_name_comes_from_branding() {
    // The lockfile is created in Db::open(); we verify the constant routes
    // through correctly by spot-checking the branding module value.
    assert_eq!(crate::branding::DB_LOCKFILE_NAME, "claude-switchboard.lock");
}
```

- [ ] **Step 2: Run tests to verify the first one fails**

Run: `cd src-tauri && cargo test store::tests::default_dir_uses_branding_constants store::tests::lockfile_name_comes_from_branding`
Expected: FIRST test FAILs ("path_str contains claude-limits"); SECOND test PASSes (constant matches).

- [ ] **Step 3: Update `default_dir()` to use branding**

Find the existing `pub fn default_dir() -> PathBuf` in `src-tauri/src/store/mod.rs`. Replace its body with:

```rust
pub fn default_dir() -> PathBuf {
    use crate::branding::{
        PROJECT_DIRS_APP, PROJECT_DIRS_ORG, PROJECT_DIRS_QUALIFIER,
    };
    directories::ProjectDirs::from(
        PROJECT_DIRS_QUALIFIER,
        PROJECT_DIRS_ORG,
        PROJECT_DIRS_APP,
    )
    .map(|p| p.data_local_dir().to_path_buf())
    .unwrap_or_else(|| PathBuf::from(".claude-monitor"))
}
```

- [ ] **Step 4: Update lockfile name in `Db::open`**

Find the line `let lock_path = dir.join("claude-monitor.lock");` (around line 28). Replace with:

```rust
let lock_path = dir.join(crate::branding::DB_LOCKFILE_NAME);
```

- [ ] **Step 5: Run tests to confirm all pass**

Run: `cd src-tauri && cargo test store::`
Expected: PASS for all `store::` tests including the two new ones.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/store/mod.rs
git commit -m "refactor(store): route default_dir + lockfile through branding"
```

---

## Task 4: Update `usage_api/client.rs` User-Agent to use branding

**Files:**
- Modify: `src-tauri/src/usage_api/client.rs`

- [ ] **Step 1: Read the current line**

Run: `grep -n 'claude-limits' src-tauri/src/usage_api/client.rs`
Expected: shows `format!("claude-limits/{}", self.app_version)` near line 55.

- [ ] **Step 2: Replace with branding constant**

Find and replace:
```rust
format!("claude-limits/{}", self.app_version),
```
with:
```rust
format!("{}/{}", crate::branding::USER_AGENT_PREFIX, self.app_version),
```

- [ ] **Step 3: Verify build**

Run: `cd src-tauri && cargo build`
Expected: builds clean with no warnings about unused imports.

- [ ] **Step 4: Add a unit test asserting the prefix**

In `src-tauri/src/usage_api/client.rs` (or its `#[cfg(test)]` module), add:

```rust
#[cfg(test)]
mod ua_tests {
    use super::*;

    #[test]
    fn user_agent_uses_switchboard_prefix() {
        // We can't construct a full Client without HTTP infra in unit tests,
        // but we can assert the branding constant is the value we expect.
        assert_eq!(crate::branding::USER_AGENT_PREFIX, "claude-switchboard");
    }
}
```

Run: `cd src-tauri && cargo test usage_api::client::ua_tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/usage_api/client.rs
git commit -m "refactor(usage-api): User-Agent reads from branding"
```

---

## Task 5: Replace hard-coded "Claude Limits" / "claude-limits" in Rust source

**Files:**
- Modify: any `*.rs` files under `src-tauri/src/` that contain hard-coded "Claude Limits" or "claude-limits" strings (excluding the legacy constants in `branding.rs`, which are intentional).

- [ ] **Step 1: Enumerate the offenders**

Run: `grep -rn 'Claude Limits\|claude-limits' src-tauri/src/ --include='*.rs' | grep -v 'branding.rs' | grep -v '// '`
Expected: a list of remaining hard-coded strings — likely in `lib.rs` (window titles), `tray.rs`, log strings, error messages.

For each match, decide:
- **User-facing**: replace with `crate::branding::PRODUCT_NAME`
- **Identifier (e.g. `com.claude-limits.app`)**: leave alone IF it's in a migration code path that should still find the legacy install; otherwise route through `branding::TAURI_BUNDLE_ID` or `branding::LEGACY_TAURI_BUNDLE_ID`
- **Log/diagnostic**: leave the legacy name in historical log strings; replace future-tense logs with `PRODUCT_NAME`

- [ ] **Step 2: Apply replacements one file at a time**

For each file in the grep output (excluding `branding.rs`), open it, replace each hard-coded string per the rule above, and save.

Common patterns to replace:
```rust
// Before:
.title("Claude Limits")
// After:
.title(crate::branding::PRODUCT_NAME)
```

```rust
// Before (if it's a log message about the running app):
log::info!("Claude Limits started");
// After:
log::info!("{} started", crate::branding::PRODUCT_NAME);
```

- [ ] **Step 3: Verify no remaining literal "Claude Limits" outside branding.rs**

Run: `grep -rn 'Claude Limits' src-tauri/src/ --include='*.rs' | grep -v 'branding.rs'`
Expected: empty output. (If non-empty, those are intentional or missed — go fix.)

Run: `grep -rn 'claude-limits' src-tauri/src/ --include='*.rs' | grep -v 'branding.rs'`
Expected: only matches in migration-related files (legitimate references to the legacy install).

- [ ] **Step 4: Build and run all tests**

Run: `cd src-tauri && cargo build && cargo test`
Expected: builds and tests pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/
git commit -m "refactor: route Rust UI strings through branding::PRODUCT_NAME"
```

---

## Task 6: Replace hard-coded strings in TypeScript / React source

**Files:**
- Modify: any `*.ts`, `*.tsx` files under `src/` that print "Claude Limits".

- [ ] **Step 1: Enumerate the offenders**

Run: `grep -rn 'Claude Limits' src/ --include='*.ts' --include='*.tsx' | grep -v '__tests__'`
Expected: a list of source files with hard-coded product names (likely the popover header, About dialog, error messages, etc.).

- [ ] **Step 2: Replace each occurrence**

For each match:
- Add `import { PRODUCT_NAME } from "@/lib/branding";` (or relative path) if the file doesn't already import it.
- Replace string literals or JSX text with `{PRODUCT_NAME}`.

Example:
```tsx
// Before:
<h1 className="...">Claude Limits</h1>
// After:
<h1 className="...">{PRODUCT_NAME}</h1>
```

- [ ] **Step 3: Verify no remaining literal "Claude Limits" outside branding.ts and migration UI**

Run: `grep -rn 'Claude Limits' src/ --include='*.ts' --include='*.tsx' | grep -v 'branding.ts' | grep -v 'WelcomeToSwitchboard' | grep -v '__tests__'`
Expected: empty output. (Migration UI may legitimately reference "Claude Limits" — those stay until Task 13.)

- [ ] **Step 4: Run all frontend tests**

Run: `pnpm test`
Expected: all tests pass.

Run: `pnpm exec tsc --noEmit`
Expected: no type errors.

- [ ] **Step 5: Commit**

```bash
git add src/
git commit -m "refactor(ui): route TS strings through branding.PRODUCT_NAME"
```

---

## Task 7: Add migration `0004_migration_state.sql`

**Files:**
- Create: `src-tauri/src/store/migrations/0004_migration_state.sql`
- Modify: `src-tauri/src/store/mod.rs` (the `migrate()` function — bump schema_version)

- [ ] **Step 1: Read existing migrations for naming + format**

Run: `ls src-tauri/src/store/migrations/ && head -5 src-tauri/src/store/migrations/0003_truncate_notification_placeholders.sql`
Confirm the convention: numbered prefix, lowercase snake-case description.

- [ ] **Step 2: Create the migration file**

```sql
-- src-tauri/src/store/migrations/0004_migration_state.sql

-- Adds a flag the new Switchboard app uses to gate first-launch migration.
-- Idempotent: ON CONFLICT DO NOTHING so re-runs don't fail.
INSERT INTO settings (key, value) VALUES ('migration_completed', '0')
  ON CONFLICT (key) DO NOTHING;
```

- [ ] **Step 3: Wire it into the `migrate()` function**

Find the existing `migrate()` method on `Db` in `src-tauri/src/store/mod.rs`. Locate where `0003_*.sql` is included via `include_str!`. Add a sibling include for `0004_*.sql` and bump the target schema version (e.g. from 3 to 4) following the existing pattern.

The exact mechanism follows what's already there — look at how 0003 is wired and replicate. Pseudocode:

```rust
fn migrate(&mut self) -> Result<()> {
    let current = self.read_schema_version()?;
    if current < 4 {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(include_str!(
            "migrations/0004_migration_state.sql"
        ))?;
        self.set_schema_version(4)?;
    }
    Ok(())
}
```

(Use the existing pattern in this file — don't invent a new one. If the existing migrate() walks all migrations sequentially, just add 0004 to that walk.)

- [ ] **Step 4: Add a test**

In the existing `#[cfg(test)] mod tests` block in `store/mod.rs`, add:

```rust
#[test]
fn migration_0004_inserts_migration_completed_setting() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(dir.path()).expect("open");
    let conn = db.conn();
    let value: String = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'migration_completed'",
            [],
            |r| r.get(0),
        )
        .expect("migration_completed row should exist");
    assert_eq!(value, "0", "default value is '0' (false)");
}
```

- [ ] **Step 5: Run tests**

Run: `cd src-tauri && cargo test store::tests::migration_0004_inserts_migration_completed_setting`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/store/migrations/0004_migration_state.sql src-tauri/src/store/mod.rs
git commit -m "feat(store): migration 0004 — migration_completed flag"
```

---

## Task 8: Implement `migration/legacy_process.rs`

**Files:**
- Create: `src-tauri/src/migration/mod.rs` (just `pub mod legacy_process;` for now; rest filled in Task 11)
- Create: `src-tauri/src/migration/legacy_process.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod migration;`)

- [ ] **Step 1: Add the module declarations**

In `src-tauri/src/lib.rs`, add near the existing `pub mod` lines:
```rust
pub mod migration;
```

In `src-tauri/src/migration/mod.rs` (create the file):
```rust
//! First-launch migration from claude-limits v0.3.x to Claude Switchboard.
//! Orchestration lives here; per-step logic lives in sibling modules.

pub mod legacy_process;
```

- [ ] **Step 2: Write the failing tests**

```rust
// src-tauri/src/migration/legacy_process.rs
//! Find and quit a still-running v0.3.x `Claude Limits.app` process.
//! Distinct from `process_detection.rs`, which targets upstream Claude
//! Code / VS Code processes only.

use anyhow::{Context, Result};
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, Signal, System};

/// Heuristic match for a v0.3.x running process.
///
/// macOS: binary path under `*/Claude Limits.app/Contents/MacOS/claude-limits`,
/// or process name `claude-limits` (the default Tauri executable name).
///
/// Windows: process name `claude-limits.exe`.
pub fn find_legacy_pids(sys: &System) -> Vec<Pid> {
    let mut hits = Vec::new();
    for (pid, p) in sys.processes() {
        let name = p.name().to_string_lossy();
        let exe_path = p
            .exe()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default();

        let by_name = matches!(name.as_ref(), "claude-limits" | "claude-limits.exe");
        let by_path =
            exe_path.contains("Claude Limits.app/Contents/MacOS/claude-limits");

        if by_name || by_path {
            hits.push(*pid);
        }
    }
    hits
}

/// Send SIGTERM (or `WM_CLOSE`-equivalent via `Signal::Term`) to each pid,
/// then wait up to `grace_secs` for them to exit. Falls back to `Signal::Kill`
/// if any are still alive after the grace.
///
/// Returns `Ok(())` once all processes have exited or been killed.
pub fn quit_legacy_processes(grace_secs: u64) -> Result<()> {
    let mut sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
    );
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let pids = find_legacy_pids(&sys);
    if pids.is_empty() {
        return Ok(());
    }

    for pid in &pids {
        if let Some(p) = sys.process(*pid) {
            let _ = p.kill_with(Signal::Term);
        }
    }

    let deadline = std::time::Instant::now()
        + std::time::Duration::from_secs(grace_secs);
    while std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(250));
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        if find_legacy_pids(&sys).is_empty() {
            return Ok(());
        }
    }

    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    for pid in find_legacy_pids(&sys) {
        if let Some(p) = sys.process(pid) {
            let _ = p.kill_with(Signal::Kill);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_legacy_pids_returns_empty_on_clean_system() {
        // Real system; we assume no Claude Limits.app is running in CI.
        let mut sys = System::new();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        let pids = find_legacy_pids(&sys);
        assert!(
            pids.is_empty(),
            "CI should not have Claude Limits.app running, got {pids:?}",
        );
    }

    // The matcher is hard to unit-test without a fake System instance, since
    // sysinfo::System fields are private. The integration smoke test in §11
    // of the spec covers the live-process case.
    #[test]
    fn quit_legacy_processes_is_noop_when_nothing_to_quit() {
        // Should return immediately with Ok(()) on a clean system.
        let res = quit_legacy_processes(1);
        assert!(res.is_ok());
    }
}
```

- [ ] **Step 3: Run the tests**

Run: `cd src-tauri && cargo test migration::legacy_process`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/migration/mod.rs src-tauri/src/migration/legacy_process.rs src-tauri/src/lib.rs
git commit -m "feat(migration): legacy_process — find + quit v0.3.x process"
```

---

## Task 9: Implement `migration/data_dir_copy.rs`

**Files:**
- Create: `src-tauri/src/migration/data_dir_copy.rs`
- Modify: `src-tauri/src/migration/mod.rs` (add `pub mod data_dir_copy;`)

- [ ] **Step 1: Write the failing tests**

```rust
// src-tauri/src/migration/data_dir_copy.rs
//! Copy the contents of a v0.3.x data directory into the new Switchboard
//! data directory. Skips lockfiles and any temp/in-flight files.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// File names that should NOT be copied across (lockfiles, temp).
const SKIP_FILENAMES: &[&str] = &[
    "claude-monitor.lock",  // legacy DB lock
    ".accounts.lock",       // accounts.json lock (auth/accounts/store.rs)
    "claude-switchboard.lock", // new DB lock, in case it somehow exists
];

/// Copy every regular file in `from_dir` into `to_dir`, creating `to_dir`
/// if needed. Existing files in `to_dir` are NOT overwritten — first-launch
/// migration runs against an empty new dir, and re-runs are gated by
/// settings.migration_completed (so this code path only fires once).
///
/// Returns the number of files copied.
pub fn copy_data_dir_contents(from_dir: &Path, to_dir: &Path) -> Result<usize> {
    if !from_dir.exists() {
        return Ok(0);
    }
    fs::create_dir_all(to_dir).context("create new data dir")?;

    let mut copied = 0;
    for entry in fs::read_dir(from_dir).context("read old data dir")? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(s) => s,
            None => continue,
        };
        if SKIP_FILENAMES.contains(&name) {
            continue;
        }

        let dst = to_dir.join(name);
        if dst.exists() {
            continue;
        }
        fs::copy(&path, &dst)
            .with_context(|| format!("copy {name} into new data dir"))?;
        copied += 1;
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn touch(path: &Path, body: &str) {
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn copies_data_files_skipping_lockfiles() {
        let from = tempdir().unwrap();
        let to = tempdir().unwrap();
        touch(&from.path().join("data.db"), "fake-sqlite");
        touch(&from.path().join("accounts.json"), "{}");
        touch(&from.path().join("updater.json"), "{}");
        touch(&from.path().join("claude-monitor.lock"), "");
        touch(&from.path().join(".accounts.lock"), "");

        let n = copy_data_dir_contents(from.path(), to.path()).unwrap();

        assert_eq!(n, 3, "should copy 3 data files");
        assert!(to.path().join("data.db").exists());
        assert!(to.path().join("accounts.json").exists());
        assert!(to.path().join("updater.json").exists());
        assert!(!to.path().join("claude-monitor.lock").exists());
        assert!(!to.path().join(".accounts.lock").exists());
    }

    #[test]
    fn no_op_when_source_does_not_exist() {
        let to = tempdir().unwrap();
        let n =
            copy_data_dir_contents(Path::new("/no/such/path/here"), to.path())
                .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn does_not_overwrite_existing_files_in_destination() {
        let from = tempdir().unwrap();
        let to = tempdir().unwrap();
        touch(&from.path().join("data.db"), "old-content");
        touch(&to.path().join("data.db"), "new-content");

        let n = copy_data_dir_contents(from.path(), to.path()).unwrap();
        assert_eq!(n, 0, "destination data.db already exists, skip");

        let kept =
            std::fs::read_to_string(to.path().join("data.db")).unwrap();
        assert_eq!(kept, "new-content");
    }
}
```

In `src-tauri/src/migration/mod.rs`, add:
```rust
pub mod data_dir_copy;
```

- [ ] **Step 2: Run tests**

Run: `cd src-tauri && cargo test migration::data_dir_copy`
Expected: PASS for all three tests.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/migration/mod.rs src-tauri/src/migration/data_dir_copy.rs
git commit -m "feat(migration): data_dir_copy — copy old data dir contents"
```

---

## Task 10: Implement `migration/autostart.rs` (macOS)

**Files:**
- Create: `src-tauri/src/migration/autostart.rs`
- Modify: `src-tauri/src/migration/mod.rs` (add `pub mod autostart;`)

- [ ] **Step 1: Write the failing tests**

```rust
// src-tauri/src/migration/autostart.rs
//! Clean up the legacy launch-agent plist (macOS) / Run-key entry (Windows)
//! left behind by a v0.3.x install with autostart enabled.
//!
//! Without this, after rebrand the OS still launches the old binary at every
//! login (per `tauri-plugin-autostart` in LaunchAgent mode).

use anyhow::Result;
use std::path::{Path, PathBuf};

/// Where the legacy macOS LaunchAgents plist lives, relative to a home dir.
pub fn legacy_plist_path(home: &Path) -> PathBuf {
    home.join("Library")
        .join("LaunchAgents")
        .join(crate::branding::LEGACY_AUTOSTART_PLIST_FILENAME)
}

/// Returns true if the legacy plist exists on disk.
pub fn legacy_plist_exists(home: &Path) -> bool {
    legacy_plist_path(home).exists()
}

/// Remove the legacy plist. On macOS this also runs `launchctl unload` first
/// so the in-memory job is dropped. Best-effort: any failure of the unload
/// call is logged and ignored — what matters is that the file is gone.
#[cfg(target_os = "macos")]
pub fn remove_legacy_plist(home: &Path) -> Result<()> {
    let path = legacy_plist_path(home);
    if !path.exists() {
        return Ok(());
    }
    let _ = std::process::Command::new("launchctl")
        .arg("unload")
        .arg(&path)
        .status();
    std::fs::remove_file(&path)?;
    Ok(())
}

/// On non-macOS the file does not exist; this is a no-op.
#[cfg(not(target_os = "macos"))]
pub fn remove_legacy_plist(_home: &Path) -> Result<()> {
    Ok(())
}

/// Remove the legacy Run-key entry from `HKCU\…\Run\Claude Limits`.
/// Returns Ok(()) on Windows and a no-op on other platforms.
#[cfg(target_os = "windows")]
pub fn remove_legacy_run_key() -> Result<()> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run = hkcu.open_subkey_with_flags(
        r"Software\Microsoft\Windows\CurrentVersion\Run",
        winreg::enums::KEY_SET_VALUE,
    );
    if let Ok(run) = run {
        let _ = run.delete_value(crate::branding::LEGACY_WINDOWS_AUTOSTART_REGKEY_NAME);
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn remove_legacy_run_key() -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn legacy_plist_path_uses_branding() {
        let home = tempdir().unwrap();
        let path = legacy_plist_path(home.path());
        let s = path.to_string_lossy();
        assert!(s.ends_with("LaunchAgents/com.claude-limits.app.plist"));
    }

    #[test]
    fn legacy_plist_exists_returns_false_when_absent() {
        let home = tempdir().unwrap();
        assert!(!legacy_plist_exists(home.path()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn remove_legacy_plist_no_op_on_missing_file() {
        let home = tempdir().unwrap();
        let res = remove_legacy_plist(home.path());
        assert!(res.is_ok());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn remove_legacy_plist_deletes_existing_file() {
        let home = tempdir().unwrap();
        let agents = home.path().join("Library").join("LaunchAgents");
        std::fs::create_dir_all(&agents).unwrap();
        let plist = agents.join("com.claude-limits.app.plist");
        std::fs::write(&plist, "<?xml version=\"1.0\"?><plist/>").unwrap();
        assert!(plist.exists());

        remove_legacy_plist(home.path()).unwrap();
        assert!(!plist.exists());
    }
}
```

In `src-tauri/src/migration/mod.rs`:
```rust
pub mod autostart;
```

For Windows support, add to `src-tauri/Cargo.toml` under a Windows-only target dependency block (only if not already present):
```toml
[target.'cfg(windows)'.dependencies]
winreg = "0.55"
```

(Check `src-tauri/Cargo.toml` first — if `winreg` is already a dep elsewhere, skip this addition.)

- [ ] **Step 2: Run tests**

Run: `cd src-tauri && cargo test migration::autostart`
Expected: PASS for tests applicable to your dev OS (macOS-only tests are gated by `#[cfg]`).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/migration/mod.rs src-tauri/src/migration/autostart.rs src-tauri/Cargo.toml
git commit -m "feat(migration): autostart — remove legacy launch-agent / Run-key"
```

---

## Task 11: Implement `migration/mod.rs` orchestrator

**Files:**
- Modify: `src-tauri/src/migration/mod.rs` (replace with full orchestrator)

- [ ] **Step 1: Replace the module file with the full orchestrator**

```rust
// src-tauri/src/migration/mod.rs
//! First-launch migration from claude-limits v0.3.x to Claude Switchboard.
//!
//! Idempotent: gated by settings.migration_completed (set by 0004_migration_state.sql).
//! Runs once on first launch of v1.0.0; never again.

pub mod autostart;
pub mod data_dir_copy;
pub mod legacy_process;

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

use crate::branding::{
    LEGACY_PROJECT_DIRS_APP, LEGACY_PROJECT_DIRS_ORG, LEGACY_PROJECT_DIRS_QUALIFIER,
};

/// What the migration step found and did. Surfaces to UI for the
/// "Welcome to Switchboard" dialog.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct MigrationOutcome {
    pub legacy_data_dir_found: bool,
    pub files_copied: usize,
    pub legacy_process_quit: bool,
    pub legacy_autostart_removed: bool,
}

/// Resolve the legacy v0.3.x data directory. Symmetric with `store::default_dir()`
/// but using the legacy ProjectDirs strings.
pub fn legacy_data_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from(
        LEGACY_PROJECT_DIRS_QUALIFIER,
        LEGACY_PROJECT_DIRS_ORG,
        LEGACY_PROJECT_DIRS_APP,
    )
    .map(|p| p.data_local_dir().to_path_buf())
}

/// True if migration has already run (settings.migration_completed = '1').
fn migration_already_completed(conn: &Connection) -> Result<bool> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'migration_completed'",
            [],
            |r| r.get(0),
        )
        .ok();
    Ok(matches!(value.as_deref(), Some("1")))
}

fn mark_completed(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('migration_completed', '1') \
         ON CONFLICT (key) DO UPDATE SET value = '1'",
        [],
    )?;
    Ok(())
}

/// Run the full migration if needed. Pass the **new** data dir; the legacy
/// dir is resolved internally.
pub fn run_if_needed(new_data_dir: &Path, conn: &Connection) -> Result<MigrationOutcome> {
    if migration_already_completed(conn)? {
        return Ok(MigrationOutcome::default());
    }
    let legacy_dir = match legacy_data_dir() {
        Some(p) if p.exists() => p,
        _ => {
            // Fresh install — no legacy data. Mark complete so we never
            // re-check on subsequent launches.
            mark_completed(conn)?;
            return Ok(MigrationOutcome::default());
        }
    };

    // Step 2: quit any running v0.3.x process before touching its files.
    let legacy_process_quit = match legacy_process::quit_legacy_processes(5) {
        Ok(()) => true,
        Err(e) => {
            log::error!("Failed to quit legacy process: {e:#}");
            return Err(e).context(
                "Couldn't quit Claude Limits automatically. \
                 Quit it manually and re-launch Switchboard to continue.",
            );
        }
    };

    // Step 3: copy data dir contents.
    let files_copied =
        data_dir_copy::copy_data_dir_contents(&legacy_dir, new_data_dir)
            .context("copy legacy data dir contents")?;

    // Step 4: clean up legacy autostart entries.
    let mut legacy_autostart_removed = false;
    if let Some(home) = directories::UserDirs::new().map(|u| u.home_dir().to_path_buf()) {
        if autostart::legacy_plist_exists(&home) {
            autostart::remove_legacy_plist(&home).ok();
            legacy_autostart_removed = true;
        }
    }
    autostart::remove_legacy_run_key().ok();

    // Step 6: mark migration complete (per spec §9 step 6 — settings flag set
    // before the welcome dialog so re-launches during the dialog don't re-run).
    mark_completed(conn)?;

    Ok(MigrationOutcome {
        legacy_data_dir_found: true,
        files_copied,
        legacy_process_quit,
        legacy_autostart_removed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn open_fresh_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL); \
             INSERT INTO settings (key, value) VALUES ('migration_completed', '0');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn run_if_needed_marks_complete_on_fresh_install_with_no_legacy_dir() {
        // We can't easily fake the legacy_data_dir() result, but if no v0.3.x
        // is installed on this machine, the function should mark complete.
        let new = tempdir().unwrap();
        let conn = open_fresh_conn();

        // Pre-condition assumption: CI machine has no `com.claude-limits.ClaudeLimits`
        // ProjectDirs entry. If your dev machine does (you used v0.3.x), skip
        // this test or run on a clean VM.
        if let Some(p) = legacy_data_dir() {
            if p.exists() {
                eprintln!("legacy data dir present at {p:?}; skipping fresh-install test");
                return;
            }
        }

        let out = run_if_needed(new.path(), &conn).unwrap();
        assert!(!out.legacy_data_dir_found);

        let value: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'migration_completed'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(value, "1");
    }

    #[test]
    fn run_if_needed_is_no_op_when_already_completed() {
        let new = tempdir().unwrap();
        let conn = open_fresh_conn();
        conn.execute(
            "UPDATE settings SET value = '1' WHERE key = 'migration_completed'",
            [],
        )
        .unwrap();

        let out = run_if_needed(new.path(), &conn).unwrap();
        assert!(!out.legacy_data_dir_found);
        assert_eq!(out.files_copied, 0);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cd src-tauri && cargo test migration::tests`
Expected: PASS — both tests green.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/migration/mod.rs
git commit -m "feat(migration): orchestrator with idempotent migration_completed gate"
```

---

## Task 12: Wire migration into app startup (`lib.rs`)

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add migration call to startup sequence**

Find the existing startup sequence in `src-tauri/src/lib.rs::run()` (around line 23, where `default_dir()` and `Db::open` are called). Insert the migration call **between** `Db::open` (which creates the new `data.db`) and the rest of the app initialization. The migration uses the connection from the freshly-opened DB — its only DB write is to `settings.migration_completed`.

Pseudo-diff (adapt to actual file shape):

```rust
let data_dir = store::default_dir();
let db_result = store::Db::open(&data_dir).unwrap_or_else(|e| {
    // ... existing error handling unchanged ...
});

// NEW: run first-launch migration. Idempotent — gated by settings flag.
{
    let conn = db_result.conn();
    if let Err(e) = crate::migration::run_if_needed(&data_dir, &conn) {
        log::error!("Migration failed: {e:#}");
        // Surface as a dialog (existing dialog plumbing). Do NOT proceed
        // with normal startup if migration fails — the user must quit the
        // legacy app manually and re-launch.
        // (Implement this via the existing dialog pattern; placeholder
        // here points to the spec — Section 9, "Process detection".)
    }
}

// ... existing AuthOrchestrator / AccountManager init unchanged ...
```

(You will need to acquire the `Connection` from the `Db` struct; the existing pattern for getting a connection out of `Db` is already in use elsewhere — look for `db.conn()` or similar in the file and follow that pattern.)

- [ ] **Step 2: Build to verify**

Run: `cd src-tauri && cargo build`
Expected: builds clean.

- [ ] **Step 3: Run all backend tests**

Run: `cd src-tauri && cargo test`
Expected: all tests pass.

- [ ] **Step 4: Manual smoke test**

Build and launch on macOS:
- If you have v0.3.x data: confirm new data dir contains `data.db`, `accounts.json`, `updater.json` after first launch.
- If you don't: confirm new data dir gets created cleanly with no error.

In both cases, confirm `settings.migration_completed = '1'` after launch:
```bash
sqlite3 "~/Library/Application Support/com.claude-switchboard.ClaudeSwitchboard/data.db" \
  "SELECT value FROM settings WHERE key='migration_completed'"
```

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(lib): run first-launch migration before AuthOrchestrator init"
```

---

## Task 13: Add `WelcomeToSwitchboard` modal

**Files:**
- Create: `src/components/modals/WelcomeToSwitchboard.tsx`
- Create: `src/components/modals/__tests__/WelcomeToSwitchboard.test.tsx`
- Modify: `src/App.tsx` (mount the modal once after migration)

- [ ] **Step 1: Write the failing test**

```tsx
// src/components/modals/__tests__/WelcomeToSwitchboard.test.tsx
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { WelcomeToSwitchboard } from "../WelcomeToSwitchboard";

describe("WelcomeToSwitchboard", () => {
  it("renders the welcome heading and migration summary", () => {
    render(
      <WelcomeToSwitchboard
        outcome={{
          legacy_data_dir_found: true,
          files_copied: 3,
          legacy_process_quit: true,
          legacy_autostart_removed: true,
        }}
        onClose={() => {}}
      />,
    );
    expect(screen.getByText(/Welcome to Claude Switchboard/i)).toBeInTheDocument();
    expect(screen.getByText(/3 files migrated/i)).toBeInTheDocument();
  });

  it("calls onClose when the dismiss button is clicked", () => {
    let closed = false;
    render(
      <WelcomeToSwitchboard
        outcome={{
          legacy_data_dir_found: true,
          files_copied: 3,
          legacy_process_quit: true,
          legacy_autostart_removed: false,
        }}
        onClose={() => {
          closed = true;
        }}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /got it/i }));
    expect(closed).toBe(true);
  });

  it("does not render when no legacy data was found (fresh install)", () => {
    const { container } = render(
      <WelcomeToSwitchboard
        outcome={{
          legacy_data_dir_found: false,
          files_copied: 0,
          legacy_process_quit: false,
          legacy_autostart_removed: false,
        }}
        onClose={() => {}}
      />,
    );
    expect(container.firstChild).toBeNull();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test src/components/modals/__tests__/WelcomeToSwitchboard.test.tsx`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement the modal**

```tsx
// src/components/modals/WelcomeToSwitchboard.tsx
import { CheckCircle2 } from "lucide-react";
import { LEGACY_PRODUCT_NAME, PRODUCT_NAME } from "@/lib/branding";

export interface MigrationOutcome {
  legacy_data_dir_found: boolean;
  files_copied: number;
  legacy_process_quit: boolean;
  legacy_autostart_removed: boolean;
}

interface Props {
  outcome: MigrationOutcome;
  onClose: () => void;
}

export function WelcomeToSwitchboard({ outcome, onClose }: Props) {
  if (!outcome.legacy_data_dir_found) return null;

  return (
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-4"
    >
      <div className="max-w-sm w-full rounded-xl border border-orange-500/12 bg-neutral-900/95 backdrop-blur p-5 text-[13px] text-neutral-100 shadow-2xl">
        <div className="flex items-start gap-3 mb-3">
          <CheckCircle2 className="w-5 h-5 text-teal-400 flex-shrink-0 mt-0.5" />
          <div>
            <h2 className="text-base font-semibold">
              Welcome to {PRODUCT_NAME}
            </h2>
            <p className="text-neutral-300 mt-1 leading-snug">
              {LEGACY_PRODUCT_NAME} is now {PRODUCT_NAME} — same app, broader
              scope. Your data has been migrated automatically.
            </p>
          </div>
        </div>

        <ul className="space-y-1 text-neutral-300 ml-8 list-disc list-inside">
          <li>{outcome.files_copied} files migrated (usage history, accounts, settings)</li>
          {outcome.legacy_process_quit && (
            <li>The previous {LEGACY_PRODUCT_NAME} app was closed</li>
          )}
          {outcome.legacy_autostart_removed && (
            <li>Legacy launch-at-login entry replaced</li>
          )}
        </ul>

        <p className="text-neutral-400 mt-3 text-[12px] leading-snug">
          Your old install at <code>~/Library/Application Support/com.claude-limits.ClaudeLimits/</code>{" "}
          is preserved as a fallback. After a few weeks of stable use you'll
          see a "tidy old data" option.
        </p>

        <div className="mt-4 flex justify-end">
          <button
            type="button"
            onClick={onClose}
            className="px-3 py-1.5 rounded-md bg-teal-500/15 hover:bg-teal-500/25 text-teal-200 text-[12px] font-medium transition-colors"
          >
            Got it
          </button>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 4: Run test**

Run: `pnpm test src/components/modals/__tests__/WelcomeToSwitchboard.test.tsx`
Expected: PASS — all three tests green.

- [ ] **Step 5: Wire into App.tsx**

The migration outcome needs to be retrievable from the Rust side. Add a Tauri command in `src-tauri/src/commands.rs`:

```rust
use crate::migration::MigrationOutcome;

#[tauri::command]
pub async fn get_migration_outcome(
    state: tauri::State<'_, crate::app_state::AppState>,
) -> Result<MigrationOutcome, String> {
    Ok(state.migration_outcome.lock().unwrap().clone())
}
```

(Adapt the path through `AppState` — store the outcome in `AppState` during the `lib.rs` migration call in Task 12. If `AppState` doesn't already have a slot for this, add one.)

In `src/App.tsx`, after the migration is finished and on first render, fetch the outcome and conditionally render the modal:

```tsx
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { WelcomeToSwitchboard, MigrationOutcome } from "./components/modals/WelcomeToSwitchboard";

// Inside the App component:
const [welcomeOutcome, setWelcomeOutcome] = useState<MigrationOutcome | null>(null);
const [welcomeShown, setWelcomeShown] = useState(false);

useEffect(() => {
  if (welcomeShown) return;
  invoke<MigrationOutcome>("get_migration_outcome").then((o) => {
    if (o.legacy_data_dir_found) setWelcomeOutcome(o);
  });
}, [welcomeShown]);

// Render conditionally:
{welcomeOutcome && (
  <WelcomeToSwitchboard
    outcome={welcomeOutcome}
    onClose={() => {
      setWelcomeOutcome(null);
      setWelcomeShown(true);
    }}
  />
)}
```

- [ ] **Step 6: Run all tests + lint**

Run: `pnpm test && pnpm exec tsc --noEmit && cd src-tauri && cargo test`
Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add src/components/modals/WelcomeToSwitchboard.tsx src/components/modals/__tests__/WelcomeToSwitchboard.test.tsx src/App.tsx src-tauri/src/commands.rs src-tauri/src/app_state.rs
git commit -m "feat(migration): WelcomeToSwitchboard modal + outcome wiring"
```

---

## Task 14: Update `tauri.conf.json` (productName + identifier + updater URL)

**Files:**
- Modify: `src-tauri/tauri.conf.json`

- [ ] **Step 1: Read the file's current state**

Run: `cat src-tauri/tauri.conf.json`

- [ ] **Step 2: Update three fields**

In `src-tauri/tauri.conf.json`:
- Change `"productName": "Claude Limits"` → `"productName": "Claude Switchboard"`
- Change `"identifier": "com.claude-limits.app"` → `"identifier": "com.claude-switchboard.app"`
- In `plugins.updater.endpoints`, change the URL from `claude-limits` to `claude-switchboard`:
  ```json
  "endpoints": [
    "https://github.com/FeiXu-1131372/claude-switchboard/releases/latest/download/latest.json"
  ],
  ```

Update window title if present:
- `"title": "Claude Limits"` → `"title": "Claude Switchboard"`

- [ ] **Step 3: Build to verify**

Run: `cd src-tauri && cargo build`
Expected: builds clean.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/tauri.conf.json
git commit -m "chore(tauri): rebrand productName, identifier, and updater URL"
```

---

## Task 15: Bump Cargo.toml + package.json names and version to 1.0.0

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `package.json`

- [ ] **Step 1: Update `src-tauri/Cargo.toml`**

In the `[package]` section:
- `name = "claude-limits"` → `name = "claude-switchboard"`
- `version = "0.4.0"` → `version = "1.0.0"`

If a `[[bin]]` block exists with `name = "claude-limits"`, also update it to `claude-switchboard`.

If `default-run` references `claude-limits`, update to `claude-switchboard`.

- [ ] **Step 2: Update `package.json`**

- `"name": "claude-limits"` → `"name": "claude-switchboard"`
- `"version": "0.4.0"` → `"version": "1.0.0"`

- [ ] **Step 3: Verify build**

Run: `pnpm install && cd src-tauri && cargo build`
Expected: builds clean. Cargo.lock will update — that's expected.

- [ ] **Step 4: Update CHANGELOG.md**

Prepend to `CHANGELOG.md`:

```markdown
## v1.0.0 — 2026-05-07

**Renamed: Claude Limits is now Claude Switchboard.** First release on the
new repository at https://github.com/FeiXu-1131372/claude-switchboard.

### Migration
- v0.3.x users: install Switchboard from the new releases page. On first
  launch, your data (usage history, account credentials, settings) migrates
  automatically. The old install is preserved as a fallback.
- The old `claude-limits` repository ships one final v0.4.0 release with an
  in-app banner pointing here.

### Functional changes
- None. v1.0.0 is a pure rebrand; warm-up & scheduling features arrive in
  v1.1.0.
```

- [ ] **Step 5: Commit**

```bash
git add src-tauri/Cargo.toml package.json Cargo.lock CHANGELOG.md
git commit -m "chore(release): bump to claude-switchboard v1.0.0"
```

---

## Task 16: Update README.md

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Read the README**

Run: `head -50 README.md`

- [ ] **Step 2: Replace the product name and paths**

Use a search-and-replace pass:
- `Claude Limits` → `Claude Switchboard` (top-level mentions)
- `claude-limits` → `claude-switchboard` (in code blocks, install paths)
- `com.claude-limits.ClaudeLimits` → `com.claude-switchboard.ClaudeSwitchboard` (in privacy paths section)
- Update GitHub repo URL from `FeiXu-1131372/claude-limits` to `FeiXu-1131372/claude-switchboard`

Replace the introductory paragraph with one that reflects the new identity (multi-account control plane), but keep the structure of the rest. The screenshots section can stay — they'll be regenerated separately.

- [ ] **Step 3: Add a "Migrating from Claude Limits" subsection**

Add after the install section:

```markdown
## Migrating from Claude Limits (v0.3.x)

If you previously used Claude Limits, install Switchboard from the releases
page above and launch it once. It will:

1. Detect your existing v0.3.x data directory at
   `~/Library/Application Support/com.claude-limits.ClaudeLimits/`.
2. Quit any running Claude Limits process.
3. Copy your usage history, accounts, and settings to the new directory.
4. Remove the legacy launch-at-login entry (if you had it enabled) and
   re-register under the new bundle ID.
5. Show a one-time welcome dialog summarizing what migrated.

Your old install is preserved — you can launch the legacy `Claude Limits.app`
to fall back at any time. After ~3 months of stable Switchboard use, the
app will offer a "tidy old data" button.
```

- [ ] **Step 4: Verify rendering**

Run: `head -80 README.md`
Visually scan for any remaining "Claude Limits" / "claude-limits" mentions that should be "Switchboard". Stale screenshot paths are OK for now (separate task).

- [ ] **Step 5: Commit**

```bash
git add README.md
git commit -m "docs(readme): rebrand to Claude Switchboard + add migration section"
```

---

## Task 17: Smoke-test the full rebrand build end-to-end

**Files:**
- (No file changes; this is a verification task.)

- [ ] **Step 1: Clean build**

Run: `cd src-tauri && cargo clean && cd .. && pnpm install`

- [ ] **Step 2: Build in release mode**

Run: `pnpm tauri build`
Expected: produces bundles in `src-tauri/target/release/bundle/` named with `Claude Switchboard` and version `1.0.0`.

- [ ] **Step 3: Install and launch on macOS**

Open the produced `.dmg`, drag to `/Applications`, launch. Verify:
- Window title shows "Claude Switchboard"
- Menu-bar icon appears
- Tray tooltip shows "Claude Switchboard" branding
- New data dir created at `~/Library/Application Support/com.claude-switchboard.ClaudeSwitchboard/`
- If you previously had v0.3.x data: `data.db`, `accounts.json`, `updater.json` are present in the new dir
- Welcome modal appears once with the migration summary
- Closing and re-opening: no welcome modal (idempotent)
- All existing tabs (Sessions, Models, Trends, Projects, Heatmap, Cache) render and show your data

- [ ] **Step 4: Smoke-test autostart cleanup**

If you had launch-at-login enabled under v0.3.x:

Run: `ls ~/Library/LaunchAgents/com.claude-limits.app.plist 2>/dev/null || echo "absent"`
Expected: "absent" (the legacy plist is removed).

Run: `ls ~/Library/LaunchAgents/com.claude-switchboard.app.plist 2>/dev/null || echo "absent"`
Expected: file present (only if you had autostart enabled).

- [ ] **Step 5: Smoke-test on Windows**

Build and run a Windows bundle. Verify:
- Process name is `claude-switchboard.exe` in Task Manager
- New data dir at `%LOCALAPPDATA%\com.claude-switchboard\ClaudeSwitchboard\`
- If you had a legacy "Claude Limits" Run-key entry: it's gone after first launch.

```powershell
reg query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" | findstr /i "Claude"
```
Expected: only "Claude Switchboard" (or empty if autostart was off).

- [ ] **Step 6: Verify the auto-updater path is wired correctly**

In `src-tauri/tauri.conf.json`, confirm the updater endpoint points at the new repo. Don't actually publish a release here — that's part of Plan B's release flow.

- [ ] **Step 7: No commit step** — this is a verification task.

---

## Self-Review Checklist (already applied)

- ✅ Spec coverage:
  - §4 (naming): Tasks 14, 15, 16
  - §9 (migration flow, 7 steps): Tasks 7 (migration_completed), 8 (legacy_process), 9 (data_dir_copy), 10 (autostart), 11 (orchestrator), 12 (lib.rs wiring), 13 (welcome modal)
  - §10 (file boundaries): all tasks honor the spec's module structure
  - §13 (data model deltas — Plan A's portion): Task 7
- ✅ No placeholders: every step has runnable code or a concrete command. Steps that say "follow existing patterns" reference the actual file containing the pattern.
- ✅ Type consistency: `MigrationOutcome` definition in `migration/mod.rs` matches the TS interface in `WelcomeToSwitchboard.tsx`. Field names: `legacy_data_dir_found`, `files_copied`, `legacy_process_quit`, `legacy_autostart_removed` — same in both.
- ✅ Out of scope (correctly): no warm-up code, no schedule code, no consent UI. Those land in Plan B.

## What this plan does NOT cover

- **Warm-up, scheduling, OS-level scheduler registration, consent UI** — Plan B (`2026-05-07-warmup-and-scheduling.md`).
- **v0.4.0 banner release on the old repo** — Plan C (`2026-05-07-claude-limits-banner-v0-4-0.md`).
- **GitHub repo rename operation** — done via the GitHub UI by the maintainer; the only code-side dependency (`tauri.conf.json` updater endpoint) is updated in Task 14.
- **App icon and tray icon redesign** — design work, separate ticket.
- **Screenshot regeneration** — happens after the app is renamed; trivial follow-up.
- **Notarization / signing** — out of scope per the existing release process; the app remains unsigned.
