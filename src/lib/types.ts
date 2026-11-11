// Re-export all generated types. This file is the public surface for the rest
// of the frontend; the authoritative definitions live in generated/bindings.ts.
export type {
  AuthSource,
  BurnRateProjection,
  CacheStats,
  CachedUsage,
  CostBreakdown,
  DailyBucket,
  DailyModelBucket,
  ExtraUsage,
  ModelStats,
  PricingEntry,
  PricingTier,
  ProjectStats,
  SessionCostRow,
  Settings,
  SubagentCost,
  TokenBreakdown,
  UsageSnapshot,
  Utilization,
} from './generated/bindings';

// StoredSessionEvent is the Rust name. Re-export as SessionEvent for
// backwards-compat with existing imports throughout the frontend.
export type { StoredSessionEvent as SessionEvent } from './generated/bindings';
export type { StoredCompaction as Compaction } from './generated/bindings';

// Frontend-only types — no Rust equivalent, not in generated bindings.
