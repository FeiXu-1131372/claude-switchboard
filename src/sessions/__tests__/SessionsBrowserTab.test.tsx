import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { SessionSummary } from '../../lib/generated/bindings';

const ipcMock = vi.hoisted(() => ({
  listResumableSessions: vi.fn(),
  listProviders: vi.fn().mockResolvedValue([]),
  listAvailableTerminals: vi.fn().mockResolvedValue(['ghostty']),
  launchProviderSession: vi.fn().mockResolvedValue('/tmp/a.sh'),
  getSettings: vi.fn().mockResolvedValue({ terminal: null }),
  vscodeTabAvailable: vi.fn().mockResolvedValue(true),
}));
vi.mock('../../lib/ipc', () => ({ ipc: ipcMock }));

import { SessionsBrowserTab } from '../SessionsBrowserTab';

function s(over: Partial<SessionSummary>): SessionSummary {
  return {
    session_id: 'id-1',
    cwd: '/w/alpha',
    project_name: 'alpha',
    git_branch: 'main',
    title: 'Alpha work',
    recap: null,
    asked: 'do alpha',
    left_off: null,
    touched_files: [],
    touched_overflow: 0,
    model: 'claude-opus-5',
    peak_context_tokens: 120_000,
    turns: 3,
    started_at: '2026-07-29T10:00:00Z',
    ended_at: '2026-07-29T11:00:00Z',
    total_tokens: 12_345,
    total_cost_usd: 1.23,
    permission_mode: null,
    cwd_exists: true,
    ...over,
  };
}

describe('SessionsBrowserTab', () => {
  beforeEach(() => vi.clearAllMocks());

  it('keeps the search text, filters and open row after the tab is left and reopened', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([
      s({ session_id: 'a', project_name: 'alpha', title: 'Webhook retries', asked: 'first ask' }),
      s({ session_id: 'b', project_name: 'beta', cwd: '/w/beta', title: 'Beta work' }),
    ]);
    const first = render(<SessionsBrowserTab />);
    await waitFor(() => expect(screen.getByText('Beta work')).toBeTruthy());
    fireEvent.change(screen.getByLabelText('Search sessions'), { target: { value: 'webhook' } });
    fireEvent.change(screen.getByLabelText('Sort sessions by'), { target: { value: 'cost' } });
    fireEvent.click(screen.getByText('Webhook retries'));
    await waitFor(() => expect(screen.getByText('first ask')).toBeTruthy());
    first.unmount();

    render(<SessionsBrowserTab />);
    expect(screen.getByLabelText('Search sessions')).toHaveValue('webhook');
    // The list is there on the first render: no loading state in between.
    expect(screen.getByText('first ask')).toBeTruthy();
    expect(screen.queryByText('Beta work')).toBeNull();
    expect(screen.getByLabelText('Sort sessions by')).toHaveValue('cost');
  });

  it('groups sessions under their project', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([
      s({ session_id: 'a', project_name: 'alpha', title: 'Alpha work' }),
      s({ session_id: 'b', project_name: 'beta', cwd: '/w/beta', title: 'Beta work' }),
    ]);
    render(<SessionsBrowserTab />);
    await waitFor(() => expect(screen.getByText('Alpha work')).toBeTruthy());
    expect(screen.getByRole('heading', { name: 'alpha' })).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'beta' })).toBeTruthy();
  });

  it('filters on search and flattens the grouping', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([
      s({ session_id: 'a', project_name: 'alpha', title: 'Alpha work' }),
      s({ session_id: 'b', project_name: 'beta', cwd: '/w/beta', title: 'Beta work' }),
    ]);
    render(<SessionsBrowserTab />);
    await waitFor(() => expect(screen.getByText('Alpha work')).toBeTruthy());
    fireEvent.change(screen.getByLabelText(/search/i), { target: { value: 'beta' } });
    await waitFor(() => expect(screen.queryByText('Alpha work')).toBeNull());
    expect(screen.getByText('Beta work')).toBeTruthy();
    expect(screen.queryByRole('heading', { name: 'beta' })).toBeNull();
  });

  it('clears search when the clear button is clicked', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([
      s({ session_id: 'a', project_name: 'alpha', title: 'Alpha work' }),
      s({ session_id: 'b', project_name: 'beta', cwd: '/w/beta', title: 'Beta work' }),
    ]);
    render(<SessionsBrowserTab />);
    await waitFor(() => expect(screen.getByText('Alpha work')).toBeTruthy());

    fireEvent.change(screen.getByLabelText(/search/i), { target: { value: 'beta' } });
    await waitFor(() => expect(screen.queryByText('Alpha work')).toBeNull());

    fireEvent.click(screen.getByRole('button', { name: 'Clear' }));
    await waitFor(() => expect(screen.getByText('Alpha work')).toBeTruthy());
    expect(screen.getByText('Beta work')).toBeTruthy();
  });

  it('searches left_off and touched files, not just the title', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([
      s({ session_id: 'a', title: 'Opaque title', left_off: 'the migration broke' }),
      s({
        session_id: 'b',
        title: 'Other',
        touched_files: ['walker.rs'],
        cwd: '/w/beta',
        project_name: 'beta',
      }),
    ]);
    render(<SessionsBrowserTab />);
    await waitFor(() => expect(screen.getByText('Opaque title')).toBeTruthy());

    fireEvent.change(screen.getByLabelText(/search/i), { target: { value: 'migration' } });
    await waitFor(() => expect(screen.getByText('Opaque title')).toBeTruthy());

    fireEvent.change(screen.getByLabelText(/search/i), { target: { value: 'walker' } });
    await waitFor(() => expect(screen.getByText('Other')).toBeTruthy());
  });

  it('expands only one row at a time', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([
      s({ session_id: 'a', title: 'First', asked: 'first ask' }),
      s({ session_id: 'b', title: 'Second', asked: 'second ask' }),
    ]);
    render(<SessionsBrowserTab />);
    await waitFor(() => expect(screen.getByText('First')).toBeTruthy());
    fireEvent.click(screen.getByText('First'));
    await waitFor(() => expect(screen.getByText('first ask')).toBeTruthy());
    fireEvent.click(screen.getByText('Second'));
    await waitFor(() => expect(screen.getByText('second ask')).toBeTruthy());
    expect(screen.queryByText('first ask')).toBeNull();
  });

  it('distinguishes empty-corpus from no-match', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([]);
    const { rerender } = render(<SessionsBrowserTab />);
    await waitFor(() => expect(screen.getByText(/no sessions yet/i)).toBeTruthy());

    ipcMock.listResumableSessions.mockResolvedValue([s({})]);
    rerender(<SessionsBrowserTab key="2" />);
    await waitFor(() => expect(screen.getByText('Alpha work')).toBeTruthy());
    fireEvent.change(screen.getByLabelText(/search/i), { target: { value: 'zzzz' } });
    await waitFor(() => expect(screen.getByText(/no sessions match/i)).toBeTruthy());
  });

  it('sorts sessions by highest cost and most turns', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([
      s({ session_id: 'a', title: 'Low Cost', total_cost_usd: 0.5, turns: 20 }),
      s({ session_id: 'b', title: 'High Cost', total_cost_usd: 15.0, turns: 2 }),
      s({ session_id: 'c', title: 'Mid Cost', total_cost_usd: 5.0, turns: 10 }),
    ]);
    render(<SessionsBrowserTab />);
    await waitFor(() => expect(screen.getByText('Low Cost')).toBeTruthy());

    // Sort by highest cost
    fireEvent.change(screen.getByLabelText(/sort sessions/i), { target: { value: 'cost' } });

    // Untoggle group by project to easily inspect flat row order
    fireEvent.click(screen.getByRole('button', { name: /group by project/i }));

    const rowsCost = screen.getAllByRole('button', { name: /cost/i });
    expect(rowsCost[0]).toHaveTextContent('High Cost');
    expect(rowsCost[1]).toHaveTextContent('Mid Cost');
    expect(rowsCost[2]).toHaveTextContent('Low Cost');

    // Sort by most turns
    fireEvent.change(screen.getByLabelText(/sort sessions/i), { target: { value: 'turns' } });
    const rowsTurns = screen.getAllByRole('button', { name: /cost/i });
    expect(rowsTurns[0]).toHaveTextContent('Low Cost');
    expect(rowsTurns[1]).toHaveTextContent('Mid Cost');
    expect(rowsTurns[2]).toHaveTextContent('High Cost');
  });

  it('sorts sessions alphabetically by title', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([
      s({ session_id: '1', title: 'Zebra feature' }),
      s({ session_id: '2', title: 'Apple bug' }),
      s({ session_id: '3', title: 'Mango chore' }),
    ]);
    render(<SessionsBrowserTab />);
    await waitFor(() => expect(screen.getByText('Zebra feature')).toBeTruthy());

    // Switch to flat view and sort by title
    fireEvent.click(screen.getByRole('button', { name: /group by project/i }));
    fireEvent.change(screen.getByLabelText(/sort sessions/i), { target: { value: 'title' } });

    const titles = screen.getAllByText(/feature|bug|chore/).map((el) => el.textContent);
    expect(titles).toEqual(['Apple bug', 'Mango chore', 'Zebra feature']);
  });

  it('filters by project dropdown', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([
      s({ session_id: 'a', project_name: 'frontend', title: 'Frontend task' }),
      s({ session_id: 'b', project_name: 'backend', cwd: '/w/b', title: 'Backend task' }),
    ]);
    render(<SessionsBrowserTab />);
    await waitFor(() => expect(screen.getByText('Frontend task')).toBeTruthy());

    fireEvent.change(screen.getByLabelText(/filter by project/i), {
      target: { value: 'backend' },
    });
    await waitFor(() => expect(screen.queryByText('Frontend task')).toBeNull());
    expect(screen.getByText('Backend task')).toBeTruthy();
  });

  it('filters by model dropdown', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([
      s({ session_id: 'a', model: 'claude-opus-5', title: 'Opus work' }),
      s({ session_id: 'b', model: 'claude-haiku-4', title: 'Haiku work' }),
    ]);
    render(<SessionsBrowserTab />);
    await waitFor(() => expect(screen.getByText('Opus work')).toBeTruthy());

    fireEvent.change(screen.getByLabelText(/filter by model/i), {
      target: { value: 'claude-haiku-4' },
    });
    await waitFor(() => expect(screen.queryByText('Opus work')).toBeNull());
    expect(screen.getByText('Haiku work')).toBeTruthy();
  });

  it('filters by recap only and resumable only pills', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([
      s({ session_id: 'a', title: 'Has recap', recap: 'Goal achieved', cwd_exists: false }),
      s({ session_id: 'b', title: 'No recap resumable', recap: null, cwd_exists: true }),
      s({ session_id: 'c', title: 'Has recap resumable', recap: 'Done', cwd_exists: true }),
    ]);
    render(<SessionsBrowserTab />);
    await waitFor(() => expect(screen.getByText('Has recap')).toBeTruthy());

    // Toggle Recap only
    fireEvent.click(screen.getByRole('button', { name: /recap only/i }));
    await waitFor(() => expect(screen.queryByText('No recap resumable')).toBeNull());
    expect(screen.getByText('Has recap')).toBeTruthy();
    expect(screen.getByText('Has recap resumable')).toBeTruthy();

    // Toggle Resumable only
    fireEvent.click(screen.getByRole('button', { name: /resumable only/i }));
    await waitFor(() => expect(screen.queryByText('Has recap')).toBeNull());
    expect(screen.getByText('Has recap resumable')).toBeTruthy();
  });

  it('resets all filters when clicking Clear filters', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([
      s({ session_id: 'a', project_name: 'alpha', title: 'Alpha work', recap: 'Done' }),
      s({ session_id: 'b', project_name: 'beta', cwd: '/w/beta', title: 'Beta work', recap: null }),
    ]);
    render(<SessionsBrowserTab />);
    await waitFor(() => expect(screen.getByText('Alpha work')).toBeTruthy());

    // Apply multiple filters
    fireEvent.change(screen.getByLabelText(/filter by project/i), { target: { value: 'alpha' } });
    fireEvent.click(screen.getByRole('button', { name: /recap only/i }));
    await waitFor(() => expect(screen.queryByText('Beta work')).toBeNull());

    // Click Clear filters in the toolbar
    const clearBtn = screen.getByRole('button', { name: /clear filters/i });
    fireEvent.click(clearBtn);

    await waitFor(() => expect(screen.getByText('Beta work')).toBeTruthy());
    expect(screen.getByText('Alpha work')).toBeTruthy();
    expect(screen.queryByRole('button', { name: /clear filters/i })).toBeNull();
  });

  it('shows each project group’s summed cost in its header', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([
      s({ session_id: 'a', project_name: 'alpha', title: 'Alpha one', total_cost_usd: 1.23 }),
      s({ session_id: 'b', project_name: 'alpha', title: 'Alpha two', total_cost_usd: 4.56 }),
      s({ session_id: 'c', project_name: 'beta', cwd: '/w/beta', title: 'Beta one', total_cost_usd: 2 }),
    ]);
    render(<SessionsBrowserTab />);
    await waitFor(() => expect(screen.getByText('Alpha one')).toBeTruthy());

    const alphaHeader = screen.getByRole('heading', { name: 'alpha' }).closest('header');
    expect(alphaHeader).toHaveTextContent('$5.79');
    const betaHeader = screen.getByRole('heading', { name: 'beta' }).closest('header');
    expect(betaHeader).toHaveTextContent('$2.00');
  });

  it('omits the group total when the group has no ingested usage', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([
      s({ session_id: 'a', project_name: 'alpha', title: 'Alpha one', total_tokens: 0, total_cost_usd: 0 }),
    ]);
    render(<SessionsBrowserTab />);
    await waitFor(() => expect(screen.getByText('Alpha one')).toBeTruthy());

    const header = screen.getByRole('heading', { name: 'alpha' }).closest('header');
    expect(header).not.toHaveTextContent('$0.00');
  });

  it('sums only the rows whose cost is actually displayed', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([
      s({ session_id: 'a', project_name: 'alpha', title: 'Visible', total_tokens: 150, total_cost_usd: 1.23 }),
      // No ingested tokens: the row hides its cost, so the header must not
      // count it either.
      s({ session_id: 'b', project_name: 'alpha', title: 'Hidden', total_tokens: 0, total_cost_usd: 5 }),
    ]);
    render(<SessionsBrowserTab />);
    await waitFor(() => expect(screen.getByText('Visible')).toBeTruthy());

    const header = screen.getByRole('heading', { name: 'alpha' }).closest('header');
    expect(header).toHaveTextContent('$1.23');
    expect(header).not.toHaveTextContent('$6.23');
  });

  it('scopes the group total to the active filters', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([
      s({ session_id: 'a', project_name: 'alpha', title: 'Opus work', model: 'claude-opus-5', total_cost_usd: 1.23 }),
      s({ session_id: 'b', project_name: 'alpha', title: 'Haiku work', model: 'claude-haiku-4', total_cost_usd: 4.56 }),
    ]);
    render(<SessionsBrowserTab />);
    await waitFor(() => expect(screen.getByText('Opus work')).toBeTruthy());

    fireEvent.change(screen.getByLabelText(/filter by model/i), {
      target: { value: 'claude-haiku-4' },
    });
    await waitFor(() => expect(screen.queryByText('Opus work')).toBeNull());

    const header = screen.getByRole('heading', { name: 'alpha' }).closest('header');
    expect(header).toHaveTextContent('$4.56');
    expect(header).not.toHaveTextContent('$5.79');
  });

  it('shows empty filter state with reset action when no sessions match', async () => {
    ipcMock.listResumableSessions.mockResolvedValue([
      s({ session_id: 'a', title: 'Normal task', recap: null }),
    ]);
    render(<SessionsBrowserTab />);
    await waitFor(() => expect(screen.getByText('Normal task')).toBeTruthy());

    // Filter by recap only when none has recap
    fireEvent.click(screen.getByRole('button', { name: /recap only/i }));
    await waitFor(() => expect(screen.getByText(/no sessions match the selected filters/i)).toBeTruthy());

    // Click the Reset filters button in the empty state
    fireEvent.click(screen.getByRole('button', { name: /reset filters/i }));
    await waitFor(() => expect(screen.getByText('Normal task')).toBeTruthy());
  });
});
