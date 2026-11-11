# Claude Limits

Cross-platform (macOS + Windows) menu-bar utility for tracking Claude subscription rate limits.

## Design Context

### Users
Developers using Claude (primarily Claude Code) who need to track rate-limit consumption against 5-hour and 7-day buckets. They check this dozens of times per day — usually a quick glance to decide "can I keep coding?" Context: mid-workflow, mental model already loaded, low patience for UI friction.

### Brand Personality
Calm, precise, premium. Not playful, not corporate, not technical-for-its-own-sake. Three words: **quiet, confident, trustworthy**.

### Aesthetic Direction
Glassy native widget — feels like it belongs in the OS, not like a standalone product with its own chrome. References: macOS Control Center, Raycast popover, Linear's density. Anti-references: generic admin dashboards, ai-token-monitor's rounded Nunito aesthetic, claude-usage-bar's stock SwiftUI.

**Color palette:** Anthropic warm orange/terracotta as the surface tint foundation. Terracotta is the interactive accent and the only hue used for volume in charts. Teal marks non-Anthropic models. Amber and red are reserved for limits. Neutral warm grays for surfaces and text. Usage thresholds use the expected traffic-light progression (safe/amber/danger) but with warm bias — amber-orange rather than pure yellow, coral-red rather than pure red.

**Colour themes:** Terracotta is the default of five themes chosen in Settings (Terracotta, Graphite, Tide, Moss, Plum), each in light and dark. A theme sets the surface tint and the accent; the warn and danger colours and the model colours are the same in every theme. Every theme is defined in `src/styles/tokens.css` and must pass `src/styles/contrast.test.ts`.

**Typography:** System UI font (Inter on Windows, SF Pro on macOS via system stack). Monospace accents for all numeric data (JetBrains Mono). Tight tracking, generous line-height for readability.

**Depth:** Translucent backgrounds with blur, not drop shadows. Soft borders at 8-12% opacity. No hard edges, no heavy elevation.

### Design Principles

1. **Scannable in under 1 second.** The compact popover must communicate status at a glance — two bars, two timers, done. If the user has to read, we failed.
2. **Every element earns its place.** No decorative gradients, no gratuitous animations, no visual clutter. Simplicity and consistency over cleverness.
3. **One tight token set.** Every color, radius, spacing, and animation curve comes from tokens. No hard-coded values anywhere. Two screens must feel like the same app.
4. **Native feel, not native boring.** Match the OS conventions for window behavior and chrome, but bring personality through type contrast, color temperature, and subtle glass effects.
5. **Cross-platform parity is non-negotiable.** Design once, render the same. Platform differences limited to: vibrancy (macOS) vs Mica (Win11) vs translucent solid (Win10).
6. **Tables, not cards.** Lists are dense rows on a shared grid with right-aligned tabular numbers.

### Stack
- Tauri v2 + React 19 + TypeScript
- Tailwind CSS v4 (custom properties for tokens)
- Framer Motion (spring physics only)
- Recharts (charts only)
- Lucide React (icons, no emojis)
- Zustand (state)

### Competitor Insights
- **claude-usage-bar:** Best-in-class data model and chart interaction. Menu bar icon is genuinely clever. But visually generic — all stock SwiftUI, no design system, no personality. Our opportunity: bring the same functional quality with real visual craft.
- **ai-token-monitor:** Creative 3D heatmap and good multi-theme system. But all inline styles, repetitive patterns, utilitarian feel. Our opportunity: same feature richness with architectural cleanliness and a cohesive design system.

## Project Structure
- `src/` — React frontend (UI-only, no filesystem/network access)
- `src-tauri/` — Rust backend (auth, API, parser, store, notifier)
- `docs/` — Spec, design docs
- Full file layout in `docs/superpowers/specs/2026-04-24-claude-limits-design.md` Section 3
