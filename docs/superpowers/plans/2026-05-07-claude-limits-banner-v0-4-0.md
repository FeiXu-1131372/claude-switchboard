# claude-limits v0.4.0 Migration Banner — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship v0.4.0 of claude-limits with a dismissible banner in the popover that announces the rebrand to Claude Switchboard and links users to the new GitHub releases page.

**Architecture:** A small new React component (`MigrationBanner.tsx`) mounted at the top of the popover, using existing design tokens. Dismissal persisted to `localStorage`. Version bumped to 0.4.0; release published via the existing `release-claude-limits-gh` flow. No Rust changes. No backend logic. This is the *last* release on the `claude-limits` releases feed.

**Tech Stack:** React 19, TypeScript, Tailwind v4, Lucide icons, Tauri v2, Vitest.

**Spec reference:** `docs/superpowers/specs/2026-05-07-switchboard-rebrand-and-warmup-design.md` §9 ("Banner-only, last release on the old feed").

---

## File Structure

| Path | Action | Responsibility |
|---|---|---|
| `src/components/MigrationBanner.tsx` | Create | Renders the banner; manages dismiss state via `localStorage` |
| `src/components/__tests__/MigrationBanner.test.tsx` | Create | Vitest + RTL tests for render, link, dismiss |
| `src/App.tsx` (or popover root) | Modify | Mount the banner above existing content |
| `src-tauri/Cargo.toml` | Modify | Bump version 0.3.x → 0.4.0 |
| `package.json` | Modify | Bump version 0.3.x → 0.4.0 |
| `src-tauri/tauri.conf.json` | Modify | Bump version 0.3.x → 0.4.0 |
| `CHANGELOG.md` | Modify | Add v0.4.0 entry |

---

## Task 1: Add `MigrationBanner` React component

**Files:**
- Create: `src/components/MigrationBanner.tsx`
- Test: `src/components/__tests__/MigrationBanner.test.tsx`

- [ ] **Step 1: Write the failing test**

```tsx
// src/components/__tests__/MigrationBanner.test.tsx
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";
import { MigrationBanner } from "../MigrationBanner";

const STORAGE_KEY = "claude-limits.migration-banner.dismissed";

describe("MigrationBanner", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("renders the rebrand announcement and download link", () => {
    render(<MigrationBanner />);
    expect(screen.getByText(/Claude Switchboard/i)).toBeInTheDocument();
    const link = screen.getByRole("link", { name: /download/i });
    expect(link).toHaveAttribute(
      "href",
      "https://github.com/FeiXu-1131372/claude-switchboard/releases/latest",
    );
  });

  it("hides itself after the dismiss button is clicked", () => {
    render(<MigrationBanner />);
    fireEvent.click(screen.getByRole("button", { name: /dismiss/i }));
    expect(screen.queryByText(/Claude Switchboard/i)).not.toBeInTheDocument();
  });

  it("does not render when dismissed flag is set in localStorage", () => {
    localStorage.setItem(STORAGE_KEY, "1");
    render(<MigrationBanner />);
    expect(screen.queryByText(/Claude Switchboard/i)).not.toBeInTheDocument();
  });

  it("persists dismissal across renders", () => {
    const { unmount } = render(<MigrationBanner />);
    fireEvent.click(screen.getByRole("button", { name: /dismiss/i }));
    unmount();
    render(<MigrationBanner />);
    expect(screen.queryByText(/Claude Switchboard/i)).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test src/components/__tests__/MigrationBanner.test.tsx`
Expected: FAIL — "Cannot find module '../MigrationBanner'"

- [ ] **Step 3: Implement the component**

```tsx
// src/components/MigrationBanner.tsx
import { useState } from "react";
import { ArrowRight, X } from "lucide-react";

const STORAGE_KEY = "claude-limits.migration-banner.dismissed";
const DOWNLOAD_URL =
  "https://github.com/FeiXu-1131372/claude-switchboard/releases/latest";

export function MigrationBanner() {
  const [dismissed, setDismissed] = useState(
    () => localStorage.getItem(STORAGE_KEY) === "1",
  );

  if (dismissed) return null;

  const handleDismiss = () => {
    localStorage.setItem(STORAGE_KEY, "1");
    setDismissed(true);
  };

  return (
    <div className="flex items-start gap-2 px-3 py-2 border-b border-orange-500/15 bg-orange-500/8 text-[12px] leading-snug">
      <div className="flex-1">
        <div className="font-medium text-orange-200/90">
          Claude Limits is now Claude Switchboard
        </div>
        <div className="text-orange-200/70 mt-0.5">
          Multi-account control plane with warm-up &amp; scheduling.{" "}
          <a
            href={DOWNLOAD_URL}
            target="_blank"
            rel="noreferrer"
            className="underline decoration-dotted underline-offset-2 inline-flex items-center gap-0.5 hover:text-orange-100"
          >
            Download v1.0
            <ArrowRight className="w-3 h-3" />
          </a>
        </div>
      </div>
      <button
        type="button"
        aria-label="Dismiss"
        onClick={handleDismiss}
        className="p-0.5 -mr-0.5 rounded hover:bg-orange-500/12 text-orange-200/60 hover:text-orange-100 transition-colors"
      >
        <X className="w-3.5 h-3.5" />
      </button>
    </div>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test src/components/__tests__/MigrationBanner.test.tsx`
Expected: PASS — all four tests green.

- [ ] **Step 5: Commit**

```bash
git add src/components/MigrationBanner.tsx src/components/__tests__/MigrationBanner.test.tsx
git commit -m "feat(banner): add MigrationBanner component for v0.4.0"
```

---

## Task 2: Mount the banner in the popover root

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1: Locate the popover root component and read it**

Run: `grep -n "function App\|export default\|return (" src/App.tsx | head -5`

Confirm `src/App.tsx` is the popover root that wraps everything else.

- [ ] **Step 2: Add the import**

Find the existing import block at the top of `src/App.tsx`. Add:

```tsx
import { MigrationBanner } from "./components/MigrationBanner";
```

- [ ] **Step 3: Mount the banner above existing content**

Find the outermost `return (...)` of the App component. Wrap the existing children so `<MigrationBanner />` sits at the top:

```tsx
return (
  <div className="popover-root">
    <MigrationBanner />
    {/* ...existing children unchanged... */}
  </div>
);
```

If the App component already has a top-level `<div>`, just add `<MigrationBanner />` as its first child. Do not change any existing layout, padding, or class names.

- [ ] **Step 4: Manual smoke test**

Run: `pnpm tauri dev`

Verify in the popover:
- Banner appears at the top with the rebrand text and download link
- Clicking the X dismisses it
- Closing and re-opening the app keeps it dismissed
- Clicking the link opens GitHub in the default browser

- [ ] **Step 5: Commit**

```bash
git add src/App.tsx
git commit -m "feat(banner): mount MigrationBanner in popover root"
```

---

## Task 3: Bump version to 0.4.0 and update CHANGELOG

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `package.json`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Read the current version**

Run: `grep -E '^version|"version"' src-tauri/Cargo.toml package.json src-tauri/tauri.conf.json`

Confirm the current version (likely `0.3.x`). The new version is `0.4.0`.

- [ ] **Step 2: Bump `src-tauri/Cargo.toml`**

Find the `[package]` block. Change:
```toml
version = "0.3.x"
```
to:
```toml
version = "0.4.0"
```

- [ ] **Step 3: Bump `package.json`**

Change `"version": "0.3.x"` to `"version": "0.4.0"`.

- [ ] **Step 4: Bump `src-tauri/tauri.conf.json`**

If a `version` key exists at the top level, update it. (Tauri may read from Cargo.toml — check. If absent, skip this file.)

Run: `grep -n '"version"' src-tauri/tauri.conf.json` to confirm.

- [ ] **Step 5: Add CHANGELOG entry**

Add at the top of `CHANGELOG.md` (under any existing top-level header):

```markdown
## v0.4.0 — 2026-05-07

**Final release on the `claude-limits` repository.** This version of the app
is now Claude Switchboard, available at
https://github.com/FeiXu-1131372/claude-switchboard.

- Added a dismissible migration banner in the popover that links to the
  Switchboard releases page.
- All other functionality unchanged from v0.3.x.

After installing Claude Switchboard, your existing data (usage history,
account credentials, settings) is migrated automatically on first launch.
```

- [ ] **Step 6: Verify the build still works**

Run: `pnpm exec tsc --noEmit && pnpm test && cd src-tauri && cargo check`
Expected: All commands succeed with no errors.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/Cargo.toml package.json src-tauri/tauri.conf.json CHANGELOG.md
git commit -m "chore(release): bump to v0.4.0 with migration banner"
```

---

## Task 4: Build the bundles locally and verify

- [ ] **Step 1: Build for release**

Run: `pnpm install && pnpm tauri build`
Expected: builds complete; bundles appear in `src-tauri/target/release/bundle/`.

- [ ] **Step 2: Install the built bundle locally**

macOS: open `src-tauri/target/release/bundle/dmg/Claude Limits_0.4.0_*.dmg`, drag to Applications, launch.

Verify:
- Banner appears in the popover
- Clicking the link opens the Switchboard GitHub page
- Dismissal persists across app restarts
- All existing tabs (Sessions, Models, Trends, Projects, Heatmap, Cache) still render correctly

- [ ] **Step 3: No commit step** — local builds aren't versioned.

---

## Task 5: Cut and publish the v0.4.0 release

- [ ] **Step 1: Confirm the working tree is clean**

Run: `git status`
Expected: nothing to commit, working tree clean. All v0.4.0 commits are on `main`.

- [ ] **Step 2: Run the existing release flow**

Run: `/release-claude-limits-gh`

This skill bumps tags, pushes, pre-creates the GitHub release record, and watches the build. It is the same flow used for prior releases.

- [ ] **Step 3: Verify the release**

Once the workflow completes:
- Confirm the release page at `https://github.com/FeiXu-1131372/claude-limits/releases/tag/v0.4.0` shows the macOS `.dmg` and Windows `.msi` bundles.
- Confirm `latest.json` is published — open `https://github.com/FeiXu-1131372/claude-limits/releases/latest/download/latest.json` and check it lists v0.4.0.

- [ ] **Step 4: Smoke-test the auto-updater path**

Install a v0.3.x build on a separate machine (or revert locally). Wait for the in-app updater to detect v0.4.0. Click "Install & restart". After restart, the banner is visible.

This is the only mechanism by which existing v0.3.x users learn about Switchboard.

---

## Self-Review Checklist (already applied)

- ✅ Spec coverage: §9's "v0.4.0 banner-only" requirement is fully implemented by Tasks 1–5.
- ✅ No placeholders: every step has runnable code or a concrete command.
- ✅ Type consistency: `STORAGE_KEY` matches across test and component; `DOWNLOAD_URL` matches the spec's `claude-switchboard` repo path.
- ✅ Out of scope (correctly): no Rust changes, no schema changes, no warm-up code. This plan is intentionally tiny — it just wires up the migration nudge.
