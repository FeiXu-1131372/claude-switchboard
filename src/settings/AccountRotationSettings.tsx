import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import type { AccountListEntry, RotationSettings } from '../lib/generated/bindings';
import { ipc } from '../lib/ipc';
import { Card } from '../components/ui/Card';
import { Toggle } from '../components/ui/Toggle';
import { Slider } from '../components/ui/Slider';
import { Select } from '../components/ui/Select';

const DEFAULTS: RotationSettings = {
  enabled: false,
  five_hour_threshold: 96,
  seven_day_threshold: 98,
  preferred_account_id: null,
};

interface Props {
  value: RotationSettings | null | undefined;
  accounts: AccountListEntry[];
  onChange: (value: RotationSettings) => void;
}

export function AccountRotationSettings({ value, accounts, onChange }: Props) {
  const settings = value ?? DEFAULTS;
  const [status, setStatus] = useState<string | null>(null);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let receivedEvent = false;
    listen<string>('account_rotation_status', (event) => {
      receivedEvent = true;
      if (!disposed) setStatus(event.payload);
    }).then((stop) => { if (disposed) stop(); else unlisten = stop; }).catch(() => {});
    ipc.getAccountRotationStatus().then((message) => {
      if (!disposed && !receivedEvent) setStatus(message);
    }).catch(() => {});
    return () => { disposed = true; unlisten?.(); };
  }, []);

  const preferredMissing = settings.preferred_account_id != null
    && !accounts.some((a) => a.account_uuid === settings.preferred_account_id);

  return (
    <section className="flex flex-col gap-[var(--space-sm)]">
      <h2 className="text-[length:var(--text-label)] font-[var(--weight-semibold)] text-[color:var(--color-text-muted)] uppercase tracking-[0.04em] px-[var(--space-2xs)]">
        Account rotation
      </h2>
      <Card className="p-[var(--space-md)] flex flex-col gap-[var(--space-md)]">
        <Toggle
          label="Automatically rotate accounts"
          description="Switch Claude Code when a usage limit reaches its threshold"
          checked={settings.enabled}
          onChange={(e) => onChange({ ...settings, enabled: e.target.checked })}
        />
        <Slider label="5-hour rotation threshold" min={1} max={100} step={1}
          value={settings.five_hour_threshold} disabled={!settings.enabled}
          onChange={(e) => onChange({ ...settings, five_hour_threshold: Number(e.target.value) })}
          formatValue={(v) => `${v}%`} />
        <Slider label="Weekly rotation threshold" min={1} max={100} step={1}
          value={settings.seven_day_threshold} disabled={!settings.enabled}
          onChange={(e) => onChange({ ...settings, seven_day_threshold: Number(e.target.value) })}
          formatValue={(v) => `${v}%`} />
        <Select label="Preferred next account" value={settings.preferred_account_id ?? ''}
          disabled={!settings.enabled}
          onChange={(e) => onChange({ ...settings, preferred_account_id: e.target.value || null })}
          options={[
            { value: '', label: 'Automatic (most headroom)' },
            ...(preferredMissing ? [{ value: settings.preferred_account_id!, label: 'Removed account — using automatic selection' }] : []),
            ...accounts.map((a) => ({ value: a.account_uuid, label: `${a.email}${a.is_active ? ' (active)' : ''}` })),
          ]} />
        <p className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
          Your preferred account is used when available; otherwise the account with the most headroom is chosen.
          The weekly threshold also applies to the Fable weekly limit. Accounts need fresh usage below their thresholds. If none qualify, Switchboard waits and checks again.
          Rotation runs while Switchboard is open, with at least 60 seconds between switches.
          Running Claude Code sessions usually adopt the account within about 30 seconds.
        </p>
        {accounts.length < 2 && <p className="text-[length:var(--text-micro)] text-[color:var(--color-warn-text)]">
          Add at least two accounts to use automatic rotation.
        </p>}
        {status && <p role="status" className="text-[length:var(--text-micro)] text-[color:var(--color-text-secondary)]">{status}</p>}
      </Card>
    </section>
  );
}
