# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **Settings save themselves.** The Save button is gone. Every setting applies as you change it, the way Appearance, live usage and warm-up already did; before, a change made higher up was lost if you left without scrolling down to Save. A slider is written once, when you stop dragging. If a save fails, a bar at the bottom of Settings says why and offers Retry.
- **New colours for all five themes.** Surfaces are now a near-white with only a hint of the theme's hue, and a near-neutral charcoal in dark mode, where they used to be visibly tinted (beige, brown, sage, mauve). Graphite is true grey with an ink accent, so the only colour in it is the amber and red of a limit. Tide is now an ocean blue; it was a teal close to the one that marks non-Anthropic models. Moss and Plum have cleaner accents.
- **Settings wording.** The notification levels are named "Warning at" and "Critical at" and say that they also colour the usage bars. Background warm-up is a switch. Controls that are off because their feature is off are dimmed.

### Fixed

- **The scrollbar was close to invisible.** It was a 5 px thumb in the border colour on the window's edge, where Windows draws its border and rounds the corners over it. It is now wider, darker and inset from the edge, and a scrolling Settings page fades out at the edge that has more behind it.
- **The Switch button of the switch-account dialog could be cut off at a Windows text size above 100%.** The dialog scrolled as one block when the window was short. Its title and buttons now stay in place and only the details scroll.
- **Settings in the expanded window could be taller than the window.** The dialog had a fixed 480 px body; it now stops at the window.
- **An empty card under Polling** when there was no Claude Code setup to offer live usage for.

## v1.5.1 — 2026-11-12

### Added

- **Warm-up runs through Claude Code.** A direct request to the messages API is sometimes refused for a subscription token (a bare HTTP 429 on an account reading 0% of its 5-hour window). Warm-up now runs Claude Code headless (`claude -p` on Haiku) in a throwaway empty config folder, with the token passed only through the child's environment and anything that could sign it in as someone else removed. It falls back to the direct request only when Claude Code is missing or cannot start; if Claude Code ran and failed, that failure is what you see.
- **Why a warm-up failed is now visible.** A 429 that carries none of the usage-limit headers is reported as "Anthropic refused the request" with its status and request id, not "Already at rate limit". The log records the status, the rate-limit headers and the start of the body of any refused warm-up (never credentials), and, for each account, which session's status-line reading is being used or why none is.
- **Live usage from Claude Code's status line now works on Windows.** The switch under Settings → Polling → **Live usage from Claude Code** was hidden on Windows in 1.4 and 1.5 because the Windows side had never been run. It now is: Switchboard runs the bridge in every shell Claude Code could use there (Git Bash, PowerShell 7, Windows PowerShell) before it changes `settings.json`, installs the first spelling of the bridge's path that all of them accept (a plain forward-slash path, else the 8.3 short path, else `~/…`), and refuses, naming the shell, if one cannot run it. The bridge replaces itself on update even while Claude Code is running it. The bridge hands your command to the shell with the same quoting Claude Code uses, so a command like `"true"` (quotes, no spaces) behaves as it does without the bridge. The Windows uninstaller does not restore your original status line; the README has the one-line PowerShell command to do that by hand. Verified on Windows 11; see the README for what was not tested.

### Fixed

- **The compact popover could be cut off at a Windows text size above 100%.** WebView2 applies the system text-size setting as page zoom, so the window hands the webview fewer CSS pixels than the app asks for; the fixed 360 px compact shell then painted past the right edge and clipped the account badges and percentages. The shell now fills the window and caps at the design size, so it reflows instead of clipping.
- **One email address in two organizations is two accounts.** A Team seat and a personal login under the same address share an Anthropic account id, and the app treated them as one: signing in with the second replaced the first, and each would also have been shown the other's usage. Accounts are now told apart by organization as well, so both are kept, with their own tokens, limits and warm-up. The first one keeps its existing id; the second is stored as `<id>@<organization>`. When an address is saved twice, each row shows its organization's name under the address. Nothing is read from Claude Code's shared usage cache for such an address, because that cache names only the account, not which organization it was for.
- **The expanded window could be cut off on Windows.** The window animates inside a fixed 960 × 640 box, and when Windows gave the window less than that (for instance across monitors with different display scaling) the right and bottom edges of the report were clipped, where macOS simply reflows. The box is now never larger than the window.
- **A strip of scrolling rows showed above the day heading on the Cost tab.** The heading stopped as far below the header as the scroll area's top padding, and rows passing under it were visible in that gap. It now sits flush under the header.
- **"Try again" on a warm-up was refused for 30 minutes after a failure.** The back-off after a failed warm-up exists to stop the scheduler hammering an account; it also blocked a deliberate click, which then said "already attempted recently or waiting to retry". A manual warm-up now ignores the back-off. It is still refused while another warm-up for that account is running or started less than a minute ago, and says so.
- **A warm-up result the row did not recognise took the whole account row down.** An unknown outcome now shows "Warm-up failed".
- **Installing the status-line bridge could fail on a busy Windows machine.** The first launch of a freshly copied program queues behind the system's scanner and can take several seconds; the check for it now waits up to 30 seconds instead of 5, and moving it into place retries for about 5 seconds.

## v1.5.0 — 2026-11-11

### Added

- **Fable weekly limit.** Max and Ultra accounts have a separate weekly cap for Fable. It now sits beside 5H and 7D in the popover, on each account row and in the tray tooltip, and it fires the same threshold notifications as the 5-hour and 7-day windows. Its reset countdown is shown only when it differs from the weekly one.
- **Colour themes.** Settings → Appearance now offers five themes (Terracotta, Graphite, Tide, Moss and Plum), each in light and dark. Terracotta stays the default. Warning and danger colours are the same in every theme.
- **Ultra plan.** The $200 tier is named Ultra everywhere. Ultra has no 5-hour window, so the 5h column is hidden for accounts that report none and the weekly bar takes the full width.
- **Fable 5.2 and Mythos 5.2 pricing.** Both are priced at $8 in / $40 out per million tokens and counted as 1M-context models; stored session costs are repriced once on first launch. The OpenRouter preset defaults to Fable 5.2 and warm-up pings Haiku 5.5.

### Changed

- **Five tabs instead of eight.** The expanded window is now Sessions, Cost, Trends, Breakdown and Providers, in one header row with the window buttons. Models, Projects and Cache are sections of the new Breakdown tab, and the heatmap is an activity calendar at the foot of Trends. The window reopens on the tab you left.
- **One range for Cost, Trends and Breakdown.** Pick 7 days, 30 days, 90 days or a year once and all three tabs follow it; each shows its total and the range it covers in the same place. Ranges are whole calendar days, so the three tabs add up to the same figure.
- **Breakdown tab.** Spend or tokens by model (with a share bar, and small models folded into "Other"), by project (top ten, with a filter and "Show all"), and four cache figures, on one page.
- **Trends.** The chart has a slot for every day of the range, switches between days and weeks, and keeps the click-a-bar breakdown. The activity calendar below it covers the last 26 weeks and can be walked with the arrow keys.
- **Cost.** Follows the shared range, shows each day's total, and pages long lists 250 rows at a time.
- **Accounts sidebar.** The Switch button is always visible on every other account, not only on hover. The sidebar can be hidden and shown from the header, and stays that way next time. When the weekly and Fable limits reset together, the reset time is shown once.
- **Popover Details.** The Details row is offered only when there is something behind it (the Opus and Sonnet split, or pay-as-you-go).
- **Returning to a tab is instant.** Each tab keeps what it last loaded and refreshes it in the background, and Sessions keeps its filters and scroll position.

### Fixed

- **Plan badge stuck on the plan you signed in with.** An account that signed in on Pro and was upgraded later kept its Pro badge, and an account added through the browser sign-in showed no plan at all. Claude Code only writes the plan into its credentials at sign-in; the plan is now read from the profile Claude Code keeps refreshing, and the active account's badge follows a plan change within a minute. An inactive account still updates the next time you switch to it.
- **Fable row on plans without Fable.** Anthropic's usage endpoint reports a Fable row for every account, so Pro accounts showed "Fable 0%". The row is now shown only on Max and Ultra, or when it carries actual usage.
- **Project session counts and order.** A project's session count was the number of API calls, not sessions, and the order of the list could change from one load to the next. Each session is now counted once, with subagent runs folded into the conversation that started them, and projects come back most expensive first.
- **Automatic rotation skipped accounts with no 5-hour window, and ignored the Fable limit.** An Ultra account, or any account whose 5-hour window had not started, was never chosen as the account to switch to. Such an account now counts as unused on that limit. Reaching the Fable weekly limit now triggers a rotation too, at the weekly threshold, and an account already at its Fable limit is not chosen.
- **Low-contrast button text in dark mode.** Text on accent-coloured buttons and tags was white on a light accent in dark mode. It now uses a dark ink that passes contrast in every theme.

## v1.4.0 — 2026-11-10

### Added

- **Clearer session costs.** Cost rows show their session titles, with the project kept in the detail line. Sessions project groups show the total cost of their visible rows, respecting filters and omitting unmeasured usage.
- **Automatic account rotation.** Opt in under Settings to switch accounts when five-hour usage reaches 96% or weekly usage reaches 98%, with adjustable thresholds and a preferred next account. Automatic selection uses fresh usage below both limits and waits when no account qualifies.
- **Warm-up activity in Settings.** See each account's next scheduled check, last successful warm-up, and latest result or error.
- **Live usage from Claude Code's status line.** Accounts added with a long-lived token (`claude setup-token`) can't poll Anthropic's usage endpoint, so their 5-hour and 7-day usage was blank even though Claude Code's own status line showed it. A new switch under Settings → Polling, **Live usage from Claude Code**, installs a tiny bridge (`switchboard-statusline`) as Claude Code's `statusLine.command`. The bridge runs your existing status line unchanged (same output, same exit code) and records the usage numbers Claude Code already hands it. Switchboard matches each record to the right account, so a swap never shows one account's numbers on another, and adopts it as the usage source. Accounts that can poll keep polling and use the status line only when it is fresher or when polling is rate-limited. Turning it off puts `settings.json` back byte for byte; if the app is gone, `~/.claude-switchboard/statusline/bridge --restore --all` does the same, and `settings.json.switchboard-statusline-anchor` keeps a copy of the file from before the first change. macOS only for now; the Windows runtime is written but not yet verified, so the switch is hidden there. See [README](README.md#live-usage-from-claude-codes-status-line) for what does not report.
- **"Waiting for Claude Code" instead of "unavailable".** With the status line bridge on, a token account that you are signed in with and that has no reading yet now says it is waiting for Claude Code activity (a reading appears about a minute after you use Claude Code with that token) rather than that usage tracking is unavailable.
- **Import long-lived OAuth tokens.** Add an account by pasting a `claude setup-token` token into a masked field on Windows or macOS, with an optional label and no browser or Claude Code process required. Token-only accounts support switching without fabricated profile metadata, clearly explain unavailable usage tracking and unknown expiry, and offer token replacement without overwriting existing browser sign-ins.

### Fixed

- **Background warm-up now runs with the app closed.** The `--tick` command uses the same runner as the desktop app, with persistent claims, missed-run catch-up, active-window checks, consent enforcement, and retry delays. Windows tasks run without console popups, allow battery operation, and refresh their executable path after app upgrades. Verified on Windows; macOS background execution still needs a native smoke test.
- **Windows subagent costs could be counted twice.** Normalize newly ingested transcript paths so subagent calls reconcile against the parent's official cost total. Also keep the gated Windows status-line helpers warning-free in production builds.

## v1.3.1 — 2026-08-04

### Fixed

- **Launching a session did nothing on macOS.** A bundled `.app` is started by launchd, which hands it a bare `PATH` (`/usr/bin:/bin:/usr/sbin:/sbin`) — every location Claude Code or the `code` CLI actually installs into (`~/.local/bin`, `/opt/homebrew/bin`, `/usr/local/bin`, `~/.claude/local`, VS Code's own bin) was outside it, so the launcher couldn't find either binary and no session could start. Windows was unaffected (a GUI process there inherits the full registry PATH) and the bug was invisible in `tauri dev` (which inherits a real terminal PATH), which is how it reached a release. Resolution now checks the inherited PATH, then the locations these tools actually install into, then falls back to the login shell's own PATH for nvm/asdf/fnm installs.

## v1.3.0 — 2026-07-31

### Added

- **Custom model providers.** Run Claude Code against third-party Anthropic-compatible endpoints (GLM, Kimi, DeepSeek, MiniMax, OpenRouter, or a custom URL) by launching provider-scoped terminal sessions from the new Providers tab. Sessions use per-process environment variables, so several providers can run at once and your own launch scripts keep working.
- **Optional global default.** A provider can additionally be set as the default for bare `claude` invocations. This writes `~/.claude/settings.json` and is guarded by a backup, an undo manifest and a confirmation prompt — it overrides `ANTHROPIC_*` variables exported by your shell.
- **Session browser.** A new Sessions tab lists past Claude Code sessions grouped by project, each expanding to show Claude Code's own end-of-session recap, what you asked, where you left off, and which files it touched. One click resumes any session in a new terminal running the provider it originally used — resuming always forks, so a session still open elsewhere is never disturbed. The previous Sessions tab, which reports tokens and cost, is now called **Cost**.

### Fixed

- **Two accounts could show identical usage.** The statusline daemon's shared snapshot (`~/.claude/statusline-usage.json`) carries no account identity, so after the live account changed — a swap, an external `cswap`, or a Claude Code / cowork instance started on another machine — the previous account's numbers were adopted for the new active account. Both rows then showed the same percentages and reset times for up to one polling interval. Snapshots written before the active account last changed are now ignored.
- **Subagent usage was under-counted.** The startup backfill skipped subagent transcripts that the live watcher already collected, so sessions run while the app was closed never had their subagent API calls counted. Existing data is re-ingested automatically on upgrade.

## v1.2.0 — 2026-07-27

### Added

- **Trends tab: click a day to see its per-model breakdown.** Clicking any bar reveals that day's token, cost, and cache breakdown per model, sorted by cost — the whole window's per-day/per-model data loads once alongside the chart, so opening the breakdown is instant with no extra fetch.

### Changed

- **Sessions tab groups subagent transcripts under their parent conversation.** Task/Agent-tool subagent runs used to show up as separate top-level rows; they now roll into the conversation that spawned them, as an indented row per subagent, with their usage counted toward the parent's totals. Model badges also read more clearly: Anthropic tiers collapse to a short name (opus/sonnet/haiku/fable), and third-party/relay models keep a cleaned version of their real name instead of a generic label. Headless or scheduled invocations (no working directory) are labeled "headless" and visually demoted instead of showing a bare, broken-looking "-".
- **Per-model 7-day usage only renders when the account's plan actually reports it.** Pro and Team-Pro plans share a single combined 7-day quota and return null for both the Opus and Sonnet sub-buckets — those rows no longer render empty on plans that don't provide the split.

### Fixed

- **Third-party relay models (GLM, k3, MiniMax, kimi) could show inflated cost and token totals — up to 2-8x.** These relays never write a `requestId`, and write each response's usage across multiple JSONL lines (one per content block); the dedup key fell back to a per-line identifier, so every duplicate line was counted separately. Session history now dedupes on the response's own `message.id` when `requestId` is absent, and existing history is automatically re-ingested once on first launch after this update so past totals correct themselves.
- **Several tray-repositioning call sites could still crash the app.** `move_window(Position::TrayCenter)` panics inside the positioner plugin when the window isn't on any monitor yet — e.g. shown before being placed, or mid display-reconfiguration — and under this app's `panic = "abort"` release profile that takes down the whole process, not just the one window. v1.1.3 closed this for the cold-launch popover-sizing path specifically; window resize, relaunching while already running, the tray menu's Show item, and clicking the tray icon could all still hit it. All four now go through one helper that checks for a monitor first and falls back to a safe screen position, same as the existing first-run fallback.

## v1.1.3 — 2026-07-24

### Fixed

- **App could crash immediately on launch.** On a cold start, the popover could call into the tray-positioning logic before the tray icon had ever reported its on-screen location. That path panicked, and since release builds run with `panic = "abort"`, the panic took down the entire app instead of just failing quietly. The window is now sized correctly immediately and re-anchors to the tray as soon as its position is known — normally on the very next launch or the first click.
- **Wrong bundle identifier on macOS.** The committed `Info.plist` still shipped `com.claude-limits.app` — a leftover from the pre-rename project name — while every other part of the app expected `com.claude-switchboard.app`.

### Changed

- **Expanded view drops a redundant usage summary.** The condensed 5h/7d + Opus/Sonnet + pay-as-you-go readout at the top of the expanded report duplicated the accounts sidebar and cost significant vertical space above the tabs.

## v1.1.2 — 2026-07-23 (not published — see v1.1.3)

### Fixed

- **Windows release build was still broken in v1.1.1.** CI's own artifact-upload workaround (added for an older, buggier `tauri-action`) looked for a `.nsis.zip` updater bundle that current Tauri versions no longer produce — Tauri now signs the NSIS `.exe` directly. The workaround failed on every Windows run looking for a file that would never exist, even though the underlying build was fine. Removed it; `tauri-action` now handles updater artifacts and the `latest.json` manifest natively on both platforms.

## v1.1.1 — 2026-07-23 (not published — see v1.1.2)

### Fixed

- **Auto-updater signing was broken in v1.1.0.** The release build failed on both macOS and Windows at the artifact-signing step because the CI signing key was corrupted; no v1.1.0 binaries were ever published. This release rotates to a fresh signing keypair and ships the working auto-updater, pricing updates, and fixes originally intended for v1.1.0 (see below).

## v1.1.0 — 2026-07-23 (not published — see v1.1.2)

### Added

- **Auto-updater is now fully wired.** The bundler produces signed updater artifacts (`.app.tar.gz` + `.sig` on macOS, `.nsis.zip` + `.sig` on Windows) and the release workflow uploads them explicitly, fixing a tauri-action quirk that dropped the `.sig` files.
- **Support for the latest Anthropic models:** Fable 5, Mythos 5, Sonnet 5, and Opus 4.8, with current per-token and cache pricing.
- **Third-party relay pricing:** MiniMax M2.7, GLM 5.1, and Kimi K3 are now costed in session history.
- **Historical cost re-computation.** On first launch after this update, Switchboard re-prices past session events using the new table, so reports reflect correct costs for previously unknown or corrected models.
- **Shared usage snapshot.** When the statusline daemon has already polled the active account’s `/usage` endpoint, Switchboard adopts that fresh snapshot instead of competing for the same rate-limit budget.
- **Startup hydration.** Last-known-good usage data is restored from the local database on launch, so the popover no longer flashes “usage unavailable” while the first poll is in flight.
- **MIT license.**

### Changed

- **Compact popover redesign.** The default glance view is now 208 px tall and shows only the 5 h and 7 d hero numbers plus a Details disclosure; expanding reveals the Opus/Sonnet split and pay-as-you-go row.
- **Reset countdowns now sit beside their bucket labels** instead of on a separate caption row, freeing vertical space.
- **Warm-up controls are collapsed by default** in each account row; the one-line summary shows Off / On · manual / Every 5 h / Custom (n).
- **Manual refresh is faster across all accounts:** the stagger gap drops from 30 s to 5 s for a user-initiated refresh.
- **README install section** now points to the releases page and explains first-launch quarantine / SmartScreen steps.
- The `Unreleased` comparison link now points to the `claude-switchboard` repository.

### Fixed

- **“Usage unavailable” on transient failures.** Cached usage numbers now stay visible during a 429 or network hiccup, with a stale hint like “rate-limited (429) — showing last good data from 3m ago.”
- **429 backoff escalation.** `Retry-After: 0` no longer triggers ever-increasing exponential backoff; Switchboard retries at the next scheduled poll instead of backing off for minutes.
- **Refresh spinner never stopping.** The spinner now stops on the first `usage_updated` event or a 10 s cap.
- **Popover detached from the menu bar** when expanding Details or re-showing the window. Compact modes now keep the top edge glued to the tray.
- **Cache-savings calculation** is now per-model instead of a Sonnet-only flat rate, so Opus and Fable sessions report accurate savings.

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

[Unreleased]: https://github.com/FeiXu-1131372/claude-switchboard/compare/v1.5.1...HEAD
[1.5.1]: https://github.com/FeiXu-1131372/claude-switchboard/compare/v1.5.0...v1.5.1
[1.5.0]: https://github.com/FeiXu-1131372/claude-switchboard/compare/v1.4.0...v1.5.0
[1.4.0]: https://github.com/FeiXu-1131372/claude-switchboard/compare/v1.3.1...v1.4.0

[1.3.1]: https://github.com/FeiXu-1131372/claude-switchboard/compare/v1.3.0...v1.3.1
[1.3.0]: https://github.com/FeiXu-1131372/claude-switchboard/compare/v1.2.0...v1.3.0
[1.2.0]: https://github.com/FeiXu-1131372/claude-switchboard/compare/v1.1.3...v1.2.0
[1.1.0]: https://github.com/FeiXu-1131372/claude-switchboard/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/FeiXu-1131372/claude-switchboard/releases/tag/v1.0.0
[0.3.0]: https://github.com/FeiXu-1131372/claude-limits/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/FeiXu-1131372/claude-limits/releases/tag/v0.2.0
