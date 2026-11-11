import { render, screen, fireEvent, waitFor, within } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { DailyBucket, DailyModelBucket } from '../lib/types';

const TRENDS: DailyBucket[] = [
  { date: '2026-07-24', input_tokens: 8_000_000, output_tokens: 4_400_000, cost_usd: 4.82 },
  { date: '2026-07-25', input_tokens: 2_000_000, output_tokens: 1_000_000, cost_usd: 1.1 },
];

const BREAKDOWN: DailyModelBucket[] = [
  {
    date: '2026-07-24',
    models: [
      { model: 'claude-sonnet-4-6', input_tokens: 6_000_000, output_tokens: 2_100_000, cache_read_tokens: 6_200_000, cache_creation_tokens: 300_000, cost_usd: 2.94 },
      { model: 'claude-opus-4-7', input_tokens: 1_700_000, output_tokens: 1_700_000, cache_read_tokens: 2_100_000, cache_creation_tokens: 100_000, cost_usd: 1.61 },
      { model: 'claude-haiku-4-5', input_tokens: 300_000, output_tokens: 600_000, cache_read_tokens: 50_000, cache_creation_tokens: 10_000, cost_usd: 0.27 },
    ],
  },
  {
    date: '2026-07-25',
    models: [
      { model: 'claude-sonnet-4-6', input_tokens: 2_000_000, output_tokens: 1_000_000, cache_read_tokens: 500_000, cache_creation_tokens: 20_000, cost_usd: 1.1 },
    ],
  },
];

const ipcMock = vi.hoisted(() => ({
  getDailyTrends: vi.fn(),
  getDailyModelBreakdown: vi.fn(),
}));

vi.mock('../lib/ipc', () => ({ ipc: ipcMock }));

vi.mock('../lib/store', async () => {
  const actual = await vi.importActual<typeof import('../lib/store')>('../lib/store');
  const state = { sessionDataVersion: 0 };
  const useAppStore: any = (sel: any) => sel(state);
  useAppStore.getState = () => state;
  return { ...actual, useAppStore };
});

// The calendar loads its own data; here it only has to be in the right place.
vi.mock('./ActivityCalendar', () => ({ ActivityCalendar: () => <div data-testid="activity-calendar" /> }));

import { TrendsTab, fillDays, xLabel } from './TrendsTab';
import { useReportStore } from './reportState';

beforeEach(() => {
  useReportStore.setState({ range: '30d', metric: 'cost' });
});

/** Tokens and spend disagree on purpose: the cheap day has 100× the tokens. */
const DIVERGENT_TRENDS: DailyBucket[] = [
  { date: '2026-07-24', input_tokens: 1_000_000, output_tokens: 0, cost_usd: 0.1 },
  { date: '2026-07-25', input_tokens: 10_000, output_tokens: 0, cost_usd: 5 },
];

describe('TrendsTab — chart metric', () => {
  beforeEach(() => {
    ipcMock.getDailyTrends.mockResolvedValue(DIVERGENT_TRENDS);
    ipcMock.getDailyModelBreakdown.mockResolvedValue([]);
  });

  it('sizes bars by money spent by default', async () => {
    render(<TrendsTab />);
    const cheap = await screen.findByTestId('day-bar-2026-07-24');
    const pricey = screen.getByTestId('day-bar-2026-07-25');
    expect(parseFloat(pricey.style.height)).toBeCloseTo(100, 0);
    expect(parseFloat(cheap.style.height)).toBeCloseTo(2, 0); // $0.10 / $5.00
  });

  it('switches bar heights to tokens when Tokens is selected', async () => {
    render(<TrendsTab />);
    await screen.findByTestId('day-bar-2026-07-24');

    fireEvent.click(screen.getByRole('button', { name: 'Tokens' }));

    const cheap = screen.getByTestId('day-bar-2026-07-24');
    const pricey = screen.getByTestId('day-bar-2026-07-25');
    expect(parseFloat(cheap.style.height)).toBeCloseTo(100, 0);
    expect(parseFloat(pricey.style.height)).toBeCloseTo(1, 0); // 10k / 1M
  });

  it('leads the bar label with the selected metric', async () => {
    render(<TrendsTab />);
    expect(
      await screen.findByRole('button', { name: 'Jul 24: $0.10, 1.0M tokens' }),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Tokens' }));
    expect(
      screen.getByRole('button', { name: 'Jul 24: 1.0M tokens, $0.10' }),
    ).toBeInTheDocument();
  });
});

const WEEKLY_TRENDS: DailyBucket[] = [
  { date: '2026-07-20', input_tokens: 1_000_000, output_tokens: 0, cost_usd: 1 },
  { date: '2026-07-22', input_tokens: 2_000_000, output_tokens: 0, cost_usd: 2 },
  { date: '2026-07-27', input_tokens: 4_000_000, output_tokens: 0, cost_usd: 4 },
];

const WEEKLY_BREAKDOWN: DailyModelBucket[] = [
  {
    date: '2026-07-20',
    models: [
      { model: 'claude-sonnet-4-6', input_tokens: 1_000_000, output_tokens: 0, cache_read_tokens: 0, cache_creation_tokens: 0, cost_usd: 1 },
    ],
  },
  {
    date: '2026-07-22',
    models: [
      { model: 'claude-sonnet-4-6', input_tokens: 2_000_000, output_tokens: 0, cache_read_tokens: 0, cache_creation_tokens: 0, cost_usd: 2 },
    ],
  },
];

describe('TrendsTab — ranges and weekly bars', () => {
  beforeEach(() => {
    ipcMock.getDailyTrends.mockResolvedValue(WEEKLY_TRENDS);
    ipcMock.getDailyModelBreakdown.mockResolvedValue(WEEKLY_BREAKDOWN);
  });

  it('offers ranges longer than 30 days and refetches for them', async () => {
    render(<TrendsTab />);
    await screen.findByTestId('day-bar-2026-07-20');
    expect(ipcMock.getDailyTrends).toHaveBeenCalledWith(30);

    fireEvent.click(screen.getByRole('button', { name: '1y' }));
    await waitFor(() => expect(ipcMock.getDailyTrends).toHaveBeenCalledWith(365));
    expect(ipcMock.getDailyModelBreakdown).toHaveBeenCalledWith(365);
  });

  it('defaults the 90-day range to weekly bars', async () => {
    render(<TrendsTab />);
    await screen.findByTestId('day-bar-2026-07-20');

    fireEvent.click(screen.getByRole('button', { name: '90d' }));
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Weekly' })).toHaveAttribute('aria-pressed', 'true'),
    );
  });

  it('aggregates daily buckets into one bar per week', async () => {
    render(<TrendsTab />);
    await screen.findByTestId('day-bar-2026-07-20');

    fireEvent.click(screen.getByRole('button', { name: 'Weekly' }));

    const firstWeek = await screen.findByTestId('day-bar-2026-07-20');
    const secondWeek = screen.getByTestId('day-bar-2026-07-27');
    expect(screen.queryByTestId('day-bar-2026-07-22')).toBeNull();
    // $3.00 and $4.00 against the $5 top of the axis.
    expect(parseFloat(firstWeek.style.height)).toBeCloseTo(60, 0);
    expect(parseFloat(secondWeek.style.height)).toBeCloseTo(80, 0);
    // Volume is one colour whatever it adds up to.
    expect(firstWeek.className).toContain('bg-[var(--color-accent)]');
    expect(
      screen.getByRole('button', { name: 'Jul 20 – Jul 26: $3.00, 3.0M tokens' }),
    ).toBeInTheDocument();
  });

  it('rolls a selected week’s model breakdown up across its days', async () => {
    render(<TrendsTab />);
    await screen.findByTestId('day-bar-2026-07-20');
    fireEvent.click(screen.getByRole('button', { name: 'Weekly' }));

    fireEvent.click(await screen.findByRole('button', { name: /Jul 20 – Jul 26/ }));
    const panel = await screen.findByTestId('day-breakdown-panel');
    expect(within(panel).getByText('Week of Jul 20')).toBeInTheDocument();
    expect(within(panel).getByText('sonnet 4-6')).toBeInTheDocument();
    expect(within(panel).getByText('3.0M')).toBeInTheDocument();
    expect(within(panel).getByText('$3.00')).toBeInTheDocument();
  });
});

describe('TrendsTab — day breakdown panel', () => {
  beforeEach(() => {
    ipcMock.getDailyTrends.mockClear();
    ipcMock.getDailyModelBreakdown.mockClear();
    ipcMock.getDailyTrends.mockResolvedValue(TRENDS);
    ipcMock.getDailyModelBreakdown.mockResolvedValue(BREAKDOWN);
  });

  it('reveals the per-model breakdown for a day on click, sorted by cost descending', async () => {
    render(<TrendsTab />);
    const bar = await screen.findByRole('button', { name: /Jul 24/i });
    fireEvent.click(bar);

    const panel = await screen.findByTestId('day-breakdown-panel');
    const badges = within(panel).getAllByText(/^(sonnet|opus|haiku) \d/);
    expect(badges.map((b) => b.textContent)).toEqual(['sonnet 4-6', 'opus 4-7', 'haiku 4-5']);

    const sonnetFill = within(panel).getByTestId('model-fill-claude-sonnet-4-6');
    // The fill follows the Spent metric for that one day: $2.94 / $4.82 =
    // 60.99585%, not the model's share of the whole range.
    expect(parseFloat(sonnetFill.style.width)).toBeCloseTo(61.0, 1);
  });

  it('collapses the panel when the same bar is clicked again', async () => {
    render(<TrendsTab />);
    const bar = await screen.findByRole('button', { name: /Jul 24/i });
    fireEvent.click(bar);
    await screen.findByTestId('day-breakdown-panel');

    fireEvent.click(bar);
    await waitFor(() => expect(screen.queryByTestId('day-breakdown-panel')).not.toBeInTheDocument());
  });

  it('switches the panel to a different day when a different bar is clicked', async () => {
    render(<TrendsTab />);
    const bar24 = await screen.findByRole('button', { name: /Jul 24/i });
    fireEvent.click(bar24);
    let panel = await screen.findByTestId('day-breakdown-panel');
    expect(within(panel).getByText('opus 4-7')).toBeInTheDocument();

    const bar25 = screen.getByRole('button', { name: /Jul 25/i });
    fireEvent.click(bar25);
    await waitFor(() => {
      panel = screen.getByTestId('day-breakdown-panel');
      expect(within(panel).queryByText('opus 4-7')).not.toBeInTheDocument();
    });
    expect(within(panel).getByText('sonnet 4-6')).toBeInTheDocument();

    expect(ipcMock.getDailyTrends).toHaveBeenCalledTimes(1);
    expect(ipcMock.getDailyModelBreakdown).toHaveBeenCalledTimes(1);
  });

  it('clears the selection when the range toggle changes', async () => {
    render(<TrendsTab />);
    const bar = await screen.findByRole('button', { name: /Jul 24/i });
    fireEvent.click(bar);
    await screen.findByTestId('day-breakdown-panel');

    fireEvent.click(screen.getByText('7d'));
    await waitFor(() => expect(screen.queryByTestId('day-breakdown-panel')).not.toBeInTheDocument());
  });
});

describe('TrendsTab — scale, colour and toolbar', () => {
  beforeEach(() => {
    ipcMock.getDailyTrends.mockClear();
    ipcMock.getDailyTrends.mockResolvedValue(TRENDS);
    ipcMock.getDailyModelBreakdown.mockResolvedValue(BREAKDOWN);
  });

  it('does not colour bars by how much was spent', async () => {
    ipcMock.getDailyTrends.mockResolvedValue([
      { date: '2026-07-24', input_tokens: 1, output_tokens: 1, cost_usd: 900 },
    ]);
    render(<TrendsTab />);
    const bar = await screen.findByTestId('day-bar-2026-07-24');
    expect(bar.className).not.toMatch(/color-danger|color-warn/);
    expect(bar.className).toContain('bg-[var(--color-accent)]');
  });

  it('labels the axis with round numbers', async () => {
    ipcMock.getDailyTrends.mockResolvedValue(DIVERGENT_TRENDS); // tallest bar $5
    render(<TrendsTab />);
    const axis = await screen.findByTestId('chart-axis');
    expect(within(axis).getByText('$5.00')).toBeInTheDocument();
    expect(within(axis).getByText('$2.50')).toBeInTheDocument();
    expect(within(axis).getByText('$0')).toBeInTheDocument();
  });

  it('relabels the axis in tokens when Tokens is selected', async () => {
    ipcMock.getDailyTrends.mockResolvedValue(DIVERGENT_TRENDS); // tallest bar 1M tokens
    render(<TrendsTab />);
    fireEvent.click(await screen.findByRole('button', { name: 'Tokens' }));
    const axis = screen.getByTestId('chart-axis');
    expect(within(axis).getByText('1M')).toBeInTheDocument();
    expect(within(axis).getByText('500K')).toBeInTheDocument();
  });

  it('sizes bars against the top tick, not the tallest bar', async () => {
    render(<TrendsTab />); // tallest bar $4.82, top tick $5
    const tallest = await screen.findByTestId('day-bar-2026-07-24');
    expect(parseFloat(tallest.style.height)).toBeCloseTo(96.4, 1);
  });

  it('uses the shared range', async () => {
    useReportStore.setState({ range: '90d' });
    render(<TrendsTab />);
    await waitFor(() => expect(ipcMock.getDailyTrends).toHaveBeenCalledWith(90));
    expect(ipcMock.getDailyTrends).not.toHaveBeenCalledWith(30);
  });

  it('uses the shared measure', async () => {
    useReportStore.setState({ metric: 'tokens' });
    render(<TrendsTab />);
    expect(await screen.findByRole('button', { name: /^Jul 24: 12\.4M tokens/ })).toBeInTheDocument();
  });

  it('puts the total and average in the toolbar', async () => {
    render(<TrendsTab />); // $4.82 + $1.10 over two days
    expect(await screen.findByText('$5.92')).toBeInTheDocument();
    // Two of the thirty days had any use, and the average says it is over those.
    expect(screen.getByText('Last 30 days · 2 active · avg $2.96/day')).toBeInTheDocument();
    expect(screen.queryByText(/total$/)).toBeNull();
  });

  it('keeps a slot for a day when nothing was used', async () => {
    ipcMock.getDailyTrends.mockResolvedValue([
      { date: '2026-07-20', input_tokens: 1, output_tokens: 1, cost_usd: 1 },
      { date: '2026-07-23', input_tokens: 1, output_tokens: 1, cost_usd: 2 },
    ]);
    render(<TrendsTab />);
    await screen.findByTestId('day-bar-2026-07-20');
    // The 21st and 22nd are there, empty, so the 23rd sits three days along.
    const slots = screen.getAllByTestId(/^day-slot-/).map((el) => el.getAttribute('data-testid'));
    expect(slots.slice(0, 4)).toEqual([
      'day-slot-2026-07-20',
      'day-slot-2026-07-21',
      'day-slot-2026-07-22',
      'day-slot-2026-07-23',
    ]);
    expect(screen.queryByTestId('day-bar-2026-07-21')).toBeNull();
    // An empty day is not a button and does not move the average.
    expect(screen.queryByRole('button', { name: /^Jul 21/ })).toBeNull();
    expect(screen.getByText(/\$1\.50\/day/)).toBeInTheDocument();
  });
});

describe('TrendsTab — the time axis', () => {
  const day = (date: string, cost_usd = 0): DailyBucket => ({ date, input_tokens: 0, output_tokens: 0, cost_usd });

  it('covers the whole range up to today, with zeros where nothing was used', () => {
    const filled = fillDays([day('2026-11-09', 2)], 7, new Date(2026, 10, 11, 15, 30));
    expect(filled.map((d) => d.date)).toEqual([
      '2026-11-05', '2026-11-06', '2026-11-07', '2026-11-08', '2026-11-09', '2026-11-10', '2026-11-11',
    ]);
    expect(filled.map((d) => d.cost_usd)).toEqual([0, 0, 0, 0, 2, 0, 0]);
  });

  it('never drops a day that came back, even outside the range it expects', () => {
    const filled = fillDays([day('2026-11-01', 1), day('2026-11-12', 3)], 7, new Date(2026, 10, 11));
    expect(filled[0].date).toBe('2026-11-01');
    expect(filled[filled.length - 1].date).toBe('2026-11-12');
    expect(filled.reduce((s, d) => s + d.cost_usd, 0)).toBe(4);
  });

  it('steps by calendar day across a daylight-saving change', () => {
    const filled = fillDays([day('2026-09-25', 1)], 5, new Date(2026, 8, 29));
    expect(filled.map((d) => d.date)).toEqual([
      '2026-09-25', '2026-09-26', '2026-09-27', '2026-09-28', '2026-09-29',
    ]);
  });

  it('has nothing to draw when nothing came back', () => {
    expect(fillDays([], 30, new Date(2026, 10, 11))).toEqual([]);
  });

  it('names the month on the first label and where the month changes', () => {
    const buckets = ['2026-10-29', '2026-10-30', '2026-10-31', '2026-11-01', '2026-11-02'].map((d) => ({
      key: d, start: d, end: d, input_tokens: 0, output_tokens: 0, cost_usd: 0,
    }));
    expect(buckets.map((b, i) => xLabel(b, i, buckets, '7d'))).toEqual(['Oct 29', '30', '31', 'Nov 1', '2']);
  });
});

describe('TrendsTab — activity calendar', () => {
  it('sits under the chart', async () => {
    ipcMock.getDailyTrends.mockResolvedValue(TRENDS);
    ipcMock.getDailyModelBreakdown.mockResolvedValue(BREAKDOWN);
    render(<TrendsTab />);
    const axis = await screen.findByTestId('chart-axis');
    const calendar = screen.getByTestId('activity-calendar');
    expect(axis.compareDocumentPosition(calendar) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('is still there when the chosen range is empty', async () => {
    ipcMock.getDailyTrends.mockResolvedValue([]);
    ipcMock.getDailyModelBreakdown.mockResolvedValue([]);
    render(<TrendsTab />);
    expect(await screen.findByText('Nothing in this range')).toBeInTheDocument();
    expect(screen.getByTestId('activity-calendar')).toBeInTheDocument();
  });
});
