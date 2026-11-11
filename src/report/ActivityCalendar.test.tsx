import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useAppStore } from '../lib/store';
import { useReportStore } from './reportState';

const ipcMock = vi.hoisted(() => ({
  getDailyTrends: vi.fn(),
  getSessionHistory: vi.fn(),
}));
vi.mock('../lib/ipc', () => ({ ipc: ipcMock }));

import { ActivityCalendar } from './ActivityCalendar';

const day = (date: string, cost_usd: number, tokens = 0) => ({
  date,
  input_tokens: tokens,
  output_tokens: 0,
  cost_usd,
});

describe('ActivityCalendar', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Only the clock is faked: promises and timers still run.
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(new Date('2026-11-08T12:00:00'));
    ipcMock.getDailyTrends.mockResolvedValue([day('2026-11-07', 1370.12, 2_500_000)]);
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('asks for daily totals, not raw events', async () => {
    render(<ActivityCalendar />);
    await waitFor(() => expect(ipcMock.getDailyTrends).toHaveBeenCalledWith(182));
    expect(ipcMock.getSessionHistory).not.toHaveBeenCalled();
  });

  it('names the day and its value', async () => {
    render(<ActivityCalendar />);
    expect(await screen.findByLabelText('Nov 7: $1,370.12')).toBeInTheDocument();
    expect(screen.getByLabelText('Nov 6: $0.00')).toBeInTheDocument();
  });

  it('follows the shared measure', async () => {
    useReportStore.setState({ metric: 'tokens' });
    render(<ActivityCalendar />);
    expect(await screen.findByLabelText('Nov 7: 2.5M tokens')).toBeInTheDocument();
  });

  it('says what it covers and what the shades mean', async () => {
    render(<ActivityCalendar />);
    expect(await screen.findByText('Activity')).toBeInTheDocument();
    expect(screen.getByText(/Last 26 weeks/)).toBeInTheDocument();
    expect(screen.getByText('Less')).toBeInTheDocument();
    expect(screen.getByText('More')).toBeInTheDocument();
  });

  it('colours a day by its step, in the accent hue only', async () => {
    render(<ActivityCalendar />);
    const busy = await screen.findByLabelText('Nov 7: $1,370.12');
    const idle = screen.getByLabelText('Nov 6: $0.00');
    expect(busy.style.background).toBe('var(--color-heat-1)');
    expect(idle.style.background).toBe('var(--color-heat-0)');
    expect(document.body.innerHTML).not.toMatch(/color-(warn|danger)/);
  });

  it('shows the day in a popover on hover and on focus', async () => {
    render(<ActivityCalendar />);
    const cell = await screen.findByLabelText('Nov 7: $1,370.12');
    expect(screen.queryByRole('tooltip')).toBeNull();
    fireEvent.mouseEnter(cell);
    expect(screen.getByRole('tooltip')).toHaveTextContent('Nov 7: $1,370.12');
    fireEvent.mouseLeave(cell);
    expect(screen.queryByRole('tooltip')).toBeNull();
    fireEvent.focus(cell);
    expect(screen.getByRole('tooltip')).toHaveTextContent('Nov 7: $1,370.12');
  });

  it('is one tab stop, moved through with the arrow keys', async () => {
    render(<ActivityCalendar />);
    const today = await screen.findByLabelText('Nov 8: $0.00');
    const cells = screen.getAllByRole('img');
    expect(cells.filter((c) => c.tabIndex === 0)).toEqual([today]);

    today.focus();
    fireEvent.keyDown(today, { key: 'ArrowUp' });
    expect(document.activeElement).toBe(screen.getByLabelText('Nov 7: $1,370.12'));
    fireEvent.keyDown(document.activeElement!, { key: 'ArrowLeft' });
    expect(document.activeElement).toBe(screen.getByLabelText('Oct 31: $0.00'));
    fireEvent.keyDown(document.activeElement!, { key: 'ArrowRight' });
    fireEvent.keyDown(document.activeElement!, { key: 'ArrowDown' });
    expect(document.activeElement).toBe(today);
    // Nothing after today to move to.
    fireEvent.keyDown(today, { key: 'ArrowDown' });
    expect(document.activeElement).toBe(today);
    expect(screen.getAllByRole('img').filter((c) => c.tabIndex === 0)).toHaveLength(1);
  });

  it('draws nothing when there is no history at all', async () => {
    ipcMock.getDailyTrends.mockResolvedValue([]);
    const { container } = render(<ActivityCalendar />);
    await waitFor(() => expect(ipcMock.getDailyTrends).toHaveBeenCalled());
    await act(async () => {});
    expect(container).toBeEmptyDOMElement();
  });

  it('says so and offers a retry when it cannot load', async () => {
    ipcMock.getDailyTrends.mockRejectedValueOnce(new Error('db locked'));
    render(<ActivityCalendar />);
    fireEvent.click(await screen.findByRole('button', { name: 'Retry' }));
    expect(await screen.findByLabelText('Nov 7: $1,370.12')).toBeInTheDocument();
  });

  it('refreshes when new usage is ingested', async () => {
    render(<ActivityCalendar />);
    await screen.findByLabelText('Nov 7: $1,370.12');
    ipcMock.getDailyTrends.mockResolvedValue([day('2026-11-07', 1400)]);
    await act(async () => {
      useAppStore.setState((s) => ({ sessionDataVersion: s.sessionDataVersion + 1 }));
    });
    expect(await screen.findByLabelText('Nov 7: $1,400.00')).toBeInTheDocument();
  });
});
