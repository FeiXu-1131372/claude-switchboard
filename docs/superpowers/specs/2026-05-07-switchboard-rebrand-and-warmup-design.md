# Claude Switchboard — Rebrand + Warm-up & Scheduling

**Date:** 2026-05-07
**Status:** Design approved, plan pending
**Supersedes scope of:** `2026-04-24-claude-limits-design.md` (renames product, adds warm-up pillar)

## 1. Context

The app currently ships as **Claude Limits** — a menu-bar utility that started as a rate-limit tracker and has accreted multi-account swap, hot reload, KeychainGuardian, and a 6-tab analytics report. The "Limits" name now mis-describes the product: it reads like an observation tool, but the actual identity is a multi-account control plane.

Two changes ship together in this design:

1. **Rebrand to Claude Switchboard** — full rename including bundle ID, repo, install paths, with a migration path for existing v0.3.x users.
2. **Warm-up & scheduling pillar** — deliberately start the 5-hour rolling window on a chosen account, manually or on a schedule, so users can plan rotations across accounts with predictable reset times.

The brand brief in `CLAUDE.md` (calm, precise, premium; quiet, confident, trustworthy; warm orange/teal palette; OS-Control-Center references) does **not** change. Only the badge changes.

## 2. Goals

- Rebrand the product end-to-end without stranding existing users' data.
- Add a "warm-up" action that starts a slot's 5-hour window deliberately via a minimal `/v1/messages` call.
- Add a per-slot schedule (Off / Every 5h anchored / Custom HH:MM list) that fires warm-ups automatically.
- Schedules fire reliably whether the app is running or not — OS-level scheduler (launchd / Task Scheduler) primary, in-app scheduler as fallback + catch-up.
- Make the privacy boundary loud: warm-up is strictly opt-in per slot, gated by an explicit one-time consent modal.
- Centralize all branding constants so future renames don't scatter across files.

## 3. Non-goals

- AI-driven schedule optimization (auto-detect best times). Users pick times manually.
- Cron-expression UI. Three presets cover the realistic cases.
- Cross-account smart routing ("automatically switch to the freshest account before each Claude Code session"). Out of scope; future work.
- Warm-up via Claude CLI shell-out. The app calls the API directly.
- Mac App Store / Microsoft Store distribution. GitHub releases as today.
- Telemetry on warm-up usage. Failures stay local.

## 4. Naming decision

**Claude Switchboard.** Researched against ~20 candidates; this and three others (Helm, Steward, Station) were the only ones not already taken in the Claude ecosystem. Switchboard wins on semantic load — directly evokes routing connections across accounts, which is the core action.

Names rejected on collision:
- **Console** — Anthropic's own product (`console.anthropic.com`)
- **Hub** — `claude-did-this/claude-hub` and `claude-hub.com`
- **Cockpit** — `benedictcopping/claude-cockpit`
- **Conductor** — 5+ active repos
- **Pilot** — 4+ active repos including a multi-Claude-Code session manager
- **Control** — `sverrirsig/claude-control`
- **Workspace** — Anthropic's API console primitive
- **Operator** — OpenAI product confusion
- **Studio** — Anthropic Labs is actively claiming "Claude [Creative-Tool-Noun]" namespace

## 5. Architecture overview

```
┌─ Claude Switchboard ─────────────────────────────┐
│  Observe       │ Orchestrate       │ Cadence     │
│  • tray badge  │ • account swap    │ • warm-up   │
│  • popover     │ • hot reload      │ • schedule  │
│  • analytics   │ • KeychainGuard   │   (OS+app)  │
└──────────────────────────────────────────────────┘
       (existing)        (existing)       (NEW)
```

An account's complete state extends to (a "slot" in the UI is the in-memory `u32` wrapper around an account in `auth/accounts/manager.rs`; persistence is keyed by `accounts.id`):
- **Live usage** (existing) — 5h/7d percentages, model breakdown, cache, projection
- **Auth** (existing) — OAuth credentials in OS keychain / Credential Manager
- **Cadence** (new) — `accounts.warmup_enabled`, `accounts.schedule`, `accounts.last_warmup_at`

## 6. Warm-up mechanics

### Wire-level request

```
POST https://api.anthropic.com/v1/messages
Authorization: Bearer <slot's OAuth access token>
{
  "model": <warmup::WARMUP_MODEL>,         # see "Model rot" below
  "max_tokens": 1,
  "messages": [{"role": "user", "content": "hi"}]
}
Timeout: 10s
```

**Why a 1-max-token Haiku request.** The 5h window is keyed to the account, not the model — any successful `/v1/messages` call after a quiet period starts the window. A Haiku request at `max_tokens: 1` is the cheapest viable shape. **Pinned cost**: at current Haiku 4.5 pricing (~$0.80/M input, ~$4/M output), one warm-up consumes ≤4 input + 1 output tokens ≈ **$0.000007**. Five warm-ups/day across a year ≈ **$0.013/year/account**. For subscription accounts the rate-limit math doesn't really weigh Haiku at all.

**Model rot.** The exact model identifier is held as a single constant in `warmup/config.rs` (or `branding.rs`):
```rust
pub const WARMUP_MODEL: &str = "claude-haiku-4-5";
```
When a cheaper / newer Haiku ships, the rename is one-touch.

### Precondition: skip if window already active

The 5h bucket has two states the API surfaces (verified in `usage_api/types.rs:8-14`): when active, `resets_at` is a `DateTime<Utc>`; when inactive, `resets_at = null` and `utilization = 0.0`. The window is established on the first request after a quiet period; it does **not** restart with each subsequent request.

A warm-up against an already-active bucket is therefore a no-op for the user's stated goal — the clock is already running. Before issuing the call, the dispatcher checks the most recent snapshot for the slot:

| Bucket state at fire time | Action |
|---|---|
| `resets_at == None` (inactive) | Fire warm-up. On 200, the next poll sees a populated `resets_at`. |
| `resets_at != None` (active) | Skip. Log `last_warmup_at = now` (treat as a successful no-op for dedup purposes) and record reason `"window already active"`. |

This also dissolves a UX concern: a manual "Warm up now" or scheduled fire mid-window is harmless — it's silently no-op'd, not a partial truncation. (No truncation happens; that was an earlier mis-statement of the API behavior.)

### Side effect (when fire actually occurs)

When the bucket was inactive and the call returns 200, the next poll-loop tick observes `resets_at = T_call + 5h`. The poll loop already drives all UI from the usage endpoint, so no special-case wiring is needed in the popover or the analytics tabs — they simply read fresh data.

### Failure handling

| Response | Meaning | Action |
|---|---|---|
| 200 | Success | Set `last_warmup_at = now`, log success |
| (precondition) | Window already active | Skip without HTTP call; set `last_warmup_at = now`; log `"already-active no-op"` |
| 401 / 403 | Token expired or revoked | Mark slot needs reauth, surface in UI, skip future schedules until fixed |
| 429 | Already at limit | Window running and at cap — log no-op, don't retry until next scheduled fire |
| 5xx | Anthropic server | Retry once after 30s; if still failing, log and surface at next popover open |
| Network / timeout | Offline | Retry once after 60s; if still failing, log silently |

### Rate-limit etiquette

Warm-ups are staggered across slots using the existing throttle mechanism from `2026-05-07-throttled-stagger-polling-design.md`. No parallel fan-out across all slots simultaneously.

## 7. Scheduling architecture (layered)

### Two schedulers, one dispatcher

```
┌──── OS-level scheduler (primary) ────┐    ┌──── In-app scheduler (fallback + catch-up) ────┐
│ launchd (macOS) / Task Sched (Win)   │    │ tokio task; runs while app is running           │
│ One agent fires every 1 min          │    │ Polls every 30s                                 │
│   → invokes:                         │    │   → resolves due slots                          │
│   claude-switchboard --tick          │    │ ALSO: on app launch, runs catch-up sweep        │
│ Runs even when app is closed         │    │ (detects fires missed during sleep / app close) │
└──────────────┬───────────────────────┘    └──────────────────┬──────────────────────────────┘
               │                                               │
               └────────────► Single dispatcher in Rust ◄──────┘
                              reads accounts.schedule
                              attempts transactional claim
                              (UPDATE … WHERE last_warmup_at < now-60s)
                              fires warm-up only if rows_affected == 1
```

Both schedulers call the same `scheduler::tick()` function. The dedup window is **transactional**, not advisory — without that, two processes (GUI tokio task + headless `--tick` invocation) could both read `last_warmup_at = T-100s`, both decide "fire," and both fire.

The dispatcher must claim the right to fire via a single SQL statement that is also the dedup check:

```sql
UPDATE accounts
SET last_warmup_at = :now
WHERE id = :account_id
  AND warmup_enabled = 1
  AND (last_warmup_at IS NULL OR last_warmup_at < :now - 60);
```

Firing proceeds only if `rows_affected == 1`. If the row was already updated by the other scheduler in the last 60 seconds, this UPDATE matches zero rows and the caller skips. SQLite's WAL + serialized writes make this atomic across processes; this is the *only* synchronization point between the two schedulers.

### Schedule shape (per slot)

Stored as a tagged JSON union in SQLite:

```rust
enum Schedule {
    Off,
    Every5h { anchor: HhMm },          // wall-clock anchor in user's local TZ
    Custom { times: Vec<HhMm> },       // wall-clock entries in user's local TZ
}
```

UI exposes these as three presets only — no raw cron input.

**Timezone semantics.** All `HhMm` values are **wall-clock in the user's current local timezone** (read from `chrono::Local`), evaluated fresh at each tick. Implications:
- DST transition (spring forward): a 02:30 fire is skipped that day, and the next-occurrence math advances to the following day's 02:30. DST-fall: a 01:30 fire happens once, on the first occurrence of 01:30 (the duplicated hour does not double-fire — dedup absorbs the second).
- Travel: if the user crosses a timezone, schedules retarget to the new local TZ on next tick. No coordination across machines.
- Anchor stability: `Every5h { anchor: 06:00 }` produces fires at 06:00, 11:00, 16:00, 21:00 — fixed wall-clock, regardless of DST.

### OS-level scheduler install flow

First time the user enables any schedule:

1. App shows consent dialog: *"Schedules need to register with the OS so they fire when the app is closed. This writes a user-level launch agent (no admin / sudo needed). Continue?"*
2. macOS path: write `~/Library/LaunchAgents/com.claude-switchboard.scheduler.plist` (program `<install path>/Claude Switchboard.app/Contents/MacOS/claude-switchboard`, args `--tick`, `StartInterval 60`), then `launchctl load`.
3. Windows path: `schtasks /Create /SC MINUTE /MO 1 /TN "Claude Switchboard Tick" /TR "<install path>\\claude-switchboard.exe --tick"` — user-level, no admin.
4. If user declines: only in-app scheduler runs. Banner in popover surfaces the gap: *"Schedules only fire while the app is open. Enable OS-level scheduling →"*.

### Catch-up sweep on launch

`last_expected_fire` is **computed**, not stored. On app start, for each slot with `warmup_enabled = true` and `schedule != Off`:

```text
ALGORITHM most_recent_expected_fire(schedule, now) -> Option<DateTime>
  lookback_floor := now - 24h    # bounded scan; older misses are not caught up
  candidates := []
  match schedule:
    Off => return None
    Every5h { anchor } =>
      # Generate 24/5 ≈ 5 candidate fires at anchor + k·5h within today's
      # local wall-clock day, plus the same set for yesterday. Prune to
      # entries in [lookback_floor, now]. Take the latest.
    Custom { times } =>
      # For each HhMm in times, materialize today and yesterday's local
      # wall-clock occurrence. Prune to [lookback_floor, now]. Take the latest.
  return candidates.max()  # the most recent expected fire, if any

ALGORITHM catchup_sweep(now)
  for each account with warmup_enabled = 1 and schedule != Off:
    last_expected := most_recent_expected_fire(account.schedule, now)
    if last_expected exists AND last_expected > account.last_warmup_at:
      # Use the same transactional UPDATE from the dedup section
      attempt one warm-up; the SQL update absorbs any race
```

- Capped at one catch-up per slot per launch (the algorithm returns at most the *latest* expected fire, even if multiple were missed).
- Lookback floor is 24 hours: opening the app after 3 days away gives at most one warm-up per slot, anchored to the most recent expected fire within the last 24h.

### Edge cases

- **Account in over-limit state at fire time.** Dispatcher sees `429`, logs no-op, skips until next scheduled fire.
- **Sleep across a fire (launchd, macOS).** `StartInterval`-based jobs in launchd coalesce missed fires into **one** wake-up tick, not the full backlog. After an 8-hour sleep, launchd runs `--tick` once on wake. The single tick relies on the same catch-up algorithm to fire any due warm-ups (up to one per slot).
- **Sleep across a fire (Task Scheduler, Windows).** Trigger is created with `/RU` (run-as-user) and the task XML's `StartWhenAvailable=true` so missed fires run as soon as possible after wake — same single-fire coalescing semantics. Catch-up algorithm handles the rest.
- **App uninstall.** Uninstall step calls `launchctl unload` / `schtasks /Delete`.
- **Account removed.** `remove_account` command also clears the schedule and any pending OS-level registrations for that slot.
- **Bundle ID / install path drift.** The launchd plist's `ProgramArguments[0]` and the Task Scheduler `TR` argument both reference the absolute install path. If the user moves `Claude Switchboard.app`, scheduled fires silently no-op until the user next launches the app from the new path — at which point the dispatcher detects the path change (compares current `std::env::current_exe()` vs the registered path) and re-registers automatically. The silent-no-op interval is bounded by however long the user goes without opening the app.

## 8. Privacy boundary & consent UI

### Consent gate (one-time, app-wide)

When the user toggles warm-up on for the first slot, a one-time modal:

```
┌────────────────────────────────────────────────────┐
│  Warm-up sends messages on your behalf             │
│                                                    │
│  Enabling warm-up will let Switchboard send a tiny │
│  message (1 token, on Haiku) to api.anthropic.com  │
│  using this account's credentials, whenever you    │
│  trigger it manually or a schedule fires.          │
│                                                    │
│  This is the same API surface Claude Code uses.    │
│  Cost: rounding-error against your subscription.   │
│  Effect: starts the 5-hour window deliberately.    │
│                                                    │
│  You can disable per-account at any time.          │
│                                                    │
│       [ Don't enable ]    [ Enable warm-up ]       │
└────────────────────────────────────────────────────┘
```

After acceptance: `settings.warmup_consent_granted = true`. Subsequent slots toggle warm-up directly. New slots start with `warmup_enabled = false` and require an explicit per-slot toggle.

### Per-slot UI (in Accounts panel slot card)

- **Warm-up toggle** — on/off per slot
- **Schedule selector** — Off / Every 5h (with anchor picker) / Custom (HH:MM list with `+ add` button); only visible when warm-up is on
- **Warm up now** button — one-shot trigger; only enabled when warm-up is on

### Revocation

- Per-slot toggle off — that slot stops; others unaffected.
- Global revoke from Settings — flips `warmup_consent_granted = false`, disables all per-slot toggles, requires the modal again to re-enable.

### README change

New bullet under Privacy:

> *"With your explicit per-account opt-in, Switchboard can send 1-token warm-up messages to /v1/messages to start the 5-hour window deliberately. No other content is ever sent. Off by default; revocable any time."*

### What we deliberately don't do

No telemetry on warm-up frequency. No aggregate reporting. No upload of failure logs. Failures stay local in the existing log file.

## 9. Rebrand & migration

### Two-binary transition

```
┌─ Final claude-limits release (v0.4.0) ──────────┐
│ • Banner in popover: "Claude Limits is now      │
│   Claude Switchboard. Download v1.0 →"          │
│ • All other functionality unchanged             │
│ • Auto-update via existing updater key          │
│ • Last release on this repo path                │
└──────────────────────────────────────────────────┘
                   │
                   ▼ (user downloads new app)
┌─ claude-switchboard v1.0.0 (fresh bundle ID) ───┐
│ On first launch, runs migration AUTOMATICALLY,  │
│ once, gated by settings.migration_completed:    │
│   1. Detect old install (data dir at            │
│      ~/Library/Application Support/             │
│      com.claude-limits.ClaudeLimits/ exists)    │
│   2. Quit any running claude-limits process     │
│      (reuse src-tauri/src/process_detection.rs; │
│      SIGTERM/WM_CLOSE, 5s grace)                │
│   3. Copy SQLite db to new path (don't delete)  │
│   4. Copy credentials                           │
│      (macOS Keychain / Windows Cred Manager)    │
│   5. Import settings.json (warmup_consent       │
│      starts false regardless — Section 8)       │
│   6. Set settings.migration_completed = true    │
│   7. Show one-time "Welcome to Switchboard"     │
│      dialog summarizing what migrated           │
│                                                 │
│ Fresh-install branch (no old data dir found):   │
│   - Skip steps 1–4 entirely.                    │
│   - Set settings.migration_completed = true.    │
│   - Skip the "Welcome to Switchboard" dialog;   │
│     show only the standard first-run onboarding │
│     (existing AuthPanel routing).               │
└──────────────────────────────────────────────────┘
```

The final v0.4.0 of claude-limits ships **a banner only** — no forced modal. It's not a security update. **v0.4.0 is the last release published on the `claude-limits` releases feed**; the auto-updater inside v0.4.0 will see no further releases (they all live on `claude-switchboard`'s feed). Existing v0.3.x clients update to v0.4.0 once, see the banner, and from then on the user's next move — clicking the banner — is a manual download of the new app.

**Idempotency.** Migration runs at most once per install. The `settings.migration_completed` flag (in the *new* app's SQLite) gates the entire flow — if true, skip steps 1–7 on every subsequent launch. This protects against re-migration if a user reinstalls v1.0.0 over an existing v1.x or restores from backup.

**Process detection.** Reuse `src-tauri/src/process_detection.rs` (already in the tree). On macOS: matches by binary path under `Claude Limits.app/Contents/MacOS/` and by bundle identifier `com.claude-limits.app` (verified in current `tauri.conf.json:3`). On Windows: enumerates processes and matches the executable name. SIGTERM (`WM_CLOSE` on Windows) with a 5-second grace; SIGKILL only as fallback. If quit fails, abort migration with a user-actionable error: *"Couldn't quit Claude Limits automatically. Quit it manually and re-launch Switchboard to continue."*

**Credential storage migration (step 4).** The two platforms have different stores and require separate code paths:

| Platform | Old service / target | New service / target | Mechanism |
|---|---|---|---|
| macOS | Keychain service prefix `claude-limits-*` | `claude-switchboard-*` | `security` API via existing `auth/creds/macos.rs` patterns; iterate old service names, write new entries, leave old intact (per "copy not move") |
| Windows | Credential Manager target prefix `claude-limits-*` | `claude-switchboard-*` | `wincred` API via existing `auth/creds/windows.rs` patterns; same copy-not-move semantics |

Per-account access tokens, refresh tokens, and the keychain-blob format do not change. Only the *service/target name prefix* changes.

### Surface-by-surface rename

| Surface | Old | New |
|---|---|---|
| Product name (UI, README, About) | Claude Limits | Claude Switchboard |
| GitHub repo | `FeiXu-1131372/claude-limits` | `FeiXu-1131372/claude-switchboard` (GitHub auto-redirects old URLs) |
| Tauri bundle identifier (`tauri.conf.json:3`) | `com.claude-limits.app` | `com.claude-switchboard.app` |
| `productName` in tauri.conf.json | `Claude Limits` | `Claude Switchboard` |
| `ProjectDirs::from(...)` in `store/mod.rs:135-138` | `("com", "claude-limits", "ClaudeLimits")` | `("com", "claude-switchboard", "ClaudeSwitchboard")` |
| Resulting data dir (macOS) | `~/Library/Application Support/com.claude-limits.ClaudeLimits/` | `~/Library/Application Support/com.claude-switchboard.ClaudeSwitchboard/` |
| SQLite path | `<data dir>/data.db` | `<data dir>/data.db` (path follows from data dir) |
| DB lockfile | `<data dir>/claude-monitor.lock` | `<data dir>/claude-switchboard.lock` |
| Credential service prefix | `claude-limits-*` | `claude-switchboard-*` |
| Updater URL | `https://github.com/FeiXu-1131372/claude-limits/releases/latest/download/latest.json` | `https://github.com/FeiXu-1131372/claude-switchboard/releases/latest/download/latest.json` |
| App icon, tray icon | current | new (Switchboard-themed; brand brief unchanged) |
| Cargo / package.json names | `claude-limits` | `claude-switchboard` |

### What does NOT change

Design tokens, color palette, layout, animation curves, popover/expanded-report structure, all UI components. Visual identity is preserved — same app, new badge.

### Why "copy, not move"

Leaving old keychain entries and SQLite in place means: if migration ever breaks a user, they can still launch the old binary and recover. After ~3 months of stable v1.x, ship a small "tidy old data" button — never automatic.

## 10. File & module boundaries

### New Rust modules (in `src-tauri/src/`)

```
warmup/
  mod.rs        warmup_account(account_id) -> WarmupOutcome  (public API)
  api_call.rs   the /v1/messages POST per Section 6
  errors.rs     401/403/429/5xx + active-window precondition
  config.rs     WARMUP_MODEL constant (single point of model rot)

scheduler/
  mod.rs        dispatcher: tick() called by both schedulers
  claim.rs      transactional UPDATE-with-WHERE-clause from §7
                (the ONLY synchronization point between schedulers)
  catchup.rs    most_recent_expected_fire(schedule, now) algorithm + sweep
  presets.rs    Off | Every5h(anchor) | Custom(Vec<HhMm>) serde

os_scheduler/
  mod.rs        trait { register(), unregister(), is_registered() }
  macos.rs      launchd plist + launchctl wrapper
  windows.rs    schtasks wrapper (StartWhenAvailable=true)

migration/
  mod.rs        first-launch flow per Section 9
  sqlite.rs     copy old DB
  creds.rs      platform-dispatched cred copy (macOS / Windows)

branding.rs     central constants: PRODUCT_NAME, TAURI_BUNDLE_ID,
                PROJECT_DIRS_QUALIFIER/ORG/APPLICATION,
                CRED_SERVICE_PREFIX, GITHUB_REPO_PATH
cli.rs          --tick (dispatcher entry), --migrate (manual rerun)
```

**Reuse, don't duplicate:** `process_detection.rs` already exists in `src-tauri/src/` — `migration/mod.rs` calls into it for the "quit running claude-limits" step rather than reimplementing process enumeration.

### DB lock handling for `--tick` (resolves the headless-vs-GUI conflict)

The current `Db::open` in `store/mod.rs:25-40` acquires `try_lock_exclusive()` on `claude-monitor.lock` for process lifetime. This was belt-and-suspenders against two concurrent GUI instances corrupting the DB; SQLite's WAL mode (`schema.sql:3`) already serializes writes at the SQL layer.

Add a new constructor that skips the file lock for headless dispatcher mode:

```rust
impl Db {
    /// Existing: GUI mode. Holds the exclusive file lock.
    pub fn open(dir: &Path) -> Result<Self> { /* unchanged */ }

    /// New: headless --tick mode. Opens the DB without the file lock,
    /// relying on SQLite WAL + the transactional dedup UPDATE in
    /// `scheduler::claim` for correctness.
    pub fn open_for_tick(dir: &Path) -> Result<Self> {
        // Same as open(), minus the try_lock_exclusive() call.
    }
}
```

`cli::run_tick()` calls `Db::open_for_tick`; `lib.rs::run()` (GUI path) keeps calling `Db::open`. The two coexist cleanly: at most one GUI holds the lock, and any number of `--tick` invocations can run alongside it without contention.

### Modified Rust surfaces

- `main.rs` — routes CLI args to `cli.rs` before normal Tauri startup. `claude-switchboard --tick` runs the dispatcher headlessly and exits without showing UI.
- `lib.rs` — read `default_dir()` only after `branding.rs` is initialized (so the `ProjectDirs` qualifier/org/app match the rebrand).
- `store/mod.rs` — adds `Db::open_for_tick` (no file lock) and renames the lockfile per the rename table; updates `default_dir()` to read from `branding.rs`.
- `app_state.rs` — adds `WarmupState` and per-account `last_warmup_at` cache.
- `commands.rs` — new Tauri commands: `warmup_account`, `set_schedule`, `grant_warmup_consent`, `revoke_warmup_consent`, `os_scheduler_register`, `os_scheduler_unregister`.
- `process_detection.rs` — extended (if needed) to identify the legacy `Claude Limits.app` process for migration step 2.
- `store/migrations/` — new SQL file `0004_warmup.sql` (numbering follows the existing `0002_*`, `0003_*` convention). Adds three columns to **`accounts`** (NOT a new `slots` table — the codebase uses `accounts` keyed by id, with slots being an in-memory `u32` wrapper in `auth/accounts/manager.rs`):
  ```sql
  ALTER TABLE accounts ADD COLUMN warmup_enabled INTEGER NOT NULL DEFAULT 0;
  ALTER TABLE accounts ADD COLUMN schedule TEXT NOT NULL DEFAULT '{"type":"Off"}';
  ALTER TABLE accounts ADD COLUMN last_warmup_at INTEGER;
  ```
  Plus row inserts in the existing key/value `settings` table — see §13.

### New React modules (in `src/`)

```
components/AccountsPanel/
  WarmupToggle.tsx        on/off per slot
  ScheduleSelector.tsx    Off | Every5h | Custom UI
  WarmupNowButton.tsx     one-shot trigger

components/modals/
  WarmupConsentModal.tsx     first-enable consent (Section 8)
  WelcomeToSwitchboard.tsx   post-migration dialog (Section 9)

components/settings/
  WarmupSettings.tsx      global revoke

lib/branding.ts           central brand constants (mirror of Rust)
```

### Modified React surfaces

- `components/AccountsPanel/SlotCard.tsx` — adds the three warm-up controls.
- `stores/slotsStore.ts` (Zustand) — adds `warmup_enabled`, `schedule`, `last_warmup_at` to slot state.
- `lib/generated/bindings.ts` — picks up new Tauri commands automatically (regenerated).
- All existing components — replace any hard-coded "Claude Limits" string with `PRODUCT_NAME` from `lib/branding.ts`.

### Design principles enforced

1. **Branding centralized.** Every "Claude Switchboard" / `com.claude-switchboard.app` / `ProjectDirs("com", "claude-switchboard", "ClaudeSwitchboard")` reference reads from `branding.{rs,ts}`. No hard-coded strings scattered across files. A future rename touches a handful of constants in two files.
2. **Schedulers share a dispatcher.** `scheduler::tick()` is the only place that fires warm-ups. OS-level (launchd → CLI `--tick`) and in-app (tokio task) both call it. One code path, one place to debug, one set of tests.
3. **CLI mode is in the same binary.** Tauri app detects CLI args before initializing the GUI. launchd/Task Scheduler invoke `claude-switchboard --tick`. One binary to sign, ship, maintain.

## 11. Testing strategy

### Unit (Rust)

- `warmup::api_call` — mock HTTP client; verify request shape, header construction, all 5 failure-mode response codes route correctly.
- `scheduler::dedup` — table-driven: pairs of `(last_warmup_at, now)` resolve to fire / skip correctly at the 60s boundary.
- `scheduler::catchup` — synthetic `slots` × `schedules` × elapsed-time inputs verify "exactly one catch-up per slot per launch".
- `scheduler::presets` — Off / Every5h / Custom serde round-trip.
- `os_scheduler::macos` — plist generation given a known install path; no actual `launchctl` invocation in unit tests.
- `migration::sqlite` and `migration::keychain` — fixture-based; an old-install-shaped tmpdir migrates correctly.

### Integration (Rust)

- End-to-end `--tick` mode in CI: spin up SQLite fixture, mocked HTTP server, invoke `claude-switchboard --tick`, verify warm-up fires and `last_warmup_at` updates.
- Migration smoke: build a v0.3.x-shaped fixture, run v1.0.0 first-launch, verify all 7 migration steps complete (per §9).
- Concurrency smoke: hold a `Db::open` lock from a fixture process, then invoke `claude-switchboard --tick` from a second process — verify it opens via `Db::open_for_tick`, performs the transactional claim, and exits cleanly without lock contention.

### Manual / smoke

- macOS: enable schedule, verify launchd plist appears, kill app, wait 1+ minute, verify `last_warmup_at` advances in SQLite.
- Windows: same with `schtasks /Query` confirming the task.
- Real account warm-up against staging slot — verify `reset_at` advances after a 200 response.
- First-launch consent modal — verify per-slot toggle remains off after consent until user flips it.
- Migration from real v0.3.x install on a fresh machine.

## 12. Open questions / future work

- **Per-day quiet hours** (e.g. "no warm-ups between 23:00 and 06:00"). Trivial to add later as a slot-level field; deferred until users ask.
- **Smart routing** (auto-pick the freshest account when launching Claude Code). Out of scope; would require hooking Claude Code's launch.
- **Aggregate health view** ("3 of 5 accounts warm right now"). Could be a small banner in the popover; deferred.
- **Catch-up policy variants** (e.g. "warm twice if more than 10 hours missed"). Single-fire-per-launch is the v1 simplification.
- **Cleanup of old `com.claude-limits.*` data** after long-stable v1.x — design later (~3 months out).

## 13. Appendix: data model deltas

New migration file: `src-tauri/src/store/migrations/0004_warmup.sql` (follows existing `0002_*`, `0003_*` numbering).

```sql
-- New columns on the existing `accounts` table.
-- (The codebase has no `slots` table; slots are an in-memory u32 wrapper
-- around accounts in src-tauri/src/auth/accounts/manager.rs.)
-- SQLite uses INTEGER for booleans (0 / 1).
ALTER TABLE accounts ADD COLUMN warmup_enabled INTEGER NOT NULL DEFAULT 0;
ALTER TABLE accounts ADD COLUMN schedule        TEXT    NOT NULL DEFAULT '{"type":"Off"}';
ALTER TABLE accounts ADD COLUMN last_warmup_at  INTEGER;  -- unix epoch seconds, NULL = never

-- New rows in the existing key/value `settings` table (schema.sql:68-71).
-- Settings is key/value, not column-per-flag — these are INSERTs, not ALTERs.
INSERT INTO settings (key, value) VALUES ('warmup_consent_granted', '0')
  ON CONFLICT (key) DO NOTHING;
INSERT INTO settings (key, value) VALUES ('migration_completed', '0')
  ON CONFLICT (key) DO NOTHING;
```

The transactional claim used by `scheduler::claim` (§7):

```sql
UPDATE accounts
SET last_warmup_at = :now
WHERE id = :account_id
  AND warmup_enabled = 1
  AND (last_warmup_at IS NULL OR last_warmup_at < :now - 60);
-- Caller fires the warm-up only if rows_affected == 1.
```
