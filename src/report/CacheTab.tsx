import { Card } from '../components/ui/Card';
import { EmptyState } from '../components/ui/EmptyState';
import { Button } from '../components/ui/Button';
import { formatCost, formatTokens } from '../lib/format';
import { IconCache } from '../lib/icons';
import { ipc } from '../lib/ipc';
import { useTabData } from '../lib/useTabData';
import { useAppStore } from '../lib/store';

export function CacheTab() {
  const version = useAppStore((s) => s.sessionDataVersion);
  const { data, error, loading, reload } = useTabData(
    () => ipc.getCacheStats(30),
    [version],
    'cache-tab:30',
  );

  if (error) {
    return (
      <EmptyState
        icon={<IconCache size={32} />}
        title="Couldn't load cache stats"
        description={error}
        action={<Button variant="ghost" size="sm" onClick={reload}>Retry</Button>}
      />
    );
  }
  if (loading || !data) {
    return <p className="text-[color:var(--color-text-muted)]">Loading…</p>;
  }

  const totalCacheTokens = data.total_cache_read_tokens + data.total_cache_creation_tokens;
  if (totalCacheTokens === 0) {
    return (
      <EmptyState
        icon={<IconCache size={32} />}
        title="No cache data"
        description="Cache statistics will appear as you use Claude with prompt caching."
      />
    );
  }

  const hitRatePct = data.hit_ratio * 100;
  const circumference = 2 * Math.PI * 50;
  const strokeLength = (hitRatePct / 100) * circumference;

  return (
    <div className="flex flex-col gap-[var(--space-lg)]">
      {/* Hero: cache hit rate ring */}
      <div className="flex items-center justify-center py-[var(--space-lg)]">
        <div className="relative">
          <svg width="160" height="160" viewBox="0 0 160 160">
            <circle
              cx="80"
              cy="80"
              r="50"
              fill="none"
              stroke="var(--color-track)"
              strokeWidth="12"
            />
            <circle
              cx="80"
              cy="80"
              r="50"
              fill="none"
              stroke="var(--color-accent)"
              strokeWidth="12"
              strokeLinecap="round"
              strokeDasharray={`${strokeLength} ${circumference - strokeLength}`}
              transform="rotate(-90 80 80)"
              className="transition-[stroke-dasharray] duration-[var(--duration-slow)] ease-[var(--ease-spring)]"
            />
          </svg>
          <div className="absolute inset-0 flex flex-col items-center justify-center">
            <span className="mono text-[length:var(--text-display)] font-[var(--weight-semibold)] text-[color:var(--color-accent-text)] leading-[var(--leading-display)]">
              {Math.round(hitRatePct)}%
            </span>
            <span className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">cache hit rate</span>
          </div>
        </div>
      </div>

      {/* Stats grid */}
      <div className="grid grid-cols-2 gap-[var(--space-sm)]">
        <Card className="p-[var(--space-md)] flex flex-col gap-[4px]">
          <span className="text-[length:var(--text-label)] text-[color:var(--color-text-muted)]">Cache reads</span>
          <span className="mono text-[length:var(--text-body)] font-[var(--weight-semibold)] text-[color:var(--color-text)]">
            {formatTokens(data.total_cache_read_tokens)}
          </span>
        </Card>
        <Card className="p-[var(--space-md)] flex flex-col gap-[4px]">
          <span className="text-[length:var(--text-label)] text-[color:var(--color-text-muted)]">Cache writes</span>
          <span className="mono text-[length:var(--text-body)] font-[var(--weight-semibold)] text-[color:var(--color-text)]">
            {formatTokens(data.total_cache_creation_tokens)}
          </span>
        </Card>
        <Card className="p-[var(--space-md)] flex flex-col gap-[4px]">
          <span className="text-[length:var(--text-label)] text-[color:var(--color-text-muted)]">Total cached</span>
          <span className="mono text-[length:var(--text-body)] font-[var(--weight-semibold)] text-[color:var(--color-text)]">
            {formatTokens(totalCacheTokens)}
          </span>
        </Card>
        <Card className="p-[var(--space-md)] flex flex-col gap-[4px]">
          <span className="text-[length:var(--text-label)] text-[color:var(--color-text-muted)]">Estimated savings</span>
          <span className="mono text-[length:var(--text-body)] font-[var(--weight-semibold)] text-[color:var(--color-accent-text)]">
            {formatCost(data.estimated_savings_usd)}
          </span>
        </Card>
      </div>

      {/* Breakdown bar */}
      <Card className="p-[var(--space-md)]">
        <div className="flex flex-col gap-[var(--space-sm)]">
          <span className="text-[length:var(--text-label)] text-[color:var(--color-text-muted)]">Cache token breakdown</span>
          <div className="flex h-[10px] rounded-[var(--radius-pill)] overflow-hidden gap-[1px]">
            <div
              className="h-full bg-[var(--color-accent)] rounded-l-[var(--radius-pill)]"
              style={{ width: `${hitRatePct}%` }}
            />
            <div
              className="h-full bg-[var(--color-warn)] rounded-r-[var(--radius-pill)]"
              style={{ width: `${100 - hitRatePct}%` }}
            />
          </div>
          <div className="flex gap-[var(--space-md)]">
            <div className="flex items-center gap-[var(--space-2xs)]">
              <div className="w-[8px] h-[8px] rounded-full bg-[var(--color-accent)]" />
              <span className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">Cache reads</span>
            </div>
            <div className="flex items-center gap-[var(--space-2xs)]">
              <div className="w-[8px] h-[8px] rounded-full bg-[var(--color-warn)]" />
              <span className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">Cache writes</span>
            </div>
          </div>
        </div>
      </Card>
    </div>
  );
}
