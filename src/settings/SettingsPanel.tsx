import { useState, useEffect, useRef, useCallback } from 'react';
import { Card } from '../components/ui/Card';
import { Toggle } from '../components/ui/Toggle';
import { Slider } from '../components/ui/Slider';
import { Badge } from '../components/ui/Badge';
import { Button } from '../components/ui/Button';
import { Select } from '../components/ui/Select';
import { ScrollArea } from '../components/ui/ScrollArea';
import type { Terminal } from '../lib/generated/bindings';
import { useAppStore } from '../lib/store';
import { AppearanceCard } from './AppearanceCard';
import type { Settings } from '../lib/types';
import {
  enable as enableAutostart,
  disable as disableAutostart,
  isEnabled as isAutostartEnabled,
} from '@tauri-apps/plugin-autostart';
import { ipc } from '../lib/ipc';
import { WarmupSettings } from './WarmupSettings';
import { StatuslineBridgeCard } from './StatuslineBridgeCard';
import { AccountRotationSettings } from './AccountRotationSettings';

const POLL_MIN_SECS = 60;
const POLL_MAX_SECS = 1800;

/**
 * Every setting saves on its own, the way the OS's settings do. There used to
 * be a Save button at the very bottom of a long scroll, while Appearance,
 * live usage and warm-up applied at once: a change made at the top was lost
 * on Back without a word. Writes wait this long after the last change, so a
 * slider drag is one write, not thirty, and leaving the panel flushes it.
 */
export const SAVE_DELAY_MS = 400;

const clamp = (n: number, min: number, max: number) => Math.min(max, Math.max(min, n));

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/**
 * Only touch the OS autostart entry when the desired state differs from the
 * current one. On Windows, calling disable() against a registry value that
 * doesn't exist returns ERROR_FILE_NOT_FOUND (os error 2).
 */
async function syncAutostart(want: boolean) {
  const enabled = await isAutostartEnabled();
  if (want && !enabled) await enableAutostart();
  else if (!want && enabled) await disableAutostart();
}

const SECTION_HEADING =
  'text-[length:var(--text-label)] font-[var(--weight-semibold)] text-[color:var(--color-text-muted)] uppercase tracking-[0.04em] px-[var(--space-2xs)]';

/** The first two thresholds also colour the usage bars amber and red. */
const THRESHOLD_LABELS = ['Warning at', 'Critical at'];

/// Display names for the `Terminal` enum. The backend's `label()` is not
/// exposed over IPC, so the mapping is duplicated here — keep in sync with
/// `providers::launcher::Terminal::label`.
const TERMINAL_LABELS: Record<Terminal, string> = {
  ghostty: 'Ghostty',
  terminal_app: 'Terminal.app',
  iterm_2: 'iTerm2',
  kitty: 'kitty',
  wez_term: 'WezTerm',
  windows_terminal: 'Windows Terminal',
  power_shell: 'PowerShell',
};

interface Props {
  /** Padding for the scrolling body; the compact popover and the modal differ. */
  bodyClassName?: string;
}

export function SettingsPanel({ bodyClassName = '' }: Props) {
  const settings = useAppStore((s) => s.settings);
  const setSettings = useAppStore((s) => s.setSettings);
  const usage = useAppStore((s) => s.usage);
  const accounts = useAppStore((s) => s.accounts);
  const [local, setLocal] = useState<Settings | null>(() => settings);
  const [saveError, setSaveError] = useState<string | null>(null);
  /** What the controls show, readable outside render. */
  const latest = useRef<Settings | null>(settings);
  /** The edit not yet written, if any. */
  const unsaved = useRef<Settings | null>(null);
  /** What was last written, to tell whether launch-at-login changed. */
  const saved = useRef<Settings | null>(settings);
  const timer = useRef<number | null>(null);
  /** Writes run one at a time, in order. */
  const writing = useRef<Promise<void>>(Promise.resolve());
  const [consentGranted, setConsentGranted] = useState(false);
  const [osRegistered, setOsRegistered] = useState(false);
  const [availableTerminals, setAvailableTerminals] = useState<Terminal[]>([]);

  useEffect(() => {
    ipc.getWarmupConsentGranted().then(setConsentGranted).catch(() => {});
    ipc.osSchedulerIsRegistered().then(setOsRegistered).catch(() => {});
    ipc.listAvailableTerminals().then(setAvailableTerminals).catch(() => {});
  }, []);

  const handleRevoke = async () => {
    await ipc.revokeWarmupConsent();
    setConsentGranted(false);
  };

  const handleRegisterOs = async () => {
    await ipc.osSchedulerRegister();
    setOsRegistered(true);
  };

  const handleUnregisterOs = async () => {
    await ipc.osSchedulerUnregister();
    setOsRegistered(false);
  };

  // Settings that arrive after the panel opened: without this the panel
  // stayed on "Loading…" until it was closed and opened again.
  useEffect(() => {
    if (latest.current || !settings) return;
    latest.current = settings;
    saved.current = settings;
    setLocal(settings);
  }, [settings]);

  const flush = useCallback(() => {
    if (timer.current !== null) {
      window.clearTimeout(timer.current);
      timer.current = null;
    }
    writing.current = writing.current.then(async () => {
      const edit = unsaved.current;
      if (!edit) return;
      unsaved.current = null;
      const next: Settings = {
        ...edit,
        polling_interval_secs: clamp(edit.polling_interval_secs, POLL_MIN_SECS, POLL_MAX_SECS),
      };
      try {
        await setSettings(next);
      } catch (e) {
        // Keep it for Retry, unless a newer edit is already waiting.
        unsaved.current ??= edit;
        setSaveError(`Couldn't save settings: ${errorText(e)}`);
        return;
      }
      const before = saved.current;
      saved.current = next;
      if (before?.launch_at_login !== next.launch_at_login) {
        try {
          await syncAutostart(next.launch_at_login);
        } catch (e) {
          // Best effort: the setting is saved, the OS entry is not.
          setSaveError(`Saved, but launch at login couldn't be changed: ${errorText(e)}`);
          return;
        }
      }
      setSaveError(null);
    });
  }, [setSettings]);

  // Back, close, or the modal going away must not drop a pending edit.
  useEffect(
    () => () => {
      if (unsaved.current) flush();
    },
    [flush],
  );

  if (!local) return <p className="text-[color:var(--color-text-muted)]">Loading…</p>;

  const pollingMinutes = Math.max(1, Math.round(local.polling_interval_secs / 60));

  function update<K extends keyof Settings>(key: K, value: Settings[K]) {
    const base = latest.current;
    if (!base) return;
    const next = { ...base, [key]: value };
    latest.current = next;
    unsaved.current = next;
    setLocal(next);
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(flush, SAVE_DELAY_MS);
  }

  function retry() {
    unsaved.current ??= latest.current;
    // Forget what was written, so a failed launch-at-login is tried again.
    saved.current = null;
    flush();
  }

  const authSourceLabel = (src: string) => src === 'ClaudeCode' ? 'Claude Code' : src;
  const activeAccount = accounts.find((a) => a.is_active);
  const accountStatus = usage
    ? { connected: true, email: usage.account_email, source: authSourceLabel(usage.auth_source) }
    : activeAccount
      ? { connected: true, email: activeAccount.email, source: activeAccount.cached_usage ? authSourceLabel(activeAccount.cached_usage.auth_source) : 'Claude Code' }
      : { connected: false, email: null, source: null };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <ScrollArea className={`min-h-0 flex-1 ${bodyClassName}`}>
        <div className="flex flex-col gap-[var(--space-lg)]">
          {/* Appearance */}
          <section className="flex flex-col gap-[var(--space-sm)]">
            <h2 className={SECTION_HEADING}>Appearance</h2>
            <AppearanceCard />
          </section>

          {/* General */}
          <section className="flex flex-col gap-[var(--space-sm)]">
            <h2 className={SECTION_HEADING}>General</h2>
            <Card className="p-[var(--space-md)] flex flex-col">
              <Toggle
                label="Launch at login"
                description="Start monitoring when you log in"
                checked={local.launch_at_login}
                onChange={(e) => update('launch_at_login', e.target.checked)}
              />
            </Card>
          </section>

          {/* Polling */}
          <section className="flex flex-col gap-[var(--space-sm)]">
            <h2 className={SECTION_HEADING}>Polling</h2>
            <Card className="p-[var(--space-md)] flex flex-col gap-[var(--space-md)]">
              <div>
                <Slider
                  label="Poll interval"
                  min={1}
                  max={30}
                  step={1}
                  value={pollingMinutes}
                  onChange={(e) => update('polling_interval_secs', Number(e.target.value) * 60)}
                  formatValue={(v) => `${v}m`}
                />
                {pollingMinutes <= 2 && (
                  <p className="text-[length:var(--text-micro)] text-[color:var(--color-warn-text)] mt-[var(--space-xs)]">
                    Frequent polling may cause rate limiting
                  </p>
                )}
              </div>
              <div>
                <Slider
                  label="Stagger gap"
                  min={5}
                  max={120}
                  step={5}
                  value={local.stagger_gap_secs}
                  onChange={(e) => update('stagger_gap_secs', Number(e.target.value))}
                  formatValue={(v) => `${v}s`}
                />
                <p className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)] mt-[var(--space-xs)]">
                  Spacing between consecutive account polls in one round.
                </p>
                {accounts.length > 1 &&
                  accounts.length * local.stagger_gap_secs > local.polling_interval_secs && (
                    <p className="text-[length:var(--text-micro)] text-[color:var(--color-warn-text)] mt-[var(--space-xs)]">
                      {accounts.length} accounts × {local.stagger_gap_secs}s won't fit in{' '}
                      {Math.round(local.polling_interval_secs / 60)}m — gap will compress to{' '}
                      {Math.floor(local.polling_interval_secs / accounts.length)}s per slot.
                    </p>
                  )}
              </div>
            </Card>
            <StatuslineBridgeCard />
          </section>

          <AccountRotationSettings
            value={local.account_rotation}
            accounts={accounts}
            onChange={(value) => update('account_rotation', value)}
          />

          {/* Thresholds */}
          <section className="flex flex-col gap-[var(--space-sm)]">
            <h2 className={SECTION_HEADING}>Notifications</h2>
            <Card className="p-[var(--space-md)] flex flex-col gap-[var(--space-md)]">
              {local.thresholds.map((t, i) => (
                <Slider
                  key={i}
                  label={THRESHOLD_LABELS[i] ?? `Alert ${i + 1} at`}
                  min={25}
                  max={95}
                  step={5}
                  value={t}
                  onChange={(e) => {
                    const next = [...local.thresholds];
                    next[i] = Number(e.target.value);
                    update('thresholds', next);
                  }}
                  formatValue={(v) => `${v}%`}
                />
              ))}
              <p className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
                You get one notification per level until the limit resets. The levels also turn the
                usage bars amber, then red.
              </p>
            </Card>
          </section>

          {/* Terminal */}
          <section className="flex flex-col gap-[var(--space-sm)]">
            <h2 className={SECTION_HEADING}>Terminal</h2>
            <Card className="p-[var(--space-md)]">
              <Select
                label="Terminal"
                value={local.terminal ?? ''}
                onChange={(e) => update('terminal', (e.target.value || null) as Settings['terminal'])}
                options={[
                  { value: '', label: 'System default' },
                  ...availableTerminals.map((t) => ({ value: t, label: TERMINAL_LABELS[t] ?? t })),
                ]}
              />
              <p className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)] mt-[var(--space-xs)]">
                Used when launching or resuming a session from the Providers and Sessions tabs. Only
                terminals installed on this machine are listed.
              </p>
            </Card>
          </section>

          {/* Account */}
          <section className="flex flex-col gap-[var(--space-sm)]">
            <h2 className={SECTION_HEADING}>Account</h2>
            <Card className="p-[var(--space-md)]">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-[var(--space-sm)]">
                  <span className="text-[length:var(--text-body)] text-[color:var(--color-text)]">
                    {accountStatus.connected ? (accountStatus.email ?? 'Connected') : 'Not signed in'}
                  </span>
                  {accountStatus.source && <Badge variant="live">{accountStatus.source}</Badge>}
                </div>
                {!accountStatus.connected && (
                  <span className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
                    Manage in Accounts
                  </span>
                )}
              </div>
            </Card>
          </section>

          {/* Warm-up */}
          <section className="flex flex-col gap-[var(--space-sm)]">
            <h2 className={SECTION_HEADING}>Warm-up</h2>
            <Card className="p-[var(--space-md)]">
              <WarmupSettings
                accounts={accounts}
                consentGranted={consentGranted}
                osSchedulerRegistered={osRegistered}
                onRevoke={handleRevoke}
                onRegisterOs={handleRegisterOs}
                onUnregisterOs={handleUnregisterOs}
              />
            </Card>
          </section>
        </div>
      </ScrollArea>

      {/* Outside the scroll, so a failed save shows wherever the change was made. */}
      {saveError && (
        <div
          role="alert"
          className="flex shrink-0 items-center justify-between gap-[var(--space-sm)] border-t border-[var(--color-rule)] px-[var(--popover-pad)] py-[var(--space-xs)]"
        >
          <span className="min-w-0 text-[length:var(--text-micro)] text-[color:var(--color-danger)]">
            {saveError}
          </span>
          <Button size="sm" onClick={retry}>
            Retry
          </Button>
        </div>
      )}
    </div>
  );
}
