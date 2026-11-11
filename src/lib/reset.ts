/** Two reset times closer than this are one window. */
const SAME_RESET_MS = 60_000;

/**
 * Whether two limits reset together. The usage API reports the weekly and
 * Fable windows a few microseconds apart even when they are one window, so
 * "the same" is "within a minute". Missing or unreadable is never the same.
 */
export function sameReset(a?: string | null, b?: string | null): boolean {
  if (!a || !b) return false;
  const gap = Math.abs(Date.parse(a) - Date.parse(b));
  return Number.isFinite(gap) && gap <= SAME_RESET_MS;
}
