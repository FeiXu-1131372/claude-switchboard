# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

## [0.3.0] — 2026-05-07

The multi-account release. Manage every Claude account you sign in to from the same popover, see them all stacked, and switch which one Claude Code uses with a single click — running CC sessions adopt the new account within ~30 seconds, no restart.

### Added

- **Multi-account support.** First-class slots for as many Claude accounts as you want. Each slot has its own polled usage, its own backoff state, and its own row in the Accounts sub-screen. Backed by `accounts.json` (mode-0600, owner-ACL'd, file-locked, atomic-write) and a per-slot in-memory cache.
- **Accounts sub-screen.** Stacked rows for every managed account with 5h/7d bars and reset countdowns. The currently-active slot is rendered with an accent-tinted background and 3px left border. Inactive rows show a hover-revealed **Switch account** button. Plan badges (MAX / PRO / etc.) inline with the email.
- **One-click account swap.** `swap_to_account` writes the target's CC creds to the OS Keychain (macOS) or `~/.claude/.credentials.json` (Windows), splices the matching `oauthAccount` slice into `~/.claude.json`, and reconciles `state.active_slot` eagerly so the UI reflects the new active without waiting on a poll tick. Two-step transaction with rollback: if the global-config write fails, the credential write is reverted.
- **Hot-reload after swap.** Running `claude` CLI sessions and the VS Code extension adopt the new account within ~30 seconds (CC's own in-process keychain cache TTL on macOS, one API tick on Windows). A 60-second `KeychainGuardian` polls every 2s and re-applies the swap target if a stale in-flight OAuth refresh from the previous account writes back rotated tokens.
- **Add-account chooser.** Two paths: "Use upstream's current login" (imports the live `claude` creds in one click) and "Sign in with Claude" (in-app OAuth via local redirect server with PKCE). Idempotent on `accountUuid` — re-adding refreshes the stored blobs in place.
- **Throttled stagger polling.** Replaces the previous parallel fan-out. The poll loop maintains a per-slot `next_poll_at` schedule, picks at most one due slot per tick, and staggers slots by 30 seconds (compressed automatically when the configured interval can't fit `slots × 30s`). The active slot polls first; previously-active and other inactive slots trail at fixed offsets. Schedule is re-seeded on swap/add/remove and respects per-slot 429 backoff.
- **Refresh scope.** `forceRefresh` (and the underlying `force_refresh` command) takes a scope argument: `"active"` (popover home refresh icon, fast path, only re-fetches the current slot) or `"all"` (Accounts panel refresh button, kicks off a staggered round across every slot).
- **Process-detection hint on swap.** The swap-confirm card surfaces how many CLI processes / VS Code windows are currently running CC, with a "adopting in ~30s" hint so the user understands the hot-reload semantics.
- **Unmanaged-active banner.** When CC is logged into an account that hasn't been imported yet, the popover shows a one-tap banner offering to add it.
- **Inactive-slot token rotation.** `refresh_inactive` runs from the poll loop's `token_for_slot` path: refreshes any inactive slot whose token is within 2 minutes of expiry, persists the rotated refresh token back to `accounts.json` under the file lock. Active slot is never refreshed by us — CC owns that.
- **First-launch migration.** Existing single-account installs are automatically migrated into Slot 1 of the new multi-account store on first launch, with a `migrated_accounts` event so the UI can confirm.
- **`accounts_changed` reconciliation event.** The poll loop emits `accounts_changed` whenever `state.active_slot` transitions, so the UI's `is_active` flags stay correct even when the active account changes outside an in-app swap (e.g., `claude login` from the terminal).

### Changed

- **OAuth flow now uses a local redirect server** instead of paste-back. Callback lands on `http://127.0.0.1:<port>/callback`, the listener consumes the code, exchanges it for a token, and shuts down. Far less chance of paste corruption.
- **Tray badge / popover header now reflect the active slot's email.** Header shows the active email; clicking it opens the Accounts sub-screen.
- **Empty-state routing.** With zero managed accounts the app routes straight to AuthPanel — covers both first-run and the post-`claude-login`-but-not-yet-imported case.
- **`@tauri-apps/api`** bumped to `2.11.0`.

### Fixed

- **macOS Keychain write was storing the literal string `"-"`.** `security add-generic-password -w "-"` plus a piped stdin was a misread of the CLI: `security` has no stdin-mode for `-w`, so the dash was stored verbatim and the JSON payload silently discarded. Every swap appeared to succeed but every subsequent `claude` invocation read `-` from the Keychain and showed "logged out". Now passes the JSON to `-w <payload>` directly.
- **AccountsPanel lost the active-slot highlight whenever the active account changed outside an in-app swap.** The frontend's `is_active` flags only refreshed on `init()`, in-app swap, or `migrated_accounts` events; nothing emitted `accounts_changed` from the poll loop's reconciliation. Now emitted whenever `state.active_slot` transitions.
- **In-app swap raced the poll loop's reconciliation.** `swap_to_account` now eagerly sets `state.active_slot = Some(slot)` before returning, so `list_accounts` (called immediately by the UI) sees the post-swap state without waiting on a tick.
- **`schedule_by_slot` retained entries for removed accounts.** `remove_account` now drops the entry; the poll loop also reconciles on each tick.
- **Backoff state survived a swap.** Cleared after every swap — backoff was earned by a different token and shouldn't lock out the new bearer.
- **`AuthPanel` routing for fresh logins.** Routes to AuthPanel whenever `accounts.length === 0`, regardless of `requiresSetup`. Prevents the "live CC creds exist but unimported" case from bottoming out on `LoadingShell` forever.
- Nullable `resets_at` and `utilization` in usage payloads are guarded throughout the UI.
- Popover background made fully opaque on Windows; spurious DWM shadow removed.
- Settings panel cleanups; corner radius / gitignore / clippy tidying across the multi-account merge.

### Removed

- **Single-account `token_store` and the `Conflict` auth variant.** Multi-account makes "OAuth vs CC keychain conflict" structurally impossible — each is just a different slot. `preferred_auth_source` plumbing removed.
- The legacy paste-back OAuth UI (replaced by the local-redirect flow).

### Migration notes

- **Upgrading from v0.2.0 is automatic.** On first launch the existing single-account creds are migrated into Slot 1 of the new multi-account store. No manual action required.
- **The macOS Keychain entry created by ≤ 0.3.0-rc may hold the literal string `"-"`** if you swapped accounts on an affected pre-release build. After upgrading to 0.3.0, the next swap rewrites it correctly. If your terminal still shows "logged out" after one swap, run `claude login` once to re-seed the entry — subsequent swaps will keep it correct.
- Auto-update from v0.2.0 onward is silent; only the *first* install on a new machine still triggers Gatekeeper / SmartScreen.

## [0.2.0] — 2026-04-29

First public release. See the [GitHub release notes](https://github.com/FeiXu-1131372/claude-limits/releases/tag/v0.2.0) for the full changelog — highlights:

- Token persistence moved off the OS Keychain to a mode-0600 / owner-ACL'd file (no first-launch keychain prompt that looks like malware).
- Auto-updater wired up (ed25519-signed bundles).
- Refresh-token rotation working for both OAuth and Claude Code sources.
- Settings persistence via SQLite; CSP set; corrupted-DB recovery.
- Anthropic-warm token system with native vibrancy (macOS) / Mica (Windows 11) / translucent solid (Windows 10).

[Unreleased]: https://github.com/FeiXu-1131372/claude-limits/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/FeiXu-1131372/claude-limits/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/FeiXu-1131372/claude-limits/releases/tag/v0.2.0
