import type { ModelStats } from '../lib/types';
import type { ReportMetric } from './reportState';

/**
 * One id for every spelling of the same model.
 *
 * The same model reaches the log under several names: with a vendor prefix
 * from a relay (`anthropic/claude-fable-5.1`), with dots for dashes, with a
 * `[1m]` context marker, or with a release date on the end. Listed as they
 * come, one model takes three rows and none of them shows its real share.
 * A name that differs in any other way is a different model and stays apart.
 */
export function canonicalModelId(model: string): string {
  let id = model.trim().toLowerCase().replace(/\[1m\]$/, '');
  const slash = id.indexOf('/');
  if (slash >= 0) id = id.slice(slash + 1);
  // Only Claude ids use a dash where others write a dot: `glm-5.3` is a name.
  if (id.startsWith('claude-')) id = id.replace(/\./g, '-');
  return id.replace(/-\d{8}$/, '');
}

/** The rows with every spelling of a model added into one, under its id. */
export function mergeModels(rows: ModelStats[]): ModelStats[] {
  const byId = new Map<string, ModelStats>();
  for (const row of rows) {
    const id = canonicalModelId(row.model);
    const sum = byId.get(id);
    if (!sum) {
      byId.set(id, { ...row, model: id });
      continue;
    }
    sum.input_tokens += row.input_tokens;
    sum.output_tokens += row.output_tokens;
    sum.cache_read_tokens += row.cache_read_tokens;
    sum.cache_creation_tokens += row.cache_creation_tokens;
    sum.cost_usd += row.cost_usd;
  }
  return [...byId.values()];
}

const CLAUDE_ID = /^claude-(fable|mythos|opus|sonnet|haiku)-(\d+(?:-\d+)?)(?:-(.+))?$/;

/** How a model is named on its row: `fable 5-1`, and `fable 5-1 (reversed)`
 *  for a variant, so two rows never read the same. Anything else keeps its
 *  own name. */
export function modelRowLabel(canonicalId: string): string {
  const match = CLAUDE_ID.exec(canonicalId);
  if (!match) return canonicalId;
  const [, family, version, variant] = match;
  return variant ? `${family} ${version} (${variant})` : `${family} ${version}`;
}

export interface ShareRow<T> {
  row: T;
  /** This row's part of the whole, 0 to 100. */
  pct: number;
}

/** What a model row is measured by. Tokens are input plus output, the same
 *  count the Trends chart uses. */
export function modelValue(row: ModelStats, metric: ReportMetric): number {
  return metric === 'cost' ? row.cost_usd : row.input_tokens + row.output_tokens;
}

/** Each row with its share of the total in the chosen measure, largest first. */
export function withShares(rows: ModelStats[], metric: ReportMetric): ShareRow<ModelStats>[] {
  const total = rows.reduce((sum, row) => sum + modelValue(row, metric), 0);
  return rows
    .map((row) => ({ row, pct: total > 0 ? (modelValue(row, metric) / total) * 100 : 0 }))
    .sort((a, b) => b.pct - a.pct || a.row.model.localeCompare(b.row.model));
}

/**
 * Splits rows, already largest first, into the ones worth a line each and a
 * tail to fold into "Other": rows under `minPct`, once `keepAtLeast` are
 * showing. A tail of one is not folded; "Other (1)" would hide a row to save
 * none.
 */
export function splitOther<T>(
  rows: ShareRow<T>[],
  minPct = 1,
  keepAtLeast = 6,
): { main: ShareRow<T>[]; other: ShareRow<T>[] } {
  const worthALine = rows.filter((r) => r.pct >= minPct).length;
  const keep = Math.max(worthALine, keepAtLeast);
  if (rows.length - keep <= 1) return { main: rows, other: [] };
  return { main: rows.slice(0, keep), other: rows.slice(keep) };
}
