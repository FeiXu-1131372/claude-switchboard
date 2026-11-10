import { useState } from 'react';
import { ipc } from '../lib/ipc';
import { useAppStore } from '../lib/store';

export function UnmanagedActiveBanner() {
  const unmanagedActive = useAppStore((s) => s.unmanagedActive);
  const dismissBanner = useAppStore((s) => s.dismissBanner);
  const refreshAccounts = useAppStore((s) => s.refreshAccounts);
  const [busy, setBusy] = useState(false);

  if (!unmanagedActive) return null;

  async function add() {
    setBusy(true);
    try {
      await ipc.addAccountFromClaudeCode();
      await refreshAccounts();
      dismissBanner('unmanagedActive');
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex items-center gap-[var(--space-sm)] rounded-[var(--radius-sm)] border border-[var(--color-warn)] bg-[var(--color-warn-dim)] px-[var(--space-sm)] py-[var(--space-2xs)]">
      <span className="flex-1 text-[length:var(--text-micro)]">
        Upstream is signed in as {unmanagedActive.email} — not managed.
      </span>
      <button
        type="button"
        onClick={add}
        disabled={busy}
        className="text-[length:var(--text-micro)] text-[color:var(--color-accent-text)] hover:underline"
      >
        Add to accounts
      </button>
      <button
        type="button"
        onClick={() => dismissBanner('unmanagedActive')}
        className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]"
      >
        Dismiss
      </button>
    </div>
  );
}
