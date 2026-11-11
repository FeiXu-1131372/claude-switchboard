import type { DailyBucket } from '../lib/types';
import type { ReportMetric } from './reportState';

/** One day on the activity calendar. */
export interface HeatCell {
  /** Local calendar day, `YYYY-MM-DD`. */
  date: string;
  value: number;
  /** 0 is a day with no use; 1 to 4 rank the days that had some. */
  level: 0 | 1 | 2 | 3 | 4;
}

function dayKey(d: Date): string {
  const month = String(d.getMonth() + 1).padStart(2, '0');
  const day = String(d.getDate()).padStart(2, '0');
  return `${d.getFullYear()}-${month}-${day}`;
}

/** Nearest-rank percentile of an ascending list. */
function percentile(sorted: number[], p: number): number {
  return sorted[Math.max(0, Math.ceil(p * sorted.length) - 1)];
}

/**
 * The last `weeks` weeks as one cell per day: the first cell is a Monday, the
 * last is `today`, so the current week is simply shorter.
 *
 * Days are ranked against each other rather than against the busiest one. A
 * step is a quartile of the days that had any use, so one enormous day takes
 * the top step and the rest still spread over the others, where a linear
 * scale would leave them all on the faintest.
 */
export function toHeatCells(
  daily: DailyBucket[],
  metric: ReportMetric,
  today: Date,
  weeks = 26,
): HeatCell[] {
  const valueOf = (d: DailyBucket) =>
    metric === 'cost' ? d.cost_usd : d.input_tokens + d.output_tokens;
  const byDate = new Map(daily.map((d) => [d.date, valueOf(d)]));

  // Monday of this week, then back the remaining weeks. Built from calendar
  // fields and stepped with setDate, so a 23- or 25-hour day at a
  // daylight-saving change is still exactly one cell.
  const cursor = new Date(today.getFullYear(), today.getMonth(), today.getDate());
  cursor.setDate(cursor.getDate() - ((cursor.getDay() + 6) % 7) - (weeks - 1) * 7);
  const last = dayKey(today);

  const dates: string[] = [];
  for (let key = dayKey(cursor); key <= last; key = dayKey(cursor)) {
    dates.push(key);
    cursor.setDate(cursor.getDate() + 1);
  }

  const used = dates
    .map((date) => byDate.get(date) ?? 0)
    .filter((v) => v > 0)
    .sort((a, b) => a - b);
  const steps = used.length > 0 ? [0.25, 0.5, 0.75].map((p) => percentile(used, p)) : [];

  return dates.map((date) => {
    const value = byDate.get(date) ?? 0;
    const level = value > 0 ? 1 + steps.filter((step) => step < value).length : 0;
    return { date, value, level: level as HeatCell['level'] };
  });
}
