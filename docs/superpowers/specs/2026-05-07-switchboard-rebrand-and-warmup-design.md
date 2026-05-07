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

A slot's complete state extends to:
- **Live usage** (existing) — 5h/7d percentages, model breakdown, cache, projection
- **Auth** (existing) — OAuth credentials in OS keychain, KeychainGuardian
- **Cadence** (new) — `warmup_enabled`, `schedule`, `last_warmup_at`

## 6. Warm-up mechanics

### Wire-level request

```
POST https://api.anthropic.com/v1/messages
Authorization: Bearer <slot's OAuth access token>
{
  "model": "claude-haiku-4-5",
  "max_tokens": 1,
  "messages": [{"role": "user", "content": "hi"}]
}
Timeout: 10s
```

**Why Haiku at 1 max-token.** The 5h rolling window is keyed to the account, not the model — any successful `/v1/messages` call starts the timer. Haiku at 1 max-token is the cheapest possible request that does the job; cost is rounding-error against any subscription.

### Side effect

Exactly one: the slot's `reset_at` becomes `now + 5h`. The poll loop picks this up on the next tick from the usage endpoint (no special-case wiring — usage endpoint is authoritative).

### Failure handling

| Response | Meaning | Action |
|---|---|---|
| 200 | Success | Set `last_warmup_at = now`, log success |
| 401 / 403 | Token expired or revoked | Mark slot needs reauth, surface in UI, skip future schedules until fixed |
| 429 | Already at limit | Slot's window is already running — log no-op, don't retry until next scheduled fire |
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
                              reads slot.schedule
                              checks last_warmup_at (60s dedup)
                              fires warm-up via Section 6 mechanics
```

Both schedulers call the same `scheduler::tick()` function. Duplicate fires within 60s are absorbed by the `last_warmup_at` dedup check. There is no other coordination.

### Schedule shape (per slot)

Stored as a tagged JSON union in SQLite:

```rust
enum Schedule {
    Off,
    Every5h { anchor: HhMm },
    Custom { times: Vec<HhMm> },
}
```

UI exposes these as three presets only — no raw cron input.

### OS-level scheduler install flow

First time the user enables any schedule:

1. App shows consent dialog: *"Schedules need to register with the OS so they fire when the app is closed. This writes a user-level launch agent (no admin / sudo needed). Continue?"*
2. macOS path: write `~/Library/LaunchAgents/com.claude-switchboard.scheduler.plist` (program `<install path>/Claude Switchboard.app/Contents/MacOS/claude-switchboard`, args `--tick`, `StartInterval 60`), then `launchctl load`.
3. Windows path: `schtasks /Create /SC MINUTE /MO 1 /TN "Claude Switchboard Tick" /TR "<install path>\\claude-switchboard.exe --tick"` — user-level, no admin.
4. If user declines: only in-app scheduler runs. Banner in popover surfaces the gap: *"Schedules only fire while the app is open. Enable OS-level scheduling →"*.

### Catch-up sweep on launch

On app start, dispatcher walks each slot:
- If `slot.schedule.last_expected_fire ∈ (last_warmup_at, now]`, fire one catch-up.
- Capped at one catch-up per slot per launch — opening the app after 3 days away gives at most one warm-up per slot.

### Edge cases

- **Account in over-limit state at fire time.** Dispatcher sees `429`, logs no-op, skips until next scheduled fire.
- **Sleep across a fire.** launchd's default behavior holds the missed fire and runs it on wake; Windows Task Scheduler with "run as soon as possible" flag.
- **App uninstall.** Uninstall step calls `launchctl unload` / `schtasks /Delete`.
- **Account removed.** `remove_account` command also removes the schedule and any pending OS-level registrations for that slot.
- **Bundle ID drift.** OS-level registration always reads the current bundle ID from `branding.rs`; if a user moves the .app bundle, re-registration is triggered on next launch.

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
│   1. Detect old install (~/Library/...          │
│      com.claude-limits.ClaudeLimits/ exists)    │
│   2. Quit any running claude-limits process     │
│      (find by macOS bundle id /                 │
│      Windows app id; SIGTERM, 5s grace)         │
│   3. Copy SQLite db to new path (don't delete)  │
│   4. Copy keychain entries (service rename;     │
│      old entries left intact for safety)        │
│   5. Import settings.json (warmup_consent       │
│      starts false regardless — Section 8)       │
│   6. Set settings.migration_completed = true    │
│   7. Show one-time "Welcome to Switchboard"     │
│      dialog summarizing what migrated           │
└──────────────────────────────────────────────────┘
```

The final v0.4.0 of claude-limits ships **a banner only** — no forced modal. It's not a security update.

**Idempotency.** Migration runs at most once per install. The `settings.migration_completed` flag (in the *new* app's SQLite) gates the entire flow — if true, skip everything in steps 1–7. This protects against re-migration if a user reinstalls v1.0.0 over an existing v1.x or restores from backup.

**Process detection.** macOS: `ps -A | grep` for the binary path under `Claude Limits.app/Contents/MacOS/`, or query `NSWorkspace.runningApplications` filtered by `bundleIdentifier == "com.claude-limits.ClaudeLimits"`. Windows: enumerate processes and match the executable name. SIGTERM (or `WM_CLOSE` on Windows) with a 5-second grace; SIGKILL only as fallback. If quit fails, abort migration with a user-actionable error: *"Couldn't quit Claude Limits automatically. Quit it manually and re-launch Switchboard to continue."*

### Surface-by-surface rename

| Surface | Old | New |
|---|---|---|
| Product name (UI, README, About) | Claude Limits | Claude Switchboard |
| GitHub repo | `FeiXu-1131372/claude-limits` | `FeiXu-1131372/claude-switchboard` (GitHub auto-redirects old URLs) |
| Bundle ID (macOS) | `com.claude-limits.ClaudeLimits` | `com.claude-switchboard.ClaudeSwitchboard` |
| App ID (Windows) | (current) | `ClaudeSwitchboard` |
| SQLite path (macOS) | `…/com.claude-limits.ClaudeLimits/data.db` | `…/com.claude-switchboard.ClaudeSwitchboard/data.db` |
| Keychain service | `claude-limits-*` | `claude-switchboard-*` |
| Updater URL | old releases endpoint | new releases endpoint |
| App icon, tray icon | current | new (Switchboard-themed; brand brief unchanged) |
| Cargo / package.json names | `claude-limits` | `claude-switchboard` |
| Tauri config bundle id | `com.claude-limits.ClaudeLimits` | `com.claude-switchboard.ClaudeSwitchboard` |

### What does NOT change

Design tokens, color palette, layout, animation curves, popover/expanded-report structure, all UI components. Visual identity is preserved — same app, new badge.

### Why "copy, not move"

Leaving old keychain entries and SQLite in place means: if migration ever breaks a user, they can still launch the old binary and recover. After ~3 months of stable v1.x, ship a small "tidy old data" button — never automatic.

## 10. File & module boundaries

### New Rust modules (in `src-tauri/src/`)

```
warmup/
  mod.rs        warmup_slot(slot_id) -> WarmupOutcome  (public API)
  api_call.rs   the /v1/messages POST per Section 6
  errors.rs     401/403/429/5xx handling per Section 6 table

scheduler/
  mod.rs        dispatcher: tick() called by both schedulers
  dedup.rs      60s last_warmup_at idempotency check
  catchup.rs    on-launch sweep (1 fire per slot per launch)
  presets.rs    Off | Every5h(anchor) | Custom(Vec<HhMm>) serde

os_scheduler/
  mod.rs        trait { register(), unregister() }
  macos.rs      launchd plist + launchctl wrapper
  windows.rs    schtasks wrapper

migration/
  mod.rs        first-launch flow per Section 9
  sqlite.rs     copy old DB
  keychain.rs   copy old keychain entries

branding.rs     central constants: PRODUCT_NAME, BUNDLE_ID, paths
cli.rs          --tick (dispatcher entry), --migrate (manual rerun)
```

### Modified Rust surfaces

- `main.rs` — routes CLI args to `cli.rs` before normal Tauri startup. `--tick` runs the dispatcher headlessly and exits without showing UI.
- `app_state.rs` — adds `WarmupState` and per-slot `last_warmup_at`.
- `commands.rs` — new Tauri commands: `warmup_slot`, `set_schedule`, `grant_warmup_consent`, `revoke_warmup_consent`, `os_scheduler_register`, `os_scheduler_unregister`.
- `db/migrations/` — new SQL: `ALTER TABLE slots ADD COLUMN warmup_enabled BOOLEAN NOT NULL DEFAULT 0, schedule TEXT NOT NULL DEFAULT '{"type":"Off"}', last_warmup_at INTEGER`; new `settings.warmup_consent_granted BOOLEAN NOT NULL DEFAULT 0`.

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

1. **Branding centralized.** Every "Claude Switchboard" / `com.claude-switchboard.ClaudeSwitchboard` reference reads from `branding.{rs,ts}`. No hard-coded strings scattered across files. A future rename touches two constants.
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
- Migration smoke: build a v0.3.x-shaped fixture, run v1.0.0 first-launch, verify all 6 migration steps complete.

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

```sql
-- New columns
ALTER TABLE slots ADD COLUMN warmup_enabled BOOLEAN NOT NULL DEFAULT 0;
ALTER TABLE slots ADD COLUMN schedule TEXT NOT NULL DEFAULT '{"type":"Off"}';
ALTER TABLE slots ADD COLUMN last_warmup_at INTEGER;  -- unix epoch seconds, NULL = never

-- New rows in settings table
INSERT INTO settings (key, value) VALUES ('warmup_consent_granted', '0')
  ON CONFLICT (key) DO NOTHING;
INSERT INTO settings (key, value) VALUES ('migration_completed', '0')
  ON CONFLICT (key) DO NOTHING;
```
