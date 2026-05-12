# Cream Theme + Restrained Palette Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Introduce a default cream theme aligned with Anthropic's surface aesthetic, refine the existing dark theme to share the same restrained status palette, and ship a user-selectable theme toggle (Cream / Dark / Auto).

**Architecture:** Token-driven. `tokens.css` declares cream values inside Tailwind v4's `@theme` block (so generated utilities resolve to cream by default) and overrides every token inside `body[data-theme="dark"]`. Theme preference lives in Zustand (frontend-only state) and writes the `data-theme` attribute on `<body>`. A small inline script in `index.html` applies the theme before React mounts to avoid a flash of unstyled content. Components stop hard-coding three-color gradients and instead select one of `--color-accent` / `--color-warn` / `--color-danger` based on threshold.

**Tech Stack:** Tailwind CSS v4 (`@theme`), React 19, Zustand, Tauri v2 store plugin (for persistence), Recharts (chart consumers), vitest + React Testing Library (tests).

**Spec:** `docs/superpowers/specs/2026-05-13-cream-theme-design.md`

---

## File Structure

| File | Status | Responsibility |
|---|---|---|
| `src/lib/theme.ts` | **create** | `useThemePreference` Zustand slice + `resolvedTheme` selector + OS `matchMedia` listener attach helper |
| `src/lib/chart-palette.ts` | **create** | `useChartPalette()` hook returning resolved OKLCH strings for Recharts; OKLCH-interpolation helper for the heatmap ramp |
| `src/lib/__tests__/theme.test.ts` | **create** | Tests for `resolvedTheme` selector behaviour |
| `src/lib/__tests__/chart-palette.test.ts` | **create** | Tests for the OKLCH-interpolation helper |
| `src/styles/tokens.css` | modify | Cream values in `@theme`; dark overrides in `body[data-theme="dark"]`; model-chip palette |
| `src/styles/globals.css` | modify | `.glass` mixin scoped to dark; popover root background theme-aware; radial wash dropped on cream |
| `src/App.tsx` | modify | Effect writing `resolvedTheme` to `document.body.dataset.theme`; matchMedia listener for auto mode |
| `index.html` | modify | Inline pre-mount script reading `localStorage` → applies `data-theme` synchronously |
| `src/settings/SettingsPanel.tsx` | modify | New Appearance section above General with three radio options |
| `src/components/ui/ProgressBar.tsx` | modify | Replace 3-stop gradient with flat fill that swaps token at thresholds |
| `src/components/ui/Badge.tsx` | modify | Collapse `safe`/`accent` visuals; rewire `live`/`opus`/`sonnet`/`haiku` to model-chip palette |
| `src/report/HeatmapTab.tsx` | modify | Replace hard-coded ramp with 5-stop ramp computed by `chart-palette.ts` |
| `src/report/ModelsTab.tsx` | modify | Use `useChartPalette()` for Recharts color props |
| `src/report/TrendsTab.tsx` | modify | Use `useChartPalette()` for Recharts color props |
| `src/report/ProjectsTab.tsx` | modify | Use `useChartPalette()` for Recharts color props |
| `src/report/CacheTab.tsx` | modify | Use `useChartPalette()` for Recharts color props |
| `docs/design-system.md` | modify | Replace single-theme color tables with dual-theme tables; document Appearance setting |

The Rust side (`src-tauri/`) is untouched. The popover window's `transparent: true` flag stays — the cream surface comes from `#root`'s background, not the OS chrome.

---

## Task Order Rationale

State and runtime support land first (Tasks 1–3) so the rest of the work has somewhere to write to. Then tokens (4–5), then components that depend on tokens (6–8), then settings UI (9), then charts (10–12), then docs (13). This ordering keeps the app in a working state after every commit.

---

## Cross-Platform Considerations

Per CLAUDE.md: **"Cross-platform parity is non-negotiable. Design once, render the same."** The plan has to land identically on macOS (vibrancy) and Windows 11 (Mica) / Windows 10 (translucent solid). Implementer must verify on **both** macOS and Windows before declaring the work done.

### Platform-specific surfaces that interact with the theme

| Concern | macOS behaviour | Windows behaviour | Plan handling |
|---|---|---|---|
| Window transparency | Tauri `transparent: true` + native `NSVisualEffectView` for vibrancy | Tauri `transparent: true` + DOM container (`.win-animated-container`) carries the background, because Windows DWM doesn't composite content the same way | Both code paths exist already in `globals.css`; Task 5 updates **both** so the cream/dark treatment is identical to the user. |
| Backdrop blur on cream | `backdrop-filter` deliberately not applied — opaque cream covers any vibrancy the OS still renders underneath | Mica may still tint the window edge; our opaque inner container fully covers it | `.glass` is scoped to `body[data-theme="dark"]` only. Cream surfaces are fully opaque on both platforms by definition. |
| Radial-wash popover background (dark theme) | Painted on `#root` | Painted on `.win-animated-container` | Task 5 updates both selectors with `body[data-theme="dark"]` qualifier. |
| Native form controls (radio buttons in Appearance section) | macOS draws Aqua radios | Windows draws Fluent radios | Both honour `accent-color` CSS prop. Task 8 uses Tailwind's `accent-[color:var(--color-accent)]` utility which compiles to that property. |
| System font stack | SF Pro Text picked up by `-apple-system` | Segoe UI picked up by next entry in stack | Unchanged. |
| Scrollbar (cream theme) | macOS overlay scrollbars — invisible until used, ignore our color | Windows shows our `::-webkit-scrollbar-thumb` color, must read on cream | `--color-border` works in both themes — verify it's visible against `--color-bg-card` on Windows. |
| `color-scheme` | Affects native form widgets (date pickers, checkboxes) | Same | Task 4 changes from `dark` to `light dark` so native widgets pick the right defaults in both themes. |

### Visual checks must happen on both OSes

Every task with a "Visual check" step needs to be performed on both macOS and Windows before that task's commit, or — if the implementer only has one OS available — explicitly noted: "verified on \<OS\>; cross-OS pass pending." A dedicated cross-platform smoke task (Task 14) collects the final verification.

### Specific cross-platform gotchas

- **Tauri window resize animation on Windows uses the DOM container** (see commit `dc6637c`). The `.win-animated-container` carries `background: var(--color-bg-base)` so the cream/dark surface is what's animated. Don't break this — Task 5 preserves the `.win-animated-container` rule and just makes it theme-aware.
- **macOS vibrancy is a window-level effect.** Even with `transparent: true`, the OS draws vibrancy *inside* the window if the Tauri Rust side configured `NSVisualEffectView`. Opaque cream content (the `#root` background) covers it — no Rust changes needed. If vibrancy ever shows through, check whether `#root` is fully opaque on cream (it should be, per Task 5).
- **Windows 10 has no Mica.** It falls through to a solid translucent surface. Cream covers it the same way.
- **The Windows-only `--window-radius: 18px` override** in `App.tsx` (line 30) is layout-only, not theme-related — it stays untouched.
- **First-paint script runs the same on both OSes.** `localStorage` and `matchMedia` are both standard Web APIs in the Tauri webview on both platforms.

---

### Task 1: Theme state slice (Zustand)

**Files:**
- Create: `src/lib/theme.ts`
- Create: `src/lib/__tests__/theme.test.ts`

> **Divergence from spec §7.4:** The spec proposed dual persistence to both `@tauri-apps/plugin-store` and `localStorage`. The plan uses `localStorage` only. In Tauri's webview, `localStorage` is persisted under the app data dir and survives restart, so the dual-store coordination the spec proposes adds complexity without observable benefit for a single-string preference. If durability concerns arise later, the spec's pattern can be layered on top without API changes.

- [ ] **Step 1: Write the failing test**

Create `src/lib/__tests__/theme.test.ts`:

```typescript
import { describe, it, expect, beforeEach } from 'vitest';
import { useThemeStore, resolveTheme } from '../theme';

describe('resolveTheme', () => {
  it('returns the preference when explicit', () => {
    expect(resolveTheme('cream', /* prefersDark */ true)).toBe('cream');
    expect(resolveTheme('dark', /* prefersDark */ false)).toBe('dark');
  });

  it('follows the OS for auto', () => {
    expect(resolveTheme('auto', /* prefersDark */ true)).toBe('dark');
    expect(resolveTheme('auto', /* prefersDark */ false)).toBe('cream');
  });
});

describe('useThemeStore', () => {
  beforeEach(() => {
    useThemeStore.setState({ themePreference: 'cream' });
  });

  it('defaults to cream', () => {
    expect(useThemeStore.getState().themePreference).toBe('cream');
  });

  it('updates the preference', () => {
    useThemeStore.getState().setThemePreference('dark');
    expect(useThemeStore.getState().themePreference).toBe('dark');
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test src/lib/__tests__/theme.test.ts`
Expected: FAIL with "Cannot find module '../theme'"

- [ ] **Step 3: Create the implementation**

Create `src/lib/theme.ts`:

```typescript
import { create } from 'zustand';

export type ThemePreference = 'cream' | 'dark' | 'auto';
export type ResolvedTheme = 'cream' | 'dark';

const STORAGE_KEY = 'theme-preference';

function readStoredPreference(): ThemePreference {
  if (typeof localStorage === 'undefined') return 'cream';
  const raw = localStorage.getItem(STORAGE_KEY);
  return raw === 'cream' || raw === 'dark' || raw === 'auto' ? raw : 'cream';
}

export function resolveTheme(pref: ThemePreference, prefersDark: boolean): ResolvedTheme {
  if (pref === 'auto') return prefersDark ? 'dark' : 'cream';
  return pref;
}

interface ThemeStore {
  themePreference: ThemePreference;
  setThemePreference: (pref: ThemePreference) => void;
}

export const useThemeStore = create<ThemeStore>((set) => ({
  themePreference: readStoredPreference(),
  setThemePreference: (pref) => {
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem(STORAGE_KEY, pref);
    }
    set({ themePreference: pref });
  },
}));
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test src/lib/__tests__/theme.test.ts`
Expected: PASS — both describe blocks green.

- [ ] **Step 5: Commit**

```bash
git add src/lib/theme.ts src/lib/__tests__/theme.test.ts
git commit -m "feat(theme): add Zustand theme preference store + resolve helper"
```

---

### Task 2: Wire data-theme to <body> in App.tsx

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1: Add the effect**

In `src/App.tsx`, add this import next to the existing `useAppStore` import:

```typescript
import { useThemeStore, resolveTheme, type ResolvedTheme } from './lib/theme';
```

Then add this hook above the `useEffect` that writes `viewMode`:

```typescript
const themePreference = useThemeStore((s) => s.themePreference);

useEffect(() => {
  const mql = window.matchMedia('(prefers-color-scheme: dark)');
  const apply = () => {
    const resolved: ResolvedTheme = resolveTheme(themePreference, mql.matches);
    document.body.dataset.theme = resolved;
  };
  apply();
  if (themePreference === 'auto') {
    mql.addEventListener('change', apply);
    return () => mql.removeEventListener('change', apply);
  }
}, [themePreference]);
```

- [ ] **Step 2: Verify TypeScript compiles**

Run: `pnpm lint`
Expected: no errors.

- [ ] **Step 3: Verify the attribute is written**

Run: `pnpm dev`, open the app, open DevTools, evaluate `document.body.dataset.theme`.
Expected: `"cream"` (the default).

Then in DevTools console, evaluate `useThemeStore.getState().setThemePreference('dark')` (requires the store to be exposed for debugging — if it isn't already, skip this step; the inline test in Task 9 will cover the live toggle).

- [ ] **Step 4: Commit**

```bash
git add src/App.tsx
git commit -m "feat(theme): write resolved theme to body data-attribute"
```

---

### Task 3: Inline pre-mount script in index.html

**Files:**
- Modify: `index.html`

- [ ] **Step 1: Edit index.html**

Replace the existing `<body>` block with:

```html
<body>
  <script>
    (function () {
      try {
        var pref = localStorage.getItem('theme-preference') || 'cream';
        var resolved;
        if (pref === 'auto') {
          resolved = window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'cream';
        } else if (pref === 'dark') {
          resolved = 'dark';
        } else {
          resolved = 'cream';
        }
        document.body.dataset.theme = resolved;
      } catch (e) {
        document.body.dataset.theme = 'cream';
      }
    })();
  </script>
  <div id="root"></div>
  <script type="module" src="/src/main.tsx"></script>
</body>
```

- [ ] **Step 2: Verify no flash on launch**

Run: `pnpm dev`. In DevTools, run `localStorage.setItem('theme-preference', 'dark'); location.reload();`. The page should render directly on dark — no flash of cream before React mounts.

Then run `localStorage.setItem('theme-preference', 'cream'); location.reload();` to restore.

- [ ] **Step 3: Commit**

```bash
git add index.html
git commit -m "feat(theme): apply data-theme synchronously before React mounts"
```

---

### Task 4: Token system — cream defaults + dark overrides

**Files:**
- Modify: `src/styles/tokens.css`

This is the largest single change. The full `@theme` block becomes the cream defaults, and a new `body[data-theme="dark"]` override block follows.

- [ ] **Step 1: Replace the `@theme` block contents**

Open `src/styles/tokens.css`. Replace lines 24–161 (the entire `@theme { ... }` block) with:

```css
@theme {
  /* Surfaces — cream defaults (Anthropic Bone ~#F0EEE6) */
  --color-bg-base: oklch(94% 0.008 85);
  --color-bg-surface: oklch(96% 0.006 85);
  --color-bg-surface-hover: oklch(92% 0.010 80);
  --color-bg-card: oklch(98% 0.004 85);
  --color-bg-card-hover: oklch(99.5% 0.004 85);
  --color-bg-elevated: oklch(100% 0 0);

  /* Borders — warm ink at low alpha on cream */
  --color-rule: oklch(40% 0.015 65 / 0.06);
  --color-rule-strong: oklch(40% 0.015 65 / 0.14);
  --color-border: oklch(40% 0.015 65 / 0.12);
  --color-border-subtle: oklch(40% 0.015 65 / 0.06);
  --color-border-hover: oklch(40% 0.015 65 / 0.22);
  --color-border-focus: oklch(56% 0.155 38 / 0.55);

  /* Text — deep warm charcoal */
  --color-text: oklch(22% 0.018 50);
  --color-text-secondary: oklch(38% 0.020 55);
  --color-text-muted: oklch(55% 0.022 60);

  /* Terracotta accent — darkened for cream contrast */
  --color-accent: oklch(56% 0.155 38);
  --color-accent-dim: oklch(56% 0.155 38 / 0.10);
  --color-accent-muted: oklch(56% 0.155 38 / 0.24);

  /* Status — restrained, only warn/danger carry color */
  --color-warn: oklch(62% 0.150 55);
  --color-warn-dim: oklch(62% 0.150 55 / 0.10);
  --color-danger: oklch(48% 0.180 28);
  --color-danger-dim: oklch(48% 0.180 28 / 0.10);

  /* Deprecated — alias kept one release for backward compat with consumers
   * that still reference --color-safe. Slated for removal. */
  --color-safe: var(--color-accent);
  --color-safe-dim: var(--color-accent-dim);

  /* Model chips — tonal variations on warm family */
  --color-model-opus: oklch(56% 0.155 38);
  --color-model-opus-text: oklch(98% 0.005 85);
  --color-model-sonnet: oklch(60% 0.105 50);
  --color-model-sonnet-text: oklch(98% 0.005 85);
  --color-model-haiku: oklch(86% 0.040 70);
  --color-model-haiku-text: var(--color-text);

  /* Utility */
  --color-track: oklch(40% 0.015 65 / 0.10);
  --color-overlay: oklch(20% 0.010 60 / 0.35);

  /* Glass — kept for dark theme only; on cream the mixin no-ops */
  --glass-blur: 32px;
  --glass-saturate: 1.3;
  --glass-tint: oklch(72% 0.10 55 / 0.04);

  /* Type — families */
  --font-sans: -apple-system, BlinkMacSystemFont, 'SF Pro Text', 'Segoe UI',
    system-ui, sans-serif;
  --font-mono: 'JetBrains Mono', 'SF Mono', ui-monospace, 'Cascadia Code',
    'Fira Code', monospace;

  /* Type scale */
  --text-hero: 56px;
  --text-display: 28px;
  --text-pct: 24px;
  --text-title: 14px;
  --text-body: 13px;
  --text-label: 11px;
  --text-micro: 10.5px;

  /* Line heights */
  --leading-hero: 0.9;
  --leading-display: 1.1;
  --leading-title: 1.3;
  --leading-body: 1.5;
  --leading-label: 1.4;
  --leading-micro: 1.3;

  /* Letter spacing */
  --tracking-hero: -0.04em;
  --tracking-display: -0.02em;
  --tracking-label: 0.04em;

  /* Font weights */
  --font-weight-regular: 400;
  --font-weight-medium: 500;
  --font-weight-semibold: 600;

  --weight-regular: 400;
  --weight-medium: 500;
  --weight-semibold: 600;

  /* Progress bar heights */
  --bar-height-hairline: 2px;
  --bar-height-sm: 3px;
  --bar-height-md: 4px;
  --bar-height-lg: 6px;

  /* Spacing */
  --spacing-2xs: 2px;
  --spacing-xs: 4px;
  --spacing-sm: 8px;
  --spacing-md: 12px;
  --spacing-lg: 16px;
  --spacing-xl: 20px;
  --spacing-2xl: 24px;
  --spacing-3xl: 32px;
  --spacing-4xl: 48px;
  --spacing-pop: 18px;

  /* Spacing aliases */
  --space-2xs: var(--spacing-2xs);
  --space-xs: var(--spacing-xs);
  --space-sm: var(--spacing-sm);
  --space-md: var(--spacing-md);
  --space-lg: var(--spacing-lg);
  --space-xl: var(--spacing-xl);
  --space-2xl: var(--spacing-2xl);
  --space-3xl: var(--spacing-3xl);
  --space-4xl: var(--spacing-4xl);

  /* Popover paddings */
  --popover-pad: 18px;

  /* Radii */
  --radius-sharp: 0;
  --radius-sm: 6px;
  --radius-md: 10px;
  --radius-card: 12px;
  --radius-lg: 14px;
  --radius-pill: 100px;

  /* Motion */
  --ease-out: cubic-bezier(0.22, 1, 0.36, 1);
  --ease-out-strong: cubic-bezier(0.16, 1, 0.3, 1);
  --ease-in-out: cubic-bezier(0.45, 0, 0.55, 1);
  --ease-spring: cubic-bezier(0.34, 1.56, 0.64, 1);

  --duration-instant: 100ms;
  --duration-fast: 200ms;
  --duration-normal: 320ms;
  --duration-slow: 520ms;
  --duration-bar: 700ms;
}
```

- [ ] **Step 2: Add the dark override block**

Append after the closing `}` of the `@theme` block (just before the `:root` block at line 165):

```css
/* Dark theme overrides — same tokens, dark values. Tailwind v4's generated
 * utilities resolve through these custom properties at runtime, so flipping
 * data-theme on <body> repaints the whole app. */
body[data-theme="dark"] {
  /* Surfaces — refined dark, lifted ~4 lightness points from the old base */
  --color-bg-base: oklch(24% 0.014 65);
  --color-bg-surface: oklch(28% 0.016 65);
  --color-bg-surface-hover: oklch(32% 0.018 65);
  --color-bg-card: oklch(34% 0.022 65);
  --color-bg-card-hover: oklch(38% 0.024 65);
  --color-bg-elevated: oklch(42% 0.026 65);

  --color-rule: oklch(95% 0.020 65 / 0.06);
  --color-rule-strong: oklch(95% 0.020 65 / 0.14);
  --color-border: oklch(95% 0.020 65 / 0.20);
  --color-border-subtle: oklch(95% 0.020 65 / 0.10);
  --color-border-hover: oklch(95% 0.020 65 / 0.30);
  --color-border-focus: oklch(70% 0.140 38 / 0.50);

  --color-text: oklch(96% 0.010 65 / 0.96);
  --color-text-secondary: oklch(86% 0.020 65 / 0.78);
  --color-text-muted: oklch(78% 0.025 65 / 0.62);

  --color-accent: oklch(70% 0.140 38);
  --color-accent-dim: oklch(70% 0.140 38 / 0.14);
  --color-accent-muted: oklch(70% 0.140 38 / 0.28);

  --color-warn: oklch(76% 0.155 60);
  --color-warn-dim: oklch(76% 0.155 60 / 0.14);
  --color-danger: oklch(64% 0.195 25);
  --color-danger-dim: oklch(64% 0.195 25 / 0.14);

  /* Aliases */
  --color-safe: var(--color-accent);
  --color-safe-dim: var(--color-accent-dim);

  /* Model chips — same tonal idea but darker for dark text on light text */
  --color-model-opus: oklch(70% 0.140 38);
  --color-model-opus-text: oklch(20% 0.012 65);
  --color-model-sonnet: oklch(58% 0.090 50);
  --color-model-sonnet-text: oklch(20% 0.012 65);
  --color-model-haiku: oklch(48% 0.045 70);
  --color-model-haiku-text: var(--color-text);

  --color-track: oklch(95% 0.020 65 / 0.18);
  --color-overlay: oklch(10% 0.010 65 / 0.55);
}
```

- [ ] **Step 3: Remove the now-outdated comment about light theme**

In the `:root { color-scheme: dark; ... }` block (currently around line 165), change `color-scheme: dark;` to `color-scheme: light dark;` so the OS draws appropriate form-control widget defaults in both themes.

Also delete the trailing block comment about the previously-defined light variant (around lines 184–191 of the original file, starting `/* Note: a light-theme variant ...`). It's no longer accurate.

- [ ] **Step 4: TypeScript still compiles, app still builds**

Run: `pnpm lint`
Expected: no errors.

Run: `pnpm dev` and confirm the popover loads on cream by default.

- [ ] **Step 5: Commit**

```bash
git add src/styles/tokens.css
git commit -m "feat(theme): cream defaults in @theme + dark overrides under data-theme"
```

---

### Task 5: globals.css — glass scoped, popover background theme-aware

**Files:**
- Modify: `src/styles/globals.css`

- [ ] **Step 1: Scope the radial-wash popover background to dark only**

Replace the existing `body[data-view-mode="compact"] #root` rule (currently lines 60–70) with:

```css
body[data-view-mode="compact"] #root {
  /* Flat cream surface by default — opaque, wallpaper-independent. */
  background: var(--color-bg-base);
}

body[data-theme="dark"][data-view-mode="compact"] #root {
  /* Dark popover keeps a subtle warm radial wash for a "light source" feel. */
  background:
    radial-gradient(120% 80% at 0% 0%, oklch(72% 0.10 55 / 0.06), transparent 55%),
    radial-gradient(120% 80% at 100% 100%, oklch(70% 0.140 38 / 0.04), transparent 55%),
    var(--color-bg-base);
}
```

- [ ] **Step 2: Update the Windows container background**

Replace the existing `body[data-os="windows"][data-view-mode="compact"] .win-animated-container` rule (currently lines 85–91) with:

```css
body[data-os="windows"][data-view-mode="compact"] .win-animated-container {
  background: var(--color-bg-base);
  border-radius: var(--window-radius, var(--radius-lg));
}

body[data-os="windows"][data-theme="dark"][data-view-mode="compact"] .win-animated-container {
  background:
    radial-gradient(120% 80% at 0% 0%, oklch(72% 0.10 55 / 0.06), transparent 55%),
    radial-gradient(120% 80% at 100% 100%, oklch(70% 0.140 38 / 0.04), transparent 55%),
    var(--color-bg-base);
  border-radius: var(--window-radius, var(--radius-lg));
}
```

- [ ] **Step 3: Scope `.glass` to dark only**

Replace the `.glass` block (currently lines 126–142) with:

```css
/* Glass mixin — on cream, this is just a soft surface with a border (no
 * backdrop-filter, no warm-tint overlay). On dark, backdrop-filter and the
 * radial tint ::before re-engage to keep the existing depth. */
.glass {
  background: var(--color-bg-surface);
  border: 1px solid var(--color-border-subtle);
}

body[data-theme="dark"] .glass {
  backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturate));
  -webkit-backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturate));
}

body[data-theme="dark"] .glass::before {
  content: '';
  position: absolute;
  top: -30%;
  right: -15%;
  width: 200px;
  height: 200px;
  background: radial-gradient(circle, var(--glass-tint) 0%, transparent 70%);
  pointer-events: none;
}
```

- [ ] **Step 4: Verify visually — both platforms**

Run: `pnpm dev`. The popover should now render on cream. Toggle theme manually via DevTools console: `document.body.dataset.theme = 'dark'`. The popover should switch to dark with vibrancy back in play.

**macOS:** Confirm dark theme shows vibrancy bleeding through (move the popover over a colourful window — the dark background should pick up a faint tint). Cream must NOT show vibrancy — it stays the same `#F0EEE6` regardless of what's behind it.

**Windows 11:** Confirm `.win-animated-container` carries the cream surface (inspect it in DevTools — `background` should resolve to `oklch(94% 0.008 85)` on cream, the radial-wash gradient on dark). The window's rounded corners should clip cleanly.

**Windows 10:** No Mica, but the inner container still wears the surface. Same check as Win 11.

If you only have one OS available right now: note this in the commit message as "macOS verified; Windows pending Task 14" (or vice versa).

- [ ] **Step 5: Commit**

```bash
git add src/styles/globals.css
git commit -m "feat(theme): scope glass + radial-wash to dark; flat cream surface elsewhere"
```

---

### Task 6: ProgressBar — flat fill, threshold-based color swap

**Files:**
- Modify: `src/components/ui/ProgressBar.tsx`
- Modify: `src/popover/UsageBar.test.tsx` (existing test may need a snapshot reset)

- [ ] **Step 1: Replace the gradient maps with token-name maps**

Open `src/components/ui/ProgressBar.tsx`. Replace lines 30–40 (the `gradientMap` and `colorMap` constants) with:

```typescript
const fillMap: Record<ThresholdLevel, string> = {
  safe: 'bg-[var(--color-accent)]',
  warn: 'bg-[var(--color-warn)]',
  danger: 'bg-[var(--color-danger)]',
};

const colorMap: Record<ThresholdLevel, string> = {
  safe: 'text-[color:var(--color-text)]',
  warn: 'text-[color:var(--color-warn)]',
  danger: 'text-[color:var(--color-danger)]',
};
```

Then in the JSX, change the className that referenced `gradientMap[level]` (line 81) to `fillMap[level]`. Also append `transition-colors duration-[var(--duration-fast)]` to the same className list so the color swap at thresholds animates over 200ms (separate from the bar-length animation which keeps the 700ms spring on `transition-[width]`).

The resulting filled-bar div className list:

```typescript
className={[
  'h-full rounded-[var(--radius-pill)]',
  fillMap[level],
  'transition-[width,background-color]',
  'duration-[var(--duration-bar)] ease-[var(--ease-spring)]',
  // color swap is short so it doesn't drag behind the bar-length spring:
  '[transition-duration:var(--duration-bar),var(--duration-fast)]',
].join(' ')}
```

(The compound `transition-[width,background-color]` plus the bracketed compound `[transition-duration:...]` lets Tailwind emit a CSS rule that maps to two durations — bar length on the spring duration, color on `--duration-fast`. If Tailwind v4 doesn't honour that arbitrary-value form on this codebase's setup, fall back to inline `style={{ transition: 'width 700ms cubic-bezier(...), background-color 200ms ease' }}` — but try the utility first.)

- [ ] **Step 2: Run the existing UsageBar test to make sure nothing structurally broke**

Run: `pnpm test src/popover/UsageBar.test.tsx`
Expected: tests still pass. If a test snapshots class names it will need updating — re-read the failure and adjust.

- [ ] **Step 3: Verify visually**

Run: `pnpm dev`. The 5h / 7d bars should now render as flat-color fills (no gradient). At values <75 the bar is terracotta; if you can mock a higher value (or wait for one), confirm the color swaps at 75 and 90.

- [ ] **Step 4: Commit**

```bash
git add src/components/ui/ProgressBar.tsx
git commit -m "feat(progress-bar): flat fill with threshold-based color swap"
```

---

### Task 7: Badge — collapse safe/accent, rewire model chips

**Files:**
- Modify: `src/components/ui/Badge.tsx`

- [ ] **Step 1: Update the variantClasses map**

In `src/components/ui/Badge.tsx`, replace lines 11–22 (the `variantClasses` object) with:

```typescript
const variantClasses: Record<BadgeVariant, string> = {
  default: 'bg-[var(--color-track)] text-[color:var(--color-text-secondary)]',
  accent: 'bg-[var(--color-accent-dim)] text-[color:var(--color-accent)]',
  safe: 'bg-[var(--color-accent-dim)] text-[color:var(--color-accent)]', // collapsed to accent; kept for source compat
  warn: 'bg-[var(--color-warn-dim)] text-[color:var(--color-warn)]',
  danger: 'bg-[var(--color-danger-dim)] text-[color:var(--color-danger)]',
  live: 'bg-[var(--color-accent-dim)] text-[color:var(--color-accent)]',
  stale: 'bg-[var(--color-track)] text-[color:var(--color-text-muted)]',
  opus: 'bg-[var(--color-model-opus)] text-[color:var(--color-model-opus-text)]',
  sonnet: 'bg-[var(--color-model-sonnet)] text-[color:var(--color-model-sonnet-text)]',
  haiku: 'bg-[var(--color-model-haiku)] text-[color:var(--color-model-haiku-text)]',
};
```

- [ ] **Step 2: Verify the live pulse dot still reads against the new accent-dim**

The live variant uses `bg-current` on the inner pulse dot (line 40), which inherits the badge's `text-` color (now `--color-accent`). That's correct — the dot will be terracotta on the light accent-dim background.

- [ ] **Step 3: Run lint**

Run: `pnpm lint`
Expected: no errors.

- [ ] **Step 4: Visual check**

Run: `pnpm dev`. Open the popover, confirm:
- Live badge (top of popover) → terracotta dot on dim terracotta background.
- Model chips on the Models card → Opus solid terracotta with cream text; Sonnet solid clay with cream text; Haiku solid pale sand with dark ink text.

- [ ] **Step 5: Commit**

```bash
git add src/components/ui/Badge.tsx
git commit -m "feat(badge): collapse safe/accent + rewire model chips to tonal palette"
```

---

### Task 8: SettingsPanel Appearance section

**Files:**
- Modify: `src/settings/SettingsPanel.tsx`

- [ ] **Step 1: Import the theme store**

Add to the existing imports near the top of `src/settings/SettingsPanel.tsx`:

```typescript
import { useThemeStore, type ThemePreference } from '../lib/theme';
```

- [ ] **Step 2: Read the preference + setter inside the component**

Inside `SettingsPanel()`, near the other `useAppStore` calls:

```typescript
const themePreference = useThemeStore((s) => s.themePreference);
const setThemePreference = useThemeStore((s) => s.setThemePreference);
```

- [ ] **Step 3: Add the Appearance section above General**

Find the `{/* General */}` section (currently around line 106). Insert this new section immediately above it (so Appearance becomes the first card):

```tsx
{/* Appearance */}
<section className="flex flex-col gap-[var(--space-sm)]">
  <h2 className="text-[length:var(--text-label)] font-[var(--weight-semibold)] text-[color:var(--color-text-muted)] uppercase tracking-[0.04em] px-[var(--space-2xs)]">
    Appearance
  </h2>
  <Card className="p-[var(--space-md)] flex flex-col gap-[var(--space-xs)]">
    {(['cream', 'dark', 'auto'] as ThemePreference[]).map((opt) => (
      <label
        key={opt}
        className="flex items-center gap-[var(--space-sm)] cursor-pointer py-[var(--space-2xs)]"
      >
        <input
          type="radio"
          name="theme-preference"
          value={opt}
          checked={themePreference === opt}
          onChange={() => setThemePreference(opt)}
          className="accent-[color:var(--color-accent)]"
        />
        <span className="text-[length:var(--text-body)] text-[color:var(--color-text)]">
          {opt === 'cream' && 'Cream'}
          {opt === 'dark' && 'Dark'}
          {opt === 'auto' && 'Auto (follow system)'}
        </span>
      </label>
    ))}
  </Card>
</section>
```

- [ ] **Step 4: Remove the now-redundant "Theme: dark" placeholder in General**

In the General section's Card, delete the `<p>` element at lines 118–125 of the original file (the comment + paragraph reading "Theme: dark (light theme coming later)"). The Toggle for "Launch at login" stays.

- [ ] **Step 5: Verify — both platforms**

Run: `pnpm dev`. Open Settings (gear icon). Confirm on both macOS and Windows:
- Appearance section is first.
- Three radio options: Cream / Dark / Auto.
- The radio buttons themselves show terracotta as their accent (via `accent-color`). On macOS they're Aqua-style; on Windows they're Fluent-style — both should pick up the accent.
- Clicking Dark immediately repaints the whole popover dark — no Save button needed.
- Clicking Auto, then toggling the OS into the opposite light/dark mode: the popover repaints without restart.
- Restart the app (Cmd-Q / close-and-reopen) after picking Dark — confirm no flash of cream before the popover settles on dark.

- [ ] **Step 6: Commit**

```bash
git add src/settings/SettingsPanel.tsx
git commit -m "feat(settings): add Appearance section with cream/dark/auto"
```

---

### Task 9: Chart palette helper + OKLCH interpolation (TDD)

**Files:**
- Create: `src/lib/chart-palette.ts`
- Create: `src/lib/__tests__/chart-palette.test.ts`

- [ ] **Step 1: Write the failing test**

Create `src/lib/__tests__/chart-palette.test.ts`:

```typescript
import { describe, it, expect } from 'vitest';
import { oklchInterpolate, oklchRamp, parseOklch } from '../chart-palette';

describe('parseOklch', () => {
  it('parses an OKLCH string into components', () => {
    expect(parseOklch('oklch(56% 0.155 38)')).toEqual({
      l: 56,
      c: 0.155,
      h: 38,
      alpha: 1,
    });
  });

  it('parses an OKLCH string with alpha', () => {
    expect(parseOklch('oklch(56% 0.155 38 / 0.5)')).toEqual({
      l: 56,
      c: 0.155,
      h: 38,
      alpha: 0.5,
    });
  });
});

describe('oklchInterpolate', () => {
  it('returns the start at t=0', () => {
    const result = oklchInterpolate('oklch(20% 0.05 60)', 'oklch(80% 0.10 60)', 0);
    expect(result).toBe('oklch(20% 0.05 60)');
  });

  it('returns the end at t=1', () => {
    const result = oklchInterpolate('oklch(20% 0.05 60)', 'oklch(80% 0.10 60)', 1);
    expect(result).toBe('oklch(80% 0.1 60)');
  });

  it('interpolates linearly at t=0.5', () => {
    const result = oklchInterpolate('oklch(20% 0.05 60)', 'oklch(80% 0.15 60)', 0.5);
    expect(result).toBe('oklch(50% 0.1 60)');
  });
});

describe('oklchRamp', () => {
  it('produces n stops from start to end', () => {
    const ramp = oklchRamp('oklch(20% 0 60)', 'oklch(80% 0 60)', 5);
    expect(ramp).toHaveLength(5);
    expect(ramp[0]).toBe('oklch(20% 0 60)');
    expect(ramp[4]).toBe('oklch(80% 0 60)');
    expect(ramp[2]).toBe('oklch(50% 0 60)');
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test src/lib/__tests__/chart-palette.test.ts`
Expected: FAIL with "Cannot find module '../chart-palette'"

- [ ] **Step 3: Write the implementation**

Create `src/lib/chart-palette.ts`:

```typescript
import { useThemeStore, resolveTheme, type ResolvedTheme } from './theme';
import { useSyncExternalStore } from 'react';

interface Oklch {
  l: number; // 0–100
  c: number; // chroma
  h: number; // hue 0–360
  alpha: number; // 0–1
}

export function parseOklch(input: string): Oklch {
  const match = input.match(
    /oklch\(\s*([\d.]+)%\s+([\d.]+)\s+([\d.]+)(?:\s*\/\s*([\d.]+))?\s*\)/,
  );
  if (!match) throw new Error(`Invalid oklch string: ${input}`);
  return {
    l: parseFloat(match[1]),
    c: parseFloat(match[2]),
    h: parseFloat(match[3]),
    alpha: match[4] != null ? parseFloat(match[4]) : 1,
  };
}

function formatOklch({ l, c, h, alpha }: Oklch): string {
  const round = (n: number, places: number) => {
    const p = 10 ** places;
    return Math.round(n * p) / p;
  };
  const lhc = `${round(l, 2)}% ${round(c, 3)} ${round(h, 2)}`;
  return alpha === 1 ? `oklch(${lhc})` : `oklch(${lhc} / ${round(alpha, 3)})`;
}

export function oklchInterpolate(from: string, to: string, t: number): string {
  const a = parseOklch(from);
  const b = parseOklch(to);
  return formatOklch({
    l: a.l + (b.l - a.l) * t,
    c: a.c + (b.c - a.c) * t,
    h: a.h + (b.h - a.h) * t,
    alpha: a.alpha + (b.alpha - a.alpha) * t,
  });
}

export function oklchRamp(from: string, to: string, steps: number): string[] {
  if (steps < 2) return [from];
  return Array.from({ length: steps }, (_, i) =>
    oklchInterpolate(from, to, i / (steps - 1)),
  );
}

// ─── Resolved palettes ───────────────────────────────────────────────

interface ChartPalette {
  accent: string;
  warn: string;
  danger: string;
  text: string;
  textMuted: string;
  rule: string;
  bgCard: string;
  modelOpus: string;
  modelSonnet: string;
  modelHaiku: string;
  /** 5-step ramp from bgCard (empty) to accent (peak), for the heatmap. */
  heatmapRamp: string[];
}

const CREAM: ChartPalette = {
  accent: 'oklch(56% 0.155 38)',
  warn: 'oklch(62% 0.150 55)',
  danger: 'oklch(48% 0.180 28)',
  text: 'oklch(22% 0.018 50)',
  textMuted: 'oklch(55% 0.022 60)',
  rule: 'oklch(40% 0.015 65 / 0.06)',
  bgCard: 'oklch(98% 0.004 85)',
  modelOpus: 'oklch(56% 0.155 38)',
  modelSonnet: 'oklch(60% 0.105 50)',
  modelHaiku: 'oklch(86% 0.040 70)',
  heatmapRamp: oklchRamp('oklch(98% 0.004 85)', 'oklch(56% 0.155 38)', 5),
};

const DARK: ChartPalette = {
  accent: 'oklch(70% 0.140 38)',
  warn: 'oklch(76% 0.155 60)',
  danger: 'oklch(64% 0.195 25)',
  text: 'oklch(96% 0.010 65)',
  textMuted: 'oklch(78% 0.025 65)',
  rule: 'oklch(95% 0.020 65 / 0.06)',
  bgCard: 'oklch(34% 0.022 65)',
  modelOpus: 'oklch(70% 0.140 38)',
  modelSonnet: 'oklch(58% 0.090 50)',
  modelHaiku: 'oklch(48% 0.045 70)',
  heatmapRamp: oklchRamp('oklch(34% 0.022 65)', 'oklch(70% 0.140 38)', 5),
};

/**
 * Subscribes to the Zustand theme preference and OS prefers-color-scheme,
 * returns the appropriate Recharts-ready palette. Re-renders the caller
 * when the resolved theme changes.
 */
export function useChartPalette(): ChartPalette {
  const themePreference = useThemeStore((s) => s.themePreference);
  const prefersDark = useSyncExternalStore(
    (notify) => {
      const mql = window.matchMedia('(prefers-color-scheme: dark)');
      mql.addEventListener('change', notify);
      return () => mql.removeEventListener('change', notify);
    },
    () => window.matchMedia('(prefers-color-scheme: dark)').matches,
    () => false,
  );
  const resolved: ResolvedTheme = resolveTheme(themePreference, prefersDark);
  return resolved === 'dark' ? DARK : CREAM;
}
```

> **Sync note:** The OKLCH values in `CREAM` and `DARK` above duplicate the token values in `src/styles/tokens.css`. Recharts requires concrete color strings (not CSS variables) on its `fill`/`stroke` props. If a token value in `tokens.css` is edited, the corresponding entry here must be updated. A leading comment at the top of both files calls this out.

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test src/lib/__tests__/chart-palette.test.ts`
Expected: PASS — all three describe blocks green.

- [ ] **Step 5: Add the sync-note comment to both files**

At the top of `src/styles/tokens.css` (just below the existing leading comment block), add:

```css
/* WARNING: chart colors duplicated in src/lib/chart-palette.ts because
 * Recharts requires concrete color strings, not CSS variables. If you edit
 * --color-accent, --color-warn, --color-danger, --color-model-*, or
 * --color-bg-card here, mirror the change in chart-palette.ts. */
```

At the top of `src/lib/chart-palette.ts`, add:

```typescript
// WARNING: OKLCH values in CREAM/DARK below duplicate src/styles/tokens.css.
// Recharts requires concrete color strings, not CSS variables. If a token
// changes in tokens.css, update the matching field here.
```

- [ ] **Step 6: Commit**

```bash
git add src/lib/chart-palette.ts src/lib/__tests__/chart-palette.test.ts src/styles/tokens.css
git commit -m "feat(chart-palette): theme-aware Recharts palette + OKLCH interp helper"
```

---

### Task 10: Update Recharts consumers — ModelsTab

**Files:**
- Modify: `src/report/ModelsTab.tsx`

- [ ] **Step 1: Read current state of ModelsTab.tsx**

Open `src/report/ModelsTab.tsx`. Identify every place that passes a `fill=`, `stroke=`, or color prop to a Recharts component (`Cell`, `Bar`, `Line`, `Area`, `XAxis`, `YAxis`, `CartesianGrid`, etc.). Note the current sources — likely inline hex strings or `var(--color-*)`.

- [ ] **Step 2: Import and call the hook**

Add at the top:

```typescript
import { useChartPalette } from '../lib/chart-palette';
```

Inside the component, near the top of the function body:

```typescript
const palette = useChartPalette();
```

- [ ] **Step 3: Replace hard-coded colors**

For each Recharts prop:
- Use `palette.modelOpus` for the Opus segment/bar.
- Use `palette.modelSonnet` for Sonnet.
- Use `palette.modelHaiku` for Haiku.
- Use `palette.rule` for `<CartesianGrid stroke=…>`.
- Use `palette.textMuted` for `<XAxis tick={{ fill: … }}>` / `<YAxis tick={…}>`.
- Use `palette.text` for any tooltip text.

If a current value reads `var(--color-accent)` etc., that won't work in Recharts — Recharts inserts inline `style` only on the SVG element it owns, and resolving custom properties from arbitrary SVG contexts is unreliable. Replace with the matching palette field.

- [ ] **Step 4: Verify**

Run: `pnpm dev`, open the expanded report, Models tab. The chart should render on cream with terracotta/clay/sand fills. Switch theme to dark — chart re-renders with the dark palette.

- [ ] **Step 5: Commit**

```bash
git add src/report/ModelsTab.tsx
git commit -m "feat(report): wire ModelsTab to theme-aware chart palette"
```

---

### Task 11: Update remaining Recharts consumers — Trends, Projects, Cache

**Files:**
- Modify: `src/report/TrendsTab.tsx`
- Modify: `src/report/ProjectsTab.tsx`
- Modify: `src/report/CacheTab.tsx`

- [ ] **Step 1: Apply the same pattern from Task 10 to each tab**

For each of the three files, repeat Steps 1–3 from Task 10:
1. Import `useChartPalette` from `'../lib/chart-palette'`.
2. Call `const palette = useChartPalette()` at the top of the component.
3. Replace every Recharts color prop with the matching `palette.*` field.

Field-to-context mapping:
- **TrendsTab** (daily token usage bar chart): primary bar fill → `palette.accent`; threshold lines → `palette.warn`, `palette.danger`.
- **ProjectsTab** (stacked bars with model split): three stack segments → `palette.modelOpus`, `palette.modelSonnet`, `palette.modelHaiku`.
- **CacheTab** (ring chart for hit rate): filled arc → `palette.accent`; empty arc → `palette.bgCard`.

- [ ] **Step 2: Verify each tab**

Run: `pnpm dev`. Click through Trends / Projects / Cache. Each should render correctly on cream, then toggle to dark via Settings to confirm reactivity.

- [ ] **Step 3: Commit**

```bash
git add src/report/TrendsTab.tsx src/report/ProjectsTab.tsx src/report/CacheTab.tsx
git commit -m "feat(report): wire remaining chart tabs to theme-aware palette"
```

---

### Task 12: HeatmapTab — 5-stop ramp from palette

**Files:**
- Modify: `src/report/HeatmapTab.tsx`

- [ ] **Step 1: Find the current ramp**

Open `src/report/HeatmapTab.tsx`. Find the function or constant that maps cell value → fill color (likely an array of hex/oklch strings or a function returning one). Note the current 5 stops.

- [ ] **Step 2: Replace with the palette ramp**

Add at the top:

```typescript
import { useChartPalette } from '../lib/chart-palette';
```

Inside the component:

```typescript
const palette = useChartPalette();
const ramp = palette.heatmapRamp; // [empty, low, mid, high, peak]
```

Replace the existing color-bucket logic with a function that picks a stop based on the cell's relative intensity:

```typescript
function cellFill(value: number, max: number): string {
  if (max === 0 || value === 0) return ramp[0];
  const ratio = value / max;
  if (ratio < 0.25) return ramp[1];
  if (ratio < 0.5) return ramp[2];
  if (ratio < 0.75) return ramp[3];
  return ramp[4];
}
```

(If the existing component uses a `useMemo` of `max` already, keep it — `cellFill` is intended to be called inline per cell.)

- [ ] **Step 3: Verify**

Run: `pnpm dev`, expand to report, Heatmap tab. The ramp should go from near-white cream (empty) to terracotta (peak) on cream theme; from card-dark to accent on dark theme.

- [ ] **Step 4: Commit**

```bash
git add src/report/HeatmapTab.tsx
git commit -m "feat(report): heatmap ramp from theme-aware chart palette"
```

---

### Task 13: Cross-platform smoke test

**Files:** none modified — verification only.

This task is mandatory. Skipping it because "it looked right on my OS" is the failure mode CLAUDE.md explicitly calls out. If only one OS is available, escalate to the user before proceeding.

- [ ] **Step 1: Build a release-mode bundle for both targets**

```bash
pnpm tauri build
```

(On macOS this builds a `.app` + `.dmg`; on Windows it builds an `.msi` / `.exe`. If cross-compilation isn't available, run `pnpm tauri build` once per host machine.)

- [ ] **Step 2: macOS smoke**

Install and launch the macOS build. For each of {Cream, Dark, Auto}:
1. Open the popover from the menu bar.
2. Confirm the surface colour matches expectations (cream = solid `#F0EEE6`; dark = warm-dark with subtle radial wash).
3. Drag the popover region over a colourful application window — cream must remain colour-stable; dark may pick up a faint vibrancy tint.
4. Expand to the full report. Click through all tabs (Sessions / Models / Trends / Projects / Heatmap / Cache). All Recharts colours render correctly in the active theme.
5. Open Settings → Appearance and flip themes. Every screen repaints without artefacts.

Set theme to Dark, fully quit (Cmd-Q), relaunch — no cream flash before the dark surface paints.

- [ ] **Step 3: Windows smoke**

Install and launch the Windows build. Repeat all macOS smoke checks. Additionally:
1. Confirm the popover's rounded corners clip cleanly on Windows 11 (Mica) and Windows 10 (no Mica) — no hard rectangle artefact.
2. Resize the expanded report and confirm the `.win-animated-container` background tracks the theme without flicker.
3. Open the popover repeatedly to confirm the `data-appearing` mount animation works in both themes.

- [ ] **Step 4: Wallpaper independence**

Switch to a deep red / blue / black wallpaper on each OS. The cream surface must read as the same `#F0EEE6` regardless. If it shifts, vibrancy is leaking — check Step 1 of Task 5.

- [ ] **Step 5: Contrast spot-check**

In both themes, on both OSes, use a system colour-picker (macOS Digital Color Meter, Windows PowerToys Color Picker) to sample primary text vs background. Both must hit ≥4.5:1 (target ≥9:1 per spec).

- [ ] **Step 6: If everything passes, commit a smoke-test marker**

This is the only commit in this task; it's intentionally tiny so that subsequent bisects can point at "the moment the theme was certified."

```bash
git commit --allow-empty -m "chore(theme): cross-platform smoke verified on macOS + Windows"
```

If something fails, fix the underlying issue (likely in Task 4 or 5), then re-run smoke. Don't ship a partial pass.

---

### Task 14: Update design-system.md

**Files:**
- Modify: `docs/design-system.md`

- [ ] **Step 1: Replace the color tables**

Open `docs/design-system.md`. In section "Color System → Semantic tokens" (around line 14), replace the existing 3-column table (Token / Dark / Light / Usage) with the dual-theme table from the spec's §5.2. The spec is the source of truth; copy the cream and dark values from `docs/superpowers/specs/2026-05-13-cream-theme-design.md` §5.2.

- [ ] **Step 2: Update "Foundation" paragraph**

Replace the existing paragraph (around line 11) with:

```markdown
The palette is built on an Anthropic warm cream/bone foundation (cream theme, default) or a refined warm-dark base (dark theme). Surfaces use warm neutrals with an orange undertone in both themes — never cool gray, never pure black/white. The single accent is terracotta. Status color is used sparingly: most data lives in warm-neutral or terracotta; amber and coral only appear at the 75% and 90% thresholds.
```

- [ ] **Step 3: Update "Status color mapping" section**

Replace its body with:

```markdown
Progress bars use a flat fill that swaps color at thresholds:

- **Safe (0–74%):** terracotta accent (`--color-accent`).
- **Warning (75–89%):** warm amber (`--color-warn`).
- **Danger (90–100%):** deep coral (`--color-danger`).

The 200ms color transition is separate from the bar-length spring (700ms). There is no longer a "green" safe state — color is reserved for actionable signals.
```

- [ ] **Step 4: Update "Model colors" section**

```markdown
Model chips use tonal variations on the warm family:

- **Opus:** deep terracotta (`--color-model-opus`)
- **Sonnet:** clay (`--color-model-sonnet`)
- **Haiku:** pale sand (`--color-model-haiku`)

Text on each chip uses the paired `--color-model-*-text` token to maintain ≥4.5:1 contrast in both themes.
```

- [ ] **Step 5: Add an "Appearance setting" subsection under "Screens → SettingsPanel"**

After the existing SettingsPanel bullet list, add:

```markdown
- **Appearance:** Three radio options — Cream (default), Dark, Auto (follow OS). Selection writes through to local storage immediately; no Save button.
```

- [ ] **Step 6: Commit**

```bash
git add docs/design-system.md
git commit -m "docs(design-system): document cream theme + restrained palette"
```

---

## Self-Review

After all tasks land, run the full validation pass from spec §9 — on **both macOS and Windows**:

- [ ] **Cross-theme parity:** Screenshot the popover and the expanded report on both themes against the same usage data. They must read as the same product on each OS.
- [ ] **Cross-OS parity:** Screenshot cream theme on macOS vs Windows side by side. The surface colour, card borders, type, accent must be visually identical (allowing for Aqua vs Fluent native form-control differences only).
- [ ] **Wallpaper independence on cream:** Screenshot the popover over a dark wallpaper and a light wallpaper on each OS — the cream cards must render identically.
- [ ] **Threshold transitions:** With dev tools, dial a UsageBar from 70 → 78 → 92 and confirm the fill swaps cleanly with a brief color transition. Verify on both OSes.
- [ ] **Auto-mode follow:** Set theme to Auto, toggle OS dark mode, popover repaints without restart. Verify on both OSes.
- [ ] **First-paint flash:** Set Dark, fully quit and relaunch, no flash of cream. Verify on both OSes.
- [ ] **Accessibility contrast:** spot-check primary text on `--color-bg-base` in both themes against a WCAG contrast checker. Both should be ≥9:1.
- [ ] **Run the full test suite:** `pnpm test` — all green.
- [ ] **Build cleanly:** `pnpm build` — no TS errors, no Vite warnings about unresolved CSS variables.
- [ ] **Tauri release builds succeed:** `pnpm tauri build` on macOS produces a working `.app` / `.dmg`; on Windows produces a working `.msi` / `.exe`.

---

## Notes for the implementer

- **Don't refactor adjacent code.** Tasks 6, 7, 10–12 only change the color-prop wiring. Don't restructure the components.
- **Don't add fallbacks for missing tokens.** Every token used here exists in tokens.css after Task 4. If a `var(--color-foo)` reads as empty in DevTools, you have a typo, not a runtime concern.
- **Don't add error handling to the Appearance radio.** Setting a Zustand store value can't fail.
- **The `--color-safe` alias is intentional.** It exists to keep any consumer not enumerated in this plan from breaking. After one release cycle, audit consumers and remove the alias.
- **No emojis in commit messages.** Recent commits in this repo follow conventional-commits prefixes (`feat`, `fix`, `docs`); match that style.
