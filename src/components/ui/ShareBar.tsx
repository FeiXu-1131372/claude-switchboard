/**
 * A thin bar showing one row's share of a whole. Decorative: the row always
 * prints the number beside it, so the bar is hidden from assistive tech.
 */
export function ShareBar({ pct, color = 'var(--color-accent)' }: { pct: number; color?: string }) {
  const width = Number.isFinite(pct) ? Math.min(100, Math.max(0, pct)) : 0;
  return (
    <div
      aria-hidden
      className="h-[var(--bar-height-md)] w-full overflow-hidden rounded-[var(--radius-bar)] bg-[var(--color-track)]"
    >
      <div
        data-testid="share-bar-fill"
        className="h-full rounded-[var(--radius-bar)] transition-[width] duration-[var(--duration-normal)] ease-[var(--ease-out)]"
        style={{ width: width + '%', background: color }}
      />
    </div>
  );
}
