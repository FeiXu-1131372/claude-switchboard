import { describe, expect, it } from 'vitest';
import { toHeatCells } from './calendar';

const day = (date: string, cost: number, tokens = 0) => ({
  date,
  input_tokens: tokens,
  output_tokens: 0,
  cost_usd: cost,
});

// A Sunday, so the last week is complete.
const SUNDAY = new Date('2026-11-08T12:00:00');

describe('toHeatCells', () => {
  it('steps by quartile so one huge day does not flatten the rest', () => {
    const cells = toHeatCells(
      [day('2026-11-03', 10), day('2026-11-04', 20), day('2026-11-05', 30), day('2026-11-06', 40), day('2026-11-07', 1000)],
      'cost',
      SUNDAY,
    );
    expect(cells.filter((c) => c.value > 0).map((c) => c.level)).toEqual([1, 1, 2, 3, 4]);
  });

  it('gives a day with no use level 0', () => {
    const cells = toHeatCells([day('2026-11-03', 10)], 'cost', SUNDAY);
    const idle = cells.filter((c) => c.date !== '2026-11-03');
    expect(idle.length).toBe(cells.length - 1);
    expect(idle.every((c) => c.level === 0 && c.value === 0)).toBe(true);
  });

  it('starts on a Monday and ends today', () => {
    const cells = toHeatCells([], 'cost', SUNDAY);
    expect(new Date(`${cells[0].date}T00:00:00`).getDay()).toBe(1);
    expect(cells[cells.length - 1].date).toBe('2026-11-08');
    expect(cells).toHaveLength(26 * 7);

    // A Wednesday: four days of this week are still to come.
    const midweek = toHeatCells([], 'cost', new Date('2026-11-11T09:00:00'));
    expect(new Date(`${midweek[0].date}T00:00:00`).getDay()).toBe(1);
    expect(midweek[midweek.length - 1].date).toBe('2026-11-11');
    expect(midweek).toHaveLength(26 * 7 - 4);
  });

  it('has each date once, in order, across a daylight-saving change', () => {
    const cells = toHeatCells([], 'cost', new Date('2026-11-11T09:00:00'));
    const dates = cells.map((c) => c.date);
    expect(new Set(dates).size).toBe(dates.length);
    expect(dates).toEqual([...dates].sort());
    expect(dates).toContain('2026-09-26');
    expect(dates).toContain('2026-09-27');
  });

  it('uses tokens when the measure is tokens', () => {
    const cells = toHeatCells(
      [{ date: '2026-11-07', input_tokens: 700, output_tokens: 300, cost_usd: 5 }],
      'tokens',
      SUNDAY,
    );
    expect(cells.find((c) => c.date === '2026-11-07')?.value).toBe(1000);
  });

  it('ranks by the chosen measure', () => {
    // The dearest day used the fewest tokens, and the cheapest the most.
    const daily = [
      day('2026-11-03', 50, 10),
      day('2026-11-04', 40, 200),
      day('2026-11-05', 30, 300),
      day('2026-11-06', 20, 400),
      day('2026-11-07', 10, 9000),
    ];
    const level = (metric: 'cost' | 'tokens', date: string) =>
      toHeatCells(daily, metric, SUNDAY).find((c) => c.date === date)?.level;
    expect(level('cost', '2026-11-03')).toBe(4);
    expect(level('tokens', '2026-11-03')).toBe(1);
    expect(level('cost', '2026-11-07')).toBe(1);
    expect(level('tokens', '2026-11-07')).toBe(4);
  });

  it('puts a lone active day on the first step', () => {
    const cells = toHeatCells([day('2026-11-07', 3)], 'cost', SUNDAY);
    expect(cells.find((c) => c.date === '2026-11-07')?.level).toBe(1);
  });

  it('handles no history', () => {
    expect(toHeatCells([], 'cost', SUNDAY).every((c) => c.level === 0)).toBe(true);
  });

  it('can cover a different number of weeks', () => {
    expect(toHeatCells([], 'cost', SUNDAY, 4)).toHaveLength(28);
  });
});
