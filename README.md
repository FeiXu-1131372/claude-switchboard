# Claude Switchboard

A menu-bar utility that manages multiple Claude subscriptions and tracks rate-limit usage — on macOS and Windows, with the same designed experience.

> Multi-account control plane: observe, swap, and (soon) schedule warm-ups across every Claude account you sign in to.

The tray icon shows your active account's live 5-hour percentage as a ring badge; click for the compact popover with all accounts side-by-side; one click swaps which account Claude Code uses.

## Screenshots

### Windows 11

The tray icon shows the live 5-hour percentage as a ring badge — the answer to "where am I at?" is visible without opening anything. Hover for a one-line text summary; click for the full popover.

<p align="center">
  <img src="docs/screenshots/windows-tray.png" alt="System tray with live percentage badge" width="320" />
</p>

<p align="center">
  <img src="docs/screenshots/windows-tray-tooltip.png" alt="Tray tooltip on hover — 5h and 7d percentages with reset times" width="480" />
</p>

<p align="center">
  <img src="docs/screenshots/windows-popover.png" alt="Compact popover — 5H and 7D buckets, per-model usage, pay-as-you-go" width="420" />
</p>

Click the expand arrow for the full report — Sessions, Models, Trends, Projects, Heatmap, and Cache tabs.

<p align="center">
  <img src="docs/screenshots/windows-expanded-models.png" alt="Expanded report — Models tab with per-model token breakdown" width="720" />
</p>

<p align="center">
  <img src="docs/screenshots/windows-expanded-heatmap.png" alt="Expanded report — Heatmap tab showing six months of usage" width="720" />
</p>

### macOS

Same layout, rendered with native vibrancy. The tray icon shows the same live ring badge in the menu bar; hover for a one-line summary, click for the popover.

<p align="center">
  <img src="docs/screenshots/macos-tray.png" alt="Menu bar status item with live percentage badge" width="320" />
</p>

<p align="center">
  <img src="docs/screenshots/macos-tray-tooltip.png" alt="Menu bar tooltip on hover — 5h and 7d percentages with reset times" width="480" />
</p>

<p align="center">
  <img src="docs/screenshots/macos-popover.png" alt="Compact popover — 5H and 7D buckets, per-model usage, pay-as-you-go" width="420" />
</p>

Click the expand arrow for the full report — Sessions, Models, Trends, Projects, Heatmap, and Cache tabs.

<p align="center">
  <img src="docs/screenshots/macos-expanded-models.png" alt="Expanded report — Models tab with per-model token breakdown" width="720" />
</p>

<p align="center">
  <img src="docs/screenshots/macos-expanded-cache.png" alt="Expanded report — Cache tab with hit rate and savings" width="720" />
</p>

## What makes it different

Most Claude usage trackers fall into one of two shapes — CLI tools you have to remember to run (`ccusage`, terminal monitors), or stock menu-bar widgets with a percent number and not much else. Claude Switchboard is the third shape: a designed app with both visual craft and analytical depth.

- **Glance, don't query.** The ring badge on the menu bar is the answer. No window to open, no command to run — your 5-hour utilization is always in peripheral vision.
- **Native feel, not native boring.** macOS vibrancy and Windows 11 Mica (acrylic on Win10), system fonts, monospace numerics, springs not easings. The reference is macOS Control Center and Raycast, not stock SwiftUI. Every color, radius, spacing, and animation comes from one token set.
- **Real analytics depth.** Six tabs in the expanded report — Sessions, Models, Trends, Projects, Heatmap, and Cache — sourced from your local Claude Code transcripts. Not a single bar with a percent on it.
- **Cross-platform parity.** Same layout, same interactions, same design language on macOS and Windows 10/11. Rare in this category — most native menu-bar apps are macOS-only.
- **Tier-aware cost math.** Sonnet 4's 1M-context tier (rates double above 200k input) and the 5-minute / 1-hour cache write split are calculated correctly, not approximated.

## Features

- **Live tray badge** — 5-hour percentage as a ring around the menu-bar icon, refreshed at your configured interval. Pulled from Anthropic's official usage endpoint — the same numbers their console shows.
- **Multi-account, one-click swap** — manage every Claude account you sign in to in the same popover, see all their 5H/7D bars side-by-side, and switch which one Claude Code uses with a single click. Running CLI / VS Code sessions adopt the new account within ~30 seconds — no restart required.
- **Hover summary** — 5h and 7d percentages with reset times in a single line, without clicking.
- **Compact popover** — 5H / 7D buckets, per-model bars (Opus, Sonnet), and pay-as-you-go credits when enabled.
- **Expanded report** — Sessions, Models, Trends, Projects, Heatmap, and Cache tabs, sourced from local Claude Code JSONL transcripts.
- **Burn-rate projection** — extrapolates your current pace and shows where utilization will land at reset, color-cued against your threshold.
- **Threshold notifications** — warn / danger levels you choose; one alert per bucket cycle.
- **Tier-aware cost** — Sonnet 4's 1M-context tier and 5-minute / 1-hour cache write split, calculated correctly.
- **Cross-platform** — macOS (vibrancy) and Windows 10/11 (Mica / acrylic), same design.

## Install

No signed release yet — build from source:

```bash
pnpm install
pnpm tauri dev
```

When binaries ship, first-launch notes for unsigned apps:

- **macOS:** `xattr -d com.apple.quarantine "/Applications/Claude Switchboard.app"` or right-click → Open from Finder.
- **Windows:** SmartScreen → "More info" → "Run anyway". WebView2 is required on Windows 10 (Windows 11 ships it).

## Updates

Claude Switchboard checks for new versions automatically — on launch and every 6 hours
while running. When a new version is downloaded and ready, a small banner appears
at the top of the popover with an **Install & restart** button. Click it to upgrade.

You can also trigger a check manually from the popover footer ("Check for updates")
or the tray menu ("Check for Updates…").

**Important:** Auto-update was added in **v0.2.0**. If you're upgrading from v0.1.x,
you'll need to download and install v0.2.0 manually from the
[releases page](https://github.com/FeiXu-1131372/claude-switchboard/releases) — the
v0.1.x build has no updater wired up. Every release after v0.2.0 will auto-update.

The app is unsigned (no paid Apple Developer ID / Windows EV cert), so the *first
install* on a new machine still goes through the OS-specific first-launch flow
described above. After the first install, all updates are silent — Gatekeeper /
SmartScreen don't re-check signed-by-the-same-developer apps on update.

Update integrity: every release artifact is signed with our ed25519 updater key,
and the app refuses any update whose signature doesn't match the public key
embedded at build time.

## Authentication & multi-account

Claude Switchboard can manage as many Claude accounts as you have. Each account lives in its own slot with its own polled usage; the **Accounts** sub-screen shows all of them stacked, with the currently-active one highlighted. Every slot has a **Switch account** button — click it to make Claude Code (CLI and the VS Code extension) point at that account.

Two ways to add an account:

1. **Import the upstream login** — sign in with `claude login` first, then in claude-switchboard hit "Use upstream's current login". Captures the live credentials in one click.
2. **OAuth in-app** — click "Sign in with Claude" to start a local-redirect OAuth flow. Useful for adding accounts you haven't logged into via `claude` yet.

The app never logs in on your behalf. It only ever reads tokens that your OS already holds and uses them against `api.anthropic.com`.

**Hot reload, not restart.** A swap rewrites the OS-level credentials atomically — running CC sessions pick up the new account within ~30 seconds (CC's own keychain cache TTL on macOS, one API tick on Windows). A 60-second `KeychainGuardian` covers the narrow race where a CC process started its OAuth refresh just before the swap.

## Warm-up & scheduling

Switchboard can deliberately start an account's 5-hour rolling window so its
reset time is known and predictable — useful when rotating across multiple
accounts. **Off by default per account; strictly opt-in.**

The first time you enable warm-up on any account, a one-time consent dialog
explains exactly what gets sent. Per-account toggle and global revoke are
available in Settings at any time.

### How it works

A warm-up sends a 1-token Haiku request to `api.anthropic.com/v1/messages`
using the account's existing OAuth credentials — the same surface Claude
Code uses. Cost: rounding-error against your subscription
(≈$0.000007/fire, ≈$0.013/year per account on a 5-fires-a-day cadence).

If the account already has an active 5-hour window from your normal coding,
warm-up is a no-op — it skips the HTTP call entirely.

### Schedules

Three presets cover the realistic cases:
- **Off** — manual only.
- **Every 5h** — pick an anchor (e.g. 06:00) and the app fires at 06:00,
  11:00, 16:00, 21:00.
- **Custom** — explicit list of `HH:MM` times.

Schedules fire via the OS (launchd on macOS, Task Scheduler on Windows) so
they work even when the app is closed. If you'd rather not register an OS
agent, the in-app scheduler still fires while the app is open.

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

## Privacy

- All data stays on your machine. Usage history is in SQLite at `~/Library/Application Support/com.claude-switchboard.ClaudeSwitchboard/data.db` (macOS) or the platform equivalent on Windows.
- The only outbound traffic is to Anthropic's official API.
- No telemetry, no analytics, no third-party services.
- **Opt-in warm-up:** With your explicit per-account consent, Switchboard
  can send 1-token warm-up messages to `/v1/messages` to start the 5-hour
  window deliberately. No other content is ever sent. Off by default;
  revocable any time from Settings.

## Stack

Tauri v2 (Rust + WebView) · React 19 · TypeScript · Tailwind CSS v4 · Framer Motion · Recharts · SQLite.

## Development

```bash
# Frontend typecheck
pnpm exec tsc --noEmit

# Backend tests (75+ unit + integration tests)
cd src-tauri && cargo test
```

## License

MIT
