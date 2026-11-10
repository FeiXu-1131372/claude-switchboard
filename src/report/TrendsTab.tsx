import { useEffect, useMemo, useState } from 'react';
import { Card } from '../components/ui/Card';
import { Badge } from '../components/ui/Badge';
import { Button } from '../components/ui/Button';
import { EmptyState } from '../components/ui/EmptyState';
import { formatTokens, formatCost } from '../lib/format';
import { IconTrends } from '../lib/icons';
import { ipc } from '../lib/ipc';
import { useTabData } from '../lib/useTabData';
import { useAppStore } from '../lib/store';
import type { DailyBucket, DailyModelBucket, ModelStats } from '../lib/types';
import { MODEL_COLOR, MODEL_VARIANT, modelKey, shortName } from './modelDisplay';

type Range = '7d' | '30d' | '90d' | '1y';
type Granularity = 'day' | 'week';
type Metric = 'cost' | 'tokens';

const RANGE_DAYS: Record<Range, number> = { '7d': 7, '30d': 30, '90d': 90, '1y': 365 };

/** Long ranges open at the zoom level that stays readable: daily bars for
 *  the two short windows, weekly bars for 90 days and a year. The user can
 *  still switch granularity by hand afterwards. */
const DEFAULT_GRANULARITY: Record<Range, Granularity> = {
  '7d': 'day',
  '30d': 'day',
  '90d': 'week',
  '1y': 'week',
};

/** One bar on the chart: a calendar day, or a whole calendar week. */
interface ChartBucket {
  key: string;
  /** Inclusive first day covered by the bar (YYYY-MM-DD, local). */
  start: string;
  /** Inclusive last day covered by the bar. Equals `start` for a day bar. */
  end: string;
  input_tokens: number;
  output_tokens: number;
  cost_usd: number;
}

function parseDay(date: string): Date {
  return new Date(`${date}T00:00:00`);
}

function shortDay(date: string): string {
  return parseDay(date).toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
}

function formatDay(d: Date): string {
  const month = String(d.getMonth() + 1).padStart(2, '0');
  const day = String(d.getDate()).padStart(2, '0');
  return `${d.getFullYear()}-${month}-${day}`;
}

/** Monday of the week containing `date`, as a local YYYY-MM-DD string. */
function mondayOf(date: string): string {
  const d = parseDay(date);
  d.setDate(d.getDate() - ((d.getDay() + 6) % 7));
  return formatDay(d);
}

function addDays(date: string, days: number): string {
  const d = parseDay(date);
  d.setDate(d.getDate() + days);
  return formatDay(d);
}

function toBuckets(trends: DailyBucket[], granularity: Granularity): ChartBucket[] {
  if (granularity === 'day') {
    return trends.map((d) => ({
      key: d.date,
      start: d.date,
      end: d.date,
      input_tokens: d.input_tokens,
      output_tokens: d.output_tokens,
      cost_usd: d.cost_usd,
    }));
  }

  const byWeek = new Map<string, ChartBucket>();
  for (const d of trends) {
    const key = mondayOf(d.date);
    const bucket = byWeek.get(key);
    if (bucket) {
      bucket.input_tokens += d.input_tokens;
      bucket.output_tokens += d.output_tokens;
      bucket.cost_usd += d.cost_usd;
    } else {
      byWeek.set(key, {
        key,
        start: key,
        end: addDays(key, 6),
        input_tokens: d.input_tokens,
        output_tokens: d.output_tokens,
        cost_usd: d.cost_usd,
      });
    }
  }
  return [...byWeek.values()].sort((a, b) => a.key.localeCompare(b.key));
}

function aggregateModels(rows: DailyModelBucket[]): ModelStats[] {
  const byModel = new Map<string, ModelStats>();
  for (const row of rows) {
    for (const m of row.models) {
      const acc = byModel.get(m.model) ?? {
        model: m.model,
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        cost_usd: 0,
      };
      acc.input_tokens += m.input_tokens;
      acc.output_tokens += m.output_tokens;
      acc.cache_read_tokens += m.cache_read_tokens;
      acc.cache_creation_tokens += m.cache_creation_tokens;
      acc.cost_usd += m.cost_usd;
      byModel.set(m.model, acc);
    }
  }
  return [...byModel.values()].sort(
    (a, b) => b.cost_usd - a.cost_usd || a.model.localeCompare(b.model),
  );
}

function bucketLabel(bucket: ChartBucket, granularity: Granularity): string {
  return granularity === 'week'
    ? `${shortDay(bucket.start)} – ${shortDay(bucket.end)}`
    : shortDay(bucket.start);
}

/** Sparse x-axis labels: ~7 labels for most ranges, month names for a year. */
function xLabel(
  bucket: ChartBucket,
  index: number,
  buckets: ChartBucket[],
  range: Range,
): string | null {
  const date = parseDay(bucket.start);

  if (range === '1y') {
    const prev = buckets[index - 1];
    return !prev || parseDay(prev.start).getMonth() !== date.getMonth()
      ? date.toLocaleDateString('en-US', { month: 'short' })
      : null;
  }

  const every = Math.max(1, Math.ceil(buckets.length / 7));
  if (index % every !== 0) return null;

  return range === '7d' || range === '30d'
    ? date.toLocaleDateString('en-US', { day: 'numeric' })
    : date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
}

export function TrendsTab() {
  const version = useAppStore((s) => s.sessionDataVersion);
  const [range, setRange] = useState<Range>('30d');
  const [granularity, setGranularity] = useState<Granularity>(DEFAULT_GRANULARITY['30d']);
  const [metric, setMetric] = useState<Metric>('cost');
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const days = RANGE_DAYS[range];

  const { data, error, loading, reload } = useTabData(
    () =>
      Promise.all([ipc.getDailyTrends(days), ipc.getDailyModelBreakdown(days)]).then(
        ([trends, breakdown]) => ({ trends, breakdown }),
      ),
    [version],
    `trends:${days}`,
  );

  useEffect(() => {
    setGranularity(DEFAULT_GRANULARITY[range]);
  }, [range]);

  useEffect(() => {
    setSelectedKey(null);
  }, [range, granularity]);

  const trends = data?.trends ?? null;
  const breakdown = data?.breakdown ?? null;

  const visibleData = useMemo(
    () => (trends ? trends.slice(-days) : []),
    [trends, days],
  );
  const chartBuckets = useMemo(
    () => toBuckets(visibleData, granularity),
    [visibleData, granularity],
  );

  const selectedBucket = useMemo(
    () => chartBuckets.find((b) => b.key === selectedKey) ?? null,
    [chartBuckets, selectedKey],
  );

  const selectedModels = useMemo(() => {
    if (!breakdown || !selectedBucket) return null;
    const rows = breakdown.filter(
      (b) => b.date >= selectedBucket.start && b.date <= selectedBucket.end,
    );
    return rows.length > 0 ? aggregateModels(rows) : null;
  }, [breakdown, selectedBucket]);

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
  if (loading || !trends || !breakdown) {
    return <p className="text-[color:var(--color-text-muted)]">Loading…</p>;
  }

  if (trends.length === 0) {
    return (
      <EmptyState
        icon={<IconTrends size={32} />}
        title="No trend data"
        description="Trends will appear after a few days of usage."
      />
    );
  }

  const valueOf = (b: ChartBucket) =>
    metric === 'cost' ? b.cost_usd : b.input_tokens + b.output_tokens;
  const maxValue = Math.max(...chartBuckets.map(valueOf), 1);
  const chartHeight = 160;
  // Warning colours were tuned for one day's spend; a weekly bar totals ~7
  // days, so scale the thresholds with the bucket width.
  const spendScale = granularity === 'week' ? 7 : 1;

  const totalCost = chartBuckets.reduce((s, b) => s + b.cost_usd, 0);
  const totalTokens = chartBuckets.reduce(
    (s, b) => s + b.input_tokens + b.output_tokens,
    0,
  );
  const average = chartBuckets.length > 0
    ? (metric === 'cost' ? totalCost : totalTokens) / chartBuckets.length
    : 0;
  const averageSuffix = granularity === 'week' ? '/wk' : '/day';

  return (
    <div className="flex flex-col gap-[var(--space-md)]">
      {/* Range, granularity and metric controls */}
      <div className="flex flex-wrap items-center justify-between gap-[var(--space-sm)]">
        <div className="flex flex-wrap items-center gap-[var(--space-sm)]">
          <SegmentedControl
            label="Range"
            value={range}
            options={[
              { value: '7d', label: '7d' },
              { value: '30d', label: '30d' },
              { value: '90d', label: '90d' },
              { value: '1y', label: '1y' },
            ]}
            onChange={setRange}
          />
          <SegmentedControl
            label="Interval"
            value={granularity}
            options={[
              { value: 'day', label: 'Daily' },
              { value: 'week', label: 'Weekly' },
            ]}
            onChange={setGranularity}
          />
        </div>
        <SegmentedControl
          label="Metric"
          value={metric}
          options={[
            { value: 'cost', label: 'Spent' },
            { value: 'tokens', label: 'Tokens' },
          ]}
          onChange={setMetric}
        />
      </div>

      {/* Chart */}
      <Card className="p-[var(--space-md)]">
        <div className="flex items-end gap-[2px]" style={{ height: chartHeight }}>
          {chartBuckets.map((bucket) => {
            const tokens = bucket.input_tokens + bucket.output_tokens;
            const heightPct = (valueOf(bucket) / maxValue) * 100;
            const isDanger = bucket.cost_usd >= 3 * spendScale;
            const isWarn = bucket.cost_usd >= 1.5 * spendScale && !isDanger;
            const isSelected = bucket.key === selectedKey;
            const primary =
              metric === 'cost'
                ? formatCost(bucket.cost_usd)
                : `${formatTokens(tokens)} tokens`;
            const secondary =
              metric === 'cost' ? `${formatTokens(tokens)} tokens` : formatCost(bucket.cost_usd);
            const when = bucketLabel(bucket, granularity);
            const label = `${when}: ${primary}, ${secondary}`;

            return (
              <div
                key={bucket.key}
                className="flex-1 flex flex-col justify-end group relative"
                style={{ height: '100%' }}
              >
                <button
                  type="button"
                  aria-label={label}
                  aria-pressed={isSelected}
                  onClick={() =>
                    setSelectedKey((k) => (k === bucket.key ? null : bucket.key))
                  }
                  className="w-full h-full flex flex-col justify-end"
                >
                  <div
                    data-testid={`day-bar-${bucket.key}`}
                    className={[
                      'w-full rounded-t-[2px] transition-[height,background-color] duration-[var(--duration-normal)]',
                      isDanger
                        ? 'bg-[var(--color-danger)]'
                        : isWarn
                          ? 'bg-[var(--color-warn)]'
                          : 'bg-[var(--color-accent)]',
                      isSelected
                        ? 'opacity-100 ring-2 ring-[var(--color-border-focus)]'
                        : 'opacity-80 group-hover:opacity-100',
                    ].join(' ')}
                    style={{ height: `${heightPct}%` }}
                  />
                </button>
                {/* Tooltip */}
                <div className="absolute bottom-full left-1/2 -translate-x-1/2 mb-[var(--space-xs)] hidden group-hover:block z-10">
                  <div className="bg-[var(--color-bg-elevated)] border border-[var(--color-border)] rounded-[var(--radius-sm)] px-[var(--space-sm)] py-[var(--space-xs)] whitespace-nowrap">
                    <div className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
                      {when}
                    </div>
                    <div className="mono text-[length:var(--text-label)] text-[color:var(--color-text)]">
                      {primary}
                    </div>
                    <div className="mono text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
                      {secondary}
                    </div>
                  </div>
                </div>
              </div>
            );
          })}
        </div>

        {/* X-axis labels */}
        <div className="flex mt-[var(--space-xs)]">
          {chartBuckets.map((bucket, i) => (
            <span
              key={bucket.key}
              className="flex-1 text-[length:var(--text-micro)] text-[color:var(--color-text-muted)] mono"
            >
              {xLabel(bucket, i, chartBuckets, range)}
            </span>
          ))}
        </div>
      </Card>

      {/* Selected day/week breakdown */}
      {selectedBucket && selectedModels && (
        <Card data-testid="day-breakdown-panel" className="p-[var(--space-md)] flex flex-col gap-[var(--space-sm)]">
          <div className="flex items-center justify-between">
            <span className="text-[length:var(--text-label)] font-[var(--weight-medium)] text-[color:var(--color-text)]">
              {granularity === 'week'
                ? `Week of ${shortDay(selectedBucket.start)}`
                : shortDay(selectedBucket.start)}
            </span>
            <span className="mono text-[length:var(--text-label)] text-[color:var(--color-text-secondary)]">
              {formatTokens(selectedBucket.input_tokens + selectedBucket.output_tokens)} ·{' '}
              {formatCost(selectedBucket.cost_usd)}
            </span>
          </div>
          <div className="flex flex-col gap-[var(--space-xs)]">
            {selectedModels.map((m) => {
              const bucketTokens = selectedBucket.input_tokens + selectedBucket.output_tokens;
              const modelTotal = m.input_tokens + m.output_tokens;
              const pct = metric === 'cost'
                ? selectedBucket.cost_usd > 0
                  ? (m.cost_usd / selectedBucket.cost_usd) * 100
                  : 0
                : bucketTokens > 0
                  ? (modelTotal / bucketTokens) * 100
                  : 0;
              const key = modelKey(m.model);

              return (
                <div key={m.model} className="flex flex-col gap-[var(--space-2xs)]">
                  <div className="flex items-center gap-[var(--space-sm)]">
                    <Badge variant={MODEL_VARIANT[key] ?? 'default'}>{shortName(m.model)}</Badge>
                    <div className="flex-1">
                      <div className="w-full h-[6px] rounded-[var(--radius-pill)] bg-[var(--color-track)] overflow-hidden">
                        <div
                          data-testid={`model-fill-${m.model}`}
                          className="h-full rounded-[var(--radius-pill)] transition-[width] duration-[var(--duration-bar)] ease-[var(--ease-spring)]"
                          style={{
                            width: `${pct}%`,
                            background: MODEL_COLOR[key] ?? 'var(--color-teal)',
                          }}
                        />
                      </div>
                    </div>
                    <span className="mono text-[length:var(--text-label)] text-[color:var(--color-text-muted)] tabular-nums min-w-[48px] text-right">
                      {formatTokens(modelTotal)}
                    </span>
                    <span className="mono text-[length:var(--text-label)] text-[color:var(--color-text-secondary)] tabular-nums min-w-[48px] text-right">
                      {formatCost(m.cost_usd)}
                    </span>
                  </div>
                  <span className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)] pl-[calc(var(--space-sm)+7px)]">
                    cache: {formatTokens(m.cache_read_tokens)} read · {formatTokens(m.cache_creation_tokens)} created
                  </span>
                </div>
              );
            })}
          </div>
        </Card>
      )}

      {/* Summary */}
      <div className="flex items-center gap-[var(--space-md)] px-[var(--space-2xs)]">
        <span className="mono text-[length:var(--text-label)] text-[color:var(--color-text-secondary)]">
          Avg{' '}
          {metric === 'cost'
            ? `${formatCost(average)}${averageSuffix}`
            : `${formatTokens(average)} tokens${averageSuffix}`}
        </span>
        <span className="text-[length:var(--text-label)] text-[color:var(--color-text-muted)]">·</span>
        <span className="mono text-[length:var(--text-label)] text-[color:var(--color-text-secondary)]">
          {formatCost(totalCost)} total
        </span>
      </div>
    </div>
  );
}

function SegmentedControl<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: readonly { value: T; label: string }[];
  onChange: (value: T) => void;
}) {
  return (
    <div
      role="group"
      aria-label={label}
      className="flex gap-[var(--space-2xs)] bg-[var(--color-track)] rounded-[var(--radius-sm)] p-[2px] w-fit"
    >
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          aria-pressed={value === option.value}
          onClick={() => onChange(option.value)}
          className={[
            'px-[var(--space-sm)] py-[var(--space-2xs)]',
            'text-[length:var(--text-label)] font-[var(--weight-medium)]',
            'rounded-[var(--radius-sm)]',
            'transition-[background,color] duration-[var(--duration-fast)]',
            value === option.value
              ? 'bg-[var(--color-bg-card)] text-[color:var(--color-text)]'
              : 'text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text-secondary)]',
          ].join(' ')}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}