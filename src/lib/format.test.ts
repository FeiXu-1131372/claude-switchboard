import { describe, it, expect } from 'vitest';
import { formatTokens, formatCost, formatCount } from './format';

describe('formatTokens', () => {
  it('returns "0" for zero', () => {
    expect(formatTokens(0)).toBe('0');
  });

  it('returns plain number for values below 1 000', () => {
    expect(formatTokens(999)).toBe('999');
  });

  it('returns 1.0K at exactly 1 000', () => {
    expect(formatTokens(1_000)).toBe('1.0K');
  });

  it('returns 1.5K for 1 500', () => {
    expect(formatTokens(1_500)).toBe('1.5K');
  });

  it('returns 1.0M at exactly 1 000 000', () => {
    expect(formatTokens(1_000_000)).toBe('1.0M');
  });

  it('returns 2.5M for 2 500 000', () => {
    expect(formatTokens(2_500_000)).toBe('2.5M');
  });

  it('switches to billions at 1e9', () => {
    expect(formatTokens(35_863_760_037)).toBe('35.9B');
    expect(formatTokens(1_000_000_000)).toBe('1.0B');
  });

  it('stays in millions just below 1e9', () => {
    expect(formatTokens(999_999_999)).toBe('1000.0M');
  });
});

describe('formatCost', () => {
  it('returns "<$0.01" for zero', () => {
    expect(formatCost(0)).toBe('<$0.01');
  });

  it('returns "<$0.01" for values below 0.01', () => {
    expect(formatCost(0.005)).toBe('<$0.01');
  });

  it('returns "$0.01" for exactly 0.01', () => {
    expect(formatCost(0.01)).toBe('$0.01');
  });

  it('returns "$1.50" for 1.5', () => {
    expect(formatCost(1.5)).toBe('$1.50');
  });

  it('returns "$100.00" for 100', () => {
    expect(formatCost(100)).toBe('$100.00');
  });

  it('groups thousands and keeps cents below $10,000', () => {
    expect(formatCost(6908.46)).toBe('$6,908.46');
    expect(formatCost(9999.99)).toBe('$9,999.99');
  });

  it('rounds to whole dollars from $10,000', () => {
    expect(formatCost(13273.21)).toBe('$13,273');
    expect(formatCost(129109.54)).toBe('$129,110');
  });

  it('does not show $10,000.00 for a value that rounds up to it', () => {
    expect(formatCost(9999.996)).toBe('$10,000');
  });
});

describe('formatCount', () => {
  it('groups thousands', () => {
    expect(formatCount(25922)).toBe('25,922');
  });

  it('leaves small numbers alone', () => {
    expect(formatCount(59)).toBe('59');
  });
});
