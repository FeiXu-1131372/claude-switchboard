import { useMemo, useState } from 'react';
import { Card } from '../components/ui/Card';
import { Badge } from '../components/ui/Badge';
import { Button } from '../components/ui/Button';
import { EmptyState } from '../components/ui/EmptyState';
import { SkeletonRows } from '../components/ui/Skeleton';
import { SegmentedControl } from '../components/ui/SegmentedControl';
import { formatTokens, formatCost } from '../lib/format';
import { IconTrends } from '../lib/icons';
import { ipc } from '../lib/ipc';
import { useTabData } from '../lib/useTabData';
import { useAppStore } from '../lib/store';
import type { DailyBucket, DailyModelBucket, ModelStats } from '../lib/types';
import { formatAxis, niceTicks } from './chartScale';
import { MODEL_COLOR, MODEL_VARIANT, modelKey, shortName } from './modelDisplay';
import { ReportToolbar } from './ReportToolbar';
import { RANGE_DAYS, RANGE_LABEL, useReportStore, type ReportRange as Range } from './reportState';

type Granularity = 'day' | 'week';

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
export interface ChartBucket {
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

/** More slots than this and the dates are not a usage history (a clock set
 *  years out, say); the days are then drawn as they came. */
const MAX_SLOTS = 800;

/**
 * One entry per calendar day from the start of the range to today, with a
 * zero for a day nothing was used. The backend only returns days that have
 * usage, and drawing those side by side would put two bars a week apart next
 * to each other as if they were consecutive.
 *
 * The span is widened to hold every day that came back, so no usage is ever
 * dropped for falling outside the range this clock expects.
 */
export function fillDays(trends: DailyBucket[], days: number, today: Date): DailyBucket[] {
  if (trends.length === 0) return [];
  const rangeStart = formatDay(
    new Date(today.getFullYear(), today.getMonth(), today.getDate() - (days - 1)),
  );
  const dates = trends.map((d) => d.date);
  const first = [rangeStart, ...dates].reduce((a, b) => (a < b ? a : b));
  const last = [formatDay(today), ...dates].reduce((a, b) => (a > b ? a : b));
  const byDate = new Map(trends.map((d) => [d.date, d]));
  const out: DailyBucket[] = [];
  for (const d = parseDay(first); formatDay(d) <= last; d.setDate(d.getDate() + 1)) {
    if (out.length >= MAX_SLOTS) return [...trends].sort((a, b) => a.date.localeCompare(b.date));
    const date = formatDay(d);
    out.push(byDate.get(date) ?? { date, input_tokens: 0, output_tokens: 0, cost_usd: 0 });
  }
  return out;
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

/** Sparse x-axis labels: about seven for most ranges, month names for a year.
 *  A short range shows the day of the month, and names the month on the first
 *  label and wherever it changes, so "28 … 2" cannot be misread. */
export function xLabel(
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
  if (range === '90d') return date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });

  const lastLabelled = buckets[index - every];
  const newMonth = !lastLabelled || parseDay(lastLabelled.start).getMonth() !== date.getMonth();
  return date.toLocaleDateString('en-US', newMonth ? { month: 'short', day: 'numeric' } : { day: 'numeric' });
}

/** Bars need less air between them as they get thinner. */
function gapClass(count: number): string {
  if (count <= 100) return 'gap-[2px]';
  return count <= 200 ? 'gap-px' : 'gap-0';
}

export function TrendsTab() {
  const version = useAppStore((s) => s.sessionDataVersion);
  const range = useReportStore((s) => s.range);
  const metric = useReportStore((s) => s.metric);
  const days = RANGE_DAYS[range];

  // A hand-picked interval and a selected bar both belong to the range they
  // were chosen in. Keeping the range beside each means a new range starts
  // from its own default with nothing selected, without an effect to reset
  // them after the fact.
  const [intervalChoice, setIntervalChoice] = useState<{ range: Range; value: Granularity } | null>(null);
  const granularity =
    intervalChoice?.range === range ? intervalChoice.value : DEFAULT_GRANULARITY[range];
  const scope = `${range}:${granularity}`;
  const [selection, setSelection] = useState<{ scope: string; key: string } | null>(null);
  const selectedKey = selection?.scope === scope ? selection.key : null;

  const { data, error, loading, reload } = useTabData(
    () =>
      Promise.all([ipc.getDailyTrends(days), ipc.getDailyModelBreakdown(days)]).then(
        ([trends, breakdown]) => ({ trends, breakdown }),
      ),
    [version],
    `trends:${days}`,
  );

  const trends = data?.trends ?? null;
  const breakdown = data?.breakdown ?? null;

  const chartBuckets = useMemo(
    () => toBuckets(fillDays(trends ?? [], days, new Date()), granularity),
    [trends, days, granularity],
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
    return <SkeletonRows />;
  }

  const tokensOf = (b: ChartBucket) => b.input_tokens + b.output_tokens;
  const valueOf = (b: ChartBucket) => (metric === 'cost' ? b.cost_usd : tokensOf(b));
  const ticks = niceTicks(Math.max(0, ...chartBuckets.map(valueOf)));
  const top = ticks[2];

  // Averages are over the bars that have something in them: a fortnight away
  // should not halve what a working day is said to cost.
  const active = chartBuckets.filter((b) => b.cost_usd > 0 || tokensOf(b) > 0);
  const total = active.reduce((sum, b) => sum + valueOf(b), 0);
  const average = active.length > 0 ? total / active.length : 0;
  const show = (value: number) => (metric === 'cost' ? formatCost(value) : formatTokens(value));
  const per = granularity === 'week' ? '/wk' : '/day';
  const sparse = granularity === 'day' && active.length < days;
  const caption = [
    RANGE_LABEL[range],
    `avg ${show(average)}${per}${sparse ? ` on ${active.length} active ${active.length === 1 ? 'day' : 'days'}` : ''}`,
  ].join(' · ');

  const toolbar = (
    <ReportToolbar
      figure={metric === 'cost' ? formatCost(total) : `${formatTokens(total)} tokens`}
      caption={trends.length > 0 ? caption : RANGE_LABEL[range]}
      showMetric
    >
      <SegmentedControl
        label="Interval"
        value={granularity}
        options={[
          { value: 'day', label: 'Daily' },
          { value: 'week', label: 'Weekly' },
        ]}
        onChange={(value) => setIntervalChoice({ range, value })}
      />
    </ReportToolbar>
  );

  if (trends.length === 0) {
    return (
      <div className="flex flex-col gap-[var(--space-md)]">
        {toolbar}
        <EmptyState
          icon={<IconTrends size={32} />}
          title="Nothing in this range"
          description="Usage appears here day by day as you use Claude Code."
        />
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-[var(--space-md)]">
      {toolbar}

      {/* Chart */}
      <Card className="p-[var(--space-md)]">
        <div className="flex h-[var(--chart-height)]">
          {/* Y axis: zero, the midpoint and a round top the bars are sized against. */}
          <div data-testid="chart-axis" aria-hidden className="relative w-[var(--chart-gutter)] shrink-0">
            {ticks.map((tick, i) => (
              <span
                key={tick}
                className="absolute right-[var(--space-sm)] translate-y-1/2 mono tabular-nums text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]"
                style={{ bottom: `${i * 50}%` }}
              >
                {formatAxis(tick, metric)}
              </span>
            ))}
          </div>
          <div className="relative min-w-0 flex-1">
            {ticks.map((tick, i) => (
              <div
                key={tick}
                aria-hidden
                className="absolute inset-x-0 border-t border-[var(--color-border-subtle)]"
                style={{ bottom: `${i * 50}%` }}
              />
            ))}
            <div className={`relative flex h-full items-end ${gapClass(chartBuckets.length)}`}>
              {chartBuckets.map((bucket) => {
                const tokens = tokensOf(bucket);
                const value = valueOf(bucket);
                const isSelected = bucket.key === selectedKey;
                if (bucket.cost_usd <= 0 && tokens <= 0) {
                  // Nothing was used: the slot keeps its place on the time
                  // axis but there is nothing to select.
                  return <div key={bucket.key} data-testid={`day-slot-${bucket.key}`} className="h-full flex-1" />;
                }
                const primary =
                  metric === 'cost' ? formatCost(bucket.cost_usd) : `${formatTokens(tokens)} tokens`;
                const secondary =
                  metric === 'cost' ? `${formatTokens(tokens)} tokens` : formatCost(bucket.cost_usd);
                const when = bucketLabel(bucket, granularity);

                return (
                  <div
                    key={bucket.key}
                    data-testid={`day-slot-${bucket.key}`}
                    className="group relative flex h-full flex-1 flex-col justify-end"
                  >
                    <button
                      type="button"
                      aria-label={`${when}: ${primary}, ${secondary}`}
                      aria-pressed={isSelected}
                      onClick={() => setSelection(isSelected ? null : { scope, key: bucket.key })}
                      className="flex h-full w-full flex-col justify-end"
                    >
                      <div
                        data-testid={`day-bar-${bucket.key}`}
                        className={[
                          'w-full min-h-px rounded-t-[var(--radius-bar)] bg-[var(--color-accent)]',
                          'transition-[height,opacity] duration-[var(--duration-normal)]',
                          isSelected
                            ? 'opacity-100 ring-2 ring-[var(--color-border-focus)]'
                            : 'opacity-80 group-hover:opacity-100',
                        ].join(' ')}
                        style={{ height: `${(value / top) * 100}%` }}
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
          </div>
        </div>

        {/* X-axis labels */}
        <div className={`mt-[var(--space-xs)] flex pl-[var(--chart-gutter)] ${gapClass(chartBuckets.length)}`}>
          {chartBuckets.map((bucket, i) => (
            <span
              key={bucket.key}
              className="min-w-0 flex-1 whitespace-nowrap text-[length:var(--text-micro)] text-[color:var(--color-text-muted)] mono"
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
    </div>
  );
}
