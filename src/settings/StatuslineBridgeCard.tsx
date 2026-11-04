import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { Card } from '../components/ui/Card';
import { Toggle } from '../components/ui/Toggle';
import { Badge } from '../components/ui/Badge';
import { Button } from '../components/ui/Button';
import type {
  BridgeTargetStatus,
  DisableOutcomeDto,
  PreflightReport,
} from '../lib/generated/bindings';
import { ipc } from '../lib/ipc';

/** Above this, the consent panel tells the user their status line gets slower. */
const SLOW_BRIDGE_MS = 150;
const STATUS_REFRESH_MS = 10_000;

const CAPTION = "Reads your 5-hour and 7-day usage from Claude Code's status line instead of polling for it.";

const NOT_REPORTING = [
  'Sessions on WSL or a remote host',
  'Non-interactive runs (-p) and the Agent SDK',
  'IDE sessions',
  'Sessions started with a CLAUDE_CODE_OAUTH_TOKEN environment variable',
  'Projects that set their own status line, and sessions with status line commands turned off',
];

const mutedText = 'text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]';
const bodyText = 'text-[length:var(--text-label)] text-[color:var(--color-text-secondary)]';

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** "12s ago" / "4m ago" / "2h ago" / "3d ago" from an epoch-ms timestamp. */
export function formatAgo(thenMs: number, nowMs: number): string {
  const secs = Math.max(0, Math.round((nowMs - thenMs) / 1000));
  if (secs < 60) return `${secs}s ago`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 48) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

function Path({ children }: { children: ReactNode }) {
  return (
    <code className="mono text-[length:var(--text-label)] text-[color:var(--color-text)] break-all">
      {children}
    </code>
  );
}

interface TargetRowProps {
  target: BridgeTargetStatus;
  showPath: boolean;
  now: () => number;
  /** Replace this target's status, or reload everything when `null`. */
  onChanged: (next: BridgeTargetStatus | BridgeTargetStatus[] | null) => void;
}

type Busy = 'preflight' | 'enable' | 'disable' | 'restore' | 'rewrap' | 'recheck' | null;

function isEnabled(t: BridgeTargetStatus): boolean {
  return t.installed_command !== null && t.state.kind !== 'changed_outside';
}

function TargetRow({ target, showPath, now, onChanged }: TargetRowProps) {
  const [busy, setBusy] = useState<Busy>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [consent, setConsent] = useState<PreflightReport | null>(null);
  const [acceptedShared, setAcceptedShared] = useState(false);

  const state = target.state;
  const enabled = isEnabled(target);
  const notSetUp = state.kind === 'claude_code_not_set_up';

  async function run<T>(kind: Exclude<Busy, null>, action: () => Promise<T>): Promise<T | undefined> {
    setBusy(kind);
    setError(null);
    setNotice(null);
    try {
      return await action();
    } catch (e) {
      setError(errorText(e));
      return undefined;
    } finally {
      setBusy(null);
    }
  }

  async function beginTurnOn() {
    const report = await run('preflight', () => ipc.preflightStatuslineBridge(target.config_home));
    if (report) {
      setAcceptedShared(false);
      setConsent(report);
    }
  }

  async function confirmTurnOn() {
    if (!consent) return;
    const next = await run('enable', () =>
      ipc.enableStatuslineBridge(target.config_home, acceptedShared),
    );
    if (next) {
      setConsent(null);
      setNotice('Live usage is on.');
      onChanged(next);
    }
  }

  async function turnOff() {
    const outcome = await run('disable', () => ipc.disableStatuslineBridge(target.id));
    if (outcome) {
      setNotice(offNotice(outcome));
      onChanged(null);
    }
  }

  async function restoreOriginal() {
    const outcome = await run('restore', () => ipc.restoreOriginalStatusline(target.id));
    if (outcome) {
      setNotice(offNotice(outcome));
      onChanged(null);
    }
  }

  async function useBridgeAgain() {
    const next = await run('rewrap', () => ipc.rewrapStatuslineBridge(target.id));
    if (next) {
      setNotice('Live usage is on.');
      onChanged(next);
    }
  }

  async function retry() {
    const all = await run('recheck', () => ipc.recheckStatuslineBridge());
    if (all) onChanged(all);
  }

  const working = busy !== null;
  const toggleDescription = notSetUp ? "Claude Code isn't set up on this computer." : CAPTION;

  return (
    <div className="flex flex-col gap-[var(--space-sm)]">
      {showPath && <Path>{target.config_home}</Path>}
      <div className="flex items-start gap-[var(--space-sm)]">
        <Toggle
          className="flex-1"
          label="Live usage from Claude Code"
          description={toggleDescription}
          checked={enabled}
          disabled={working || notSetUp || consent !== null}
          onChange={() => (enabled ? void turnOff() : void beginTurnOn())}
        />
      </div>

      {busy && (
        <p className={mutedText} role="status">
          {busyText(busy)}
        </p>
      )}

      {consent && (
        <ConsentPanel
          report={consent}
          busy={busy === 'enable'}
          acceptedShared={acceptedShared}
          onAcceptedShared={setAcceptedShared}
          onConfirm={() => void confirmTurnOn()}
          onCancel={() => {
            setConsent(null);
            setError(null);
          }}
        />
      )}

      {!consent && (
        <StateLines
          target={target}
          now={now}
          working={working}
          onRestore={() => void restoreOriginal()}
          onRetry={() => void retry()}
          onUseAgain={() => void useBridgeAgain()}
          onDismiss={() => void turnOff()}
        />
      )}

      {error && (
        <p role="alert" className="text-[length:var(--text-label)] text-[color:var(--color-danger)]">
          {error}
        </p>
      )}
      {notice && !error && (
        <p role="status" className="text-[length:var(--text-label)] text-[color:var(--color-accent)]">
          {notice}
        </p>
      )}
    </div>
  );
}

function busyText(busy: Exclude<Busy, null>): string {
  switch (busy) {
    case 'preflight':
      return 'Checking that your status line still works through the bridge…';
    case 'enable':
      return 'Turning on…';
    case 'disable':
      return 'Turning off…';
    case 'restore':
      return 'Restoring your original status line…';
    case 'rewrap':
      return 'Wrapping your new status line…';
    case 'recheck':
      return 'Checking the bridge…';
  }
}

function offNotice(outcome: DisableOutcomeDto): string {
  return outcome === 'restored'
    ? 'Live usage is off. Your status line is back exactly as it was.'
    : 'Live usage is off. Your status line was changed since, so Switchboard left it as it is.';
}

interface StateLinesProps {
  target: BridgeTargetStatus;
  now: () => number;
  working: boolean;
  onRestore: () => void;
  onRetry: () => void;
  onUseAgain: () => void;
  onDismiss: () => void;
}

function StateLines({ target, now, working, onRestore, onRetry, onUseAgain, onDismiss }: StateLinesProps) {
  const state = target.state;
  switch (state.kind) {
    case 'off':
    case 'claude_code_not_set_up':
      return null;

    case 'waiting':
      return (
        <div className="flex flex-col gap-[var(--space-xs)]">
          <div className="flex items-center gap-[var(--space-sm)]">
            <Badge variant="default">Waiting</Badge>
            <span className={bodyText}>Start or continue a Claude Code session.</span>
          </div>
          <NotReporting />
        </div>
      );

    case 'live':
      return (
        <div className="flex items-center gap-[var(--space-sm)]">
          <Badge variant="live">Live</Badge>
          <span className={bodyText}>Last update {formatAgo(state.last_update_ms, now())}.</span>
        </div>
      );

    case 'quiet':
      return (
        <div className="flex flex-col gap-[var(--space-xs)]">
          <span className={bodyText}>
            {state.last_update_ms === null
              ? 'No update yet.'
              : `Last update ${formatAgo(state.last_update_ms, now())}.`}
          </span>
          <NotReporting />
        </div>
      );

    case 'running_without_usage':
      return (
        <span className={bodyText}>
          Claude Code is running the bridge but isn&apos;t sending usage: {state.reason}.
        </span>
      );

    case 'blocked':
      return (
        <span className="text-[length:var(--text-label)] text-[color:var(--color-warn)]">
          {state.reason}
        </span>
      );

    case 'broken':
      return (
        <div className="flex flex-col gap-[var(--space-sm)]">
          <span className="text-[length:var(--text-label)] text-[color:var(--color-danger)]">
            Your status line isn&apos;t working: the bridge can&apos;t run ({state.error}).
          </span>
          <div className="flex items-center gap-[var(--space-sm)]">
            <Button variant="primary" size="sm" onClick={onRestore} disabled={working}>
              Restore original
            </Button>
            <Button size="sm" onClick={onRetry} disabled={working}>
              Retry
            </Button>
          </div>
        </div>
      );

    case 'changed_outside':
      return (
        <div className="flex flex-col gap-[var(--space-sm)]">
          <span className="text-[length:var(--text-label)] text-[color:var(--color-warn)]">
            Your status line was changed outside Switchboard, so live usage is off.
          </span>
          <div className="flex items-center gap-[var(--space-sm)]">
            <Button variant="primary" size="sm" onClick={onUseAgain} disabled={working}>
              Use the bridge again
            </Button>
            <Button size="sm" onClick={onDismiss} disabled={working}>
              Dismiss
            </Button>
          </div>
        </div>
      );

    case 'edit_refused':
      return (
        <span className="text-[length:var(--text-label)] text-[color:var(--color-danger)]">
          {state.message}
        </span>
      );
  }
}

function NotReporting() {
  return (
    <details className={mutedText}>
      <summary className="cursor-pointer select-none">What doesn&apos;t report</summary>
      <ul className="list-disc pl-[var(--space-lg)] mt-[var(--space-2xs)] flex flex-col gap-[var(--space-2xs)]">
        {NOT_REPORTING.map((line) => (
          <li key={line}>{line}</li>
        ))}
      </ul>
    </details>
  );
}

interface ConsentPanelProps {
  report: PreflightReport;
  busy: boolean;
  acceptedShared: boolean;
  onAcceptedShared: (v: boolean) => void;
  onConfirm: () => void;
  onCancel: () => void;
}

function ConsentPanel({ report, busy, acceptedShared, onAcceptedShared, onConfirm, onCancel }: ConsentPanelProps) {
  const failures = report.shells.filter((s) => !s.ok);
  const needsAcceptance = report.shared_warning !== null;
  const canConfirm = failures.length === 0 && (!needsAcceptance || acceptedShared) && !busy;
  const differs =
    report.original_direct !== null &&
    report.original_via_bridge !== null &&
    report.original_direct !== report.original_via_bridge;

  return (
    <div
      role="group"
      aria-label="Turn on live usage"
      className="flex flex-col gap-[var(--space-sm)] rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-track)] p-[var(--space-md)]"
    >
      <p className={bodyText}>
        Switchboard will change one setting in <Path>{report.settings_path}</Path>:{' '}
        <Path>statusLine.command</Path> will run <Path>{report.command}</Path>
      </p>

      <p className={bodyText}>
        {report.original_command !== null ? (
          <>
            Your current status line (<Path>{report.original_command}</Path>) keeps working. The
            bridge runs it and shows its output.
          </>
        ) : (
          'Claude Code will hide its footer hint while this is on, because it now has a status line. Nothing else changes on screen.'
        )}
        {report.latency_ms > SLOW_BRIDGE_MS && (
          <>
            {' '}
            Your status line will update about {(report.latency_ms / 1000).toFixed(1)} s slower.
          </>
        )}
      </p>

      <p className={bodyText}>
        To check that it works, Switchboard runs your status line once with sample data.
      </p>

      {differs && (
        <details className={mutedText}>
          <summary className="cursor-pointer select-none">
            The output differed when run through the bridge
          </summary>
          <div className="mt-[var(--space-xs)] flex flex-col gap-[var(--space-xs)]">
            <div>
              <div>Directly</div>
              <pre className="mono whitespace-pre-wrap break-all text-[color:var(--color-text)]">
                {report.original_direct}
              </pre>
            </div>
            <div>
              <div>Through the bridge</div>
              <pre className="mono whitespace-pre-wrap break-all text-[color:var(--color-text)]">
                {report.original_via_bridge}
              </pre>
            </div>
          </div>
        </details>
      )}

      <p className={bodyText}>
        The bridge saves your usage percentages, reset times and account ID to{' '}
        <Path>~/.claude-switchboard/statusline/</Path>. Nothing leaves your computer.
      </p>

      <p className={bodyText}>
        Claude Code&apos;s <Path>/statusline</Path> command will see the bridge command. If it
        changes your status line, Switchboard notices and turns live usage off.
      </p>

      <p className={bodyText}>
        Live usage works in interactive Claude Code sessions. Sessions started with a{' '}
        <Path>CLAUDE_CODE_OAUTH_TOKEN</Path> environment variable, <Path>-p</Path> runs, and IDE
        sessions keep using normal polling.
      </p>

      <p className={bodyText}>
        Turning this off puts the setting back exactly as it was. To undo it without Switchboard,
        run <Path>{report.bridge_path} --restore --all</Path>
      </p>

      {failures.length > 0 && (
        <div role="alert" className="flex flex-col gap-[var(--space-2xs)]">
          {failures.map((f) => (
            <span key={f.shell} className="text-[length:var(--text-label)] text-[color:var(--color-danger)]">
              The bridge couldn&apos;t run in {f.shell}: {f.detail}
            </span>
          ))}
        </div>
      )}

      {report.shared_warning !== null && (
        <label className="flex items-start gap-[var(--space-sm)] cursor-pointer">
          <input
            type="checkbox"
            className="mt-[2px] accent-[color:var(--color-accent)]"
            checked={acceptedShared}
            onChange={(e) => onAcceptedShared(e.target.checked)}
          />
          <span className="text-[length:var(--text-label)] text-[color:var(--color-warn)]">
            {report.shared_warning}
          </span>
        </label>
      )}

      <div className="flex items-center gap-[var(--space-sm)]">
        <Button variant="primary" size="sm" onClick={onConfirm} disabled={!canConfirm}>
          {busy ? 'Turning on…' : 'Turn on'}
        </Button>
        <Button size="sm" onClick={onCancel} disabled={busy}>
          Cancel
        </Button>
      </div>
    </div>
  );
}

interface StatuslineBridgeCardProps {
  /** Injected for tests. */
  now?: () => number;
  /** Injected for tests; defaults to the webview's user agent. */
  isWindows?: boolean;
}

/**
 * Settings → Polling: live usage from Claude Code's status line.
 *
 * Acts immediately through its own IPC calls rather than the panel's batched
 * Save: it changes another program's file and needs its own busy, success and
 * error feedback. Every IPC failure is shown; none is swallowed.
 */
export function StatuslineBridgeCard({
  now = Date.now,
  isWindows = typeof navigator !== 'undefined' && navigator.userAgent.includes('Windows'),
}: StatuslineBridgeCardProps) {
  // The Windows runtime is written to spec but not yet verified on Windows.
  if (isWindows) return null;
  return <StatuslineBridgeCardInner now={now} />;
}

function StatuslineBridgeCardInner({ now }: { now: () => number }) {
  const [targets, setTargets] = useState<BridgeTargetStatus[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [, setTick] = useState(0);
  const mounted = useRef(true);

  const load = useCallback(async () => {
    try {
      const next = await ipc.getStatuslineBridgeStatus();
      if (!mounted.current) return;
      setTargets(next);
      setLoadError(null);
    } catch (e) {
      if (!mounted.current) return;
      setLoadError(errorText(e));
    }
  }, []);

  useEffect(() => {
    mounted.current = true;
    void load();
    const timer = setInterval(() => {
      void load();
      setTick((n) => n + 1);
    }, STATUS_REFRESH_MS);
    return () => {
      mounted.current = false;
      clearInterval(timer);
    };
  }, [load]);

  const handleChanged = useCallback(
    (id: string) => (next: BridgeTargetStatus | BridgeTargetStatus[] | null) => {
      if (next === null) {
        void load();
      } else if (Array.isArray(next)) {
        setTargets(next);
      } else {
        setTargets((prev) => (prev ? prev.map((t) => (t.id === id ? next : t)) : [next]));
      }
    },
    [load],
  );

  return (
    <Card className="p-[var(--space-md)] flex flex-col gap-[var(--space-md)]">
      {targets === null && loadError === null && (
        <p className={mutedText}>Loading…</p>
      )}
      {loadError !== null && (
        <div className="flex flex-col gap-[var(--space-xs)]">
          <p role="alert" className="text-[length:var(--text-label)] text-[color:var(--color-danger)]">
            Couldn&apos;t read the status line setup: {loadError}
          </p>
          <div>
            <Button size="sm" onClick={() => void load()}>
              Retry
            </Button>
          </div>
        </div>
      )}
      {targets?.map((t) => (
        <TargetRow
          key={t.id || t.config_home}
          target={t}
          showPath={targets.length > 1}
          now={now}
          onChanged={handleChanged(t.id)}
        />
      ))}
    </Card>
  );
}
