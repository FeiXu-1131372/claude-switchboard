// A fixed locale, not the machine's: the same number must read the same on
// every machine, and in every screenshot and test.
const CENTS = new Intl.NumberFormat('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
const WHOLE = new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 });

export function formatTokens(n: number): string {
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(1)}B`;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

/** Dollars with grouped thousands. Cents are kept below $10,000 and dropped
 * from there up, where they are noise next to five figures. */
export function formatCost(usd: number): string {
  // Nothing spent is exactly that; "less than a cent" is for a real amount
  // too small to show.
  if (!(usd > 0)) return '$0.00';
  if (usd < 0.01) return '<$0.01';
  const toCents = Math.round(usd * 100) / 100;
  return toCents < 10_000 ? `$${CENTS.format(toCents)}` : `$${WHOLE.format(usd)}`;
}

/** A whole count with grouped thousands: turns, sessions, rows. */
export function formatCount(n: number): string {
  return WHOLE.format(n);
}

/** "just now" / "3m ago" / "2h ago" for an ISO timestamp; '' when unparsable. */
export function formatRelativeTime(iso: string): string {
  const t = new Date(iso).getTime();
  if (!Number.isFinite(t)) return '';
  const diff = Date.now() - t;
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return 'just now';
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  return `${hours}h ago`;
}
