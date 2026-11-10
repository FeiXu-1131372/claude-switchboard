import { act, render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { SessionSummary, StoredSessionEvent } from '../lib/generated/bindings';
import { useAppStore } from '../lib/store';

const ipcMock = vi.hoisted(() => ({
  getSessionHistory: vi.fn(),
  getCompactions: vi.fn(),
  getPricing: vi.fn(),
  listResumableSessions: vi.fn(),
  forceRefresh: vi.fn(),
}));
vi.mock('../lib/ipc', () => ({ ipc: ipcMock }));

import { SessionsTab } from './SessionsTab';

function ev(over: Partial<StoredSessionEvent> = {}): StoredSessionEvent {
  return {
    ts: '2026-11-10T10:00:00Z',
    project: 'alpha',
    model: 'claude-sonnet-5',
    input_tokens: 100,
    output_tokens: 50,
    cache_read_tokens: 0,
    cache_creation_5m_tokens: 0,
    cache_creation_1h_tokens: 0,
    cost_usd: 1.23,
    source_file: 'proj-alpha/abc123.jsonl',
    source_line: 1,
    event_id: 'evt-1',
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
    ipcMock.getSessionHistory.mockResolvedValue([ev()]);
    ipcMock.getCompactions.mockResolvedValue([]);
    ipcMock.getPricing.mockResolvedValue([]);
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
    ipcMock.getSessionHistory.mockResolvedValue([ev({ source_file: 'proj-alpha\\abc123.jsonl' })]);
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
    await waitFor(() => expect(ipcMock.getSessionHistory).toHaveBeenCalledTimes(2));
    expect(ipcMock.listResumableSessions).toHaveBeenCalledTimes(1);
  });
});
