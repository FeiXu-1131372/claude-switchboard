import { type HTMLAttributes, forwardRef, useMemo } from 'react';

type ThresholdLevel = 'safe' | 'warn' | 'danger';
type BarSize = 'sm' | 'md' | 'lg';

interface ProgressBarProps extends Omit<HTMLAttributes<HTMLDivElement>, 'children'> {
  /** 0–100 */
  value: number;
  /** 0–100, default 75 */
  warnThreshold?: number;
  /** 0–100, default 90 */
  dangerThreshold?: number;
  size?: BarSize;
  /** Show the percentage label */
  showLabel?: boolean;
}

const sizeMap: Record<BarSize, string> = {
  sm: 'h-[var(--bar-height-sm)]',
  md: 'h-[var(--bar-height-md)]',
  lg: 'h-[var(--bar-height-lg)]',
};

function getLevel(value: number, warn: number, danger: number): ThresholdLevel {
  if (value >= danger) return 'danger';
  if (value >= warn) return 'warn';
  return 'safe';
}

const fillMap: Record<ThresholdLevel, string> = {
  safe: 'bg-[var(--color-accent)]',
  warn: 'bg-[var(--color-warn)]',
  danger: 'bg-[var(--color-danger)]',
};

const colorMap: Record<ThresholdLevel, string> = {
  safe: 'text-[color:var(--color-text)]',
  warn: 'text-[color:var(--color-warn)]',
  danger: 'text-[color:var(--color-danger)]',
};

export const ProgressBar = forwardRef<HTMLDivElement, ProgressBarProps>(
  (
    {
      value,
      warnThreshold = 75,
      dangerThreshold = 90,
      size = 'md',
      showLabel = true,
      className = '',
      ...props
    },
    ref,
  ) => {
    const clamped = Math.max(0, Math.min(100, value));
    const level = useMemo(
      () => getLevel(clamped, warnThreshold, dangerThreshold),
      [clamped, warnThreshold, dangerThreshold],
    );

    return (
      <div
        ref={ref}
        className={['flex items-center gap-[var(--space-sm)]', className].join(' ')}
        role="progressbar"
        aria-valuenow={clamped}
        aria-valuemin={0}
        aria-valuemax={100}
        {...props}
      >
        <div
          className={[
            'flex-1 rounded-[var(--radius-pill)] overflow-hidden',
            'bg-[var(--color-track)]',
            sizeMap[size],
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
        {showLabel && (
          <span
            className={[
              'mono text-[length:var(--text-title)] font-[var(--weight-semibold)] tabular-nums min-w-[44px] text-right',
              colorMap[level],
            ].join(' ')}
          >
            {Math.round(clamped)}%
          </span>
        )}
      </div>
    );
  },
);

ProgressBar.displayName = 'ProgressBar';
