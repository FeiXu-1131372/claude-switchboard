import { useEffect, useState } from 'react';
import { ipc } from '../lib/ipc';
import type { AccountListEntry, WarmupAccountState } from '../lib/generated/bindings';
import { Button } from '../components/ui/Button';

interface Props {
  consentGranted: boolean;
  osSchedulerRegistered: boolean;
  accounts?: Pick<AccountListEntry, 'account_uuid' | 'email'>[];
  onRevoke: () => void | Promise<void>;
  onRegisterOs: () => void | Promise<void>;
  onUnregisterOs: () => void | Promise<void>;
}

const RESULTS: Record<string, string> = {
  running: 'Checking account…', success: 'Warm-up succeeded', already_active: 'Five-hour window already active',
  needs_reauth: 'Sign-in required', rate_limited: 'Rate limited', network_error: 'Network error',
  server_error: 'Anthropic server error', error: 'Warm-up failed',
};
const time = (seconds: number) => new Date(seconds * 1000).toLocaleString(undefined, {
  month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit', hour12: false,
});

export function WarmupSettings({ consentGranted, osSchedulerRegistered, accounts = [], onRevoke, onRegisterOs, onUnregisterOs }: Props) {
  const [activity, setActivity] = useState<Record<string, WarmupAccountState>>({});
  const [error, setError] = useState<string | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [busy, setBusy] = useState(false);
  const accountIds = JSON.stringify(accounts.map((a) => a.account_uuid));

  useEffect(() => {
    let disposed = false;
    const ids: string[] = JSON.parse(accountIds);
    if (ids.length === 0) return;
    const load = async () => {
      const results = await Promise.allSettled(ids.map(async (id) => [id, await ipc.getWarmupState(id)] as const));
      if (disposed) return;
      setLoadError(results.some((r) => r.status === 'rejected'));
      setActivity(Object.fromEntries(results.flatMap((r) => r.status === 'fulfilled' ? [r.value] : [])));
    };
    void load();
    const timer = window.setInterval(load, 30_000);
    return () => { disposed = true; window.clearInterval(timer); };
  }, [accountIds]);

  async function operate(action: () => void | Promise<void>) {
    setBusy(true);
    setError(null);
    try { await action(); }
    catch (e) { setError(e instanceof Error ? e.message : String(e)); }
    finally { setBusy(false); }
  }

  return (
    <div className="flex flex-col gap-[var(--space-md)] text-[length:var(--text-label)] text-[color:var(--color-text-secondary)]">
      <div className="flex items-center justify-between gap-[var(--space-sm)]">
        <span>Global consent: {consentGranted ? 'granted' : 'not granted'}</span>
        {consentGranted && <Button variant="ghost" disabled={busy} onClick={() => void operate(onRevoke)}>Revoke</Button>}
      </div>
      <div className="flex items-center justify-between gap-[var(--space-sm)]">
        <span>Background warm-up: {osSchedulerRegistered ? 'enabled' : 'not enabled'}</span>
        <Button variant="ghost" disabled={busy} onClick={() => void operate(osSchedulerRegistered ? onUnregisterOs : onRegisterOs)}>
          {osSchedulerRegistered ? 'Disable background' : 'Enable in background'}
        </Button>
      </div>
      <p className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
        Background checks run every minute with Switchboard closed while your computer is awake and you are signed in.
        Missed schedules are checked when it resumes. Active five-hour windows are skipped.
      </p>
      {error && <p role="alert" className="text-[color:var(--color-danger)]">{error}</p>}
      {loadError && <p role="alert">Could not load some warm-up activity. Retrying shortly.</p>}
      {accounts.map((account) => {
        const state = activity[account.account_uuid];
        if (!state) return null;
        return <div key={account.account_uuid} className="border-t border-[var(--color-border)] pt-[var(--space-sm)] flex flex-col gap-[var(--space-2xs)]">
          <span className="text-[color:var(--color-text)] font-[var(--weight-medium)]">{account.email}</span>
          <span>Next scheduled check: {consentGranted && state.warmup_enabled && state.next_scheduled_at ? time(state.next_scheduled_at) : 'Automatic warm-up off'}</span>
          <span>Last successful warm-up: {state.last_success_at ? time(state.last_success_at) : 'None recorded'}</span>
          {state.last_result && <span>{RESULTS[state.last_result] ?? 'Unknown result'}{state.last_warmup_at ? ` · ${time(state.last_warmup_at)}` : ''}</span>}
          {state.last_error && <span className="text-[color:var(--color-danger)]">{state.last_error}</span>}
        </div>;
      })}
    </div>
  );
}
