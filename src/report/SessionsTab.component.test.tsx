import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { SessionCostRow, SessionSummary } from '../lib/generated/bindings';
import { useAppStore } from '../lib/store';

const ipcMock = vi.hoisted(() => ({
  getSessionHistory: vi.fn(),
  getSessionCosts: vi.fn(),
  getCompactions: vi.fn(),
  getPricing: vi.fn(),
  listResumableSessions: vi.fn(),
  forceRefresh: vi.fn(),
}));
vi.mock('../lib/ipc', () => ({ ipc: ipcMock }));

import { SessionsTab } from './SessionsTab';

function row(over: Partial<SessionCostRow> = {}): SessionCostRow {
  return {
    id: 'proj-alpha/abc123.jsonl#2026-11-10',
    project: 'alpha',
    day: '2026-11-10',
    latest_ts: '2026-11-10T10:00:00Z',
    turn_count: 1,
    headline_tokens: 150,
    total_cost_usd: 1.23,
    dominant_model: 'claude-sonnet-5',
    breakdown: { input: 100, output: 50, cache_read: 0, cache_write_5m: 0, cache_write_1h: 0 },
    cost_breakdown: { input: 0.3, output: 0.93, cache_read: 0, cache_write_5m: 0, cache_write_1h: 0 },
    subagents: [],
    ...over,
  };
}

function summary(over: Partial<SessionSummary> = {}): SessionSummary {
  return {
    session_id: 'abc123',
    cwd: '/w/alpha',
    project_name: 'alpha',
    git_branch: 'main',
    title: 'Fix the parser bug',
    recap: null,
    asked: 'fix it',
    left_off: null,
    touched_files: [],
    touched_overflow: 0,
    model: 'claude-sonnet-5',
    peak_context_tokens: 120_000,
    turns: 1,
    started_at: '2026-11-10T10:00:00Z',
    ended_at: '2026-11-10T10:05:00Z',
    total_tokens: 150,
    total_cost_usd: 1.23,
    permission_mode: null,
    cwd_exists: true,
    ...over,
  };
}

describe('SessionsTab (Cost) — session titles', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    ipcMock.getSessionCosts.mockResolvedValue([row()]);
    ipcMock.getCompactions.mockResolvedValue([]);
  });

  it('keeps the open row after the tab is left and reopened, with no loading state', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([summary()]);
    const first = render(<SessionsTab />);
    fireEvent.click(await screen.findByRole('button', { expanded: false }));
    expect(screen.getByRole('button', { expanded: true })).toBeInTheDocument();
    first.unmount();

    render(<SessionsTab />);
    // First render after coming back: the row is there and still open.
    expect(screen.getByRole('button', { expanded: true })).toBeInTheDocument();
  });

  it('new data arriving does not close the open row or show loading', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([summary()]);
    render(<SessionsTab />);
    fireEvent.click(await screen.findByRole('button', { expanded: false }));

    let release!: (rows: SessionCostRow[]) => void;
    ipcMock.getSessionCosts.mockReturnValueOnce(
      new Promise<SessionCostRow[]>((resolve) => {
        release = resolve;
      }),
    );
    await act(async () => {
      useAppStore.setState((s) => ({ sessionDataVersion: s.sessionDataVersion + 1 }));
    });
    // Refetch in flight: the row is still there and still open.
    expect(screen.getByRole('button', { expanded: true })).toBeInTheDocument();

    await act(async () => release([row({ turn_count: 2, total_cost_usd: 2.23 })]));
    expect(screen.getByRole('button', { expanded: true })).toBeInTheDocument();
    expect(screen.getByRole('button', { expanded: true })).toHaveTextContent('2 turns');
  });

  it('labels a cost row with the session title, not the project', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([summary()]);
    render(<SessionsTab />);

    await waitFor(() => expect(screen.getByText('Fix the parser bug')).toBeTruthy());
    const row = screen.getByText('Fix the parser bug').closest('button');
    // The title must be the row's leading text, with the project demoted to
    // the meta line rather than the other way around.
    expect(row?.textContent?.trim().startsWith('Fix the parser bug')).toBe(true);
    expect(row).toHaveTextContent(/alpha · /);
  });

  it('falls back to the project name when the session has no matching summary', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([]);
    render(<SessionsTab />);

    await waitFor(() => expect(screen.getByText('alpha')).toBeTruthy());
    expect(screen.queryByText('Fix the parser bug')).toBeNull();
  });

  it('falls back to the project when the summary title is blank', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([summary({ title: '   ' })]);
    render(<SessionsTab />);

    await waitFor(() => expect(screen.getByText('alpha')).toBeTruthy());
    expect(screen.queryByText('Fix the parser bug')).toBeNull();
  });

  it('keeps project labels when the title fetch fails', async () => {
    ipcMock.listResumableSessions.mockRejectedValue(new Error('scan failed'));
    render(<SessionsTab />);

    await waitFor(() => expect(screen.getByText('alpha')).toBeTruthy());
  });

  it('matches titles for legacy Windows transcript paths', async () => {
    ipcMock.getSessionCosts.mockResolvedValue([row({ id: 'proj-alpha\\abc123.jsonl#2026-11-10' })]);
    ipcMock.listResumableSessions.mockResolvedValue([summary()]);
    render(<SessionsTab />);
    await waitFor(() => expect(screen.getByText('Fix the parser bug')).toBeInTheDocument());
  });

  it('does not re-scan transcripts on session-data ticks', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([summary()]);
    render(<SessionsTab />);
    await waitFor(() => expect(screen.getByText('Fix the parser bug')).toBeTruthy());
    expect(ipcMock.listResumableSessions).toHaveBeenCalledTimes(1);

    await act(async () => {
      useAppStore.setState((s) => ({ sessionDataVersion: s.sessionDataVersion + 1 }));
    });

    // The tab's own history query refreshes; the transcript scan does not.
    await waitFor(() => expect(ipcMock.getSessionCosts).toHaveBeenCalledTimes(2));
    expect(ipcMock.listResumableSessions).toHaveBeenCalledTimes(1);
  });

  it('never asks for raw events', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([]);
    render(<SessionsTab />);
    await screen.findByText(/sessions?$/);
    expect(ipcMock.getSessionHistory).not.toHaveBeenCalled();
    expect(ipcMock.getPricing).not.toHaveBeenCalled();
  });

  it('counts a conversation once however many days it ran', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([]);
    ipcMock.getSessionCosts.mockResolvedValue([
      row({ id: 'proj-alpha/abc123.jsonl#2026-11-10', day: '2026-11-10', total_cost_usd: 2 }),
      row({ id: 'proj-alpha/abc123.jsonl#2026-11-09', day: '2026-11-09', latest_ts: '2026-11-09T10:00:00Z', total_cost_usd: 3 }),
    ]);
    render(<SessionsTab />);
    expect(await screen.findByText('1 session')).toBeInTheDocument();
    expect(screen.getByText('$5.00')).toBeInTheDocument();
  });

  it('lists a row\u2019s subagents and category costs when it is opened', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([]);
    ipcMock.getSessionCosts.mockResolvedValue([
      row({
        subagents: [
          {
            id: 'proj-alpha/abc123/subagents/agent-0a1b2c3d.jsonl',
            label: 'agent 0a1b2c3d',
            latest_ts: '2026-11-10T09:00:00Z',
            turn_count: 1,
            headline_tokens: 40,
            total_cost_usd: 0.2,
            dominant_model: 'claude-haiku-5-5',
          },
        ],
      }),
    ]);
    render(<SessionsTab />);
    fireEvent.click(await screen.findByRole('button', { expanded: false }));
    expect(screen.getByText('agent 0a1b2c3d')).toBeInTheDocument();
    expect(screen.getByText('$0.93')).toBeInTheDocument();
  });
});
