import { forwardRef, type HTMLAttributes, useMemo } from 'react';
import { ResetCountdown } from './ResetCountdown';
import type { Utilization } from '../lib/types';

type ThresholdLevel = 'safe' | 'warn' | 'danger';

interface UsageBarProps extends Omit<HTMLAttributes<HTMLDivElement>, 'children'> {
  label?: string;
  /** Pass a Utilization object from the API, OR use value/timer directly */
  data?: Utilization | null;
  /** Raw value override (used when data is null but we want to show a number) */
  value?: number;
  warnAt?: number;
  dangerAt?: number;
  size?: 'sm' | 'md';
  showLabel?: boolean;
  /** Raw timer string override */
  timer?: string;
  /** No-op layout hint for embedded contexts. */
  compact?: boolean;
}

const heightMap = { sm: 'h-[var(--bar-height-sm)]', md: 'h-[var(--bar-height-md)]' };

function getLevel(v: number, warn: number, danger: number): ThresholdLevel {
  if (v >= danger) return 'danger';
  if (v >= warn) return 'warn';
  return 'safe';
}

const fillMap: Record<ThresholdLevel, string> = {
  safe: 'bg-[var(--color-accent)]',
  warn: 'bg-[var(--color-warn)]',
  danger: 'bg-[var(--color-danger)]',
};

const textColorMap: Record<ThresholdLevel, string> = {
  safe: 'text-[color:var(--color-text)]',
  warn: 'text-[color:var(--color-warn)]',
  danger: 'text-[color:var(--color-danger)]',
};

export const UsageBar = forwardRef<HTMLDivElement, UsageBarProps>(
  (
    {
      label,
      data,
      value: valueProp,
      warnAt = 75,
      dangerAt = 90,
      size = 'md',
      showLabel = true,
      timer: timerProp,
      className = '',
      ...props
    },
    ref,
  ) => {
    const rawValue = data?.utilization ?? valueProp ?? 0;
    const clamped = Math.max(0, Math.min(100, rawValue));
    const level = useMemo(() => getLevel(clamped, warnAt, dangerAt), [clamped, warnAt, dangerAt]);

    if (!data && valueProp === undefined) {
      return (
        <div className={['flex items-center justify-between py-2', className].join(' ')} {...props}>
          <span className="text-[length:var(--text-label)] text-[color:var(--color-text-muted)]">{label}</span>
          <span className="mono text-[length:var(--text-label)] text-[color:var(--color-text-muted)] opacity-60">n/a</span>
        </div>
      );
    }

    // Sub-bars (size='sm') are children of a parent bar that already shows the
    // reset countdown — repeating it inside a narrow grid cell makes the
    // timer wrap onto a second line and collide with the percentage.
    const showTimer = size !== 'sm';
    const pctSize = size === 'sm' ? 'text-[length:var(--text-title)]' : 'text-[length:var(--text-pct)]';

    return (
      <div ref={ref} className={['flex flex-col gap-[6px]', className].join(' ')} {...props}>
        <div className="flex items-center justify-between gap-[var(--space-sm)]">
          <div className="flex items-baseline gap-[var(--space-sm)] min-w-0">
            <span className="text-[length:var(--text-label)] font-[var(--weight-medium)] text-[color:var(--color-text-secondary)] shrink-0">
              {label}
            </span>
            {showTimer && data?.resets_at && (
              <ResetCountdown resetsAt={data.resets_at} />
            )}
            {showTimer && timerProp && !data?.resets_at && (
              <span className="mono text-[length:var(--text-micro)] text-[color:var(--color-text-muted)] truncate">
                {timerProp}
              </span>
            )}
          </div>
          {showLabel && (
            <span
              className={[
                'mono font-[var(--weight-semibold)] tabular-nums leading-none shrink-0',
                pctSize,
                textColorMap[level],
              ].join(' ')}
            >
              {Math.round(clamped)}%
            </span>
          )}
        </div>

        <div
          className={[
            'w-full rounded-[var(--radius-pill)] overflow-hidden',
            'bg-[var(--color-track)]',
            heightMap[size],
          ].join(' ')}
        >
          <div
            className={[
              'h-full rounded-[var(--radius-pill)]',
              fillMap[level],
              'transition-[width,background-color]',
              'duration-[var(--duration-bar)] ease-[var(--ease-spring)]',
              '[transition-duration:var(--duration-bar),var(--duration-fast)]',
            ].join(' ')}
            style={{ width: `${clamped}%` }}
          />
        </div>
      </div>
    );
  },
);

UsageBar.displayName = 'UsageBar';
