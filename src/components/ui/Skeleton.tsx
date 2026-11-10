/** Name-column widths, so the placeholder reads as a list and not a block. */
const WIDTHS = ['62%', '44%', '71%', '38%', '55%', '66%', '49%', '58%'];

/**
 * Placeholder rows shown while a list loads for the first time. They sit
 * where the rows will be, so the content does not jump when it arrives.
 * The pulse is defined in globals.css and is off under reduced motion.
 */
export function SkeletonRows({ rows = 6 }: { rows?: number }) {
  return (
    <div role="status" aria-label="Loading" className="flex flex-col">
      {Array.from({ length: rows }, (_, i) => (
        <div
          key={i}
          data-testid="skeleton-row"
          className="
            flex h-[36px] items-center justify-between gap-[var(--space-lg)]
            border-b border-[var(--color-border-subtle)] px-[var(--space-sm)]
          "
        >
          <span
            className="skeleton h-[10px] rounded-[var(--radius-bar)] bg-[var(--color-track)]"
            style={{ width: WIDTHS[i % WIDTHS.length] }}
          />
          <span className="skeleton h-[10px] w-[56px] shrink-0 rounded-[var(--radius-bar)] bg-[var(--color-track)]" />
        </div>
      ))}
    </div>
  );
}
