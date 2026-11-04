// Re-export all generated types. This file is the public surface for the rest
// of the frontend; the authoritative definitions live in generated/bindings.ts.
export type {
  AuthSource,
  BurnRateProjection,
  CacheStats,
  CachedUsage,
  DailyBucket,
  DailyModelBucket,
  ExtraUsage,
  ModelStats,
  PricingEntry,
  PricingTier,
  ProjectStats,
  Settings,
  UsageSnapshot,
  Utilization,
} from './generated/bindings';

// StoredSessionEvent is the Rust name. Re-export as SessionEvent for
// backwards-compat with existing imports throughout the frontend.
export type { StoredSessionEvent as SessionEvent } from './generated/bindings';
export type { StoredCompaction as Compaction } from './generated/bindings';

/**
 * Rows the backend adds for Claude Code API calls that never reach the
 * transcript (see `jsonl_parser::hidden_calls`): `adj:` lifts a closed
 * session to Claude Code's own /cost total, `est:` estimates a live one.
 * They carry real cost and tokens but are not turns.
 */
export function isUnloggedCallRow(e: { event_id: string }): boolean {
  return e.event_id.startsWith('adj:') || e.event_id.startsWith('est:');
}

// Frontend-only types — no Rust equivalent, not in generated bindings.
export interface HeatmapCell {
  date: string;
  value: number;
  level: 0 | 1 | 2 | 3 | 4;
}
