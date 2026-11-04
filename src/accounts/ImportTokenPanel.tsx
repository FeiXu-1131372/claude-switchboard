import { useRef, useState, type FormEvent } from 'react';
import { Button } from '../components/ui/Button';
import { ipc } from '../lib/ipc';
import { useAppStore } from '../lib/store';
import type { AccountListEntry } from '../lib/generated/bindings';

interface Props {
  onBack: () => void;
  onSuccess: () => void;
  onBusyChange?: (busy: boolean) => void;
  account?: AccountListEntry | null;
}

export function ImportTokenPanel({ onBack, onSuccess, onBusyChange, account }: Props) {
  const refreshAccounts = useAppStore((s) => s.refreshAccounts);
  const tokenInput = useRef<HTMLInputElement>(null);
  const [label, setLabel] = useState(account?.email ?? '');
  const [hasToken, setHasToken] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (busy || !hasToken || account?.is_active) return;
    const token = tokenInput.current?.value.trim() ?? '';
    if (tokenInput.current) tokenInput.current.value = '';
    setHasToken(false);
    setError(null);
    setBusy(true);
    onBusyChange?.(true);
    try {
      await ipc.importLongLivedToken(token, label.trim() || null, account?.slot ?? null);
      await refreshAccounts();
      onSuccess();
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Could not import the token. Check the token and your connection, then try again.';
      setError(message.replace(/sk-ant-oat01-[A-Za-z0-9_-]+/g, '[redacted token]'));
    } finally {
      setBusy(false);
      onBusyChange?.(false);
    }
  }

  return (
    <form onSubmit={submit} className="flex max-h-full flex-col gap-[var(--space-md)] overflow-y-auto px-[var(--popover-pad)] py-[var(--space-md)]">
      <Button type="button" size="sm" className="self-start" onClick={onBack} disabled={busy}>
        Back
      </Button>
      <div className="flex flex-col gap-[var(--space-xs)]">
        <h2 className="text-[length:var(--text-body)] font-[var(--weight-semibold)] text-[color:var(--color-text)]">
          {account ? 'Replace long-lived token' : 'Import long-lived token'}
        </h2>
        <p className="text-[length:var(--text-micro)] text-[color:var(--color-text-secondary)]">
          Paste the token from <code className="mono">claude setup-token</code>. No browser sign-in or running Claude Code is needed.
        </p>
      </div>
      <label className="flex flex-col gap-[var(--space-xs)] text-[length:var(--text-label)] text-[color:var(--color-text-secondary)]">
        Long-lived token
        <input
          ref={tokenInput}
          type="password"
          autoComplete="off"
          autoCapitalize="none"
          spellCheck={false}
          disabled={busy}
          onChange={(event) => setHasToken(event.target.value.trim().length > 0)}
          className="w-full rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-bg-card)] px-[var(--space-sm)] py-[var(--space-xs)] text-[color:var(--color-text)] focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
        />
      </label>
      <label className="flex flex-col gap-[var(--space-xs)] text-[length:var(--text-label)] text-[color:var(--color-text-secondary)]">
        Label (optional)
        <input
          value={label}
          onChange={(event) => setLabel(event.target.value)}
          maxLength={80}
          autoComplete="off"
          disabled={busy}
          placeholder="For example, Work"
          className="w-full rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-bg-card)] px-[var(--space-sm)] py-[var(--space-xs)] text-[color:var(--color-text)] focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
        />
      </label>
      <p className="text-[length:var(--text-micro)] leading-[var(--leading-label)] text-[color:var(--color-text-muted)]">
        Only the token is required. Inference-only tokens cannot reveal an email or usage limits. Their expiry is unknown, and they cannot refresh automatically.
      </p>
      <p className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
        Saving does not switch your active account or test a model request. Anyone with this token can use its account&apos;s quota.
      </p>
      {account?.is_active && (
        <p role="status" className="text-[length:var(--text-micro)] text-[color:var(--color-warn)]">
          Switch to another account before replacing the active token.
        </p>
      )}
      {error && (
        <p role="alert" className="text-[length:var(--text-micro)] text-[color:var(--color-danger)]">
          {error}
        </p>
      )}
      <Button type="submit" variant="primary" disabled={busy || !hasToken || account?.is_active}>
        {busy ? 'Importing…' : account ? 'Replace token' : 'Add account'}
      </Button>
    </form>
  );
}
