import { describe, expect, it } from 'vitest';
import type { ModelStats } from '../lib/types';
import { canonicalModelId, mergeModels, modelRowLabel, splitOther, withShares } from './breakdown';

const m = (model: string, cost_usd: number, tokens = 0): ModelStats => ({
  model,
  input_tokens: tokens,
  output_tokens: 0,
  cache_read_tokens: 0,
  cache_creation_tokens: 0,
  cost_usd,
});

describe('canonicalModelId', () => {
  it('gives every spelling of one model the same id', () => {
    expect(canonicalModelId('anthropic/claude-fable-5.1')).toBe('claude-fable-5-1');
    expect(canonicalModelId('Claude-Fable-5-1')).toBe('claude-fable-5-1');
    expect(canonicalModelId('claude-haiku-4-5-20251001')).toBe('claude-haiku-4-5');
    expect(canonicalModelId('claude-opus-5-5[1m]')).toBe('claude-opus-5-5');
    expect(canonicalModelId(' claude-opus-5-5 ')).toBe('claude-opus-5-5');
  });

  it('keeps models that really are different apart', () => {
    expect(canonicalModelId('claude-fable-5-1-reversed')).toBe('claude-fable-5-1-reversed');
    expect(canonicalModelId('claude-fable-5-2')).not.toBe(canonicalModelId('claude-fable-5-1'));
  });

  it('leaves the dots in names that are not Claude models', () => {
    expect(canonicalModelId('glm-5.3')).toBe('glm-5.3');
    expect(canonicalModelId('deepseek-v4.1-flash-1017')).toBe('deepseek-v4.1-flash-1017');
  });
});

describe('modelRowLabel', () => {
  it('reads a Claude model as family and version', () => {
    expect(modelRowLabel('claude-fable-5-1')).toBe('fable 5-1');
    expect(modelRowLabel('claude-opus-4')).toBe('opus 4');
  });

  it('keeps whatever sets a variant apart', () => {
    expect(modelRowLabel('claude-fable-5-1-reversed')).toBe('fable 5-1 (reversed)');
  });

  it('shows any other model under its own name', () => {
    expect(modelRowLabel('deepseek-v4.1-flash-1017')).toBe('deepseek-v4.1-flash-1017');
    expect(modelRowLabel('claude-3-opus')).toBe('claude-3-opus');
  });
});

describe('mergeModels', () => {
  it('merges spellings of one model', () => {
    const out = mergeModels([m('claude-fable-5-1', 520, 100), m('anthropic/claude-fable-5.1', 1, 7)]);
    expect(out).toHaveLength(1);
    expect(out[0].model).toBe('claude-fable-5-1');
    expect(out[0].cost_usd).toBe(521);
    expect(out[0].input_tokens).toBe(107);
  });

  it('adds every field, and leaves its input alone', () => {
    const a = { ...m('claude-opus-5-5', 1), output_tokens: 2, cache_read_tokens: 3, cache_creation_tokens: 4 };
    const b = { ...m('claude-opus-5-5[1m]', 1), output_tokens: 20, cache_read_tokens: 30, cache_creation_tokens: 40 };
    const [out] = mergeModels([a, b]);
    expect(out).toMatchObject({ output_tokens: 22, cache_read_tokens: 33, cache_creation_tokens: 44, cost_usd: 2 });
    expect(a.output_tokens).toBe(2);
  });
});

describe('withShares', () => {
  it('shares follow the chosen measure', () => {
    const rows = [m('opus', 9912, 39), m('haiku', 45, 28)];
    expect(withShares(rows, 'cost')[0].pct).toBeCloseTo(99.55, 1);
    expect(withShares(rows, 'tokens')[0].pct).toBeCloseTo(58.2, 1);
  });

  it('puts the largest share first', () => {
    const rows = [m('small', 1, 900), m('big', 9, 100)];
    expect(withShares(rows, 'cost').map((r) => r.row.model)).toEqual(['big', 'small']);
    expect(withShares(rows, 'tokens').map((r) => r.row.model)).toEqual(['small', 'big']);
  });

  it('shares are 0, not NaN, when nothing was used', () => {
    expect(withShares([m('opus', 0)], 'cost')[0].pct).toBe(0);
    expect(withShares([], 'cost')).toEqual([]);
  });
});

describe('splitOther', () => {
  const rowsOf = (pcts: number[]) => pcts.map((pct, i) => ({ row: i, pct }));

  it('folds the long tail into Other but keeps at least six rows', () => {
    const { main, other } = splitOther(rowsOf([60, 30, 5, 2, 1, 0.9, 0.6, 0.3, 0.2]));
    expect(main).toHaveLength(6);
    expect(other).toHaveLength(3);
  });

  it('keeps every row that is at least one percent', () => {
    const { main, other } = splitOther(rowsOf([20, 15, 15, 12, 10, 10, 9, 8, 0.6, 0.4]));
    expect(main).toHaveLength(8);
    expect(other).toHaveLength(2);
  });

  it('does not fold a single row away', () => {
    const { main, other } = splitOther(rowsOf([50, 20, 10, 8, 6, 5.5, 0.5]));
    expect(main).toHaveLength(7);
    expect(other).toEqual([]);
  });

  it('has no Other for a short list', () => {
    expect(splitOther(rowsOf([99.5, 0.5])).other).toEqual([]);
  });
});
