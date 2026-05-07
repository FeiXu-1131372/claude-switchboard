import type { AccountListEntry, RunningClaudeCode } from '../lib/generated/bindings';
import { ChevronRight, X } from '../lib/icons';

interface Props {
  current: AccountListEntry | null;
  target: AccountListEntry;
  running: RunningClaudeCode;
  busy: boolean;
  errorMessage: string | null;
  onConfirm: () => void;
  onCancel: () => void;
}

function PlanTag({ plan }: { plan: string | null }) {
  if (!plan) return null;
  const isMax = plan.toLowerCase() === 'max';
  return (
    <span
      className={`
        inline-flex items-center rounded-[var(--radius-pill)]
        px-[var(--space-xs)] py-[1px]
        text-[length:var(--text-micro)] font-[var(--weight-semibold)]
        uppercase tracking-[var(--tracking-label)]
        ${isMax
          ? 'bg-[var(--color-accent-dim)] text-[color:var(--color-accent)]'
          : 'bg-[var(--color-track)] text-[color:var(--color-text-secondary)]'}
      `}
    >
      {plan}
    </span>
  );
}

function AccountLine({ entry }: { entry: AccountListEntry }) {
  return (
    <div className="flex items-center gap-[var(--space-xs)] min-w-0">
      <span
        className="flex-1 min-w-0 truncate text-[length:var(--text-body)] text-[color:var(--color-text)]"
        title={entry.email}
      >
        {entry.email}
      </span>
      <PlanTag plan={entry.subscription_type ?? null} />
    </div>
  );
}

export function SwapConfirmCard({
  current,
  target,
  running,
  busy,
  errorMessage,
  onConfirm,
  onCancel,
}: Props) {
  const cli = running.cli_processes;
  const code = running.vscode_with_extension.length;
  const hasRunning = cli > 0 || code > 0;

  return (
    <div className="flex h-full w-full flex-col">
      <div className="flex items-center justify-between gap-[var(--space-sm)] px-[var(--popover-pad)] pt-[var(--space-md)] pb-[var(--space-sm)]">
        <button
          type="button"
          onClick={onCancel}
          disabled={busy}
          className="
            inline-flex items-center gap-[var(--space-2xs)]
            text-[length:var(--text-label)] text-[color:var(--color-text-secondary)]
            tracking-[var(--tracking-label)] uppercase
            hover:text-[color:var(--color-text)]
            disabled:opacity-50
          "
        >
          <ChevronRight size={11} className="rotate-180" />
          Cancel
        </button>
        <span className="text-[length:var(--text-label)] uppercase tracking-[var(--tracking-label)] text-[color:var(--color-text-secondary)]">
          Confirm switch
        </span>
        <button
          type="button"
          aria-label="Cancel"
          onClick={onCancel}
          disabled={busy}
          className="text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)] disabled:opacity-50"
        >
          <X size={13} />
        </button>
      </div>

      <div className="flex-1 overflow-y-auto px-[var(--popover-pad)] pb-[var(--space-md)]">
        <div className="flex flex-col gap-[var(--space-md)]">
          <div className="flex flex-col gap-[var(--space-2xs)]">
            <span className="text-[length:var(--text-micro)] uppercase tracking-[var(--tracking-label)] text-[color:var(--color-text-muted)]">
              Currently active
            </span>
            {current ? (
              <AccountLine entry={current} />
            ) : (
              <span className="text-[length:var(--text-body)] text-[color:var(--color-text-muted)]">
                None
              </span>
            )}
          </div>

          <div className="flex flex-col gap-[var(--space-2xs)]">
            <span className="text-[length:var(--text-micro)] uppercase tracking-[var(--tracking-label)] text-[color:var(--color-accent)]">
              Switch to
            </span>
            <AccountLine entry={target} />
          </div>

          <div className="flex flex-col gap-[var(--space-2xs)] rounded-[var(--radius-sm)] border border-[var(--color-border-subtle)] bg-[var(--color-bg-card)] px-[var(--space-sm)] py-[var(--space-sm)]">
            <span className="text-[length:var(--text-micro)] uppercase tracking-[var(--tracking-label)] text-[color:var(--color-text-muted)]">
              What happens
            </span>
            <ul className="flex flex-col gap-[var(--space-2xs)] text-[length:var(--text-micro)] text-[color:var(--color-text-secondary)]">
              <li>• Replaces the upstream-CLI credentials in your macOS Keychain</li>
              <li>• Rewrites the <code className="mono">oauthAccount</code> slice of <code className="mono">~/.claude.json</code></li>
              <li>• New <code className="mono">claude</code> invocations use {target.email} immediately</li>
              {hasRunning ? (
                <li>
                  • {cli > 0 && `${cli} running CLI session${cli > 1 ? 's' : ''}`}
                  {cli > 0 && code > 0 && ' and '}
                  {code > 0 && `${code} VS Code workspace${code > 1 ? 's' : ''}`}
                  {' '}adopt the new account within ~30 seconds (when their cached credentials refresh)
                </li>
              ) : (
                <li>• No Claude Code sessions are running — nothing else to do</li>
              )}
            </ul>
          </div>

          {errorMessage && (
            <span className="text-[length:var(--text-micro)] text-[color:var(--color-danger)]">
              {errorMessage}
            </span>
          )}
        </div>
      </div>

      <div className="flex items-center justify-end gap-[var(--space-sm)] border-t border-[var(--color-rule)] px-[var(--popover-pad)] py-[var(--space-sm)]">
        <button
          type="button"
          onClick={onCancel}
          disabled={busy}
          className="
            text-[length:var(--text-label)] text-[color:var(--color-text-muted)]
            hover:text-[color:var(--color-text)]
            disabled:opacity-50
          "
        >
          Cancel
        </button>
        <button
          type="button"
          onClick={onConfirm}
          disabled={busy}
          className="
            rounded-[var(--radius-sm)] bg-[var(--color-accent)] px-[var(--space-md)] py-[var(--space-2xs)]
            text-[length:var(--text-label)] font-[var(--weight-semibold)] text-white
            hover:opacity-90
            disabled:opacity-50 disabled:cursor-not-allowed
          "
        >
          {busy ? 'Switching…' : 'Switch account'}
        </button>
      </div>
    </div>
  );
}
