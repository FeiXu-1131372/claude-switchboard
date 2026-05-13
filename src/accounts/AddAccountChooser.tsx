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
