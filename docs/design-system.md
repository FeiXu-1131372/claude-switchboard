# Design System — Claude Limits

## Visual Language

Calm, premium, scannable. The interface uses a warm glassy aesthetic inspired by macOS Control Center and Raycast — translucent surfaces with soft blur, warm orange-tinted neutrals, and teal accents. Every element is designed to be understood in under one second. Numbers use monospace type for instant parsing. Color communicates status (safe / warning / danger) through a warm-shifted traffic-light palette. The design avoids decoration that doesn't serve the data.

## Color System

### Foundation

The palette is built on an Anthropic warm cream/bone foundation (cream theme, default) or a refined warm-dark base (dark theme). Surfaces use warm neutrals with an orange undertone in both themes — never cool gray, never pure black/white. The single accent is terracotta. Status color is used sparingly: most data lives in warm-neutral or terracotta; amber and coral only appear at the 75% and 90% thresholds.

### Semantic tokens

| Token | Cream | Dark | Role |
|---|---|---|---|
| `--color-bg-base` | `oklch(94% 0.008 85)` | `oklch(24% 0.014 65)` | Page/window background |
| `--color-bg-surface` | `oklch(96% 0.006 85)` | `oklch(28% 0.016 65)` | Popover canvas |
| `--color-bg-surface-hover` | `oklch(92% 0.010 80)` | `oklch(32% 0.018 65)` | Surface hover |
| `--color-bg-card` | `oklch(98% 0.004 85)` | `oklch(34% 0.022 65)` | Card backgrounds |
| `--color-bg-card-hover` | `oklch(99.5% 0.004 85)` | `oklch(38% 0.024 65)` | Card hover |
| `--color-bg-elevated` | `oklch(99% 0.003 85)` | `oklch(42% 0.026 65)` | Elevated surfaces (modal cards, popovers within popover) |
| `--color-rule` | `oklch(40% 0.015 65 / 0.06)` | `oklch(95% 0.020 65 / 0.06)` | Hairline rules |
| `--color-rule-strong` | `oklch(40% 0.015 65 / 0.14)` | `oklch(95% 0.020 65 / 0.14)` | Stronger hairlines |
| `--color-border` | `oklch(40% 0.015 65 / 0.12)` | `oklch(95% 0.020 65 / 0.20)` | Card outlines |
| `--color-border-subtle` | `oklch(40% 0.015 65 / 0.06)` | `oklch(95% 0.020 65 / 0.10)` | Subtle borders |
| `--color-border-hover` | `oklch(40% 0.015 65 / 0.22)` | `oklch(95% 0.020 65 / 0.30)` | Border on hover |
| `--color-border-focus` | `oklch(56% 0.155 38 / 0.55)` | `oklch(70% 0.140 38 / 0.50)` | Focus ring |
| `--color-text` | `oklch(22% 0.018 50)` | `oklch(96% 0.010 65 / 0.96)` | Primary text |
| `--color-text-secondary` | `oklch(38% 0.020 55)` | `oklch(86% 0.020 65 / 0.78)` | Labels, secondary |
| `--color-text-muted` | `oklch(55% 0.022 60)` | `oklch(78% 0.025 65 / 0.62)` | Timestamps, tertiary |
| `--color-accent` | `oklch(56% 0.155 38)` | `oklch(70% 0.140 38)` | Interactive, terracotta |
| `--color-accent-dim` | `oklch(56% 0.155 38 / 0.10)` | `oklch(70% 0.140 38 / 0.14)` | Hover wash, selection bg |
| `--color-accent-muted` | `oklch(56% 0.155 38 / 0.24)` | `oklch(70% 0.140 38 / 0.28)` | Subdued accent surfaces |
| `--color-warn` | `oklch(62% 0.150 55)` | `oklch(76% 0.155 60)` | ≥75% threshold |
| `--color-warn-dim` | `oklch(62% 0.150 55 / 0.10)` | `oklch(76% 0.155 60 / 0.14)` | Warn-tinted backgrounds |
| `--color-danger` | `oklch(48% 0.180 28)` | `oklch(64% 0.195 25)` | ≥90% threshold |
| `--color-danger-dim` | `oklch(48% 0.180 28 / 0.10)` | `oklch(64% 0.195 25 / 0.14)` | Danger backgrounds |
| `--color-safe` | `var(--color-accent)` (alias) | `var(--color-accent)` (alias) | Deprecated; alias kept one release, removed in the next |
| `--color-safe-dim` | `var(--color-accent-dim)` (alias) | `var(--color-accent-dim)` (alias) | Deprecated; same deprecation path as `--color-safe` |
| `--color-track` | `oklch(40% 0.015 65 / 0.10)` | `oklch(95% 0.020 65 / 0.18)` | Progress bar empty track |
| `--color-overlay` | `oklch(20% 0.010 60 / 0.35)` | `oklch(10% 0.010 65 / 0.55)` | Modal overlay |

### Status color mapping

Progress bars use a flat fill that swaps color at thresholds:

- **Safe (0–74%):** terracotta accent (`--color-accent`).
- **Warning (75–89%):** warm amber (`--color-warn`).
- **Danger (90–100%):** deep coral (`--color-danger`).

The 200ms color transition is separate from the bar-length spring (700ms). There is no longer a "green" safe state — color is reserved for actionable signals.

### Model colors

Model chips use tonal variations on the warm family:

- **Opus:** deep terracotta (`--color-model-opus`)
- **Sonnet:** clay (`--color-model-sonnet`)
- **Haiku:** pale sand (`--color-model-haiku`)

Text on each chip uses the paired `--color-model-*-text` token to maintain ≥4.5:1 contrast in both themes.

## Typography

### Font stack

- **Body:** System UI stack (`-apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif`)
- **Monospace:** JetBrains Mono for all numeric data

### Scale

| Name | Size | Weight | Line-height | Usage |
|---|---|---|---|---|
| Display | 28px | 600 | 1.1 | Hero numbers (not used in current screens) |
| Title | 15px | 600 | 1.3 | Percentage values, card headings |
| Body | 13px | 400 | 1.5 | General text, project names |
| Label | 11px | 500 | 1.4 | Section labels, tab text |
| Micro | 10px | 400 | 1.3 | Timestamps, secondary meta |

## Spacing

Based on a 4px grid. Named steps:

| Token | Value |
|---|---|
| `--space-2xs` | 2px |
| `--space-xs` | 4px |
| `--space-sm` | 8px |
| `--space-md` | 12px |
| `--space-lg` | 16px |
| `--space-xl` | 20px |
| `--space-2xl` | 24px |
| `--space-3xl` | 32px |
| `--space-4xl` | 48px |

## Radii

| Token | Value | Usage |
|---|---|---|
| `--radius-sharp` | 0 | — |
| `--radius-sm` | 6px | Buttons, badges, inputs |
| `--radius-md` | 10px | Tab bars |
| `--radius-card` | 12px | Cards |
| `--radius-lg` | 14px | Popover, windows |
| `--radius-pill` | 100px | Progress bars, badges |

## Components

### Button

Three variants: **primary** (filled teal), **ghost** (transparent), **destructive** (filled red). Two sizes: `sm` (11px text) and `md` (13px text). Always includes focus-visible ring.

### IconButton

30px square button with icon only. Uses `aria-label` for accessibility. Ghost style with hover background.

### Card

Two variants: **solid** (default glass card) and **glass** (full backdrop-filter). Optional `hover` prop for interactive cards. Border and background transitions on hover.

### ProgressBar

The primary data visualization. Accepts `value` (0–100), `warnThreshold` (default 75), `dangerThreshold` (default 90). Three sizes: `sm` (3px), `md` (5px), `lg` (8px). Gradient fill shifts at thresholds. Optional percentage label.

### UsageBar

Popover-specific: wraps ProgressBar with label, reset timer, and percentage display. Uses the same threshold-based color system.

### Tabs

Segmented control with pill background. Active tab gets card background + shadow. Tab panels slide in with spring animation.

### Toggle

Switch with label and optional description. Track uses accent color when active. Includes focus-visible ring.

### Slider

Range input with label and formatted value display. Track fill uses accent color. Thumb scales on hover/active.

### Select

Dropdown with label. Custom appearance with chevron indicator.

### Banner

Alert banner with four variants: `info`, `warning`, `error`, `stale`. Optional icon and dismiss button.

### Badge

Status pill with semantic variants: `default`, `accent`, `safe`, `warn`, `danger`, `live` (with pulse dot), `opus`, `sonnet`, `haiku`.

### EmptyState

Centered layout with icon, title, optional description, and optional action button.

## Motion

All motion uses spring physics. Linear easing is never used.

### Spring presets

- **Snappy:** `stiffness: 400, damping: 25` — for small element transitions
- **Gentle:** `stiffness: 200, damping: 20` — for tab content, card entrance
- **Bouncy:** `stiffness: 500, damping: 20` — for number tickers

### Animation patterns

| Pattern | Duration | Easing | Usage |
|---|---|---|---|
| Popover mount | 300ms | Spring (snappy) | Popover open/close |
| Tab slide | 300ms | Spring (gentle) | Tab content transition |
| Number tick | 200ms | Spring (snappy) | Percentage updates |
| Bar fill | 800ms | Spring (snappy) | Progress bar animations |
| Card entrance | staggered 50ms | Spring (gentle) | Card list loading |
| Stale pulse | 2s loop | ease-in-out | Stale data indicator |

### Reduced motion

All animations respect `prefers-reduced-motion: reduce` by falling back to 100ms durations via the CSS override in globals.css.

## Screens

### CompactPopover (360 x 420px)

The default user experience. Shown as a system tray popover.

Structure:
1. **Header:** Title "Claude" + Live/Stale badge + Refresh and Settings icon buttons
2. **Usage card:** 5h and 7d bars with labels, percentages, and reset timers
3. **Models card:** Three model chips (Opus/Sonnet/Haiku) with percentage values
4. **Footer:** "Updated X ago" timestamp + "See details" primary button

### ExpandedReport (960 x 640px, min 800 x 560px)

Separate resizable window with 6 tabs:

| Tab | Content | Visualization |
|---|---|---|
| Sessions | List of recent sessions with project, model, tokens, cost | Row list |
| Models | Token distribution across Opus/Sonnet/Haiku | Donut chart + bars |
| Trends | Daily token usage over 7/30 days | Bar chart + summary cards |
| Projects | Per-project breakdown | Stacked bars with model split |
| Heatmap | GitHub-style usage calendar (6 months) | SVG grid |
| Cache | Cache hit rate and estimated savings | Ring chart + stats grid |

### SettingsPanel

Sectioned settings within the popover:
- **General:** Launch at login toggle, theme selector
- **Polling:** Interval slider (1–30m), warning about frequent polling
- **Notifications:** Warning and danger threshold sliders
- **Account:** Connection status badge, sign out button
- **Appearance:** Three radio options — Cream (default), Dark, Auto (follow OS). Selection writes through to local storage immediately; no Save button.

### AuthPanel

First-run authentication screen. Two card options:
- **Sign in with Claude:** OAuth PKCE via browser
- **Use Claude Code credentials:** Read from existing session

## Iconography

All icons use **Lucide** (`lucide-react`). Semantic aliases are defined in `src/lib/icons.ts`:

| Alias | Lucide icon | Usage |
|---|---|---|
| IconPolling | Activity | Polling status |
| IconDanger | AlertTriangle | Danger threshold |
| IconOpen | ArrowRight | "See details" |
| IconChart | BarChart3 | Models tab |
| IconHeatmap | Calendar | Heatmap tab |
| IconTimer | Clock | Reset timers |
| IconCache | Database | Cache tab |
| IconUsage | Flame | Usage indicators |
| IconThreshold | Gauge | Threshold config |
| IconAuth | Key | Authentication |
| IconSessions | LayoutGrid | Sessions tab |
| IconRefresh | RefreshCw | Refresh actions |
| IconSettings | Settings | Settings |
| IconTrends | TrendingUp | Trends tab |
| IconWarning | TriangleAlert | Warning threshold |
| IconLive | Zap | Live status |

## File Structure

```
src/
  styles/
    tokens.css          — All design tokens (colors, type, spacing, radii, motion)
    globals.css         — Tailwind base, glass mixin, scrollbar, reduced motion
  components/ui/
    Button.tsx          — Primary / ghost / destructive
    IconButton.tsx      — Icon-only button with aria-label
    Card.tsx            — Solid / glass card variants
    ProgressBar.tsx     — Threshold-aware progress bar with gradient fills
    Tabs.tsx            — Segmented tab control with panel management
    Toggle.tsx          — Switch with label and description
    Slider.tsx          — Range input with formatted value
    Select.tsx          — Dropdown with label
    Banner.tsx          — Alert banner (info / warning / error / stale)
    Badge.tsx           — Status pill (model colors, live/stale)
    EmptyState.tsx      — Centered empty state with icon
  popover/
    CompactPopover.tsx  — Default tray view
    UsageBar.tsx        — Labeled progress bar with timer
  report/
    ExpandedReport.tsx  — Tab shell with 6 tabs
    SessionsTab.tsx     — Virtualized session list
    ModelsTab.tsx       — Donut chart + model breakdown
    TrendsTab.tsx       — Bar chart with 7d/30d range
    ProjectsTab.tsx     — Per-project stacked bars
    HeatmapTab.tsx      — GitHub-style calendar grid
    CacheTab.tsx        — Cache hit ring + stats grid
  settings/
    SettingsPanel.tsx   — Configuration panels
    AuthPanel.tsx       — First-run authentication
  lib/
    icons.ts            — Lucide semantic aliases
    motion.ts           — Framer Motion variants
    store.ts            — Zustand store + types
    types.ts            — Shared TypeScript types
```

## Screenshots

See `concepts/popover-final.html` and `concepts/report-final.html` for browser-renderable previews of the two primary screens.

## Design Philosophy

The system is built around one idea: **the data is the interface**. The glassy translucent surface exists only to hold the data — it should disappear from conscious attention. Warm colors keep the tool feeling human despite being fundamentally about numbers and limits. The teal accent provides clear action affordances without competing with the status colors that carry the actual meaning. Every component traces back to the token set — there are no hard-coded visual values anywhere in the codebase. The result is a tool that feels like it belongs in the OS while being recognizably more crafted than the generic utilities it replaces.
