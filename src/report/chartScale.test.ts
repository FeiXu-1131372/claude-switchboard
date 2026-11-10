import { describe, expect, it } from 'vitest';
import { formatAxis, niceTicks } from './chartScale';

describe('niceTicks', () => {
  it('rounds the top of the scale up to a round number', () => {
    expect(niceTicks(1370)).toEqual([0, 750, 1500]);
    expect(niceTicks(4.82)).toEqual([0, 2.5, 5]);
    expect(niceTicks(100)).toEqual([0, 50, 100]);
    expect(niceTicks(101)).toEqual([0, 75, 150]);
    expect(niceTicks(0.13)).toEqual([0, 0.075, 0.15]);
    expect(niceTicks(35_863_760_037)).toEqual([0, 25_000_000_000, 50_000_000_000]);
  });

  it('never gives a zero-height scale', () => {
    expect(niceTicks(0)).toEqual([0, 0.5, 1]);
    expect(niceTicks(-3)).toEqual([0, 0.5, 1]);
    expect(niceTicks(Number.NaN)).toEqual([0, 0.5, 1]);
    expect(niceTicks(Number.POSITIVE_INFINITY)).toEqual([0, 0.5, 1]);
  });

  it('always covers the value it was given', () => {
    for (const max of [0.003, 0.99, 1, 7, 12.5, 999, 1001, 86_400, 1_234_567]) {
      const [zero, mid, top] = niceTicks(max);
      expect(zero).toBe(0);
      expect(top).toBeGreaterThanOrEqual(max);
      expect(mid).toBe(top / 2);
    }
  });
});

describe('formatAxis', () => {
  it('labels money', () => {
    expect(formatAxis(1500, 'cost')).toBe('$1.5K');
    expect(formatAxis(750, 'cost')).toBe('$750');
    expect(formatAxis(2.5, 'cost')).toBe('$2.50');
    expect(formatAxis(5, 'cost')).toBe('$5.00');
    expect(formatAxis(50, 'cost')).toBe('$50');
    expect(formatAxis(12.5, 'cost')).toBe('$12.50');
    expect(formatAxis(25_000, 'cost')).toBe('$25K');
    expect(formatAxis(1_250_000, 'cost')).toBe('$1.25M');
    expect(formatAxis(0, 'cost')).toBe('$0');
  });

  it('labels tokens', () => {
    expect(formatAxis(2_500_000, 'tokens')).toBe('2.5M');
    expect(formatAxis(5_000_000, 'tokens')).toBe('5M');
    expect(formatAxis(1_250, 'tokens')).toBe('1.25K');
    expect(formatAxis(500, 'tokens')).toBe('500');
    expect(formatAxis(25_000_000_000, 'tokens')).toBe('25B');
    expect(formatAxis(0, 'tokens')).toBe('0');
  });
});
