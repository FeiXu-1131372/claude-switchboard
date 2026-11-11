/**
 * UsageSummary — the hero usage readout of the popover. Shows one instrument
 * column per limit the account has (5h, 7d, and Fable when it has its own
 * weekly cap), the burn rate projection, Opus/Sonnet sub-rows, and
 * pay-as-you-go.
 *
 * When `condensed` is true the layout is tightened for embedding at the top
 * of the expanded report — same data, less vertical real-estate.
 */

import { motion } from 'framer-motion';
import { InstrumentColumn, InstrumentRow } from '../popover/InstrumentRow';
import { ChevronRight } from '../lib/icons';
import { isUltraPlan } from '../lib/plan';
import { sameReset } from '../lib/reset';
import type { BurnRateProjection, CachedUsage, Utilization } from '../lib/types';

interface UsageSummaryProps {
  usage: CachedUsage;
  thresholds: [number, number];
  /** Tighter layout for embedding at the top of the expanded report. */
  condensed?: boolean;
  /** Highlights the matching model sub-row as currently in use. */
  activeModel?: 'fable' | 'opus' | 'sonnet' | 'haiku' | null;
  /** When true, the model-split / pay-as-you-go detail rows collapse behind a
   * disclosure row (compact popover default). Omit for always-expanded. */
  collapsible?: boolean;
  detailsOpen?: boolean;
  onToggleDetails?: () => void;
  /** Display plan name (Pro / Max / Ultra). Ultra hides the 5h column when
   * the account reports no 5-hour window. */
  plan?: string | null;
}

/**
 * Whether anything sits behind the Details disclosure: the Opus/Sonnet split
 * (Max and Ultra only) or pay-as-you-go. Every other limit is in the hero.
 */
export function hasUsageDetails(snap: CachedUsage['snapshot']): boolean {
  const modelSplit =
    snap.seven_day != null && (snap.seven_day_opus != null || snap.seven_day_sonnet != null);
  return modelSplit || Boolean(snap.extra_usage?.is_enabled);
}

export function UsageSummary({
  usage,
  thresholds,
  condensed,
  activeModel,
  collapsible,
  detailsOpen,
  onToggleDetails,
  plan,
}: UsageSummaryProps) {
  if (usage.last_error === 'awaiting_statusline') {
    return (
      <p role="status" className="px-[var(--popover-pad)] py-[var(--space-md)] text-[length:var(--text-label)] leading-[var(--leading-label)] text-[color:var(--color-text-secondary)]">
        Waiting for Claude Code. Usage for this token comes from Claude Code&apos;s status line, so it shows up about a minute after you use Claude Code with it.
      </p>
    );
  }
  if (usage.last_error === 'usage_not_supported') {
    return (
      <p role="status" className="px-[var(--popover-pad)] py-[var(--space-md)] text-[length:var(--text-label)] leading-[var(--leading-label)] text-[color:var(--color-text-secondary)]">
        Usage tracking is unavailable for this inference-only token. Manage or replace it in Accounts.
      </p>
    );
  }
  const snap = usage.snapshot;
  const extra = snap.extra_usage;
  const [warn, danger] = thresholds;
  // Ultra removed the 5-hour window for verified accounts, and the usage API
  // reports `five_hour: null` for them. Show the weekly window alone. An
  // unverified Ultra account still reports a 5h bucket, so keep it visible
  // until verification lifts the limit.
  const showFiveHour = !isUltraPlan(plan) || snap.five_hour != null;
  // Per-model 7-day utilization is a Max/Ultra-plan-only field — the usage API
  // returns null for both opus and sonnet on Pro / Team-Pro plans (they have
  // a single combined 7-day quota, no per-model split). Render the rows only
  // when at least one bucket actually carries data, so they never sit empty
  // on plans that don't provide it.
  const hasModelSplit =
    snap.seven_day_opus != null || snap.seven_day_sonnet != null;
  // A disclosure with nothing behind it is not offered, and an account with
  // nothing to disclose is never "open", whatever the last account left.
  const hasDetails = hasUsageDetails(snap);
  const open = !collapsible || (Boolean(detailsOpen) && hasDetails);
  const showModelSplit = hasModelSplit && snap.seven_day != null && open;
  // Fable has its own weekly cap, independent of the Opus/Sonnet split, so it
  // is a limit in its own right and sits in the hero beside 5h and 7d: it is
  // visible at a glance, never behind the disclosure.
  const fable = snap.seven_day_fable;
  const columns = (showFiveHour ? 1 : 0) + 1 + (fable != null ? 1 : 0);
  const showExtra = extra?.is_enabled && open;

  return (
    <div className={condensed ? 'flex flex-col gap-[var(--space-xs)]' : 'flex flex-col'}>
      {/* Hero: one instrument column per limit — tighter padding in the
          collapsed glance view so the shorter popover height still fits the
          footer. */}
      <motion.div
        data-testid="usage-hero"
        initial={{ opacity: 0, y: 6 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.32, ease: [0.22, 1, 0.36, 1] }}
        style={{ gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))` }}
        className={`grid ${columns > 2 ? 'hero-dense' : ''} gap-x-[var(--space-lg)] px-[var(--popover-pad)] ${
          condensed
            ? 'py-[var(--space-sm)]'
            : !open
              ? 'pt-[var(--space-xs)] pb-[var(--space-sm)]'
              : 'pt-[var(--space-md)] pb-[var(--space-lg)]'
        }`}
      >
        {showFiveHour && (
          <div className="flex flex-col gap-[var(--space-xs)]">
            <InstrumentColumn
              label="5h"
              data={snap.five_hour}
              warnAt={warn}
              dangerAt={danger}
            />
            <BurnRateCaption
              burnRate={usage.burn_rate}
              warnAt={warn}
              dangerAt={danger}
            />
          </div>
        )}
        <InstrumentColumn
          label="7d"
          data={snap.seven_day}
          warnAt={warn}
          dangerAt={danger}
        />
        {fable != null && (
          <InstrumentColumn
            label="Fable"
            data={fable}
            warnAt={warn}
            dangerAt={danger}
            active={activeModel === 'fable'}
            showReset={!sameReset(snap.seven_day?.resets_at, fable.resets_at)}
          />
        )}
      </motion.div>

      {collapsible && hasDetails && (
        <>
          <Hairline />
          <button
            type="button"
            onClick={onToggleDetails}
            aria-expanded={open}
            className="flex items-center justify-between px-[var(--popover-pad)] py-[var(--space-xs)]"
          >
            <span className="text-[length:var(--text-micro)] font-[var(--weight-medium)] tracking-[var(--tracking-label)] uppercase text-[color:var(--color-text-muted)]">
              Details
            </span>
            <ChevronRight
              size={12}
              className={`text-[color:var(--color-text-muted)] transition-transform duration-[var(--duration-fast)] ${open ? 'rotate-90' : ''}`}
            />
          </button>
        </>
      )}

      {/* Opus / Sonnet sub-row — rendered whenever 7d data is present so the
       * model split stays visible even when one bucket is idle. The currently-
       * in-use family (derived from local session events) is highlighted. */}
      {showModelSplit && (
        <>
          <Hairline />
          <div className="grid grid-cols-2 gap-x-[var(--space-lg)] px-[var(--popover-pad)] py-[var(--space-sm)]">
            <InstrumentRow
              label="Opus"
              data={snap.seven_day_opus}
              warnAt={warn}
              dangerAt={danger}
              active={activeModel === 'opus'}
            />
            <InstrumentRow
              label="Sonnet"
              data={snap.seven_day_sonnet}
              warnAt={warn}
              dangerAt={danger}
              active={activeModel === 'sonnet'}
            />
          </div>
        </>
      )}

      {/* Pay-as-you-go — its own row, hairline-divided */}
      {showExtra && extra && (
        <>
          <Hairline />
          <div className="px-[var(--popover-pad)] py-[var(--space-sm)]">
            <ExtraRow
              pct={extra.utilization ?? 0}
              resetsAt={extra.resets_at ?? null}
              warnAt={warn}
              dangerAt={danger}
            />
          </div>
        </>
      )}
    </div>
  );
}

/* ───────────────────────── Private helpers ───────────────────────── */

/**
 * Tiny caption beneath the 5h instrument that answers "should I keep
 * coding?" — extrapolates current usage slope to the window reset and
 * shows the projected % at reset. Color-cued against the same warn/danger
 * thresholds the meter uses. Hidden when there's no projection yet
 * (cold start) or when the projection is essentially flat.
 */
function BurnRateCaption({
  burnRate,
  warnAt,
  dangerAt,
}: {
  burnRate: BurnRateProjection | null | undefined;
  warnAt: number;
  dangerAt: number;
}) {
  if (!burnRate) return null;
  // Hide jitter under ~0.1%/min — anything that small extrapolates to a
  // ≤6% delta over a full 5h window, which isn't actionable signal.
  if (Math.abs(burnRate.utilization_per_min) < 0.1) return null;

  const projected = Math.max(0, burnRate.projected_at_reset);
  const color =
    projected >= dangerAt
      ? 'var(--color-danger)'
      : projected >= warnAt
        ? 'var(--color-warn-text)'
        : 'var(--color-text-muted)';

  return (
    <span
      className="text-[length:var(--text-micro)] tabular-nums"
      style={{ color }}
      title={`${burnRate.utilization_per_min >= 0 ? '+' : ''}${burnRate.utilization_per_min.toFixed(2)}%/min`}
    >
      → ~{Math.round(projected)}% by reset
    </span>
  );
}

function Hairline() {
  return <div className="mx-[var(--popover-pad)] border-t border-[var(--color-rule)]" />;
}

function ExtraRow({
  pct,
  resetsAt,
  warnAt,
  dangerAt,
}: {
  pct: number;
  resetsAt: string | null;
  warnAt: number;
  dangerAt: number;
}) {
  const data: Utilization | null = resetsAt
    ? { utilization: pct, resets_at: resetsAt }
    : null;
  return (
    <InstrumentRow
      label="Pay-as-you-go"
      caption={resetsAt ? undefined : 'no reset window'}
      value={pct}
      data={data}
      warnAt={warnAt}
      dangerAt={dangerAt}
    />
  );
}
