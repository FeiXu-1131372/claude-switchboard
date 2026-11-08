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

import { TrendsTab } from './TrendsTab';

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
    // $3.00 against the $4.00 maximum = 75%
    expect(parseFloat(firstWeek.style.height)).toBeCloseTo(75, 0);
    expect(parseFloat(secondWeek.style.height)).toBeCloseTo(100, 0);
    // $3–$4 weeks are not "heavy" spend — the daily $3 danger threshold must
    // not turn every weekly bar red.
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
    // The fill follows the Spent metric: $2.94 / $4.82 = 60.99585% — NOT the
    // 30-day aggregate ModelsTab uses.
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
