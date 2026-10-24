// Mirror of `src-tauri/src/jsonl_parser/pricing.rs::cost_for`. Lets us split
// the per-event cost the backend already sums into a per-category breakdown
// for the SessionsTab expanded view, including the 200K-context tier rules
// that apply to Sonnet 4.x.

import type { PricingEntry } from './types';

export interface PerCallTokens {
  input: number;
  output: number;
  cache_read: number;
  cache_5m: number;
  cache_1h: number;
}

export interface PerCategoryCost {
  input: number;
  output: number;
  cache_read: number;
  cache_5m: number;
  cache_1h: number;
}

/** Match the Rust longest-prefix lookup, after stripping org/vendor prefixes and `[1m]`. */
export function lookupPricing(
  entries: PricingEntry[],
  model: string,
): PricingEntry | undefined {
  const without1m = model.trim().toLowerCase().replace(/\[1m\]$/, '');
  const slashIdx = without1m.indexOf('/');
  const withoutOrg = slashIdx >= 0 ? without1m.slice(slashIdx + 1) : without1m;
  const rawNeedle = withoutOrg.startsWith('claude-')
    ? withoutOrg.slice('claude-'.length)
    : withoutOrg;
  const needle = /^(fable|mythos|opus|sonnet|haiku)-/.test(rawNeedle)
    ? rawNeedle.replace(/\./g, '-')
    : rawNeedle;
  // Entries are returned sorted longest-prefix-first by the Rust side, so the
  // first match wins.
  return entries.find((e) => needle.startsWith(e.prefix));
}

/**
 * Compute the per-category cost contribution for a single API call. The
 * Sonnet 4.x 1M-context tier shifts every category's rate up when the
 * call's context size (input + cache_read + cache_5m + cache_1h) exceeds
 * the threshold — it's a per-call switch, not a per-token overage.
 */
export function costPerCategory(
  entry: PricingEntry,
  tokens: PerCallTokens,
): PerCategoryCost {
  const M = 1_000_000;
  const context = tokens.input + tokens.cache_read + tokens.cache_5m + tokens.cache_1h;
  const useTier = entry.tier && context > entry.tier.above_tokens;
  const rates = useTier
    ? {
        input: entry.tier!.input_per_mtok,
        output: entry.tier!.output_per_mtok,
        cache_read: entry.tier!.cache_read_per_mtok,
        cache_5m: entry.tier!.cache_5m_per_mtok,
        cache_1h: entry.tier!.cache_1h_per_mtok,
      }
    : {
        input: entry.input_per_mtok,
        output: entry.output_per_mtok,
        cache_read: entry.cache_read_per_mtok,
        cache_5m: entry.cache_5m_per_mtok,
        cache_1h: entry.cache_1h_per_mtok,
      };
  return {
    input: (tokens.input / M) * rates.input,
    output: (tokens.output / M) * rates.output,
    cache_read: (tokens.cache_read / M) * rates.cache_read,
    cache_5m: (tokens.cache_5m / M) * rates.cache_5m,
    cache_1h: (tokens.cache_1h / M) * rates.cache_1h,
  };
}
