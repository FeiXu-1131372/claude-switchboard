import { describe, expect, it } from 'vitest';
import { buildFixtures, windowOf } from './fixtures';

const NOW = new Date('2026-11-11T00:00:00Z');

describe('buildFixtures', () => {
  it('is deterministic', () => {
    expect(buildFixtures('heavy', NOW)).toEqual(buildFixtures('heavy', NOW));
  });

  it('heavy matches real volume', () => {
    const d = buildFixtures('heavy', NOW);
    expect(d.projects).toHaveLength(300);
    expect(d.models.length).toBeGreaterThanOrEqual(12);
    expect(d.daily).toHaveLength(136);
    expect(d.accounts.map((a) => a.subscription_type)).toEqual(['ultra', 'ultra', null, 'pro']);
  });

  it('empty has accounts but no history', () => {
    const d = buildFixtures('empty', NOW);
    expect(d.daily).toEqual([]);
    expect(d.sessions).toEqual([]);
    expect(d.accounts).toHaveLength(4);
  });

  it('uses reserved example addresses only', () => {
    const emails = buildFixtures('heavy', NOW)
      .accounts.map((a) => a.email)
      .filter((e) => e.includes('@'));
    expect(emails.every((e) => e.endsWith('@example.com'))).toBe(true);
  });

  it('light is a few projects and a few weeks', () => {
    const d = buildFixtures('light', NOW);
    expect(d.projects.length).toBeGreaterThan(0);
    expect(d.projects.length).toBeLessThanOrEqual(8);
    expect(d.daily.length).toBeLessThanOrEqual(21);
  });
});

describe('windowOf', () => {
  it('every view of one window adds up to the same spend', () => {
    const d = buildFixtures('heavy', NOW);
    for (const days of [7, 30, 90, 365]) {
      const w = windowOf(d, days, NOW);
      const sum = (xs: number[]) => xs.reduce((a, b) => a + b, 0);
      const daily = sum(w.daily.map((b) => b.cost_usd));
      expect(sum(w.models.map((m) => m.cost_usd))).toBeCloseTo(daily, 6);
      expect(sum(w.projects.map((p) => p.total_cost_usd))).toBeCloseTo(daily, 6);
      expect(sum(w.events.map((e) => e.cost_usd))).toBeCloseTo(daily, 6);
    }
  });

  it('a window of N days is N whole days, with today as the last', () => {
    const d = buildFixtures('heavy', NOW);
    const today = [NOW.getFullYear(), NOW.getMonth() + 1, NOW.getDate()]
      .map((n) => String(n).padStart(2, '0'))
      .join('-');
    for (const days of [1, 7, 30, 90]) {
      const w = windowOf(d, days, NOW);
      expect(w.daily).toHaveLength(days);
      expect(w.daily[w.daily.length - 1].date).toBe(today);
    }
  });

  it('thirty days is about thirteen thousand dollars at heavy scale', () => {
    const w = windowOf(buildFixtures('heavy', NOW), 30, NOW);
    const total = w.daily.reduce((a, b) => a + b.cost_usd, 0);
    expect(total).toBeGreaterThan(12_000);
    expect(total).toBeLessThan(14_000);
  });

  it('returns projects most expensive first, as the backend does', () => {
    const w = windowOf(buildFixtures('heavy', NOW), 30, NOW);
    const costs = w.projects.map((p) => p.total_cost_usd);
    expect(costs).toEqual([...costs].sort((a, b) => b - a));
  });
});
