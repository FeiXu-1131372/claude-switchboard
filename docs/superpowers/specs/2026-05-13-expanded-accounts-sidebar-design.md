# Expanded-view accounts sidebar + settings access

**Status:** Design ready for review
**Date:** 2026-05-13
**Tracking PR:** TBD

## 1. Problem

Account management is a core daily operation — switching the active account, monitoring per-account usage, triggering warm-ups, re-authenticating. Today it is reachable only from the compact popover (`CompactPopover` → `AccountsPanel`). The expanded view (`ExpandedReport`) has six analytics tabs and zero account affordance, so a user who is mid-analysis must collapse the window, navigate two clicks, then re-expand to do a swap.

Settings has the same gap: only reachable from the compact popover.

## 2. Goals / non-goals

**Goals**
- Make full account management — list, swap, warm-up, re-auth, add, remove — available in expanded view without collapsing.
- Surface settings access in expanded view.
- Keep compact view's existing routes intact and visually unchanged.
- Avoid duplicating the account-management state machine.

**Non-goals**
- No new per-account analytics views. The right pane still shows usage for the active account; selection in the sidebar is *not* a "view-only" scope change.
- No widening of the expanded window beyond the current 960×640.
- No new IPC commands.

## 3. UX model

### 3.1 Layout

Expanded view becomes a three-region layout:

```
┌─ Header ───────────────────────────────────────────────────┐
│ Claude · Live · last 30 days       ⟳  ⚙  ⤢  ✕              │
├─────────────┬──────────────────────────────────────────────┤
│ Sidebar     │ Report region                                │
│ (300px)     │ (flex-1, 660px)                              │
│             │                                              │
│ ACCOUNTS ⟳  │ [usage summary strip]                        │
│ (rows)      │ ─────────────────────────                    │
│             │ SESSIONS  MODELS  TRENDS  PROJECTS  …        │
│             │ ════════                                     │
│ + Add       │ (active tab content)                         │
└─────────────┴──────────────────────────────────────────────┘
```

- Window size unchanged: 960×640.
- Sidebar fixed at 300px. Vertical scroll inside the sidebar when accounts overflow.
- Report region keeps existing tabs and tab-content layout. Tab strip already uses flexbox; no widths to adjust.
- Sidebar has its own header strip (label "ACCOUNTS" left, refresh-all icon right) and footer ("+ Add account" link). Same chrome conventions as the compact AccountsPanel, minus the back button.

### 3.2 Behavior

- Clicking a non-active row triggers the existing **swap-confirm flow** via the existing `<AccountRow>` "Switch account" hover-revealed button. No new "click row = swap" gesture — fully reuses the compact AccountRow's interaction model.
- Refresh-all icon in the sidebar header invokes `ipc.forceRefresh('all')` with the existing stagger-aware spinner duration.
- Refresh icon in the report header (already exists) still does `forceRefresh('active')` — refreshes only the active account.
- Settings cog (new) opens `<SettingsModal>`. ESC and click-outside dismiss.
- Add Account opens `<AddAccountChooser>` wrapped in `<ModalShell>`. OAuth tile inside it still routes through `<AuthPanel>`.
- Swap confirm card opens `<SwapConfirmCard>` wrapped in `<ModalShell>` over both views (compact and expanded — see §6.1 for the small compact-view UX shift this introduces).

### 3.3 Compact view

Compact view's three routes (`home`, `accounts`, `settings`) are unchanged structurally. Settings stays a pane route in compact, becomes a modal in expanded — asymmetry defended in §6.4. The one shift: AddAccountChooser and SwapConfirmCard, which used to replace the entire compact pane, now overlay it as modals. Full-pane replace is disorienting in a 360-wide popover. See §6.1.

## 4. Architecture

### 4.1 New shared hook

`src/accounts/useAccountManagement.ts` lifts the state machine currently inlined in `AccountsPanel`. It owns:

- `accounts`, `currentActive`, `orgGroups` — derived from the Zustand store.
- `pending: { target, running } | null` — swap-confirm flow state.
- `swappingSlot: number | null`, `confirmError: string | null`.
- `chooserOpen: boolean`.
- `refreshing: boolean` with stagger-aware timeout.
- `reauthSlot: number | null` and the matching `oauth_complete` / `oauth_error` listeners.

**Reauth listener stability.** Today's `AccountsPanel.tsx:35-53` re-attaches listeners each time `reauthSlot` flips, which races on fast back-to-back re-auths (the unmount-on-null inside the handler can drop the next event). The hook fixes this by mounting a single stable listener whenever `reauthSlot !== null`, reading the current slot from a ref instead of a closure-captured value. Multiple pending re-auths dispatch by slot. Documented here because it's a behavior change, even if invisible in the happy path.

Returns action callbacks:

```ts
{
  // data
  accounts, currentActive, orgGroups,
  // swap
  pending, swappingSlot, confirmError,
  requestSwap, confirmSwap, cancelSwap,
  // reauth
  reauthSlot, handleReauth,
  // bulk
  refreshing, handleRefreshAll,
  // membership
  handleRemove,
  // chooser
  chooserOpen, openChooser, closeChooser,
}
```

The hook is the single source of truth for both surfaces.

### 4.2 New components

**`src/accounts/AccountsSidebar.tsx`**
- Wraps the expanded-view left rail.
- Renders sidebar header, scrollable list of `<AccountRow>`, footer "+ Add account".
- Mounts `<ModalShell>` overlays for AddAccountChooser and SwapConfirmCard when `chooserOpen` / `pending` are truthy.
- No `onBack` prop — sidebar is persistent, not a route.

**`src/components/modals/ModalShell.tsx`**
- Generic dialog backdrop. Refactored out of WarmupConsentModal so all four modals share it.
- Props: `{ onDismiss: () => void; title?: string; size?: 'sm' | 'md' | 'lg'; children }`. Optional `title` renders as the modal's header strip with a close X (replaces the per-child top bars stripped in §4.3).
- `role="dialog"`, `aria-modal="true"`, focus trap on the dialog container.
- **Stacking discipline.** A small zustand atom (`modalStack: string[]`) tracks open modals by ID. Each `ModalShell` pushes its ID on mount, pops on unmount. Only the **topmost** modal owns ESC + click-outside dismiss; older ones stay mounted but inert. z-index = `50 + 10 * stackDepth`. This covers the AccountRow→WarmupConsentModal-while-AddAccountChooser-open case in §5.
- **Token-driven surfaces** (see §10 Dependencies): backdrop uses `var(--color-overlay)`, card uses `var(--color-bg-elevated)` + `var(--color-border)`. No hardcoded `bg-black/55` or `bg-neutral-900/95`. This is what lets the modal layer adapt to whichever theme the cream-theme spec resolves at runtime.

**`src/components/modals/SettingsModal.tsx`**
- Wraps `<SettingsPanel>` in `<ModalShell size="lg" title="Settings">`.
- Used only by ExpandedReport. Compact reaches Settings via its existing pane route.

### 4.3 Modified components

The three flow components (AddAccountChooser, SwapConfirmCard, AuthPanel) all gain a `presentation: 'modal' | 'fullpane'` prop. This is from day one — not a fallback added later — so the §6.1 rollback path stays mechanical. The full-pane branch keeps the chrome (drag handle, close-window button, back link) needed by direct-routed entry points; the modal branch renders content-only.

**Defaults by component:**
- `AddAccountChooser` → default `'modal'` (only caller after this work is the modal flow).
- `SwapConfirmCard` → default `'modal'` (same).
- `AuthPanel` → default `'fullpane'` (App.tsx first-run is the historical caller and remains so).

**`AccountsPanel.tsx`** — Replace inline `useState`/`useEffect`/`useMemo` block with `useAccountManagement()`. AddAccountChooser and SwapConfirmCard rendering swap from full-pane replace to modal overlay. Behavior, copy, callbacks unchanged.

**`AddAccountChooser.tsx`** — Add `presentation` prop. In `'modal'`: the `"Add account"` h2 is promoted to `<ModalShell title>`; the footer "Cancel" button is dropped (modal dismiss via X / ESC / backdrop is now the cancel affordance); content gap/padding container is unchanged. In `'fullpane'`: current rendering preserved (unused today after the chooser becomes a modal in both compact and expanded, but kept as a stable fallback). OAuth tile still routes to `<AuthPanel presentation="modal">` *in place* — same single modal shell, child swaps.

**`SwapConfirmCard.tsx`** — Add `presentation` prop. In `'modal'`: top `← Cancel` button and top-right `X` are dropped; the "Confirm switch" title is promoted to `<ModalShell title>`. Footer **Cancel** button and **Switch account** button are kept — Cancel remains the explicit "no, go back" affordance, redundant with backdrop dismiss but expected. In `'fullpane'`: current rendering preserved.

**`AuthPanel.tsx`** — Add `presentation` prop (default `'fullpane'`, since App.tsx first-run is the historical caller). In `'modal'`: skip the top drag-handle header, skip the close-window IconButton (would close the window from inside a modal — see review item #1), drop the outer `flex flex-col h-full`. The inner content (centered 280px column with three Cards) renders directly into the modal body. **No props on the inner content change.**

**`WarmupConsentModal.tsx`** — Replace its hand-rolled backdrop with `<ModalShell size="sm">`. Keep the existing in-body `<h2>` heading (the heading copy is too long for a title-bar strip). No behavior change. Note: this modal is opened from inside `AccountRow`, which itself is inside `AccountsSidebar`/`AccountsPanel` — when a chooser modal is also open, the consent modal stacks on top per §4.2 stacking discipline.

**`ExpandedReport.tsx`** — Wrap the existing body in a flex row, mount `<AccountsSidebar>` on the left. Add `<IconSettings>` button to header between refresh and collapse. Add local `settingsOpen` state; render `<SettingsModal>` when open.

### 4.4 Data flow

```
useAccountManagement()
  ├─ reads:   useAppStore.accounts, useAppStore.settings.thresholds
  ├─ writes:  useAppStore.refreshAccounts(), useAppStore.setPendingSwapReport()
  ├─ ipc:     forceRefresh, detectRunningClaudeCode, swapToAccount,
  │           removeAccount, startOauthFlow
  └─ events:  listen('oauth_complete'), listen('oauth_error')

AccountsPanel  ──┐
                 ├──► useAccountManagement()
AccountsSidebar ─┘
```

Both surfaces consume the same hook instance per mount. Compact and expanded are never mounted simultaneously (App.tsx routes between them), so no cross-contamination concern.

## 5. Edge cases

- **Zero accounts in expanded view.** Should not occur — `App.tsx` already routes to `<AuthPanel>` when `accounts.length === 0`. The sidebar's empty-state path is not reachable but renders defensively: "No accounts managed yet." plus the "+ Add account" footer.
- **Sidebar overflow.** Use `overflow-y-auto` on the row list region; sidebar header and footer stay pinned.
- **In-modal route swap (chooser → OAuth).** AddAccountChooser's `if (showOauth) return <AuthPanel />` branch stays — a single `<ModalShell>` wraps the chooser, and its child swaps from chooser content to `<AuthPanel presentation="modal">`. No nested shell. The `<ModalShell>` `title` prop updates with the active step ("Add account" → "Connect to Claude").
- **Stacked modals (consent on top of chooser).** `WarmupConsentModal` opens from inside `AccountRow` — itself inside the sidebar/panel. If the user has the chooser modal open and a row's warm-up toggle triggers consent, two modals are mounted. Per §4.2 stacking discipline: consent pushes onto the stack with higher z-index; chooser stays mounted but its ESC/click-outside handlers are inert until consent unmounts. Each modal owns its own focus trap; only the topmost is active.
- **OAuth completion while a different modal is open.** The hook's listener clears `reauthSlot`; modals are independent of it. No collision.
- **Refresh-all spinner duration.** The existing stagger calculation `(accounts.length - 1) * 30_000 + 2_000` moves into the hook. Sidebar and compact use the same spinner.
- **Settings cog and report-header refresh share a tight cluster.** Order: refresh, settings cog, collapse, close. Sizes match existing `<IconButton>`.
- **Tab content at 660px.** Heatmap and Trends charts use Recharts with `ResponsiveContainer` — they adapt fluidly, but charts with intrinsic axis-tick density (Heatmap day×hour grid, Trends multi-series) can clip ticks at narrow widths. See §7 for the mandatory screenshot gate.

## 6. Trade-offs

### 6.1 Compact view gets a small UX change

Modal-ifying AddAccountChooser and SwapConfirmCard changes compact behavior: those flows used to replace the entire pane and require a Back button; now they overlay it. We accept this because:
- A 360-wide popover replacing itself is more disorienting than an overlay.
- Single rendering path for these flows across both surfaces is simpler than a layout fork.
- The Back-button affordance in compact's swap/chooser flows is replaced by modal dismiss (X / backdrop click / ESC).

**Rollback if disruptive.** The `presentation` prop is baked into each component from day one (§4.3), defaulting to `'modal'`. Reverting compact to full-pane is a one-line callsite change in `AccountsPanel.tsx` — pass `presentation="fullpane"`. No re-stripping of containers, no prop addition, no cliff.

### 6.2 Sidebar always visible, not collapsible

A collapse-to-rail control was considered. Skipped for v1: the 660px report region is enough for current tabs, and shipping a collapse interaction adds animation and persistence-of-state work. Revisit if user feedback says the report feels cramped.

### 6.3 No per-account view scoping

We deliberately chose "click = swap" over "click = re-scope the report". The latter requires the data layer to support per-account historical snapshots, which it currently doesn't, and clutters the mental model. Active-account-only keeps invariant: what the sidebar highlights matches what the report shows.

### 6.4 Settings entry is a modal in expanded, a pane in compact

The cog icon opens different things on different surfaces: a modal in expanded view, a pane route in compact. This creates a "two mental models for one icon" cost — acknowledged. We defend it because:

- **Compact has no room for a real modal.** At 360×380, a settings modal would occupy ≈95% of the surface. The backdrop is invisible, the "click-outside-to-dismiss" gesture has nowhere to land, and the modal's title bar would have to replicate the chrome (close X) that the pane route already provides for free.
- **Drag affordance.** Compact's pane route keeps the entire window draggable via the `Header` component's drag handle. A near-fullscreen modal in compact would remove or fragment that affordance.
- **Same content, different chrome.** `SettingsPanel` is the body in both cases — the asymmetry is at the wrapper level only. A user who learns the panel in one surface recognizes it in the other.
- **Expanded has the space.** At 960×640, a settings modal at ≈560×500 leaves visible context behind it (the sidebar, the report header), which is what makes "modal" feel like the right pattern there.

If users report confusion, the cheapest fix is to make compact's cog also open a modal — `SettingsModal` already exists; one router change in CompactPopover. Not free, but mechanical.

## 7. Testing

**Unit / component**
- `useAccountManagement.test.ts` — exercise swap flow (request → confirm → success / error), reauth flow with mocked OAuth events, refresh-all timing, remove-account propagation.
- `AccountsSidebar.test.tsx` — renders list, fires `requestSwap` on row swap, opens chooser modal, mounts swap-confirm modal on `pending`.
- `ModalShell.test.tsx` — ESC dismisses, backdrop click dismisses, content click does not.
- `SettingsModal.test.tsx` — opens via prop, closes via X / ESC / backdrop.

**Integration / manual**
- Expanded view on Windows 11 and macOS — verify glass + Mica rendering of sidebar + modal stack.
- Swap initiated from sidebar — modal overlays correctly, swap completes, sidebar reflects new active account.
- Add account flow from expanded view — OAuth opens browser, paste-back returns to app, account appears in sidebar.
- Re-auth from sidebar AccountRow with expired token — opens browser, returns, error state clears.
- Many accounts (≥6) — sidebar scrolls, refresh-all spinner runs for the staggered duration.
- ESC dismisses any open modal; doesn't propagate to close the window. With two modals stacked (e.g. chooser + consent), ESC dismisses only the topmost.
- Back-to-back re-auths on two different accounts — both `oauth_complete` events route to the correct slots (verifies the stable-listener-with-ref refactor in §4.1).

**Pre-merge gate**
- **Screenshot every report tab at the new 660px width.** Sessions, Models, Trends, Projects, Heatmap, Cache. No clipped axis ticks, no broken legends, no horizontal scroll inside tab content. If any tab regresses, fix before merge — do not ship and hope.

## 8. Open questions

None blocking. To revisit post-ship:
- Should the sidebar collapse to icons-only at user preference?
- Should `Add Account` move from a footer link to a button-style affordance in the sidebar header?
- Settings modal vs. dedicated route inside expanded — modal is fine for v1; if Settings grows, promote to a route.

## 9. File-level checklist

New:
- `src/accounts/useAccountManagement.ts`
- `src/accounts/AccountsSidebar.tsx`
- `src/components/modals/ModalShell.tsx`
- `src/components/modals/SettingsModal.tsx`

Modified:
- `src/accounts/AccountsPanel.tsx` — adopt hook, modal overlays
- `src/accounts/AddAccountChooser.tsx` — add `presentation` prop, drop footer Cancel in modal mode, promote title
- `src/accounts/SwapConfirmCard.tsx` — add `presentation` prop, drop top bar in modal mode, promote title
- `src/settings/AuthPanel.tsx` — add `presentation` prop, skip drag handle + close-window IconButton in modal mode
- `src/components/modals/WarmupConsentModal.tsx` — adopt ModalShell
- `src/report/ExpandedReport.tsx` — flex-row layout, sidebar mount, settings cog, settings modal mount
- `src/lib/store.ts` — add `modalStack` atom for §4.2 stacking discipline

Unchanged: `AccountRow`, `SettingsPanel`, all warm-up child components, all report tabs.

## 10. Dependencies & sequencing

This spec composes with the **cream-theme spec** (`2026-05-13-cream-theme-design.md`) — no file-level overlap. Two soft alignments:

1. **Token consumption.** `ModalShell` reads `--color-overlay`, `--color-bg-elevated`, `--color-border`, `--color-border-focus` rather than hardcoded values. The cream-theme spec §5.2 defines these tokens with per-theme resolution; consuming them means the modal layer (this spec's centerpiece) flips with the active theme automatically.

2. **WarmupConsentModal refactor is a small unblock for cream.** That file currently uses hand-rolled `bg-neutral-900/95` + `border-orange-500/12` that don't survive a cream surface. Refactoring it through `ModalShell` (§4.3) tokenizes it as a side effect.

**Ship order (preferred):** cream-theme lands first, then this work consumes the new tokens directly.

**Ship order (fallback, if this work lands first or in parallel):** `ModalShell` uses the current dark-theme values (`bg-black/55` backdrop, `bg-neutral-900/95` card, `border-orange-500/12`) inline. A follow-up patch swaps those four class chains for tokenized equivalents when cream tokens land. Mechanical, single-commit change.

No conflicts with the `SettingsPanel` modification either spec proposes: cream adds an Appearance section to the panel's content; this spec wraps the panel in `<ModalShell>`. The two compose — the Appearance section renders inside the new modal automatically.
