# Throttled Stagger Polling — Design Specification

**Date:** 2026-05-07
**Status:** Design pending user review
**Builds on:** `docs/superpowers/specs/2026-04-24-claude-limits-design.md` (poll-loop architecture)
**Triggered by:** Triple-429 burst observed on `2026-05-06` from `poll_loop::poll_all` parallel fan-out (three concurrent requests at `21:08:40.890`–`21:08:42.155`).

---

## 1. Overview

Replace the current "parallel fan-out every `polling_interval_secs`" poll loop with a per-slot scheduled cadence that staggers fetches by 30 seconds within each cycle. The active slot polls first, inactive slots follow at fixed 30 s offsets, and every slot uses the same user-configured interval. Manual refresh becomes context-aware: from the popover home view it fetches only the active slot; from the Accounts panel it triggers a full staggered round.

The change is motivated by a real incident — three managed slots returned `429 Retry-After: 0ns` simultaneously after the parallel fan-out, suggesting Anthropic's `/usage` endpoint rate-limits across all OAuth tokens sharing the Claude Code `client_id`. Serializing fetches with a 30 s gap eliminates the burst pattern that triggers the limit.

### Goals

| Goal | Decision |
|---|---|
| Eliminate parallel-burst 429s on `/usage` endpoint | Yes — at most one in-flight fetch at any moment |
| Treat all slots equally regardless of active/inactive status | Yes — same `polling_interval_secs` cadence for every slot |
| Keep numbers fresh enough for multi-laptop quota drift | Yes — every slot polled at user's chosen interval (60 s–1800 s) |
| Reuse existing `polling_interval_secs` setting (no new UX) | Yes — same setting drives per-slot cadence |
| Preserve existing 429 backoff behavior | Yes — orthogonal to the schedule, applies per-slot |
| Keep manual Refresh useful but burst-safe | Yes — context-aware behavior described in §2.4 |

### Non-goals

- Replacing the server `/usage` endpoint as the source of truth for utilization percentages or reset timestamps. JSONL-derived numbers can't replicate Anthropic's per-model weighting and discount math; we keep the server authoritative.
- Using JSONL `session_ingested` events as a poll trigger. Earlier brainstorming considered this; the cycle-based design supersedes it. The watcher continues to ingest JSONL into the local DB unchanged — that data path is independent of polling.
- Per-slot interval overrides. Users get one knob; all slots share it.
- Live token-count "burn" updates between server polls. The popover's reset countdowns tick locally (already true today); utilization percentages only change on a successful fetch.

---

## 2. Architecture

### 2.1 Per-slot scheduling state

Add a `next_poll_at: Instant` per slot, stored alongside (or in place of) the existing `backoff_by_slot` map. Concrete change to `app_state.rs`:

```rust
pub struct AppState {
    // existing fields…
    pub backoff_by_slot: RwLock<HashMap<u32, BackoffState>>,
    pub schedule_by_slot: RwLock<HashMap<u32, ScheduleState>>,  // NEW
}

pub struct ScheduleState {
    pub next_poll_at: Instant,
}
```

Each slot's `next_poll_at` advances by `polling_interval_secs` after every successful fetch. The poll loop never fetches a slot before this deadline.

### 2.2 Initial seeding (stagger)

When the schedule map is initialized — at app start, after slot add/remove, or after a swap — seed deadlines so the active slot fires first and inactive slots trail at 30 s intervals in slot-id order:

```
t0 = now
schedule[active_slot]      = t0
schedule[inactive_slots[0]] = t0 + 30 s
schedule[inactive_slots[1]] = t0 + 60 s
schedule[inactive_slots[k]] = t0 + (k + 1) × 30 s
```

If `active_slot` is `None` (live CC creds don't match any managed slot — surfaced as `unmanaged_active_account`), inactive slots are seeded starting at `t0` in slot-id order. This is a degraded but well-defined state; no slot gets "active" privilege.

### 2.3 The serialized loop

Replace `poll_loop::poll_all`'s parallel `join_all` fan-out with a single-slot picker:

```rust
loop {
    // 1. Reconcile active_slot from live CC creds (unchanged).
    // 2. Pick the slot with the earliest already-expired next_poll_at
    //    that is also not in backoff.
    let now = Instant::now();
    let due = pick_due_slot(&state, now);

    match due {
        Some(slot) => {
            fetch_and_update_one(slot, &state, &handle).await;
            // After fetch:
            //   schedule[slot].next_poll_at = now + polling_interval_secs
        }
        None => {
            // Sleep until the earliest future next_poll_at,
            // OR until force_refresh.notified() fires,
            // whichever comes first.
            let wake_at = state.schedule_by_slot
                .read()
                .values()
                .map(|s| s.next_poll_at)
                .min();
            tokio::select! {
                _ = sleep_until(wake_at) => {}
                _ = state.force_refresh.notified() => {}
            }
        }
    }
}
```

**Result:** at most one in-flight fetch at any moment. Subsequent fetches are gated by their per-slot deadlines, which naturally maintain the 30 s gap once seeded.

**Why "pick one slot per iteration" rather than "fetch all due slots":** at steady state only one slot becomes due at a time (because deadlines are 30 s apart and intervals are ≥ 60 s). The picker form is simpler than a "due set" form and inherently prevents accidental parallelism.

### 2.4 Manual Refresh — context-aware (option B2)

Frontend: `ipc.forceRefresh()` becomes `ipc.forceRefresh(scope)` where `scope: 'active' | 'all'`.

| Trigger | Scope | Behavior |
|---|---|---|
| Refresh icon in `CompactPopover`/`ExpandedReport` chrome bar | `'active'` | Set `schedule[active_slot].next_poll_at = now`. Loop wakes via `force_refresh.notify_one()` and fetches the active slot immediately. Inactive slots' schedules are untouched. |
| Refresh icon in `AccountsPanel` header (NEW affordance, see §3.2) | `'all'` | Set `schedule[active_slot].next_poll_at = now`, `schedule[inactive[0]].next_poll_at = now + 30 s`, `schedule[inactive[1]].next_poll_at = now + 60 s`, … Loop wakes and unfolds the staggered round. Total time: `(N − 1) × 30 s`. |

Backend command signature:
```rust
#[command]
pub async fn force_refresh(
    scope: RefreshScope,  // enum { Active, All }
    state: State<'_, Arc<AppState>>,
) -> Result<(), String>
```

The active-only path stays burst-safe even on rapid taps (the slot is just re-set to `now` — no parallelism is introduced). The all-slots path serializes through the same scheduler, so even if the user spams the Accounts-panel refresh, fetches stay 30 s apart.

### 2.5 429 backoff interaction

Existing `BackoffState { until, last_delay }` and exponential-backoff math (`poll_loop.rs:179–215`) are preserved. The picker simply skips any slot where `backoff_by_slot[slot].until > now`.

When a slot's backoff expires, its existing `schedule[slot].next_poll_at` may already be in the past — the picker will fetch it immediately. To preserve the 30 s gap from any other slot that was just polled, advance `next_poll_at` to `max(next_poll_at, last_other_fetch_at + 30s)` when entering the post-backoff state. This is a refinement; the spec accepts that rare back-to-back fetches with < 30 s gap can occur after backoff recovery.

### 2.6 Slot lifecycle

| Event | Schedule effect |
|---|---|
| App starts | Seed deadlines per §2.2. |
| User adds account | New slot inserted at end of inactive order; `next_poll_at = now + (N) × 30 s` where N = count of existing inactive slots. |
| User removes account | Slot's schedule entry dropped. No re-seeding of others. |
| User swaps active account | Re-seed all schedules per §2.2 from `now`. The new active slot polls first, others trail. This piggybacks on the existing `backoff_by_slot.write().clear()` already added in `swap_to_account` (commands.rs:497, 2026-05-07 fix). |
| `accounts_changed` event from external source (currently unused — see §4) | If/when wired, treat same as swap: re-seed from current `active_slot`. |

### 2.7 Edge case: short interval + many slots

If the user sets `polling_interval_secs = 60` and has 4 slots, the staggered round needs `3 × 30 = 90 s` to complete — longer than the interval. Two reasonable handlings:

**Chosen handling: compress stagger.** When `(N − 1) × 30 > polling_interval_secs`, use `gap = polling_interval_secs / N` instead of 30 s. Example: 4 slots at 60 s interval → 15 s gap. Burst protection degrades but doesn't disappear.

**Alternative considered:** clamp `polling_interval_secs` minimum dynamically based on slot count. Rejected — surprising to users who set 60 s and silently get 120 s.

Surface this to the user via a Settings-page hint when the condition triggers (low priority; ship without it and add later if confusion arises).

---

## 3. Frontend changes

### 3.1 IPC surface

`src/lib/ipc.ts` — extend `forceRefresh` to take a required scope:

```ts
forceRefresh(scope: 'active' | 'all'): Promise<void>
```

The argument is required (no default). All existing callers — currently only the chrome-bar Refresh in `CompactPopover.tsx:82` and `LoadingShell` — must be updated to pass `'active'` explicitly. TypeScript will surface any missed call-sites at compile time.

### 3.2 Refresh affordance in AccountsPanel

`src/accounts/AccountsPanel.tsx` — add a refresh icon to the header row, next to the "← Back" button. Clicking calls `ipc.forceRefresh('all')`. Shows the same spinning animation as the chrome-bar refresh icon for `(N − 1) × 30 s + 2 s` post-tap (client-side timer; no Rust-side `is_refreshing_all` flag). The exact "round complete" signal isn't worth a new event — users will see fresh `cached_usage.fetched_at` timestamps as each row updates.

### 3.3 No change to LoadingShell, Settings, or other views

The Settings page already exposes `polling_interval_secs` with the right bounds and copy. No new settings.

---

## 4. What's NOT changing

- `polling_interval_secs` setting bounds, default, and validation (60 s–1800 s, default 300 s).
- 429 backoff math (`next_backoff`, `clamp_backoff`, `BackoffState`).
- The set of events emitted from Rust (`usage_updated`, `auth_required_for_slot`, `unmanaged_active_account`, etc.). Note: `accounts_changed` and `swap_completed` listeners exist in `events.ts` but are not emitted by Rust — that pre-existing dead code stays as-is and is not part of this work.
- `state.snapshot()` and `state.cached_usage_by_slot` — the cache shape is unchanged. Only how/when entries are written changes.
- JSONL ingestion path (`jsonl_parser/walker.rs`, `jsonl_parser/watcher.rs` → `db`). Unrelated to polling.
- KeychainGuardian behavior post-swap.
- The eager `state.active_slot.write() = Some(slot)` and `backoff_by_slot.write().clear()` already added to `swap_to_account` on 2026-05-07. Those land before this work and remain.

---

## 5. Validation

| Scenario | Expected |
|---|---|
| App start with 3 managed slots | Active fetched at t≈0, slot 2 at t≈30 s, slot 3 at t≈60 s. No two requests in flight simultaneously. No 429 burst. |
| Idle for 5 min (default interval) | At t = 300 s active fetched. t = 330 s slot 2. t = 360 s slot 3. |
| User taps Refresh on home view | Active fetched within ~100 ms. Inactive schedules unaffected. |
| User taps Refresh on Accounts panel | Active fetched immediately. Slot 2 ~30 s later. Slot 3 ~60 s later. Subsequent taps within the round are absorbed (re-set to same deadlines). |
| User swaps account A → B mid-cycle | Schedules re-seeded from `now`. B fetched at t≈0, others at t≈30 s, t≈60 s. Stale A-as-active backoff cleared (existing fix). |
| Slot 2 hits 429 | Slot 2 enters 120 s backoff. Slots 1 and 3 continue on schedule. When slot 2's backoff expires, it fetches at the next picker tick, optionally delayed up to 30 s if another slot just fetched. |
| User sets interval to 60 s with 4 slots | Stagger compresses to 15 s; round completes within the interval. |
| User signs out / removes all accounts | App.tsx routes to AuthPanel (existing 2026-05-07 fix); poll loop has no slots and parks on the empty `min()` case (sleep until `force_refresh`). |

---

## 6. Non-coverage / known limitations

- **Multi-laptop quota drift within the interval.** If the user runs Claude Code on a second machine that consumes quota, our app's numbers are stale by up to `polling_interval_secs`. The user can shorten the interval (down to 60 s) to mitigate, accepting more server load. This is the same trade-off as before this change — not a regression.
- **Stagger compression edge.** With many accounts and a short interval, the 30 s gap shrinks, weakening burst protection. Acceptable: the failure mode degrades smoothly rather than cliff-failing.
- **No Refresh-progress indicator on Accounts panel.** Initial implementation may not show "fetching slot 2…" mid-round. Easy follow-up if users ask for it; use the existing per-slot `cached_usage.fetched_at` timestamp to drive a "just refreshed" highlight.

---

## 7. Implementation outline (for the plan that follows)

1. Add `ScheduleState` and `schedule_by_slot` to `AppState`.
2. Replace `poll_loop::poll_all`'s `join_all` fan-out with the picker loop in §2.3.
3. Implement `seed_schedules()` helper used by app start, swap, and refresh-all.
4. Extend the `force_refresh` Tauri command to take a `RefreshScope` enum; update `force_refresh.notify_one()` callers to set scoped deadlines first.
5. Update frontend `ipc.forceRefresh` signature; pass `'active'` from chrome-bar refresh and `'all'` from new AccountsPanel refresh button.
6. Add the AccountsPanel refresh button and wire its click handler.
7. Verify §5 scenarios manually in dev; existing unit tests for backoff math should still pass unchanged.

---

## 8. Spec self-review notes

- **Placeholders:** none.
- **Internal consistency:** §2.3 picker reads `schedule_by_slot`; §2.6 swap re-seeds it; §2.4 manual refresh writes to it. Consistent.
- **Scope:** single implementation plan — backend scheduler refactor + frontend IPC extension + one new button.
- **Ambiguity:** the "compress stagger" rule in §2.7 is precise (`gap = polling_interval_secs / N`). The "advance to `last_other_fetch_at + 30s`" refinement in §2.5 is explicitly accepted as best-effort.
