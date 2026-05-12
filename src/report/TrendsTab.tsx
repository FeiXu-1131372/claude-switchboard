import { useMemo, useState } from 'react';
import { Card } from '../components/ui/Card';
import { Button } from '../components/ui/Button';
import { EmptyState } from '../components/ui/EmptyState';
import { formatTokens } from '../lib/format';
import { IconTrends } from '../lib/icons';
import { ipc } from '../lib/ipc';
import { useTabData } from '../lib/useTabData';
import { useAppStore } from '../lib/store';

export function TrendsTab() {
  const version = useAppStore((s) => s.sessionDataVersion);
  const { data, error, loading, reload } = useTabData(
    () => ipc.getDailyTrends(30),
    [version],
  );
  const [range, setRange] = useState<'7d' | '30d'>('30d');

  const visibleData = useMemo(() => {
    if (!data) return [];
    const days = range === '7d' ? 7 : 30;
    return data.slice(-days);
  }, [data, range]);

  if (error) {
    return (
      <EmptyState
        icon={<IconTrends size={32} />}
        title="Couldn't load trends"
        description={error}
        action={<Button variant="ghost" size="sm" onClick={reload}>Retry</Button>}
      />
    );
  }
  if (loading || !data) {
    return <p className="text-[color:var(--color-text-muted)]">Loading…</p>;
  }

  if (data.length === 0) {
    return (
      <EmptyState
        icon={<IconTrends size={32} />}
        title="No trend data"
        description="Trends will appear after a few days of usage."
      />
    );
  }

  const maxValue = Math.max(
    ...visibleData.map((d) => d.input_tokens + d.output_tokens),
    1,
  );
  const chartHeight = 160;

  return (
    <div className="flex flex-col gap-[var(--space-md)]">
      {/* Range selector */}
      <div className="flex gap-[var(--space-2xs)] bg-[var(--color-track)] rounded-[var(--radius-sm)] p-[2px] w-fit">
        {(['7d', '30d'] as const).map((r) => (
          <button
            key={r}
            type="button"
            onClick={() => setRange(r)}
            className={[
              'px-[var(--space-sm)] py-[var(--space-2xs)]',
              'text-[length:var(--text-label)] font-[var(--weight-medium)]',
              'rounded-[var(--radius-sm)]',
              'transition-[background,color] duration-[var(--duration-fast)]',
              range === r
                ? 'bg-[var(--color-bg-card)] text-[color:var(--color-text)]'
                : 'text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text-secondary)]',
            ].join(' ')}
          >
            {r}
          </button>
        ))}
      </div>

      {/* Chart */}
      <Card className="p-[var(--space-md)]">
        <div className="flex items-end gap-[2px]" style={{ height: chartHeight }}>
          {visibleData.map((day) => {
            const total = day.input_tokens + day.output_tokens;
            const heightPct = (total / maxValue) * 100;
            const isDanger = day.cost_usd >= 3;
            const isWarn = day.cost_usd >= 1.5 && !isDanger;

            return (
              <div
                key={day.date}
                className="flex-1 flex flex-col justify-end group relative"
                style={{ height: '100%' }}
              >
                <div
                  className={[
                    'w-full rounded-t-[2px] transition-[height,background-color] duration-[var(--duration-normal)]',
                    isDanger
                      ? 'bg-[var(--color-danger)]'
                      : isWarn
                        ? 'bg-[var(--color-warn)]'
                        : 'bg-[var(--color-accent)]',
                    'opacity-80 group-hover:opacity-100',
                  ].join(' ')}
                  style={{ height: `${heightPct}%` }}
                />
                {/* Tooltip */}
                <div className="absolute bottom-full left-1/2 -translate-x-1/2 mb-[var(--space-xs)] hidden group-hover:block z-10">
                  <div className="bg-[var(--color-bg-elevated)] border border-[var(--color-border)] rounded-[var(--radius-sm)] px-[var(--space-sm)] py-[var(--space-xs)] whitespace-nowrap">
                    <div className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
                      {new Date(day.date).toLocaleDateString('en-US', { month: 'short', day: 'numeric' })}
                    </div>
                    <div className="mono text-[length:var(--text-label)] text-[color:var(--color-text)]">
                      {formatTokens(total)}
                    </div>
                    <div className="mono text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
                      ${day.cost_usd.toFixed(2)}
                    </div>
                  </div>
                </div>
              </div>
            );
          })}
        </div>

        {/* X-axis labels */}
        <div className="flex mt-[var(--space-xs)]">
          {visibleData.map((day, i) => (
            <span
              key={day.date}
              className="flex-1 text-[length:var(--text-micro)] text-[color:var(--color-text-muted)] mono"
            >
              {i % (range === '7d' ? 1 : 5) === 0
                ? new Date(day.date).toLocaleDateString('en-US', { day: 'numeric' })
                : null}
            </span>
          ))}
        </div>
      </Card>

      {/* Summary */}
      <div className="flex items-center gap-[var(--space-md)] px-[var(--space-2xs)]">
        <span className="mono text-[length:var(--text-label)] text-[color:var(--color-text-secondary)]">
          Avg {formatTokens(visibleData.reduce((s, d) => s + d.input_tokens + d.output_tokens, 0) / visibleData.length)}
        </span>
        <span className="text-[length:var(--text-label)] text-[color:var(--color-text-muted)]">·</span>
        <span className="mono text-[length:var(--text-label)] text-[color:var(--color-text-secondary)]">
          ${visibleData.reduce((s, d) => s + d.cost_usd, 0).toFixed(2)} total
        </span>
      </div>
    </div>
  );
}
