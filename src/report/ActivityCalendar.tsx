import { useMemo, useRef, useState, type KeyboardEvent } from 'react';
import { Button } from '../components/ui/Button';
import { Card } from '../components/ui/Card';
import { formatCost, formatTokens } from '../lib/format';
import { ipc } from '../lib/ipc';
import { useAppStore } from '../lib/store';
import { useTabData } from '../lib/useTabData';
import { toHeatCells, type HeatCell } from './calendar';
import { useReportStore, type ReportMetric } from './reportState';

const WEEKS = 26;
const DAYS = WEEKS * 7;
const WEEKDAYS = ['Mon', '', 'Wed', '', 'Fri', '', ''];
const LEVELS = [0, 1, 2, 3, 4] as const;

/** How far each arrow key moves, in days. Days run down a column, so a week
 *  is one column across. */
const STEP: Record<string, number> = { ArrowUp: -1, ArrowDown: 1, ArrowLeft: -7, ArrowRight: 7 };

function parseDay(date: string): Date {
  return new Date(`${date}T00:00:00`);
}

function describe(cell: HeatCell, metric: ReportMetric): string {
  const when = parseDay(cell.date).toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
  const value = metric === 'cost' ? formatCost(cell.value) : `${formatTokens(cell.value)} tokens`;
  return `${when}: ${value}`;
}

/** The month to print above each week column, where a new month begins. */
function monthLabels(cells: HeatCell[]): (string | null)[] {
  const starts = Array.from({ length: Math.ceil(cells.length / 7) }, (_, week) =>
    parseDay(cells[week * 7].date),
  );
  return starts.map((start, week) => {
    const next = starts[week + 1];
    const isNew = week === 0 || starts[week - 1].getMonth() !== start.getMonth();
    // A first column that is the tail of a month would print its name right
    // against the next one.
    const crowded = week === 0 && next !== undefined && next.getMonth() !== start.getMonth();
    return isNew && !crowded ? start.toLocaleDateString('en-US', { month: 'short' }) : null;
  });
}

/**
 * Half a year of days, shaded by how much each was used.
 *
 * It answers a different question from the chart above it: not "how much",
 * but "when": which weeks were heavy, which days of the week are idle. So it
 * always covers the same 26 weeks, whatever range the chart is set to, and
 * says so. Shades are steps of the accent only; amber and red stay reserved
 * for limits.
 */
export function ActivityCalendar() {
  const version = useAppStore((s) => s.sessionDataVersion);
  const metric = useReportStore((s) => s.metric);
  const { data, error, reload } = useTabData(
    () => ipc.getDailyTrends(DAYS),
    [version],
    `activity:${DAYS}`,
  );

  const cells = useMemo(() => toHeatCells(data ?? [], metric, new Date(), WEEKS), [data, metric]);
  const months = useMemo(() => monthLabels(cells), [cells]);

  const grid = useRef<HTMLDivElement>(null);
  // The one cell Tab lands on; the arrow keys move it. Starts on today.
  const [current, setCurrent] = useState<number | null>(null);
  const [shown, setShown] = useState<number | null>(null);
  const tabStop = Math.min(current ?? cells.length - 1, cells.length - 1);

  if (!data) {
    if (!error) return null;
    return (
      <Card className="flex items-center justify-between gap-[var(--space-md)] p-[var(--space-md)]">
        <span className="text-[length:var(--text-label)] text-[color:var(--color-text-muted)]">
          Couldn&rsquo;t load activity: {error}
        </span>
        <Button variant="ghost" size="sm" onClick={reload}>
          Retry
        </Button>
      </Card>
    );
  }
  // No use in half a year: an empty grid would only be decoration.
  if (cells.every((c) => c.value === 0)) return null;

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>, index: number) => {
    const step = STEP[e.key];
    if (step === undefined) return;
    e.preventDefault();
    const next = index + step;
    if (next < 0 || next >= cells.length) return;
    setCurrent(next);
    grid.current?.querySelector<HTMLElement>(`[data-index="${next}"]`)?.focus();
  };

  return (
    <Card className="flex flex-col gap-[var(--space-sm)] p-[var(--space-md)]">
      <div className="flex flex-wrap items-baseline justify-between gap-x-[var(--space-md)] gap-y-[var(--space-2xs)]">
        <div className="flex items-baseline gap-[var(--space-sm)]">
          <span className="text-[length:var(--text-label)] font-[var(--weight-medium)] text-[color:var(--color-text)]">
            Activity
          </span>
          <span className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
            Last {WEEKS} weeks · {metric === 'cost' ? 'spend' : 'tokens'} per day
          </span>
        </div>
        <div className="flex items-center gap-[var(--space-xs)] text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
          <span>Less</span>
          <span aria-hidden className="flex gap-[var(--space-2xs)]">
            {LEVELS.map((level) => (
              <span
                key={level}
                className="size-[var(--space-md)] rounded-[var(--radius-bar)]"
                style={{ background: `var(--color-heat-${level})` }}
              />
            ))}
          </span>
          <span>More</span>
        </div>
      </div>

      <div className="flex flex-col gap-[var(--space-2xs)]">
        {/* Months, above the week each begins in. */}
        <div
          aria-hidden
          className="grid gap-[var(--space-2xs)] pl-[var(--chart-gutter)]"
          style={{ gridTemplateColumns: `repeat(${months.length}, minmax(0, 1fr))` }}
        >
          {months.map((month, week) => (
            <span
              key={week}
              className="whitespace-nowrap mono text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]"
            >
              {month}
            </span>
          ))}
        </div>

        <div className="flex">
          <div
            aria-hidden
            className="grid w-[var(--chart-gutter)] shrink-0 grid-rows-7 gap-[var(--space-2xs)] pr-[var(--space-sm)]"
          >
            {WEEKDAYS.map((name, row) => (
              <span
                key={row}
                className="flex items-center justify-end mono text-[length:var(--text-micro)] leading-none text-[color:var(--color-text-muted)]"
              >
                {name}
              </span>
            ))}
          </div>

          {/* One column per week, Monday at the top; the last column stops at today. */}
          <div
            ref={grid}
            role="group"
            aria-label={`Activity, last ${WEEKS} weeks`}
            className="grid min-w-0 flex-1 grid-flow-col grid-rows-7 gap-[var(--space-2xs)]"
            style={{ gridTemplateColumns: `repeat(${months.length}, minmax(0, 1fr))` }}
          >
            {cells.map((cell, index) => {
              const label = describe(cell, metric);
              const week = Math.floor(index / 7);
              const side =
                week < 3 ? 'left-0' : week >= months.length - 3 ? 'right-0' : 'left-1/2 -translate-x-1/2';
              return (
                <div key={cell.date} className="relative">
                  <div
                    role="img"
                    aria-label={label}
                    data-index={index}
                    tabIndex={index === tabStop ? 0 : -1}
                    onKeyDown={(e) => onKeyDown(e, index)}
                    onMouseEnter={() => setShown(index)}
                    onMouseLeave={() => setShown((s) => (s === index ? null : s))}
                    onFocus={() => setShown(index)}
                    onBlur={() => setShown((s) => (s === index ? null : s))}
                    className="aspect-square w-full rounded-[var(--radius-bar)]"
                    style={{ background: `var(--color-heat-${cell.level})` }}
                  />
                  {shown === index && (
                    <span
                      role="tooltip"
                      className={[
                        'pointer-events-none absolute bottom-full z-10 mb-[var(--space-xs)] whitespace-nowrap',
                        'rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-bg-elevated)]',
                        'px-[var(--space-sm)] py-[var(--space-xs)]',
                        'mono text-[length:var(--text-micro)] text-[color:var(--color-text)]',
                        side,
                      ].join(' ')}
                    >
                      {label}
                    </span>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </Card>
  );
}
