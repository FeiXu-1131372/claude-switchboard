/**
 * InstrumentColumn / InstrumentRow — the typographic hero of the popover.
 *
 * The big idea: numbers ARE the design. We render the percentage as a large
 * mono digit with hairline progress underneath, in two formats:
 *
 *   - InstrumentColumn (for 5h / 7d primary): big 56px digits, full-column
 *     progress hairline, reset countdown underneath.
 *   - InstrumentRow (for sub-rows like Opus / Sonnet / Pay-as-you-go): label
 *     left-aligned, smaller mono digit right-aligned, hairline below.
 *
 * Color appears in the meter only — never on the digit itself. That keeps the
 * focal point typographic, with status read at a glance from the meter color.
 */

import { ResetCountdown } from './ResetCountdown';
import type { Utilization } from '../lib/types';

type Level = 'safe' | 'warn' | 'danger' | 'idle';

function levelOf(value: number | null, warn: number, danger: number): Level {
  if (value == null) return 'idle';
  if (value >= danger) return 'danger';
  if (value >= warn) return 'warn';
  return 'safe';
}

const meterColor: Record<Level, string> = {
  idle: 'var(--color-rule-strong)',
  safe: 'var(--color-safe)',
  warn: 'var(--color-warn)',
  danger: 'var(--color-danger)',
};

/* ─── Primary instrument: big stacked column ─── */

export function InstrumentColumn({
  label,
  data,
  warnAt,
  dangerAt,
}: {
  label: string;
  data: Utilization | null;
  warnAt: number;
  dangerAt: number;
}) {
  const value = data?.utilization ?? null;
  const level = levelOf(value, warnAt, dangerAt);
  const clamped = value == null ? 0 : Math.max(0, Math.min(100, value));

  return (
    <div className="flex flex-col gap-[10px]">
      {/* Eyebrow label */}
      <span className="text-[length:var(--text-micro)] font-[var(--weight-medium)] tracking-[var(--tracking-label)] uppercase text-[color:var(--color-text-muted)]">
        {label}
      </span>

      {/* Hero number — the % sits at the digit's CAP-height (top edge), not its
       * baseline, so the digit + unit read as a single integrated cluster. The
       * % is also bumped from 24px to 32px to give it more visual weight. */}
      <div className="flex items-start gap-[3px]">
        {value == null ? (
          <span
            className="text-[length:var(--text-display)] text-[color:var(--color-text-muted)] leading-[var(--leading-display)]"
            style={{ fontFamily: 'var(--font-mono)' }}
          >
            —
          </span>
        ) : (
          <>
            <HeroNumber value={Math.round(value)} />
            <span
              className="font-[var(--weight-medium)] text-[color:var(--color-text-secondary)] mt-[8px]"
              style={{
                fontFamily: 'var(--font-mono)',
                fontSize: '32px',
                lineHeight: 1,
                letterSpacing: '-0.02em',
              }}
            >
              %
            </span>
          </>
        )}
      </div>

      {/* Hairline meter */}
      <Meter value={clamped} level={level} />

      {/* Caption — leading-tight so it occupies a single line predictably,
       * and a min-height so all columns line up when one bucket has no
       * reset_at to show. */}
      <div className="min-h-[16px] leading-[1.4]">
        {data?.resets_at && <ResetCountdown resetsAt={data.resets_at} />}
      </div>
    </div>
  );
}

/* ─── Secondary instrument: inline row ─── */

/**
 * Inline row instrument — used for sub-rows like Opus / Sonnet (which inherit
 * the parent 7d window's reset cycle so don't need their own countdown) and
 * for Pay-as-you-go (which uses the optional `caption` prop for "no reset
 * window" notes). The reset countdown is intentionally *not* shown here —
 * the column-level instrument carries that.
 */
export function InstrumentRow({
  label,
  caption,
  value,
  data,
  warnAt,
  dangerAt,
  active,
}: {
  label: string;
  caption?: string;
  value?: number;
  data?: Utilization | null;
  warnAt: number;
  dangerAt: number;
  /** Marks this row as the currently-in-use model — accent dot + accent label. */
  active?: boolean;
}) {
  const v = value ?? data?.utilization ?? null;
  const level = levelOf(v, warnAt, dangerAt);
  const clamped = v == null ? 0 : Math.max(0, Math.min(100, v));

  return (
    <div className="flex flex-col gap-[6px]">
      <div className="flex items-baseline justify-between gap-[var(--space-sm)] min-w-0">
        <div className="flex items-baseline gap-[var(--space-xs)] min-w-0">
          {active && (
            <span
              aria-label="currently in use"
              title="Currently in use"
              className="inline-block h-[6px] w-[6px] rounded-full shrink-0 self-center"
              style={{
                background: 'var(--color-accent)',
                animation: 'pulse-dot 2.4s ease-in-out infinite',
              }}
            />
          )}
          <span
            className={`text-[length:var(--text-label)] font-[var(--weight-medium)] truncate ${
              active
                ? 'text-[color:var(--color-accent)]'
                : 'text-[color:var(--color-text-secondary)]'
            }`}
          >
            {label}
          </span>
          {caption && (
            <span className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)] truncate">
              {caption}
            </span>
          )}
        </div>
        <span
          className="text-[length:var(--text-pct)] font-[var(--weight-medium)] tabular-nums leading-none shrink-0"
          style={{
            fontFamily: 'var(--font-mono)',
            color: v == null ? 'var(--color-text-muted)' : 'var(--color-text)',
            letterSpacing: '-0.02em',
          }}
        >
          {v == null ? '—' : `${Math.round(v)}%`}
        </span>
      </div>
      <Meter value={clamped} level={level} small />
    </div>
  );
}

/* ─── Hairline meter ─── */

/**
 * Slightly thicker than a section divider so a near-full danger meter doesn't
 * read as another rule line. The hero column meter is 4px, the inline-row
 * meter is 3px — both visibly above the 1px hairline dividers.
 */
function Meter({ value, level, small }: { value: number; level: Level; small?: boolean }) {
  return (
    <div
      className="relative w-full overflow-hidden rounded-full"
      style={{
        height: small ? '3px' : '4px',
        background: 'var(--color-track)',
      }}
    >
      <div
        className="h-full rounded-full transition-[width,background] duration-[var(--duration-bar)] ease-[var(--ease-out)]"
        style={{
          width: `${value}%`,
          background: meterColor[level],
        }}
      />
    </div>
  );
}

/* ─── Hero number with subtle weight on the digit pair ─── */

function HeroNumber({ value }: { value: number }) {
  // Render so leading zero behavior is sensible: 7 → "7", 11 → "11", 100 → "100".
  return (
    <span
      className="font-[var(--weight-medium)] tabular-nums text-[color:var(--color-text)] inline-block"
      style={{
        fontFamily: 'var(--font-mono)',
        fontSize: 'var(--text-hero)',
        lineHeight: 'var(--leading-hero)',
        letterSpacing: 'var(--tracking-hero)',
        fontFeatureSettings: '"tnum", "ss01"',
      }}
    >
      {value}
    </span>
  );
}
