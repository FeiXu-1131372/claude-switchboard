import { useEffect, useState } from 'react';

function humanize(ms: number): string {
  if (ms <= 0) return 'now';
  const s = Math.floor(ms / 1000);
  const d = Math.floor(s / 86_400);
  const h = Math.floor((s % 86_400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

/**
 * Compact reset caption — sans "in" prefix in the system font + the duration
 * in mono. Splitting the family at the word/number boundary is the design's
 * core typographic rule: numbers are JetBrains Mono, copy is system sans.
 */
export function ResetCountdown({ resetsAt, compact: _compact }: { resetsAt: string; compact?: boolean }) {
  const [now, setNow] = useState(() => Date.now());
  const target = new Date(resetsAt).getTime();
  useEffect(() => {
    const remaining = target - now;
    const interval = remaining > 0 && remaining < 5 * 60 * 1000 ? 10_000 : 30_000;
    const i = setInterval(() => setNow(Date.now()), interval);
    return () => clearInterval(i);
  }, [now, target]);
  return (
    <span className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)] whitespace-nowrap">
      in{' '}
      <span
        className="tabular-nums"
        style={{ fontFamily: 'var(--font-mono)' }}
      >
        {humanize(target - now)}
      </span>
    </span>
  );
}
