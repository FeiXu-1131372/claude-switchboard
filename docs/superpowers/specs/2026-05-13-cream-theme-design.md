# Cream Theme + Restrained Palette — Design

**Date:** 2026-05-13
**Status:** Draft, awaiting user approval
**Scope:** Visual redesign of both popover and expanded report — introduce a default cream theme aligned with Anthropic's surface aesthetic, refine the existing dark theme to match the same restraint, and ship a user-selectable theme toggle.

---

## 1. Problem

The current popover lives on `oklch(20% 0.012 65)` — a low-chroma warm-gray base with translucent surfaces stacked on top. The 65° hue bias was intended to read as "Anthropic terracotta" but at 20% lightness it reads as dark-brown-gray. The user perceives this as "too dark, not having the Anthropic creamy feel." Anthropic's own surfaces (claude.ai chat, anthropic.com) sit around `#F0EEE6` — a warm cream/bone — with deep ink text and very sparing use of accent color.

The current status system also fights this aesthetic: every progress bar uses a three-color gradient (green → amber → red) regardless of value, which reads as visually busy on any cream surface. Anthropic surfaces use color almost exclusively to signal an action or alert.

## 2. Goals

- Default to a cream theme that reads as Anthropic-native — warm, paper-like, calm.
- Refine the existing dark theme so it shares the same restraint and feels like the same product family.
- Add user-selectable theme switching: Cream / Dark / Auto (follow OS).
- Cut color volume — use accent and status hues only where they earn it.
- Preserve all existing functionality: same components, same data, same screens. This is a token-level redesign with minimal component changes.

## 3. Non-goals

- No layout changes, no new screens, no new features.
- No change to the tray icon rendering (it's already theme-agnostic).
- No accessibility-mode high-contrast theme (could be added later under the same `data-theme` mechanism).
- No per-window theme override (cream applies to both popover and expanded report; dark applies to both).

## 4. Design decisions (confirmed in brainstorming)

| # | Decision | Rationale |
|---|---|---|
| 1 | Both themes ship, user-selectable, cream default | User wants cream but acknowledges some prefer dark; existing `data-theme` hook already anticipated this |
| 2 | Cream surface is opaque, not translucent | Vibrancy/Mica over a dark wallpaper makes cream read muddy — the exact thing the redesign fixes |
| 3 | Restrained status palette: data lives in warm-neutral; color only at ≥75% (amber) and ≥90% (coral). No green. | Anthropic surfaces use color sparingly; green/amber/red traffic-light reads busy on cream |
| 4 | Dark theme refined to match (lifted base lightness, same restrained status palette) | Avoids two design languages in one product |
| 5 | Model chips use tonal variations on the warm family (deep terracotta / clay / pale sand) | Keeps identification at-a-glance without three competing hues |

## 5. Token system

### 5.1 Architecture

Tokens live in `src/styles/tokens.css`. Tailwind v4's `@theme` block introspects `--color-*`, `--text-*`, `--spacing-*` etc. to generate utilities. The existing structure stays; values become theme-scoped.

**Mechanism:** the body element carries a `data-theme` attribute (`"cream"`, `"dark"`). Tokens are defined twice — once for each theme — with the `@theme` block holding the cream defaults (so Tailwind's generated utilities resolve to cream values by default), and a `body[data-theme="dark"]` selector overriding the same custom properties.

```css
@theme {
  /* Cream — defaults */
  --color-bg-base: oklch(94% 0.008 85);
  --color-text: oklch(22% 0.018 50);
  --color-accent: oklch(56% 0.155 38);
  /* ... */
}

body[data-theme="dark"] {
  --color-bg-base: oklch(24% 0.014 65);
  --color-text: oklch(96% 0.010 65 / 0.96);
  --color-accent: oklch(70% 0.140 38);
  /* ... */
}
```

Tailwind generates utilities like `bg-bg-base` once, against the cream values. At runtime, when `data-theme="dark"` is set, the same custom property resolves to the dark value and every utility flips. No utility duplication, no per-component theme branching.

### 5.2 Full token table

| Token | Cream | Dark (refined) | Role |
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

### 5.3 Model chip palette

Three tonal variations on the warm family, theme-aware:

| Chip | Cream | Dark | Text on chip |
|---|---|---|---|
| Opus | `oklch(56% 0.155 38)` deep terracotta | `oklch(70% 0.140 38)` | `oklch(98% 0.005 85)` cream |
| Sonnet | `oklch(60% 0.105 50)` clay | `oklch(58% 0.090 50)` | `oklch(98% 0.005 85)` cream |
| Haiku | `oklch(86% 0.040 70)` pale sand | `oklch(48% 0.045 70)` | `--color-text` |

On cream, Opus and Sonnet chips carry their fill as background with cream text (both ≥4.5:1 contrast); Haiku is light enough that its label uses dark ink (`--color-text`). On dark, all three darken so light-on-dark text reads cleanly.

### 5.4 Glass mixin

The `.glass` mixin in `globals.css` is **kept for dark only**, with refined values (slightly lower blur, lower saturation — the existing 40px blur was tuned against `oklch(20%)`; against `oklch(24%)` it can come down to 32px without losing depth).

On cream, `.glass` resolves to a no-op equivalent: the same border treatment, no `backdrop-filter`. Components don't branch — the mixin's CSS adapts.

```css
.glass {
  background: var(--color-bg-surface);
  border: 1px solid var(--color-border-subtle);
}
body[data-theme="dark"] .glass {
  backdrop-filter: blur(32px) saturate(1.3);
  -webkit-backdrop-filter: blur(32px) saturate(1.3);
}
```

The `.glass::before` radial-warm-tint pseudo-element is removed entirely — it was a dark-theme light-source device that doesn't translate.

## 6. Component impact

### 6.1 Progress bars (`ProgressBar`, `UsageBar`)

Three-stop gradient fill is replaced with a flat fill that swaps token at thresholds:

```tsx
const fillToken =
  value >= dangerThreshold ? '--color-danger' :
  value >= warnThreshold ? '--color-warn' :
  '--color-accent';
```

Track color is `--color-track`. Bar height stays the same. Two distinct animations to be clear:

- **Bar fill length** — keeps the existing 800ms snappy spring (defined in `lib/motion.ts`).
- **Bar fill color** — new 200ms CSS color transition that fires only when crossing the 75% or 90% threshold. Smooths the previously-gradient color change. Thresholds are typically crossed by polling deltas, not user interaction, so the transition is rarely seen.

### 6.2 Badge component

Variants `safe` and `accent` collapse to the same visual treatment (terracotta surface). `safe` is retained as a name-only alias for one release alongside `--color-safe`, then removed; new callers should use `accent`. `live` keeps its pulse-dot but the dot color is now `--color-accent` instead of green. `opus` / `sonnet` / `haiku` adopt the chip palette from §5.3.

### 6.3 Popover background

`globals.css` currently applies two radial-warm-wash gradients to `body[data-view-mode="compact"] #root` as a "light source" effect. Drop both on cream — flat `--color-bg-base`. Keep a subtler version on dark (one radial wash, top-left corner only, accent-tinted at 0.04 alpha).

### 6.4 Selection, focus, scrollbar, native widget palette

All four resolve through tokens — no per-component theme branching needed:
- `::selection` → `background: var(--color-accent-dim); color: var(--color-accent);`
- `:focus-visible` outline → `var(--color-border-focus)`
- `::-webkit-scrollbar-thumb` → `var(--color-border)`
- `color-scheme` is set theme-aware: `:root { color-scheme: light; }` (default for cream) and `body[data-theme="dark"] { color-scheme: dark; }`. This is more precise than `light dark` — it tells the browser which native widget palette to commit to, rather than leaving it to heuristics. Form controls (date pickers, native checkboxes) follow.

### 6.5 Charts (expanded report)

`package.json` declares `recharts` as a dependency, but **no source file imports it.** All charts in `src/report/*` are hand-rolled SVG: `ModelsTab` is a stroke-dasharray donut + Tailwind flat-fill bars; `TrendsTab` is a flexbox of `bg-gradient-to-t` div bars; `ProjectsTab` uses Tailwind backgrounds on divs; `CacheTab` is an SVG ring + flat-fill divs; `HeatmapTab` is an SVG grid of `<rect fill={…}>` elements.

CSS custom properties resolve correctly through SVG `fill` and `stroke` (HeatmapTab today writes `fill="var(--color-track)"` and it works). This means **no theme-aware palette helper or hook is needed** — every chart can use `var(--color-*)` directly, and a `data-theme` flip on `<body>` automatically repaints every SVG and every Tailwind utility.

The work in each chart consumer is therefore just a **token-rename pass**: replace references to the deprecated `--color-safe` and the now-unused 3-stop gradient classes with the new restrained tokens, and switch model-row colors from semantic status tokens to the dedicated `--color-model-*` tokens.

Specific changes per consumer:

| File | Current color references | Change |
|---|---|---|
| `ModelsTab.tsx` | Donut + bar use `opus→accent, sonnet→warn, haiku→safe` | Switch to `opus→model-opus, sonnet→model-sonnet, haiku→model-haiku` |
| `TrendsTab.tsx` | 3 gradient classes (`from-safe to-accent`, `from-accent to-warn`, `from-warn to-danger`) | Flat fills: `bg-accent` (low), `bg-warn` (warn), `bg-danger` (danger) — matches `ProgressBar` refactor in §6.1 |
| `ProjectsTab.tsx` | Single `bg-accent` fill | No change (already restrained) |
| `CacheTab.tsx` | `--color-safe` for savings text + `--color-warn` for the miss arc | `--color-safe` resolves through the alias; can stay until alias removal. The hit/miss split stays accent vs warn — both still read sensibly in restrained palette |

No new files, no new hooks, no OKLCH helpers, no token-duplication maintenance burden.

### 6.6 Heatmap

The 6-month usage calendar currently uses 5 status-spectrum tokens for its discrete level buckets:

```
level 0: --color-track    (empty)
level 1: --color-safe     (low)
level 2: --color-accent   (mid)
level 3: --color-warn     (high)
level 4: --color-danger   (peak)
```

The redesign keeps the **threshold-bucket model** (so a heavy-usage day still "goes red") but removes green to match the restrained palette:

```
level 0: --color-track          (empty)
level 1: --color-accent-muted   (low — terracotta at low chroma/alpha)
level 2: --color-accent         (mid — full terracotta)
level 3: --color-warn           (high)
level 4: --color-danger         (peak)
```

This preserves the calendar's role as a severity signal — densest days remain visually alarming — while aligning with the spec's "color only when it matters" stance. The old approach used green for low-usage days, which was decorative rather than informational; the new approach encodes increasing concern as the cell darkens through terracotta into warn and danger.

The change is a single map rewrite in `HeatmapTab.tsx`. No JS-side ramp computation, no OKLCH interpolation — five token names in a `Record<number, string>`.

## 7. Theme switching mechanism

### 7.1 State

`themePreference: 'cream' | 'dark' | 'auto'` lives in Zustand store, persisted via Tauri store to disk so it survives restart. **Default is `'cream'`** — the user's stated motivation for this work was that today's dark is too dark, so the first-run experience should land on cream regardless of OS theme. `'auto'` is opt-in via the Appearance setting.

`resolvedTheme: 'cream' | 'dark'` is a computed selector: returns `themePreference` if explicit, otherwise reads `window.matchMedia('(prefers-color-scheme: dark)')` and maps to dark/cream.

### 7.2 DOM application

A small effect in the root component writes `resolvedTheme` to `document.body.dataset.theme` whenever it changes. The same effect attaches a `matchMedia` change listener so `'auto'` users get live OS-theme follow.

The body's existing `data-os` and `data-view-mode` attributes are unchanged; `data-theme` is additive.

### 7.3 Settings UI

`SettingsPanel.tsx` gains an Appearance section above General:

```
Appearance
  Theme  ( • ) Cream
         (   ) Dark
         (   ) Auto (follow OS)
```

Three radio buttons. Selection writes through to the store immediately — no save button.

### 7.4 First-paint

To avoid a flash of cream when the user has `'dark'` saved, the theme attribute is written in a tiny inline `<script>` in `index.html` that reads from `localStorage` and applies `data-theme` before React mounts.

**Persistence:** `localStorage` only. In the Tauri webview, `localStorage` is persisted under the app's data directory and survives restart, so dual-write to `@tauri-apps/plugin-store` would add complexity (two stores, drift handling) without observable benefit for a single-string preference. If a future requirement needs cross-window or cross-process theme sync, the plugin-store mirror can be layered on then.

**Source-of-truth ordering:**
1. `localStorage['theme-preference']` is the persistent source of truth (`'cream'` | `'dark'` | `'auto'`).
2. `setThemePreference()` writes `localStorage` synchronously, then updates the Zustand store.
3. On launch, the inline `<script>` in `index.html` reads `localStorage` and applies the resolved theme before React mounts. If `localStorage` is empty (first run), it applies `'cream'`.
4. After mount, the Zustand store reads the same `localStorage` key during initialisation. No reconciliation pass needed — both readers see the same value.

## 8. File-by-file impact

| File | Change |
|---|---|
| `src/styles/tokens.css` | Reorganized: cream values in `@theme`, dark values in `body[data-theme="dark"]` override. Token list per §5.2. |
| `src/styles/globals.css` | `.glass` mixin scoped to dark; radial-wash gradients dropped on cream; popover root background theme-aware. |
| `src/lib/theme.ts` | **New** — `useThemeStore` Zustand slice (`themePreference`, `setThemePreference`) + `resolveTheme(pref, prefersDark)` pure helper. Persists via `localStorage`. |
| `src/App.tsx` | Effect writing the resolved theme to `document.body.dataset.theme`; `matchMedia` listener for auto mode. |
| `index.html` | Inline pre-mount script that reads `localStorage['theme-preference']` and writes `data-theme` synchronously. |
| `src/settings/SettingsPanel.tsx` | New Appearance section with three radio options (Cream / Dark / Auto). |
| `src/components/ui/ProgressBar.tsx` | Replace 3-stop gradient with flat fill that swaps token at thresholds (§6.1). |
| `src/popover/UsageBar.test.tsx` | Hard-coded `bg-gradient-to-r` + `from-…/to-…` class assertions must be rewritten for the flat-fill output. |
| `src/components/ui/Badge.tsx` | `safe`/`accent` collapse; model chip palette wired through (§5.3, §6.2). |
| `src/popover/UsageBar.tsx` | No code changes — inherits ProgressBar refactor. |
| `src/report/HeatmapTab.tsx` | Rewrite 5-token `levelColors` map (§6.6). |
| `src/report/ModelsTab.tsx` | Switch donut + bar colors from semantic status tokens to `--color-model-*` (§6.5). |
| `src/report/TrendsTab.tsx` | Replace 3-stop gradient classes with flat-fill classes matching ProgressBar (§6.5). |
| `src/report/ProjectsTab.tsx` | No change — already uses `--color-accent` only. |
| `src/report/CacheTab.tsx` | No change — references resolve through tokens; `--color-safe` keeps working via the alias. |
| `src/styles/tokens.css` | Reorganised per §5.1–5.3: cream values in `@theme`, dark in `body[data-theme="dark"]` override. |
| `src/styles/globals.css` | `.glass` mixin scoped to dark; radial-wash gradients dropped on cream; popover root background theme-aware; `color-scheme` theme-aware (§6.4). |
| `docs/design-system.md` | Replace single-theme color tables with dual-theme tables; document Appearance setting. |
| `src-tauri/` (Rust) | No changes — theme is a frontend-only concern. The popover window's `transparent: true` flag stays; opaque-on-cream is achieved by `#root` background, not by the OS chrome. |

## 9. Validation

- **Cross-theme parity:** visually compare popover and expanded report under both themes against the same data. Both must read as the same product; if the dark theme suddenly feels like a different app, the token alignment has drifted.
- **Wallpaper independence on cream:** popover screenshotted over a dark wallpaper and a light wallpaper must render identically (since vibrancy is off). Confirms §4 decision 2.
- **Threshold transitions:** dial usage from 70% → 76% → 91% and confirm bar fill swaps cleanly at 75 and 90 with a 200ms color interpolation.
- **Auto-mode follow:** with theme set to Auto, toggle OS dark mode and confirm the popover updates without restart.
- **First-paint flash:** set theme to Dark, restart the app, confirm no cream flash on launch (validates §7.4).
- **Elevated card on cream:** open the warm-up consent modal (or any modal that renders on `--color-bg-elevated`). The elevated card should read as a paper card lifted off the cream backdrop, not as a glaring white overlay. If the contrast feels harsh, drop elevated further toward `oklch(98% 0.004 85)` so it sits closer to the base.
- **Heatmap signal:** confirm that a peak-usage day in `HeatmapTab` still reads as visually alarming (level-4 cells should be deep coral, not soft terracotta). If they don't, the level→token map in §6.6 has been wired incorrectly.
- **Accessibility contrast:** primary text on `--color-bg-base` must hit WCAG AA (≥4.5:1) in both themes. Cream values in §5.2 yield ~12:1; dark refined yields ~9:1. Document in design-system.md.

## 10. Risks & rollback

- **Risk:** existing dark users dislike the refined dark (lifted lightness, restrained status palette). Mitigation: the toggle exists; they can keep dark, and dark's only meaningful change is the status-palette restraint, which lands cohesively.
- **Risk:** Recharts color contrast on cream — bar colors at low saturation can vanish against the cream surface. Mitigation: chart palette uses the same `--color-accent` (56% lightness, high chroma) as the rest of the system, which is well-separated from the 94% cream base.
- **Rollback:** revert token-defaults to dark in `@theme`, flip the `data-theme="dark"` override to `data-theme="cream"`, and ship a one-line patch that hides the Appearance section. The component and store changes are theme-agnostic and can stay.

## 11. Out of scope (future)

- A high-contrast accessibility theme (would extend the same `data-theme` mechanism: `data-theme="hc-cream"`, `data-theme="hc-dark"`).
- Custom user-defined accent hue.
- Per-window theme override (e.g., cream popover with dark expanded report).
