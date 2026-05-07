import { useMemo } from 'react';
import { UsageBar } from '../popover/UsageBar';
import { ResetCountdown } from '../popover/ResetCountdown';
import type { AccountListEntry } from '../lib/generated/bindings';

interface Props {
  entry: AccountListEntry;
  thresholds: [number, number];
  shareHint?: string | null;
  onSwap?: () => void;
  swapBusy?: boolean;
  swapping?: boolean;
  onReauth?: () => void;
  reauthBusy?: boolean;
}

function PlanBadge({ plan }: { plan: string | null }) {
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

export function AccountRow({
  entry,
  thresholds,
  shareHint,
  onSwap,
  swapBusy = false,
  swapping = false,
  onReauth,
  reauthBusy = false,
}: Props) {
  const cached = entry.cached_usage;
  const fiveHour = cached?.snapshot.five_hour ?? null;
  const sevenDay = cached?.snapshot.seven_day ?? null;

  const errLabel = useMemo(() => {
    if (entry.last_error === 'auth_required')
      return 'token expired — re-authenticate';
    if (entry.last_error) return 'usage unavailable';
    return null;
  }, [entry.last_error]);

  const showSwap = !!onSwap && !entry.is_active;

  return (
    <div
      className={`
        group flex flex-col gap-[var(--space-2xs)]
        border-l-[3px] pl-[calc(var(--popover-pad)-3px)] pr-[var(--popover-pad)] py-[var(--space-sm)]
        ${entry.is_active
          ? 'bg-[var(--color-accent-dim)] border-[var(--color-accent)]'
          : 'border-transparent'}
        ${showSwap ? 'hover:bg-[var(--color-track)] focus-within:bg-[var(--color-track)]' : ''}
      `}
    >
      <div className="flex items-center gap-[var(--space-xs)]">
        <span
          className={`flex-1 min-w-0 truncate text-[length:var(--text-body)] ${
            entry.is_active
              ? 'font-[var(--weight-semibold)] text-[color:var(--color-text)]'
              : 'text-[color:var(--color-text)]'
          }`}
          title={entry.email}
        >
          {entry.email}
        </span>
        <PlanBadge plan={entry.subscription_type ?? null} />
        {entry.is_active && (
          <span
            className="
              shrink-0 inline-flex items-center rounded-[var(--radius-pill)]
              bg-[var(--color-accent)] px-[var(--space-xs)] py-[1px]
              text-[length:var(--text-micro)] font-[var(--weight-semibold)]
              uppercase tracking-[var(--tracking-label)] text-white
            "
          >
            Active
          </span>
        )}
        {showSwap && (
          <button
            type="button"
            onClick={onSwap}
            disabled={swapBusy}
            className={`
              shrink-0 rounded-[var(--radius-pill)]
              border border-[var(--color-border)]
              px-[var(--space-xs)] py-[1px]
              text-[length:var(--text-micro)] uppercase tracking-[var(--tracking-label)]
              text-[color:var(--color-text-secondary)]
              transition-opacity duration-[var(--duration-fast)]
              opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 focus-visible:opacity-100
              hover:text-[color:var(--color-accent)] hover:border-[color:var(--color-accent)]
              disabled:cursor-not-allowed disabled:hover:text-[color:var(--color-text-muted)] disabled:hover:border-[color:var(--color-border)]
              focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)] focus-visible:outline-offset-2
            `}
          >
            {swapping ? 'Switching…' : 'Switch account'}
          </button>
        )}
      </div>

      {errLabel ? (
        <div className="flex items-center gap-[var(--space-xs)]">
          <span className="flex-1 text-[length:var(--text-micro)] text-[color:var(--color-warn)]">
            {errLabel}
          </span>
          {entry.last_error === 'auth_required' && onReauth && (
            <button
              type="button"
              onClick={onReauth}
              disabled={reauthBusy}
              className="
                shrink-0 rounded-[var(--radius-pill)]
                border border-[color:var(--color-warn)]
                px-[var(--space-xs)] py-[1px]
                text-[length:var(--text-micro)] uppercase tracking-[var(--tracking-label)]
                text-[color:var(--color-warn)]
                hover:bg-[color:var(--color-warn)] hover:text-white
                disabled:cursor-not-allowed disabled:opacity-60 disabled:hover:bg-transparent disabled:hover:text-[color:var(--color-warn)]
                focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)] focus-visible:outline-offset-2
              "
            >
              {reauthBusy ? 'Opening browser…' : 'Re-authenticate'}
            </button>
          )}
        </div>
      ) : (
        <div className="flex flex-col gap-[var(--space-2xs)]">
          {fiveHour && (
            <div className="flex items-center gap-[var(--space-sm)]">
              <span className="w-[20px] text-[length:var(--text-micro)] text-[color:var(--color-text-muted)] mono">
                5h
              </span>
              <UsageBar value={fiveHour.utilization} warnAt={thresholds[0]} dangerAt={thresholds[1]} compact />
              <span className="w-[36px] text-[length:var(--text-micro)] mono text-right">
                {Math.round(fiveHour.utilization)}%
              </span>
              {fiveHour.resets_at && <ResetCountdown resetsAt={fiveHour.resets_at} compact />}
            </div>
          )}
          {sevenDay && (
            <div className="flex items-center gap-[var(--space-sm)]">
              <span className="w-[20px] text-[length:var(--text-micro)] text-[color:var(--color-text-muted)] mono">
                7d
              </span>
              <UsageBar value={sevenDay.utilization} warnAt={thresholds[0]} dangerAt={thresholds[1]} compact />
              <span className="w-[36px] text-[length:var(--text-micro)] mono text-right">
                {Math.round(sevenDay.utilization)}%
              </span>
              {sevenDay.resets_at && <ResetCountdown resetsAt={sevenDay.resets_at} compact />}
            </div>
          )}
          {shareHint && (
            <span className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
              shares quota with {shareHint}
            </span>
          )}
        </div>
      )}
    </div>
  );
}
