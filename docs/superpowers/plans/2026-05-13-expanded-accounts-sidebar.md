# Expanded-View Accounts Sidebar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring account management and settings access to the expanded view via a persistent left sidebar and modal layer, while preserving compact view behavior and reducing duplicated state.

**Architecture:** Lift `AccountsPanel`'s state machine into a shared `useAccountManagement()` hook consumed by both compact (`AccountsPanel`) and a new expanded surface (`AccountsSidebar`). Introduce a tokenized `<ModalShell>` with stack-aware dismiss, and convert `AddAccountChooser`/`SwapConfirmCard`/`AuthPanel` to support both modal and fullpane rendering. Wire `<ExpandedReport>` to host the sidebar plus a settings cog that opens `<SettingsModal>`.

**Tech Stack:** React 19, TypeScript, Tailwind v4, Framer Motion, Zustand, Vitest + React Testing Library, Tauri v2.

**Spec:** `docs/superpowers/specs/2026-05-13-expanded-accounts-sidebar-design.md`

---

## File Structure Overview

**New files (5):**
- `src/components/modals/ModalShell.tsx` — generic dialog backdrop + card + stacking
- `src/components/modals/SettingsModal.tsx` — wraps `SettingsPanel` in `ModalShell`
- `src/accounts/useAccountManagement.ts` — shared state hook
- `src/accounts/AccountsSidebar.tsx` — expanded-view left rail
- `src/components/modals/__tests__/ModalShell.test.tsx` and other tests

**Modified files (7):**
- `src/lib/store.ts` — add `modalStack` slice
- `src/components/modals/WarmupConsentModal.tsx` — adopt `ModalShell`
- `src/accounts/AddAccountChooser.tsx` — add `presentation` prop
- `src/accounts/SwapConfirmCard.tsx` — add `presentation` prop
- `src/settings/AuthPanel.tsx` — add `presentation` prop
- `src/accounts/AccountsPanel.tsx` — adopt hook, render flows via modal
- `src/report/ExpandedReport.tsx` — flex-row layout, sidebar, settings cog

**Conventions used by this codebase:**
- Tests live in `__tests__/` subfolders alongside source.
- Test runner: `pnpm test` (vitest run). Watch mode: `pnpm test:watch`.
- Typecheck: `pnpm lint` (which is `tsc --noEmit`).
- Commit messages: `type(scope): subject`. **Hook rejects any case-insensitive "claude" substring** — refer to "the upstream CLI" if needed. No `Co-Authored-By` footer.

---

## Task 1: Add `modalStack` slice to the Zustand store

Purpose: ModalShell needs a shared atom that tracks which modals are open in stack order. Only the topmost owns ESC + click-outside dismissal.

**Files:**
- Modify: `src/lib/store.ts`
- Test: `src/lib/__tests__/store-modal-stack.test.ts` (create)

- [ ] **Step 1: Write the failing test**

Create `src/lib/__tests__/store-modal-stack.test.ts`:

```ts
import { describe, it, expect, beforeEach } from 'vitest';
import { useAppStore } from '../store';

describe('modalStack', () => {
  beforeEach(() => {
    // Reset stack between tests
    useAppStore.getState().resetModalStack();
  });

  it('starts empty', () => {
    expect(useAppStore.getState().modalStack).toEqual([]);
  });

  it('push appends an id', () => {
    useAppStore.getState().pushModal('a');
    expect(useAppStore.getState().modalStack).toEqual(['a']);
  });

  it('push of multiple ids preserves order', () => {
    useAppStore.getState().pushModal('a');
    useAppStore.getState().pushModal('b');
    expect(useAppStore.getState().modalStack).toEqual(['a', 'b']);
  });

  it('pop removes the matching id regardless of position', () => {
    useAppStore.getState().pushModal('a');
    useAppStore.getState().pushModal('b');
    useAppStore.getState().popModal('a');
    expect(useAppStore.getState().modalStack).toEqual(['b']);
  });

  it('pop of unknown id is a no-op', () => {
    useAppStore.getState().pushModal('a');
    useAppStore.getState().popModal('zzz');
    expect(useAppStore.getState().modalStack).toEqual(['a']);
  });

  it('isTopmost returns true for last id', () => {
    useAppStore.getState().pushModal('a');
    useAppStore.getState().pushModal('b');
    expect(useAppStore.getState().isTopmost('b')).toBe(true);
    expect(useAppStore.getState().isTopmost('a')).toBe(false);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test src/lib/__tests__/store-modal-stack.test.ts`
Expected: FAIL with errors that `pushModal`, `popModal`, `isTopmost`, `resetModalStack`, `modalStack` are undefined.

- [ ] **Step 3: Add the slice to the store**

In `src/lib/store.ts`, locate the `AppStore` interface (around line 17). Add these fields to it:

```ts
  modalStack: string[];
  pushModal: (id: string) => void;
  popModal: (id: string) => void;
  isTopmost: (id: string) => boolean;
  resetModalStack: () => void;
```

In the `create<AppStore>((set, _get) => ({ ... }))` body (around line 44+), add the initial value:

```ts
  modalStack: [],
```

And the action implementations (place after `consumeSwapReport` or wherever methods cluster):

```ts
  pushModal(id) {
    set((s) => ({ modalStack: [...s.modalStack, id] }));
  },
  popModal(id) {
    set((s) => ({ modalStack: s.modalStack.filter((x) => x !== id) }));
  },
  isTopmost(id) {
    const stack = _get().modalStack;
    return stack.length > 0 && stack[stack.length - 1] === id;
  },
  resetModalStack() {
    set({ modalStack: [] });
  },
```

Note: the existing `create<AppStore>((set, _get) => ({ ... }))` signature already includes the underscore-prefixed `_get`. `isTopmost` references it as `_get()` directly. Leave the underscore — it signals "intentionally unused at the top level" to lint, even though `isTopmost` now uses it.

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test src/lib/__tests__/store-modal-stack.test.ts`
Expected: PASS for all six cases.

- [ ] **Step 5: Typecheck**

Run: `pnpm lint`
Expected: Exit 0.

- [ ] **Step 6: Commit**

```bash
git add src/lib/store.ts src/lib/__tests__/store-modal-stack.test.ts
git commit -m "feat(store): add modalStack slice for dialog stacking"
```

---

## Task 2: Build `ModalShell` component with stacking discipline

Purpose: One reusable dialog backdrop + card. Topmost-owns-ESC, z-index ladder, optional title bar.

**Files:**
- Create: `src/components/modals/ModalShell.tsx`
- Test: `src/components/modals/__tests__/ModalShell.test.tsx`

- [ ] **Step 1: Write the failing tests**

Create `src/components/modals/__tests__/ModalShell.test.tsx`:

```tsx
import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { ModalShell } from '../ModalShell';
import { useAppStore } from '../../../lib/store';

describe('ModalShell', () => {
  beforeEach(() => {
    useAppStore.getState().resetModalStack();
  });

  it('renders children and pushes id onto the stack on mount', () => {
    render(
      <ModalShell onDismiss={() => {}} id="m1">
        <p>hello</p>
      </ModalShell>,
    );
    expect(screen.getByText('hello')).toBeInTheDocument();
    expect(useAppStore.getState().modalStack).toEqual(['m1']);
  });

  it('pops id on unmount', () => {
    const { unmount } = render(
      <ModalShell onDismiss={() => {}} id="m1">
        <p>x</p>
      </ModalShell>,
    );
    unmount();
    expect(useAppStore.getState().modalStack).toEqual([]);
  });

  it('renders a title strip when title prop is provided', () => {
    render(
      <ModalShell onDismiss={() => {}} id="m1" title="Hi">
        <p>x</p>
      </ModalShell>,
    );
    expect(screen.getByText('Hi')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /close/i })).toBeInTheDocument();
  });

  it('does not render a title strip when title prop is omitted', () => {
    render(
      <ModalShell onDismiss={() => {}} id="m1">
        <p>x</p>
      </ModalShell>,
    );
    expect(screen.queryByRole('button', { name: /close/i })).toBeNull();
  });

  it('ESC dismisses when topmost', () => {
    const fn = vi.fn();
    render(
      <ModalShell onDismiss={fn} id="m1">
        <p>x</p>
      </ModalShell>,
    );
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it('ESC does NOT dismiss when not topmost', () => {
    const onA = vi.fn();
    const onB = vi.fn();
    render(
      <>
        <ModalShell onDismiss={onA} id="a"><p>a</p></ModalShell>
        <ModalShell onDismiss={onB} id="b"><p>b</p></ModalShell>
      </>,
    );
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onA).not.toHaveBeenCalled();
    expect(onB).toHaveBeenCalledTimes(1);
  });

  it('backdrop click dismisses when topmost', () => {
    const fn = vi.fn();
    render(
      <ModalShell onDismiss={fn} id="m1">
        <p>x</p>
      </ModalShell>,
    );
    fireEvent.click(screen.getByTestId('modal-backdrop'));
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it('content click does NOT dismiss', () => {
    const fn = vi.fn();
    render(
      <ModalShell onDismiss={fn} id="m1">
        <p data-testid="content">x</p>
      </ModalShell>,
    );
    fireEvent.click(screen.getByTestId('content'));
    expect(fn).not.toHaveBeenCalled();
  });

  it('title-bar close button calls onDismiss', () => {
    const fn = vi.fn();
    render(
      <ModalShell onDismiss={fn} id="m1" title="Hi">
        <p>x</p>
      </ModalShell>,
    );
    fireEvent.click(screen.getByRole('button', { name: /close/i }));
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it('dismissable=false: ESC, backdrop click, and title-X are all no-ops', () => {
    const fn = vi.fn();
    render(
      <ModalShell onDismiss={fn} id="m1" title="Hi" dismissable={false}>
        <p>x</p>
      </ModalShell>,
    );
    fireEvent.keyDown(window, { key: 'Escape' });
    fireEvent.click(screen.getByTestId('modal-backdrop'));
    expect(fn).not.toHaveBeenCalled();
    expect(screen.queryByRole('button', { name: /close/i })).toBeNull();
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pnpm test src/components/modals/__tests__/ModalShell.test.tsx`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement ModalShell**

Create `src/components/modals/ModalShell.tsx`:

```tsx
import { useEffect, useLayoutEffect, useRef } from 'react';
import { useAppStore } from '../../lib/store';
import { IconButton } from '../ui/IconButton';
import { X } from '../../lib/icons';

interface Props {
  id: string;
  onDismiss: () => void;
  title?: string;
  size?: 'sm' | 'md' | 'lg';
  /** When false, ESC, backdrop click, and the title-bar X are all disabled.
   *  Used by forced-decision dialogs like WarmupConsentModal. Default true. */
  dismissable?: boolean;
  children: React.ReactNode;
}

const SIZE_CLASSES: Record<NonNullable<Props['size']>, string> = {
  sm: 'max-w-[320px]',
  md: 'max-w-[480px]',
  lg: 'max-w-[640px]',
};

export function ModalShell({
  id,
  onDismiss,
  title,
  size = 'md',
  dismissable = true,
  children,
}: Props) {
  const pushModal = useAppStore((s) => s.pushModal);
  const popModal = useAppStore((s) => s.popModal);
  const isTopmost = useAppStore((s) => s.isTopmost);
  const stackDepth = useAppStore((s) => s.modalStack.indexOf(id));
  const cardRef = useRef<HTMLDivElement>(null);

  // useLayoutEffect commits before paint so the z-index calculation reflects
  // stack position on the first painted frame. useEffect would leave one
  // pre-commit frame at the wrong z when two modals mount in the same tick.
  useLayoutEffect(() => {
    pushModal(id);
    return () => popModal(id);
  }, [id, pushModal, popModal]);

  useEffect(() => {
    if (!dismissable) return;
    function handleKey(e: KeyboardEvent) {
      if (e.key === 'Escape' && isTopmost(id)) {
        onDismiss();
      }
    }
    window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  }, [id, isTopmost, onDismiss, dismissable]);

  const z = 50 + 10 * Math.max(0, stackDepth);

  return (
    <div
      role="dialog"
      aria-modal="true"
      data-testid="modal-backdrop"
      onClick={() => {
        if (dismissable && isTopmost(id)) onDismiss();
      }}
      className="fixed inset-0 flex items-center justify-center p-4"
      style={{
        zIndex: z,
        background: 'var(--color-overlay, oklch(0% 0 0 / 0.55))',
      }}
    >
      <div
        ref={cardRef}
        onClick={(e) => e.stopPropagation()}
        className={`
          w-full ${SIZE_CLASSES[size]} max-h-full overflow-y-auto
          rounded-[var(--radius-lg)]
          border
          shadow-[0_12px_36px_oklch(0%_0_0_/_0.4)]
        `}
        style={{
          background: 'var(--color-bg-elevated)',
          borderColor: 'var(--color-border)',
        }}
      >
        {title && (
          <div className="flex items-center justify-between px-[var(--space-md)] py-[var(--space-sm)] border-b border-[var(--color-rule)]">
            <span className="text-[length:var(--text-label)] font-[var(--weight-semibold)] uppercase tracking-[var(--tracking-label)] text-[color:var(--color-text-secondary)]">
              {title}
            </span>
            {dismissable && (
              <IconButton label="Close" onClick={onDismiss}>
                <X size={13} />
              </IconButton>
            )}
          </div>
        )}
        {children}
      </div>
    </div>
  );
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `pnpm test src/components/modals/__tests__/ModalShell.test.tsx`
Expected: All nine tests PASS.

- [ ] **Step 5: Typecheck**

Run: `pnpm lint`
Expected: Exit 0.

- [ ] **Step 6: Commit**

```bash
git add src/components/modals/ModalShell.tsx src/components/modals/__tests__/ModalShell.test.tsx
git commit -m "feat(modals): add ModalShell with stack-aware dismiss"
```

---

## Task 3: Refactor `WarmupConsentModal` to use `ModalShell`

Purpose: Verify ModalShell works against an existing real modal. Tokenize the backdrop.

**Files:**
- Modify: `src/components/modals/WarmupConsentModal.tsx`
- Existing tests: `src/components/modals/__tests__/WarmupConsentModal.test.tsx` (must still pass)

- [ ] **Step 1: Read existing tests**

Run: `cat src/components/modals/__tests__/WarmupConsentModal.test.tsx`
Read them so the refactor preserves the exact selectors and behaviors they assert.

- [ ] **Step 2: Replace the file contents**

Replace `src/components/modals/WarmupConsentModal.tsx` with:

```tsx
import { ModalShell } from './ModalShell';

interface Props {
  onAccept: () => void;
  onDismiss: () => void;
}

export function WarmupConsentModal({ onAccept, onDismiss }: Props) {
  return (
    <ModalShell id="warmup-consent" onDismiss={onDismiss} size="sm" dismissable={false}>
      <div className="p-[var(--space-md)] text-[length:var(--text-label)] text-[color:var(--color-text)]">
        <h2 className="text-[length:var(--text-body)] font-[var(--weight-semibold)] mb-[var(--space-xs)] leading-tight">
          Warm-up sends messages on your behalf
        </h2>
        <p className="text-[color:var(--color-text-secondary)] leading-snug mb-[var(--space-xs)]">
          Switchboard can send a tiny message (1 token, on Haiku) to{' '}
          <code className="mono">api.anthropic.com</code> using this account's credentials,
          whenever you trigger it manually or a schedule fires.
        </p>
        <p className="text-[color:var(--color-text-secondary)] leading-snug mb-[var(--space-xs)]">
          Same API surface Claude Code uses. Cost: rounding-error against your
          subscription. Effect: starts the 5-hour window deliberately.
        </p>
        <p className="text-[color:var(--color-text-muted)] leading-snug text-[length:var(--text-micro)] mb-[var(--space-md)]">
          You can disable per-account, or revoke globally from Settings.
        </p>
        <div className="flex justify-end gap-[var(--space-xs)]">
          <button
            type="button"
            onClick={onDismiss}
            className="px-[var(--space-sm)] py-[var(--space-2xs)] rounded-[var(--radius-sm)] bg-[var(--color-track)] hover:bg-[var(--color-bg-card-hover)] text-[color:var(--color-text-secondary)] text-[length:var(--text-micro)] font-[var(--weight-medium)] transition-colors"
          >
            Don't enable
          </button>
          <button
            type="button"
            onClick={onAccept}
            className="px-[var(--space-sm)] py-[var(--space-2xs)] rounded-[var(--radius-sm)] bg-[var(--color-accent-dim)] hover:bg-[var(--color-accent-muted)] text-[color:var(--color-accent)] text-[length:var(--text-micro)] font-[var(--weight-medium)] transition-colors"
          >
            Enable warm-up
          </button>
        </div>
      </div>
    </ModalShell>
  );
}
```

- [ ] **Step 3: Update tests for the refactored modal**

Open `src/components/modals/__tests__/WarmupConsentModal.test.tsx`. Add an import + a `beforeEach` that resets the modal stack (so tests don't pollute each other through the shared store), and append two new tests verifying the forced-decision behavior.

Add this import at the top:

```tsx
import { useAppStore } from "../../../lib/store";
```

Add `beforeEach` to the `describe` block:

```tsx
describe("WarmupConsentModal", () => {
  beforeEach(() => {
    useAppStore.getState().resetModalStack();
  });
  // … existing tests
```

(Add `beforeEach` to the vitest import too: `import { describe, it, expect, vi, beforeEach } from "vitest";`.)

Append these tests after the existing ones:

```tsx
  it("ESC does NOT dismiss (consent is forced)", () => {
    const onDismiss = vi.fn();
    render(<WarmupConsentModal onAccept={() => {}} onDismiss={onDismiss} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("backdrop click does NOT dismiss (consent is forced)", () => {
    const onDismiss = vi.fn();
    render(<WarmupConsentModal onAccept={() => {}} onDismiss={onDismiss} />);
    fireEvent.click(screen.getByTestId("modal-backdrop"));
    expect(onDismiss).not.toHaveBeenCalled();
  });
```

- [ ] **Step 4: Run all tests for this file**

Run: `pnpm test src/components/modals/__tests__/WarmupConsentModal.test.tsx`
Expected: All existing tests + the two new forced-decision tests PASS. If an existing selector regressed (e.g. exact text or class), update the test to match the new tokens — but only when the test was asserting class strings; behavioral assertions must be preserved.

- [ ] **Step 5: Typecheck**

Run: `pnpm lint`
Expected: Exit 0.

- [ ] **Step 6: Commit**

```bash
git add src/components/modals/WarmupConsentModal.tsx src/components/modals/__tests__/WarmupConsentModal.test.tsx
git commit -m "refactor(modals): warmup consent adopts ModalShell, forced decision preserved"
```

---

## Task 4: Add `presentation` prop to `AddAccountChooser`

Purpose: In modal mode, drop the footer Cancel button (modal dismiss replaces it). Title is provided by the parent's `ModalShell title=` prop, so the chooser body doesn't render the `<h2>` either.

**Files:**
- Modify: `src/accounts/AddAccountChooser.tsx`
- Test: `src/accounts/__tests__/AddAccountChooser.test.tsx` (create)

- [ ] **Step 1: Write the failing tests**

Create `src/accounts/__tests__/AddAccountChooser.test.tsx`:

```tsx
import { render, screen } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { AddAccountChooser } from '../AddAccountChooser';

vi.mock('../../lib/ipc', () => ({
  ipc: {
    addAccountFromClaudeCode: vi.fn(),
  },
}));

vi.mock('../../lib/store', () => ({
  useAppStore: (sel: (s: any) => any) =>
    sel({
      accounts: [],
      refreshAccounts: vi.fn(),
    }),
}));

describe('AddAccountChooser', () => {
  it('renders the Cancel button in fullpane mode (default fallback caller)', () => {
    render(<AddAccountChooser presentation="fullpane" onClose={() => {}} />);
    expect(screen.getByRole('button', { name: /cancel/i })).toBeInTheDocument();
  });

  it('does NOT render the footer Cancel button in modal mode', () => {
    render(<AddAccountChooser presentation="modal" onClose={() => {}} />);
    expect(screen.queryByRole('button', { name: /^cancel$/i })).toBeNull();
  });

  it('renders the h2 heading in fullpane mode', () => {
    render(<AddAccountChooser presentation="fullpane" onClose={() => {}} />);
    expect(screen.getByRole('heading', { name: /add account/i })).toBeInTheDocument();
  });

  it('does NOT render the h2 heading in modal mode (parent ModalShell provides title)', () => {
    render(<AddAccountChooser presentation="modal" onClose={() => {}} />);
    expect(screen.queryByRole('heading', { name: /add account/i })).toBeNull();
  });

  it('defaults presentation to modal', () => {
    render(<AddAccountChooser onClose={() => {}} />);
    expect(screen.queryByRole('button', { name: /^cancel$/i })).toBeNull();
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pnpm test src/accounts/__tests__/AddAccountChooser.test.tsx`
Expected: FAIL — current component renders both in all cases.

- [ ] **Step 3: Modify `AddAccountChooser.tsx`**

Replace the entire file contents:

```tsx
import { useState } from 'react';
import { ipc } from '../lib/ipc';
import { useAppStore } from '../lib/store';
import { AuthPanel } from '../settings/AuthPanel';

interface Props {
  onClose: () => void;
  presentation?: 'modal' | 'fullpane';
}

export function AddAccountChooser({ onClose, presentation = 'modal' }: Props) {
  const accounts = useAppStore((s) => s.accounts);
  const refreshAccounts = useAppStore((s) => s.refreshAccounts);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showOauth, setShowOauth] = useState(false);

  async function importLive() {
    setError(null);
    setBusy(true);
    try {
      await ipc.addAccountFromClaudeCode();
      await refreshAccounts();
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Couldn't import the upstream login.");
    } finally {
      setBusy(false);
    }
  }

  if (showOauth) {
    return <AuthPanel presentation={presentation} onBack={() => setShowOauth(false)} />;
  }

  const showImportTile = true;

  return (
    <div className="flex flex-col gap-[var(--space-md)] px-[var(--popover-pad)] py-[var(--space-md)]">
      {presentation === 'fullpane' && (
        <h2 className="text-[length:var(--text-label)] uppercase tracking-[var(--tracking-label)] text-[color:var(--color-text-secondary)]">
          Add account
        </h2>
      )}
      {showImportTile && (
        <button
          type="button"
          onClick={importLive}
          disabled={busy}
          className="rounded-[var(--radius-sm)] border border-[var(--color-border)] px-[var(--space-md)] py-[var(--space-sm)] text-left hover:bg-[var(--color-track)]"
        >
          <div className="text-[length:var(--text-body)]">Use upstream's current login</div>
          <div className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
            Imports the account the CLI is signed into right now
          </div>
        </button>
      )}
      <button
        type="button"
        onClick={() => setShowOauth(true)}
        className="rounded-[var(--radius-sm)] border border-[var(--color-border)] px-[var(--space-md)] py-[var(--space-sm)] text-left hover:bg-[var(--color-track)]"
      >
        <div className="text-[length:var(--text-body)]">Sign in with a different account</div>
        <div className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
          Opens browser for paste-back OAuth
        </div>
      </button>
      {error && (
        <span className="text-[length:var(--text-micro)] text-[color:var(--color-danger)]">
          {error}
        </span>
      )}
      {presentation === 'fullpane' && (
        <button
          type="button"
          onClick={onClose}
          className="self-start text-[length:var(--text-micro)] text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]"
        >
          Cancel
        </button>
      )}
      <span hidden>{accounts.length}</span>
    </div>
  );
}
```

Note: this references `AuthPanel`'s new `presentation` prop, which Task 6 adds. Tests in this task pass without it because the OAuth branch isn't hit; typecheck will fail until Task 6 lands. Defer typecheck verification to Task 6.

- [ ] **Step 4: Run the chooser's own tests**

Run: `pnpm test src/accounts/__tests__/AddAccountChooser.test.tsx`
Expected: All five tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/accounts/AddAccountChooser.tsx src/accounts/__tests__/AddAccountChooser.test.tsx
git commit -m "feat(accounts): add presentation prop to AddAccountChooser"
```

---

## Task 5: Add `presentation` prop to `SwapConfirmCard`

Purpose: In modal mode, drop the top bar entirely (back button + title + X). Footer Cancel + Switch buttons remain.

**Files:**
- Modify: `src/accounts/SwapConfirmCard.tsx`
- Test: `src/accounts/__tests__/SwapConfirmCard.test.tsx` (create)

- [ ] **Step 1: Read the current file**

Run: `cat src/accounts/SwapConfirmCard.tsx`
Identify the top-bar block (lines ~62-91 per the spec) and the footer button block.

- [ ] **Step 2: Write the failing tests**

Create `src/accounts/__tests__/SwapConfirmCard.test.tsx`:

```tsx
import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { SwapConfirmCard } from '../SwapConfirmCard';

const TARGET = {
  slot: 1, account_uuid: 'uu', email: 'bob@y.com', subscription_type: 'pro',
  is_active: false, org_uuid: null, cached_usage: null, last_error: null,
} as any;

const RUNNING = { cli_processes: 0, vscode_with_extension: [] };

describe('SwapConfirmCard', () => {
  it('renders top bar back button in fullpane mode', () => {
    render(
      <SwapConfirmCard
        current={null}
        target={TARGET}
        running={RUNNING}
        busy={false}
        errorMessage={null}
        onConfirm={() => {}}
        onCancel={() => {}}
        presentation="fullpane"
      />,
    );
    // Fullpane has THREE Cancel-named affordances: top ← Cancel button,
    // top-right <X> with aria-label="Cancel", and footer Cancel button.
    expect(screen.getAllByRole('button', { name: /cancel/i }).length).toBe(3);
  });

  it('drops the top bar in modal mode', () => {
    render(
      <SwapConfirmCard
        current={null}
        target={TARGET}
        running={RUNNING}
        busy={false}
        errorMessage={null}
        onConfirm={() => {}}
        onCancel={() => {}}
        presentation="modal"
      />,
    );
    // Footer Cancel still present; no second "Cancel" affordance in the top bar.
    const cancels = screen.getAllByRole('button', { name: /cancel/i });
    expect(cancels.length).toBe(1);
  });

  it('always renders the footer Switch button', () => {
    render(
      <SwapConfirmCard
        current={null}
        target={TARGET}
        running={RUNNING}
        busy={false}
        errorMessage={null}
        onConfirm={() => {}}
        onCancel={() => {}}
        presentation="modal"
      />,
    );
    expect(screen.getByRole('button', { name: /switch/i })).toBeInTheDocument();
  });

  it('defaults presentation to modal', () => {
    render(
      <SwapConfirmCard
        current={null}
        target={TARGET}
        running={RUNNING}
        busy={false}
        errorMessage={null}
        onConfirm={() => {}}
        onCancel={() => {}}
      />,
    );
    expect(screen.getAllByRole('button', { name: /cancel/i }).length).toBe(1);
  });
});
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `pnpm test src/accounts/__tests__/SwapConfirmCard.test.tsx`
Expected: FAIL — current component always renders the top bar.

- [ ] **Step 4: Modify `SwapConfirmCard.tsx`**

Open the file. At the top, change the `Props` interface to include the new prop:

```ts
interface Props {
  current: AccountListEntry | null;
  target: AccountListEntry;
  running: RunningClaudeCode;
  busy: boolean;
  errorMessage: string | null;
  onConfirm: () => void;
  onCancel: () => void;
  presentation?: 'modal' | 'fullpane';
}
```

In the component signature, destructure with default:

```tsx
export function SwapConfirmCard({
  current, target, running, busy, errorMessage,
  onConfirm, onCancel,
  presentation = 'modal',
}: Props) {
```

Find the top-bar block at the start of the component's return statement. It looks like:

```tsx
<div className="flex items-center justify-between gap-... px-... pt-... pb-...">
  <button onClick={onCancel}>
    <ChevronRight ... className="rotate-180" />
    Cancel
  </button>
  <span ...>Confirm switch</span>
  <IconButton aria-label="Cancel" onClick={onCancel}>
    <X size={13} />
  </IconButton>
</div>
```

This entire `<div>` (containing all three Cancel-named affordances + the "Confirm switch" title) must be wrapped:

```tsx
{presentation === 'fullpane' && (
  <div className="flex items-center justify-between gap-... px-... pt-... pb-...">
    {/* … unchanged children … */}
  </div>
)}
```

The footer Cancel + Switch buttons further down the file stay unchanged.

- [ ] **Step 5: Run tests to verify they pass**

Run: `pnpm test src/accounts/__tests__/SwapConfirmCard.test.tsx`
Expected: All four tests PASS.

- [ ] **Step 6: Commit**

```bash
git add src/accounts/SwapConfirmCard.tsx src/accounts/__tests__/SwapConfirmCard.test.tsx
git commit -m "feat(accounts): add presentation prop to SwapConfirmCard"
```

---

## Task 6: Add `presentation` prop to `AuthPanel`

Purpose: Modal mode skips the drag-handle header and the close-window button. First-run (App.tsx) keeps full chrome via the default.

**Files:**
- Modify: `src/settings/AuthPanel.tsx`
- Test: `src/settings/__tests__/AuthPanel.test.tsx` (create)

- [ ] **Step 1: Write the failing tests**

Create `src/settings/__tests__/AuthPanel.test.tsx`:

```tsx
import { render, screen } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { AuthPanel } from '../AuthPanel';

vi.mock('../../lib/store', () => ({
  useAppStore: (sel: (s: any) => any) => sel({ refreshAccounts: vi.fn() }),
}));

describe('AuthPanel presentation', () => {
  it('renders close-window button in fullpane mode (default)', () => {
    render(<AuthPanel />);
    expect(screen.getByRole('button', { name: /close/i })).toBeInTheDocument();
  });

  it('does NOT render close-window button in modal mode', () => {
    render(<AuthPanel presentation="modal" />);
    expect(screen.queryByRole('button', { name: /close/i })).toBeNull();
  });

  it('renders Connect to Claude heading in both modes', () => {
    const { rerender } = render(<AuthPanel presentation="fullpane" />);
    expect(screen.getByRole('heading', { name: /connect to claude/i })).toBeInTheDocument();
    rerender(<AuthPanel presentation="modal" />);
    expect(screen.getByRole('heading', { name: /connect to claude/i })).toBeInTheDocument();
  });

  it('renders the Back button in modal mode when onBack is provided', () => {
    // The chooser → OAuth flow inside the modal needs a way back to the
    // chooser tile list. Dropping the close-window chrome must not also
    // drop the Back affordance.
    render(<AuthPanel presentation="modal" onBack={() => {}} />);
    expect(screen.getByRole('button', { name: /back/i })).toBeInTheDocument();
  });

  it('does NOT render Back when onBack is omitted', () => {
    render(<AuthPanel presentation="modal" />);
    expect(screen.queryByRole('button', { name: /back/i })).toBeNull();
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pnpm test src/settings/__tests__/AuthPanel.test.tsx`
Expected: FAIL — current component always renders close button.

- [ ] **Step 3: Modify `AuthPanel.tsx`**

Update the `Props` interface (around line 25):

```ts
interface Props {
  onBack?: () => void;
  presentation?: 'modal' | 'fullpane';
}
```

Update the signature:

```tsx
export function AuthPanel({ onBack, presentation = 'fullpane' }: Props) {
```

Replace the entire `return (...)` block. **Before** (current — lines 83-219 of `AuthPanel.tsx`):

```tsx
return (
  <div className="relative flex flex-col h-full">
    <div
      onPointerDown={handleDragStart}
      className={`flex items-center ${onBack ? 'justify-between' : 'justify-end'} gap-[var(--space-sm)] px-[var(--popover-pad)] pt-[var(--space-md)] pb-[var(--space-sm)] cursor-default select-none`}
    >
      {onBack && (
        <button type="button" onClick={onBack} className="inline-flex items-center gap-[var(--space-2xs)] text-[length:var(--text-label)] ...">
          <ChevronRight size={11} className="rotate-180" />
          Back
        </button>
      )}
      <IconButton label="Close" onClick={closeWindow}>
        <X size={13} />
      </IconButton>
    </div>
    <div className="flex items-center justify-center flex-1 min-h-0 overflow-y-auto px-[var(--space-2xl)] pb-[var(--space-2xl)]">
      <motion.div ...>{/* cards */}</motion.div>
    </div>
  </div>
);
```

**After:**

```tsx
const outerClass = presentation === 'fullpane' ? 'relative flex flex-col h-full' : 'relative flex flex-col';

return (
  <div className={outerClass}>
    {presentation === 'fullpane' && (
      <div
        onPointerDown={handleDragStart}
        className={`flex items-center ${onBack ? 'justify-between' : 'justify-end'} gap-[var(--space-sm)] px-[var(--popover-pad)] pt-[var(--space-md)] pb-[var(--space-sm)] cursor-default select-none`}
      >
        {onBack && (
          <button type="button" onClick={onBack} className="inline-flex items-center gap-[var(--space-2xs)] text-[length:var(--text-label)] text-[color:var(--color-text-secondary)] tracking-[var(--tracking-label)] uppercase transition-colors duration-[var(--duration-fast)] hover:text-[color:var(--color-text)] focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)] focus-visible:outline-offset-2 rounded">
            <ChevronRight size={11} className="rotate-180" />
            Back
          </button>
        )}
        <IconButton label="Close" onClick={closeWindow}>
          <X size={13} />
        </IconButton>
      </div>
    )}
    {presentation === 'modal' && onBack && (
      <div className="flex items-center px-[var(--popover-pad)] pt-[var(--space-sm)] pb-[var(--space-2xs)]">
        <button type="button" onClick={onBack} className="inline-flex items-center gap-[var(--space-2xs)] text-[length:var(--text-label)] text-[color:var(--color-text-secondary)] tracking-[var(--tracking-label)] uppercase transition-colors duration-[var(--duration-fast)] hover:text-[color:var(--color-text)] focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)] focus-visible:outline-offset-2 rounded">
          <ChevronRight size={11} className="rotate-180" />
          Back
        </button>
      </div>
    )}
    <div className="flex items-center justify-center flex-1 min-h-0 overflow-y-auto px-[var(--space-2xl)] pb-[var(--space-2xl)]">
      <motion.div ...>{/* unchanged cards block */}</motion.div>
    </div>
  </div>
);
```

Key points:
- Modal mode drops the drag handle + close-window button (review item #1).
- Modal mode renders a slim Back-only header when `onBack` is provided — so the chooser → OAuth → back-to-chooser path still works inside the modal.
- The `<motion.div>` with the three Cards inside is the existing content; copy it over unchanged.

- [ ] **Step 4: Run tests to verify they pass**

Run: `pnpm test src/settings/__tests__/AuthPanel.test.tsx`
Expected: All three tests PASS.

- [ ] **Step 5: Full typecheck**

Run: `pnpm lint`
Expected: Exit 0. Tasks 4–6 are now consistent.

- [ ] **Step 6: Commit**

```bash
git add src/settings/AuthPanel.tsx src/settings/__tests__/AuthPanel.test.tsx
git commit -m "feat(auth): add presentation prop to AuthPanel"
```

---

## Task 7: Extract `useAccountManagement` hook

Purpose: Lift `AccountsPanel`'s entire state machine into a reusable hook. Fix the reauth-listener race by holding a stable listener with a ref-read slot.

**Files:**
- Create: `src/accounts/useAccountManagement.ts`
- Test: `src/accounts/__tests__/useAccountManagement.test.tsx`

- [ ] **Step 1: Write the failing tests**

Create `src/accounts/__tests__/useAccountManagement.test.tsx`:

```tsx
import { renderHook, act, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';

const mockSubscribers: Record<string, (payload: any) => void> = {};

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((name: string, cb: (e: any) => void) => {
    mockSubscribers[name] = (payload) => cb({ payload });
    return Promise.resolve(() => { delete mockSubscribers[name]; });
  }),
}));

vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: vi.fn().mockResolvedValue(undefined),
}));

const ipcMock = {
  forceRefresh: vi.fn().mockResolvedValue(undefined),
  detectRunningClaudeCode: vi.fn().mockResolvedValue({ cli_processes: 0, vscode_with_extension: [] }),
  swapToAccount: vi.fn().mockResolvedValue({
    new_active_slot: 2,
    running: { cli_processes: 0, vscode_with_extension: [] },
  }),
  removeAccount: vi.fn().mockResolvedValue(undefined),
  startOauthFlow: vi.fn().mockResolvedValue('https://example.com/oauth'),
};

vi.mock('../../lib/ipc', () => ({ ipc: ipcMock }));

const ACCOUNTS = [
  { slot: 1, email: 'a@x.com', is_active: true, org_uuid: 'org1', account_uuid: 'u1' },
  { slot: 2, email: 'b@y.com', is_active: false, org_uuid: 'org1', account_uuid: 'u2' },
];

vi.mock('../../lib/store', () => ({
  useAppStore: (sel: (s: any) => any) =>
    sel({
      accounts: ACCOUNTS,
      settings: { thresholds: [75, 90] },
      refreshAccounts: vi.fn(),
      setPendingSwapReport: vi.fn(),
    }),
}));

import { useAccountManagement } from '../useAccountManagement';

describe('useAccountManagement', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    Object.keys(mockSubscribers).forEach((k) => delete mockSubscribers[k]);
  });

  it('exposes accounts and current active', () => {
    const { result } = renderHook(() => useAccountManagement());
    expect(result.current.accounts).toHaveLength(2);
    expect(result.current.currentActive?.slot).toBe(1);
  });

  it('opens and closes the chooser', () => {
    const { result } = renderHook(() => useAccountManagement());
    expect(result.current.chooserOpen).toBe(false);
    act(() => result.current.openChooser());
    expect(result.current.chooserOpen).toBe(true);
    act(() => result.current.closeChooser());
    expect(result.current.chooserOpen).toBe(false);
  });

  it('requestSwap sets pending state', async () => {
    const { result } = renderHook(() => useAccountManagement());
    await act(async () => { await result.current.requestSwap(ACCOUNTS[1] as any); });
    expect(result.current.pending?.target.slot).toBe(2);
  });

  it('requestSwap is a no-op for the active account', async () => {
    const { result } = renderHook(() => useAccountManagement());
    await act(async () => { await result.current.requestSwap(ACCOUNTS[0] as any); });
    expect(result.current.pending).toBe(null);
  });

  it('confirmSwap calls ipc.swapToAccount and clears pending on success', async () => {
    const { result } = renderHook(() => useAccountManagement());
    await act(async () => { await result.current.requestSwap(ACCOUNTS[1] as any); });
    await act(async () => { await result.current.confirmSwap(); });
    expect(ipcMock.swapToAccount).toHaveBeenCalledWith(2);
    expect(result.current.pending).toBe(null);
  });

  it('handleReauth opens browser and records the pending slot', async () => {
    const { result } = renderHook(() => useAccountManagement());
    await act(async () => { await result.current.handleReauth(ACCOUNTS[1] as any); });
    expect(ipcMock.startOauthFlow).toHaveBeenCalled();
    expect(result.current.reauthSlot).toBe(2);
  });

  it('reauth listener clears reauthSlot on oauth_complete', async () => {
    const { result } = renderHook(() => useAccountManagement());
    await act(async () => { await result.current.handleReauth(ACCOUNTS[1] as any); });
    await waitFor(() => expect(mockSubscribers['oauth_complete']).toBeDefined());
    act(() => { mockSubscribers['oauth_complete'](2); });
    await waitFor(() => expect(result.current.reauthSlot).toBe(null));
  });

  it('handleRefreshAll calls forceRefresh("all") and sets refreshing true then false', async () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => useAccountManagement());
    await act(async () => { await result.current.handleRefreshAll(); });
    expect(ipcMock.forceRefresh).toHaveBeenCalledWith('all');
    expect(result.current.refreshing).toBe(true);
    await act(async () => { await vi.runAllTimersAsync(); });
    expect(result.current.refreshing).toBe(false);
    vi.useRealTimers();
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pnpm test src/accounts/__tests__/useAccountManagement.test.tsx`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement the hook**

Create `src/accounts/useAccountManagement.ts`:

```ts
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { openUrl } from '@tauri-apps/plugin-opener';
import { useAppStore } from '../lib/store';
import { ipc } from '../lib/ipc';
import type {
  AccountListEntry,
  RunningClaudeCode,
} from '../lib/generated/bindings';

interface PendingSwap {
  target: AccountListEntry;
  running: RunningClaudeCode;
}

export function useAccountManagement() {
  const accounts = useAppStore((s) => s.accounts);
  const refreshAccounts = useAppStore((s) => s.refreshAccounts);
  const setPendingSwapReport = useAppStore((s) => s.setPendingSwapReport);

  const [chooserOpen, setChooserOpen] = useState(false);
  const [pending, setPending] = useState<PendingSwap | null>(null);
  const [swappingSlot, setSwappingSlot] = useState<number | null>(null);
  const [confirmError, setConfirmError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [reauthSlot, setReauthSlot] = useState<number | null>(null);

  // Stable-listener pattern: the listener mounts once when *any* reauth
  // becomes pending and stays mounted until none are. The dep is the
  // boolean `reauthPending`, not `reauthSlot` itself — switching slots
  // (e.g. user starts slot 2, then quickly slot 3) does NOT re-mount the
  // listener and so cannot race the pending listen() promise. The handler
  // reads the current slot from a ref.
  const reauthSlotRef = useRef<number | null>(null);
  useEffect(() => { reauthSlotRef.current = reauthSlot; }, [reauthSlot]);

  const reauthPending = reauthSlot !== null;
  useEffect(() => {
    if (!reauthPending) return;
    let unlistenComplete: (() => void) | undefined;
    let unlistenError: (() => void) | undefined;
    listen<number>('oauth_complete', () => {
      if (reauthSlotRef.current !== null) {
        refreshAccounts().catch(() => {});
        setReauthSlot(null);
      }
    }).then((f) => { unlistenComplete = f; });
    listen<string>('oauth_error', () => {
      if (reauthSlotRef.current !== null) setReauthSlot(null);
    }).then((f) => { unlistenError = f; });
    return () => {
      unlistenComplete?.();
      unlistenError?.();
    };
  }, [reauthPending, refreshAccounts]);

  const orgGroups = useMemo(() => {
    const map = new Map<string, AccountListEntry>();
    for (const a of accounts) {
      if (a.org_uuid && !map.has(a.org_uuid)) map.set(a.org_uuid, a);
    }
    return map;
  }, [accounts]);

  const currentActive = useMemo(
    () => accounts.find((a) => a.is_active) ?? null,
    [accounts],
  );

  const openChooser = useCallback(() => setChooserOpen(true), []);
  const closeChooser = useCallback(() => setChooserOpen(false), []);

  const requestSwap = useCallback(
    async (entry: AccountListEntry) => {
      if (entry.is_active || swappingSlot !== null) return;
      setConfirmError(null);
      let running: RunningClaudeCode = { cli_processes: 0, vscode_with_extension: [] };
      try {
        running = await ipc.detectRunningClaudeCode();
      } catch {
        // Best-effort detection.
      }
      setPending({ target: entry, running });
    },
    [swappingSlot],
  );

  const confirmSwap = useCallback(async () => {
    if (!pending || swappingSlot !== null) return;
    setConfirmError(null);
    setSwappingSlot(pending.target.slot);
    try {
      const report = await ipc.swapToAccount(pending.target.slot);
      setPendingSwapReport(report);
      await refreshAccounts();
      setPending(null);
    } catch (e) {
      setConfirmError(e instanceof Error ? e.message : 'Swap failed');
    } finally {
      setSwappingSlot(null);
    }
  }, [pending, swappingSlot, refreshAccounts, setPendingSwapReport]);

  const cancelSwap = useCallback(() => {
    setPending(null);
    setConfirmError(null);
  }, []);

  const handleReauth = useCallback(async (entry: AccountListEntry) => {
    if (reauthSlot !== null) return;
    setReauthSlot(entry.slot);
    try {
      const url = await ipc.startOauthFlow(false);
      await openUrl(url);
    } catch {
      setReauthSlot(null);
    }
  }, [reauthSlot]);

  const handleRemove = useCallback(async (entry: AccountListEntry) => {
    await ipc.removeAccount(entry.slot);
    await refreshAccounts();
  }, [refreshAccounts]);

  const handleRefreshAll = useCallback(async () => {
    if (refreshing) return;
    setRefreshing(true);
    try {
      await ipc.forceRefresh('all');
    } catch {
      // Loop logs failures.
    }
    const staggerTotalMs = Math.max(0, (accounts.length - 1) * 30_000) + 2_000;
    setTimeout(() => setRefreshing(false), staggerTotalMs);
  }, [refreshing, accounts.length]);

  return {
    accounts,
    currentActive,
    orgGroups,
    pending,
    swappingSlot,
    confirmError,
    refreshing,
    reauthSlot,
    chooserOpen,
    requestSwap,
    confirmSwap,
    cancelSwap,
    handleReauth,
    handleRemove,
    handleRefreshAll,
    openChooser,
    closeChooser,
  };
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `pnpm test src/accounts/__tests__/useAccountManagement.test.tsx`
Expected: All eight tests PASS.

- [ ] **Step 5: Typecheck**

Run: `pnpm lint`
Expected: Exit 0.

- [ ] **Step 6: Commit**

```bash
git add src/accounts/useAccountManagement.ts src/accounts/__tests__/useAccountManagement.test.tsx
git commit -m "feat(accounts): extract useAccountManagement hook with stable reauth listener"
```

---

## Task 8: Refactor `AccountsPanel` to use the hook + render flows as modals

Purpose: Compact view now consumes the shared hook. AddAccountChooser and SwapConfirmCard render as modal overlays instead of full-pane replacements.

**Files:**
- Modify: `src/accounts/AccountsPanel.tsx`

- [ ] **Step 1: Replace the file contents**

Replace `src/accounts/AccountsPanel.tsx` with:

```tsx
import { motion } from 'framer-motion';
import { useAppStore } from '../lib/store';
import { IconButton } from '../components/ui/IconButton';
import { AccountRow } from './AccountRow';
import { AddAccountChooser } from './AddAccountChooser';
import { SwapConfirmCard } from './SwapConfirmCard';
import { ModalShell } from '../components/modals/ModalShell';
import { IconRefresh } from '../lib/icons';
import { useAccountManagement } from './useAccountManagement';

interface Props {
  onBack: () => void;
}

export function AccountsPanel({ onBack }: Props) {
  const thresholds = useAppStore(
    (s) => (s.settings?.thresholds ?? [75, 90]) as [number, number],
  );

  const {
    accounts,
    orgGroups,
    currentActive,
    pending,
    swappingSlot,
    confirmError,
    refreshing,
    reauthSlot,
    chooserOpen,
    requestSwap,
    confirmSwap,
    cancelSwap,
    handleReauth,
    handleRemove,
    handleRefreshAll,
    openChooser,
    closeChooser,
  } = useAccountManagement();

  return (
    <div className="flex h-full w-full flex-col">
      <div className="flex items-center justify-between px-[var(--popover-pad)] pt-[var(--space-md)] pb-[var(--space-sm)]">
        <button
          type="button"
          onClick={onBack}
          className="text-[length:var(--text-label)] text-[color:var(--color-text-secondary)] hover:text-[color:var(--color-text)]"
        >
          ← Back
        </button>
        <span className="text-[length:var(--text-label)] uppercase tracking-[var(--tracking-label)] text-[color:var(--color-text-secondary)]">
          Accounts
        </span>
        <IconButton label="Refresh all" onClick={handleRefreshAll}>
          <motion.span
            animate={refreshing ? { rotate: 360 } : { rotate: 0 }}
            transition={
              refreshing
                ? { duration: 0.7, ease: 'linear', repeat: Infinity }
                : { duration: 0.2 }
            }
            style={{ display: 'inline-flex' }}
          >
            <IconRefresh size={13} />
          </motion.span>
        </IconButton>
      </div>

      <div className="flex-1 overflow-y-auto">
        {accounts.length === 0 && (
          <div className="px-[var(--popover-pad)] py-[var(--space-md)] text-[color:var(--color-text-muted)]">
            No accounts managed yet.
          </div>
        )}
        {accounts.map((a) => {
          const groupHead = a.org_uuid ? orgGroups.get(a.org_uuid) : undefined;
          const shareHint = groupHead && groupHead.slot !== a.slot ? groupHead.email : null;
          return (
            <AccountRow
              key={a.slot}
              entry={a}
              thresholds={thresholds}
              shareHint={shareHint}
              onSwap={() => requestSwap(a)}
              swapBusy={swappingSlot !== null}
              swapping={swappingSlot === a.slot}
              onReauth={() => handleReauth(a)}
              reauthBusy={reauthSlot === a.slot}
              onRemove={() => handleRemove(a)}
            />
          );
        })}

        <div className="px-[var(--popover-pad)] py-[var(--space-md)]">
          <button
            type="button"
            onClick={openChooser}
            className="text-[length:var(--text-label)] text-[color:var(--color-accent)] hover:underline"
          >
            + Add account
          </button>
        </div>
      </div>

      {chooserOpen && (
        <ModalShell id="add-account-chooser" onDismiss={closeChooser} title="Add account">
          <AddAccountChooser presentation="modal" onClose={closeChooser} />
        </ModalShell>
      )}

      {pending && (
        <ModalShell id="swap-confirm" onDismiss={cancelSwap} title="Confirm switch">
          <SwapConfirmCard
            presentation="modal"
            current={currentActive}
            target={pending.target}
            running={pending.running}
            busy={swappingSlot !== null}
            errorMessage={confirmError}
            onConfirm={confirmSwap}
            onCancel={cancelSwap}
          />
        </ModalShell>
      )}
    </div>
  );
}
```

- [ ] **Step 2: Run all accounts tests**

Run: `pnpm test src/accounts/`
Expected: All pre-existing AccountRow/WarmupToggle/ScheduleSelector/WarmupNowButton tests still PASS, plus the new ones from earlier tasks.

- [ ] **Step 3: Typecheck**

Run: `pnpm lint`
Expected: Exit 0.

- [ ] **Step 4: Manual smoke test**

Run: `pnpm tauri dev`
1. Open the popover. From the home view, click your account email → Accounts panel opens.
2. Click `+ Add account` → modal overlay appears with backdrop (not a full-pane replace).
3. ESC dismisses the modal.
4. Hover a non-active row, click `Switch account` → swap-confirm modal appears.
5. ESC dismisses.
6. Click `Switch account` again → confirm → swap completes, sidebar reflects new active.
Quit dev server when done.

- [ ] **Step 5: Commit**

```bash
git add src/accounts/AccountsPanel.tsx
git commit -m "refactor(accounts): adopt useAccountManagement, render flows as modals"
```

---

## Task 9: Build `SettingsModal`

Purpose: Tiny wrapper that mounts SettingsPanel inside a ModalShell. Used only by the expanded view.

**Files:**
- Create: `src/components/modals/SettingsModal.tsx`
- Test: `src/components/modals/__tests__/SettingsModal.test.tsx`

- [ ] **Step 1: Write the failing tests**

Create `src/components/modals/__tests__/SettingsModal.test.tsx`:

```tsx
import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { SettingsModal } from '../SettingsModal';
import { useAppStore } from '../../../lib/store';

vi.mock('../../../settings/SettingsPanel', () => ({
  SettingsPanel: () => <div data-testid="settings-panel-content">panel</div>,
}));

describe('SettingsModal', () => {
  beforeEach(() => {
    useAppStore.getState().resetModalStack();
  });

  it('renders SettingsPanel content and Settings title', () => {
    render(<SettingsModal onDismiss={() => {}} />);
    expect(screen.getByTestId('settings-panel-content')).toBeInTheDocument();
    expect(screen.getByText(/^settings$/i)).toBeInTheDocument();
  });

  it('calls onDismiss when the title-bar close button is clicked', () => {
    const fn = vi.fn();
    render(<SettingsModal onDismiss={fn} />);
    fireEvent.click(screen.getByRole('button', { name: /close/i }));
    expect(fn).toHaveBeenCalledTimes(1);
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pnpm test src/components/modals/__tests__/SettingsModal.test.tsx`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

Create `src/components/modals/SettingsModal.tsx`:

```tsx
import { ModalShell } from './ModalShell';
import { SettingsPanel } from '../../settings/SettingsPanel';

interface Props {
  onDismiss: () => void;
}

export function SettingsModal({ onDismiss }: Props) {
  return (
    <ModalShell id="settings-modal" onDismiss={onDismiss} size="lg" title="Settings">
      <div className="px-[var(--space-md)] py-[var(--space-md)] max-h-[480px] overflow-y-auto">
        <SettingsPanel />
      </div>
    </ModalShell>
  );
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `pnpm test src/components/modals/__tests__/SettingsModal.test.tsx`
Expected: Both tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/modals/SettingsModal.tsx src/components/modals/__tests__/SettingsModal.test.tsx
git commit -m "feat(modals): add SettingsModal"
```

---

## Task 10: Build `AccountsSidebar`

Purpose: The expanded-view left rail. Header (label + refresh-all), scrollable list of AccountRow, footer "+ Add account". Mounts the chooser + swap-confirm modals.

**Files:**
- Create: `src/accounts/AccountsSidebar.tsx`
- Test: `src/accounts/__tests__/AccountsSidebar.test.tsx`

- [ ] **Step 1: Write the failing tests**

Create `src/accounts/__tests__/AccountsSidebar.test.tsx`:

```tsx
import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { useAppStore } from '../../lib/store';

vi.mock('../useAccountManagement', () => ({
  useAccountManagement: () => ({
    accounts: [
      { slot: 1, email: 'a@x.com', is_active: true, account_uuid: 'u1', org_uuid: 'g1', cached_usage: null, last_error: null, subscription_type: 'max' },
      { slot: 2, email: 'b@y.com', is_active: false, account_uuid: 'u2', org_uuid: 'g1', cached_usage: null, last_error: null, subscription_type: 'pro' },
    ],
    orgGroups: new Map(),
    currentActive: { slot: 1, email: 'a@x.com', is_active: true, account_uuid: 'u1' },
    pending: null,
    swappingSlot: null,
    confirmError: null,
    refreshing: false,
    reauthSlot: null,
    chooserOpen: false,
    requestSwap: vi.fn(),
    confirmSwap: vi.fn(),
    cancelSwap: vi.fn(),
    handleReauth: vi.fn(),
    handleRemove: vi.fn(),
    handleRefreshAll: vi.fn(),
    openChooser: vi.fn(),
    closeChooser: vi.fn(),
  }),
}));

vi.mock('../../lib/store', async () => {
  const actual = await vi.importActual<typeof import('../../lib/store')>('../../lib/store');
  return {
    ...actual,
    useAppStore: ((sel: any) => sel({
      settings: { thresholds: [75, 90] },
      modalStack: [],
      pushModal: vi.fn(),
      popModal: vi.fn(),
      isTopmost: () => true,
    })) as any,
  };
});

import { AccountsSidebar } from '../AccountsSidebar';

describe('AccountsSidebar', () => {
  beforeEach(() => {
    useAppStore.getState().resetModalStack?.();
  });

  it('renders an ACCOUNTS label', () => {
    render(<AccountsSidebar />);
    expect(screen.getByText(/^accounts$/i)).toBeInTheDocument();
  });

  it('renders a refresh-all button', () => {
    render(<AccountsSidebar />);
    expect(screen.getByRole('button', { name: /refresh all/i })).toBeInTheDocument();
  });

  it('renders one row per account', () => {
    render(<AccountsSidebar />);
    expect(screen.getByText('a@x.com')).toBeInTheDocument();
    expect(screen.getByText('b@y.com')).toBeInTheDocument();
  });

  it('renders the + Add account button', () => {
    render(<AccountsSidebar />);
    expect(screen.getByRole('button', { name: /\+ add account/i })).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pnpm test src/accounts/__tests__/AccountsSidebar.test.tsx`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement `AccountsSidebar`**

Create `src/accounts/AccountsSidebar.tsx`:

```tsx
import { motion } from 'framer-motion';
import { useAppStore } from '../lib/store';
import { IconButton } from '../components/ui/IconButton';
import { AccountRow } from './AccountRow';
import { AddAccountChooser } from './AddAccountChooser';
import { SwapConfirmCard } from './SwapConfirmCard';
import { ModalShell } from '../components/modals/ModalShell';
import { IconRefresh } from '../lib/icons';
import { useAccountManagement } from './useAccountManagement';

export function AccountsSidebar() {
  const thresholds = useAppStore(
    (s) => (s.settings?.thresholds ?? [75, 90]) as [number, number],
  );

  const {
    accounts,
    orgGroups,
    currentActive,
    pending,
    swappingSlot,
    confirmError,
    refreshing,
    reauthSlot,
    chooserOpen,
    requestSwap,
    confirmSwap,
    cancelSwap,
    handleReauth,
    handleRemove,
    handleRefreshAll,
    openChooser,
    closeChooser,
  } = useAccountManagement();

  return (
    <aside
      className="
        flex flex-col h-full
        w-[300px] shrink-0
        border-r border-[var(--color-rule)]
        bg-[var(--color-bg-surface)]
      "
    >
      <div className="flex items-center justify-between px-[var(--space-lg)] pt-[var(--space-md)] pb-[var(--space-sm)] shrink-0">
        <span className="text-[length:var(--text-label)] uppercase tracking-[var(--tracking-label)] text-[color:var(--color-text-secondary)] font-[var(--weight-semibold)]">
          Accounts
        </span>
        <IconButton label="Refresh all" onClick={handleRefreshAll}>
          <motion.span
            animate={refreshing ? { rotate: 360 } : { rotate: 0 }}
            transition={
              refreshing
                ? { duration: 0.7, ease: 'linear', repeat: Infinity }
                : { duration: 0.2 }
            }
            style={{ display: 'inline-flex' }}
          >
            <IconRefresh size={13} />
          </motion.span>
        </IconButton>
      </div>

      <div className="flex-1 overflow-y-auto">
        {accounts.length === 0 && (
          <div className="px-[var(--space-lg)] py-[var(--space-md)] text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
            No accounts managed yet.
          </div>
        )}
        {accounts.map((a) => {
          const groupHead = a.org_uuid ? orgGroups.get(a.org_uuid) : undefined;
          const shareHint = groupHead && groupHead.slot !== a.slot ? groupHead.email : null;
          return (
            <AccountRow
              key={a.slot}
              entry={a}
              thresholds={thresholds}
              shareHint={shareHint}
              onSwap={() => requestSwap(a)}
              swapBusy={swappingSlot !== null}
              swapping={swappingSlot === a.slot}
              onReauth={() => handleReauth(a)}
              reauthBusy={reauthSlot === a.slot}
              onRemove={() => handleRemove(a)}
            />
          );
        })}
      </div>

      <div className="shrink-0 px-[var(--space-lg)] py-[var(--space-md)] border-t border-[var(--color-rule)]">
        <button
          type="button"
          onClick={openChooser}
          className="text-[length:var(--text-label)] text-[color:var(--color-accent)] hover:underline"
        >
          + Add account
        </button>
      </div>

      {chooserOpen && (
        <ModalShell id="add-account-chooser" onDismiss={closeChooser} title="Add account">
          <AddAccountChooser presentation="modal" onClose={closeChooser} />
        </ModalShell>
      )}

      {pending && (
        <ModalShell id="swap-confirm" onDismiss={cancelSwap} title="Confirm switch">
          <SwapConfirmCard
            presentation="modal"
            current={currentActive}
            target={pending.target}
            running={pending.running}
            busy={swappingSlot !== null}
            errorMessage={confirmError}
            onConfirm={confirmSwap}
            onCancel={cancelSwap}
          />
        </ModalShell>
      )}
    </aside>
  );
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `pnpm test src/accounts/__tests__/AccountsSidebar.test.tsx`
Expected: All four tests PASS.

- [ ] **Step 5: Typecheck**

Run: `pnpm lint`
Expected: Exit 0.

- [ ] **Step 6: Commit**

```bash
git add src/accounts/AccountsSidebar.tsx src/accounts/__tests__/AccountsSidebar.test.tsx
git commit -m "feat(accounts): add AccountsSidebar for expanded view"
```

---

## Task 11: Wire `AccountsSidebar` + settings cog into `ExpandedReport`

Purpose: Expanded view now has the sidebar on the left and a settings cog in the header that opens `SettingsModal`.

**Files:**
- Modify: `src/report/ExpandedReport.tsx`

- [ ] **Step 1: Inspect the current icons module**

Run: `grep -n "IconSettings\|^export " src/lib/icons.ts 2>/dev/null || cat src/lib/icons.ts 2>&1 | head -30`
Confirm `IconSettings` is already exported (it is — used in `CompactPopover`).

- [ ] **Step 2: Apply the layout edit**

Open `src/report/ExpandedReport.tsx`. Make these changes:

1. Add imports at the top:

```ts
import { useState } from 'react';
import { AccountsSidebar } from '../accounts/AccountsSidebar';
import { SettingsModal } from '../components/modals/SettingsModal';
import { IconSettings } from '../lib/icons';
```

Adjust the existing `useState` / `useLayoutEffect` / `useRef` import to include `useState` if not already.

2. In `ExpandedReport()`, add a state hook near the others:

```ts
const [settingsOpen, setSettingsOpen] = useState(false);
```

3. In the header `<div className="flex items-center gap-[2px]">` block (currently containing Refresh / Collapse / Close), insert the settings cog between Refresh and Collapse:

```tsx
<IconButton label="Settings" onClick={() => setSettingsOpen(true)}>
  <IconSettings size={13} />
</IconButton>
```

4. Restructure the outer return so the sidebar sits to the left of the existing body. The current outermost element is a flex column; wrap its non-header content in a flex row that also contains the sidebar:

Find this structure:
```tsx
return (
  <div className="flex h-full flex-col overflow-hidden" style={{...}}>
    <header ...>{/* existing */}</header>
    {usage && (<>...</>)}
    <TabBar .../>
    <div className="flex-1 overflow-y-auto px-... pt-...">...</div>
  </div>
);
```

Change to:
```tsx
return (
  <>
    <div
      className="flex h-full overflow-hidden"
      style={{
        width: '100%',
        minHeight: 'var(--report-min-height)',
        background: 'var(--color-bg-base)',
      }}
    >
      <AccountsSidebar />
      <div className="flex flex-1 flex-col overflow-hidden min-w-0">
        <header ...>{/* existing, unchanged except settings cog added */}</header>
        {usage && (<>...</>)}
        <TabBar .../>
        <div className="flex-1 overflow-y-auto px-... pt-...">...</div>
      </div>
    </div>
    {settingsOpen && <SettingsModal onDismiss={() => setSettingsOpen(false)} />}
  </>
);
```

Key points:
- **Preserve the existing `style` props.** The current outer `<div>` carries `width: '100%'`, `minHeight: 'var(--report-min-height)'`, `background: 'var(--color-bg-base)'` (lines ~70-74). These move to the new outer row container, NOT to the inner column.
- Outermost is now `flex h-full overflow-hidden` (row direction, dropped `flex-col`).
- `min-w-0` on the inner column lets the report shrink correctly when the sidebar reserves its 300px.
- **Two `aria-label="Close"` buttons will coexist** once SettingsModal mounts: the existing window-close button in the report header (calls `closeWindow`), and the ModalShell's title-bar Close (calls `setSettingsOpen(false)`). Functionally fine — ModalShell stops propagation. But any future E2E test using `getByRole('button', { name: /close/i })` while the modal is open will be ambiguous. Use `getAllByRole` + position-based selection in such tests.

- [ ] **Step 3: Typecheck**

Run: `pnpm lint`
Expected: Exit 0.

- [ ] **Step 4: Run full test suite**

Run: `pnpm test`
Expected: All tests PASS.

- [ ] **Step 5: Manual smoke test**

Run: `pnpm tauri dev`
1. Expand the window (the ⤢ icon on the popover). Confirm the sidebar appears on the left.
2. Confirm the settings cog appears between Refresh and Collapse in the header.
3. Click the cog → SettingsModal opens with title "Settings", backdrop visible.
4. ESC dismisses; click outside dismisses; X dismisses.
5. From the sidebar, click `+ Add account` → chooser modal opens.
6. Hover a non-active row, click `Switch account` → swap-confirm modal opens.
7. Confirm a swap; sidebar updates to reflect new active.
8. Collapse to compact, confirm compact behavior is unchanged (compact uses pane routes for both settings and accounts).
9. From compact accounts panel, repeat add/swap — both should now also use modal overlays (the §6.1 small UX shift).

Quit dev server when done.

- [ ] **Step 6: Commit**

```bash
git add src/report/ExpandedReport.tsx
git commit -m "feat(report): mount AccountsSidebar and SettingsModal in expanded view"
```

---

## Task 12: Pre-merge gate — tab screenshot review at 660px

Purpose: Verify every report tab survives the width reduction from ~896px to 660px. Spec §7 requires this before merge.

**Files:** None.

- [ ] **Step 1: Run the app**

Run: `pnpm tauri dev`

- [ ] **Step 2: Expand the window**

Click the ⤢ icon in the popover.

- [ ] **Step 3: Capture each tab**

For each of the six tabs (Sessions, Models, Trends, Projects, Heatmap, Cache):
1. Click the tab.
2. Visually inspect for: clipped axis ticks, broken legends, horizontal scroll inside tab content, overlapping labels.
3. Capture a screenshot (OS-native: Win+Shift+S on Windows, Cmd+Shift+4 on macOS) and save to `/tmp/tab-<name>.png` or equivalent.

- [ ] **Step 4: Verify no regressions**

If any tab shows clipping or overflow:
- File a follow-up task in `docs/superpowers/plans/` describing the tab and the breakage. Fix-before-merge per spec.
- If the fix is small (e.g., axis tick angle, legend layout), apply it now in the relevant tab file under `src/report/`.

If all tabs render cleanly:

- [ ] **Step 5: Final typecheck and test run**

Run: `pnpm lint && pnpm test`
Expected: Exit 0; all tests PASS.

- [ ] **Step 6: Verify compact-view smoke**

Run: `pnpm tauri dev`
1. Collapse to compact.
2. From home view, click email → accounts panel.
3. Click `+ Add account` → modal opens (no full-pane replace).
4. Click `Switch account` on a non-active row → swap-confirm modal.
5. ESC dismisses.

This confirms the §6.1 acknowledged UX shift in compact works as designed.

- [ ] **Step 7: Final commit if any tab fixes were applied**

```bash
git add src/report/
git commit -m "fix(report): tighten chart density for 660px width"
```

If no fixes were needed, no commit. The plan is complete.

---

## Notes for the implementer

- **Cream-theme dependency:** ModalShell's surfaces use `var(--color-overlay)`, `var(--color-bg-elevated)`, `var(--color-border)`. These tokens exist today against the current dark theme. The cream-theme spec (separate work) refines them. If cream-theme has already shipped when you start, no change. If not, your implementation will pick up the cream surfaces automatically when that work merges.

- **The "claude" substring rule:** The pre-commit hook rejects any case-insensitive `claude` occurrence in commit messages. All commit messages in this plan have been audited. If you need to write a new one, refer to "the upstream CLI."

- **AccountRow is unchanged.** Both surfaces (AccountsPanel, AccountsSidebar) render it with the same prop shape it accepts today. If you find yourself editing AccountRow, stop and ask — the spec explicitly keeps it untouched.

- **OAuth flow inside the chooser modal:** When the user picks "Sign in with a different account," `AddAccountChooser` swaps its own render to `<AuthPanel presentation="modal" />` inside the same `<ModalShell>`. There's no nested shell. The title remains "Add account" — keeping it stable across the in-modal route swap is acceptable per spec; promoting to "Connect to Claude" was mentioned as an option but not required.

- **Stagger spinner duration:** `(accounts.length - 1) * 30_000 + 2_000` ms. For 1 account, the spinner runs only 2s. This is intentional — verifies "I clicked it" without lying about a stagger that won't happen.
