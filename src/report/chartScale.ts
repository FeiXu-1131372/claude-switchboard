import type { ReportMetric } from './reportState';

/** Multiples of a power of ten a chart may top out at. */
const STEPS = [1, 1.5, 2, 2.5, 5, 10];

/**
 * Axis ticks for a chart whose tallest value is `max`: zero, the midpoint,
 * and a round top at or above `max`. Bars are sized against the top tick, so
 * the gridlines mean something and the tallest bar does not touch the edge by
 * accident. A chart with nothing in it still gets a real scale (0 to 1), so
 * nothing divides by zero.
 */
export function niceTicks(max: number): [number, number, number] {
  if (!Number.isFinite(max) || max <= 0) return [0, 0.5, 1];
  const magnitude = 10 ** Math.floor(Math.log10(max));
  for (const step of STEPS) {
    // toPrecision trims binary noise such as 0.15000000000000002.
    const top = Number((step * magnitude).toPrecision(12));
    if (top >= max) return [0, Number((top / 2).toPrecision(12)), top];
  }
  return [0, 5 * magnitude, 10 * magnitude];
}

const UNITS = [
  [1e9, 'B'],
  [1e6, 'M'],
  [1e3, 'K'],
] as const;

/** `1500` → `1.5K`, `5000000` → `5M`: at most two decimals, none if round. */
function compact(value: number): string {
  for (const [size, unit] of UNITS) {
    if (value >= size) return Number((value / size).toFixed(2)) + unit;
  }
  return String(Number(value.toFixed(2)));
}

/** A short axis label for one tick. */
export function formatAxis(value: number, metric: ReportMetric): string {
  if (metric === 'tokens') return compact(value);
  if (value === 0) return '$0';
  if (value >= 1000) return '$' + compact(value);
  return '$' + (Number.isInteger(value) && value >= 10 ? String(value) : value.toFixed(2));
}
