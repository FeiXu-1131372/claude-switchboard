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

Compact view's three routes (`home`, `accounts`, `settings`) are unchanged structurally. The only difference: AddAccountChooser and SwapConfirmCard, which used to replace the entire compact pane, will now overlay it as modals. This is a deliberate UX upgrade — full-pane replace is disorienting in a 360-wide popover. See §6.1.

## 4. Architecture

### 4.1 New shared hook

`src/accounts/useAccountManagement.ts` lifts the state machine currently inlined in `AccountsPanel`. It owns:

- `accounts`, `currentActive`, `orgGroups` — derived from the Zustand store.
- `pending: { target, running } | null` — swap-confirm flow state.
- `swappingSlot: number | null`, `confirmError: string | null`.
- `chooserOpen: boolean`.
- `refreshing: boolean` with stagger-aware timeout.
- `reauthSlot: number | null` and the matching `oauth_complete` / `oauth_error` listeners.

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
- Props: `{ onDismiss: () => void; size?: 'sm' | 'md' | 'lg'; children }`.
- `role="dialog"`, `aria-modal="true"`, ESC key handler, click-outside on backdrop, focus trap on the dialog container.
- Tailwind classes match the existing WarmupConsentModal aesthetic (orange-tinted border, neutral-900/95 background, backdrop blur).

**`src/components/modals/SettingsModal.tsx`**
- Wraps `<SettingsPanel>` in `<ModalShell size="lg">`.
- Internal header strip: "SETTINGS" label + close icon.

### 4.3 Modified components

**`AccountsPanel.tsx`** — Replace inline `useState`/`useEffect`/`useMemo` block with `useAccountManagement()`. AddAccountChooser and SwapConfirmCard rendering swap from full-pane replace to modal overlay. Behavior, copy, callbacks unchanged.

**`AddAccountChooser.tsx`** — Strip outer container (`flex flex-col gap-… px-…`). Becomes content-only; parent owns the surrounding chrome (modal shell or otherwise). The internal `<AuthPanel>` route remains intact.

**`SwapConfirmCard.tsx`** — Same: strip outer page chrome, parent wraps.

**`WarmupConsentModal.tsx`** — Replace its hand-rolled backdrop with `<ModalShell size="sm">`. No behavior change.

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
- **Modal stacking.** AddAccountChooser → OAuth path currently does `if (showOauth) return <AuthPanel />` — swapping its own render output. Keep that branch: the single `<ModalShell>` wrapping the chooser stays mounted, and its child swaps from chooser content to `<AuthPanel>`. No nested modal layer.
- **OAuth completion while a different modal is open.** The hook's listener clears `reauthSlot`; modals are independent of it. No collision.
- **Refresh-all spinner duration.** The existing stagger calculation `(accounts.length - 1) * 30_000 + 2_000` moves into the hook. Sidebar and compact use the same spinner.
- **Settings cog and report-header refresh share a tight cluster.** Order: refresh, settings cog, collapse, close. Sizes match existing `<IconButton>`.
- **Tab content already at 660px.** Heatmap and Trends charts use Recharts with `ResponsiveContainer` — they already adapt. Verify visually during implementation.

## 6. Trade-offs

### 6.1 Compact view gets a small UX change

Modal-ifying AddAccountChooser and SwapConfirmCard changes compact behavior: those flows used to replace the entire pane and require a Back button; now they overlay it. We accept this because:
- A 360-wide popover replacing itself is more disorienting than an overlay.
- Single rendering path for these flows across both surfaces is simpler than a layout fork.
- The Back-button affordance in compact's swap/chooser flows is replaced by modal dismiss (X / backdrop click / ESC).

If this proves disruptive, the fallback is a `presentation: 'modal' | 'fullpane'` prop on the affected components — additive change, not a redesign.

### 6.2 Sidebar always visible, not collapsible

A collapse-to-rail control was considered. Skipped for v1: the 660px report region is enough for current tabs, and shipping a collapse interaction adds animation and persistence-of-state work. Revisit if user feedback says the report feels cramped.

### 6.3 No per-account view scoping

We deliberately chose "click = swap" over "click = re-scope the report". The latter requires the data layer to support per-account historical snapshots, which it currently doesn't, and clutters the mental model. Active-account-only keeps invariant: what the sidebar highlights matches what the report shows.

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
- ESC dismisses any open modal; doesn't propagate to close the window.

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
- `src/accounts/AddAccountChooser.tsx` — strip outer container
- `src/accounts/SwapConfirmCard.tsx` — strip outer container
- `src/components/modals/WarmupConsentModal.tsx` — adopt ModalShell
- `src/report/ExpandedReport.tsx` — flex-row layout, sidebar mount, settings cog, settings modal mount

Unchanged: `AccountRow`, `SettingsPanel`, `AuthPanel`, all warm-up child components, all report tabs.
