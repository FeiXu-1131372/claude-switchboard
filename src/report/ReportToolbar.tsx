import type { ReactNode } from 'react';
import { SegmentedControl } from '../components/ui/SegmentedControl';
import { useReportStore, type ReportMetric, type ReportRange } from './reportState';

const RANGES: readonly { value: ReportRange; label: string }[] = [
  { value: '7d', label: '7d' },
  { value: '30d', label: '30d' },
  { value: '90d', label: '90d' },
  { value: '1y', label: '1y' },
];

const METRICS: readonly { value: ReportMetric; label: string }[] = [
  { value: 'cost', label: 'Spent' },
  { value: 'tokens', label: 'Tokens' },
];

/**
 * The first row of every report: the tab's one headline figure on the left,
 * and on the right the range and measure it was worked out for.
 *
 * Range and measure are shared. Changing either here changes it on every tab
 * that shows this toolbar, so the figures on different tabs describe the same
 * days and can be compared.
 */
export function ReportToolbar({
  figure,
  caption,
  showRange = true,
  showMetric = false,
  children,
}: {
  /** The headline number, already formatted. */
  figure: string;
  /** What the figure covers, e.g. "30 days · 412 sessions". */
  caption?: string;
  showRange?: boolean;
  showMetric?: boolean;
  /** A tab's own controls; they sit before the shared ones. */
  children?: ReactNode;
}) {
  const range = useReportStore((s) => s.range);
  const setRange = useReportStore((s) => s.setRange);
  const metric = useReportStore((s) => s.metric);
  const setMetric = useReportStore((s) => s.setMetric);

  return (
    <div className="flex flex-wrap items-end justify-between gap-x-[var(--space-md)] gap-y-[var(--space-sm)]">
      <div aria-live="polite" className="flex min-w-0 flex-col gap-[var(--space-2xs)]">
        <span className="mono tabular-nums text-[length:var(--text-title)] font-[var(--weight-semibold)] text-[color:var(--color-text)]">
          {figure}
        </span>
        {caption && (
          <span className="truncate text-[length:var(--text-label)] text-[color:var(--color-text-muted)]">
            {caption}
          </span>
        )}
      </div>
      <div className="flex flex-wrap items-center gap-[var(--space-sm)]">
        {children}
        {showMetric && (
          <SegmentedControl label="Metric" value={metric} options={METRICS} onChange={setMetric} />
        )}
        {showRange && (
          <SegmentedControl label="Range" value={range} options={RANGES} onChange={setRange} />
        )}
      </div>
    </div>
  );
}
