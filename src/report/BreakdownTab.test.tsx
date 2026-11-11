import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { CacheStats, ModelStats, ProjectStats } from '../lib/types';
import { useAppStore } from '../lib/store';
import { useReportStore } from './reportState';

const ipcMock = vi.hoisted(() => ({
  getModelBreakdown: vi.fn(),
  getProjectBreakdown: vi.fn(),
  getCacheStats: vi.fn(),
}));
vi.mock('../lib/ipc', () => ({ ipc: ipcMock }));

import { BreakdownTab } from './BreakdownTab';

const model = (name: string, cost_usd: number, tokens: number): ModelStats => ({
  model: name,
  input_tokens: tokens,
  output_tokens: 0,
  cache_read_tokens: 0,
  cache_creation_tokens: 0,
  cost_usd,
});

/** Opus is nearly all the money and a little over half the tokens. */
const MODELS: ModelStats[] = [model('claude-opus-5-5', 9912, 39_000_000), model('claude-haiku-5-5', 45, 28_000_000)];

const PROJECTS: ProjectStats[] = [
  { project: 'atlas-web', session_count: 1234, total_cost_usd: 6000 },
  { project: 'harbor-api', session_count: 12, total_cost_usd: 3000 },
  { project: '-', session_count: 40, total_cost_usd: 957 },
];

const CACHE: CacheStats = {
  total_cache_read_tokens: 2_400_000_000,
  total_cache_creation_tokens: 310_000_000,
  estimated_savings_usd: 18_250.4,
  hit_ratio: 0.886,
};

const threeHundredProjects: ProjectStats[] = Array.from({ length: 300 }, (_, i) => ({
  project: i % 50 === 7 ? `atlas-${i}` : `project-${String(i).padStart(3, '0')}`,
  session_count: 300 - i,
  total_cost_usd: 3000 - i * 10,
}));

const rowsIn = (name: string, testId: string) =>
  within(screen.getByRole('region', { name })).queryAllByTestId(testId);

describe('BreakdownTab', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useReportStore.setState({ range: '30d', metric: 'cost' });
    ipcMock.getModelBreakdown.mockResolvedValue(MODELS);
    ipcMock.getProjectBreakdown.mockResolvedValue(PROJECTS);
    ipcMock.getCacheStats.mockResolvedValue(CACHE);
  });

  it('loads all three sections for the shared range', async () => {
    useReportStore.setState({ range: '90d' });
    render(<BreakdownTab />);
    await waitFor(() => {
      expect(ipcMock.getModelBreakdown).toHaveBeenCalledWith(90);
      expect(ipcMock.getProjectBreakdown).toHaveBeenCalledWith(90);
      expect(ipcMock.getCacheStats).toHaveBeenCalledWith(90);
    });
  });

  it('heads the tab with the total and what it covers', async () => {
    render(<BreakdownTab />);
    expect(await screen.findByText('$9,957.00')).toBeInTheDocument();
    expect(screen.getByText('Last 30 days · 2 models · 3 projects')).toBeInTheDocument();
  });

  it('gives each model one row with its share, tokens and spend', async () => {
    render(<BreakdownTab />);
    await screen.findByText('By model');
    const [opus, haiku] = rowsIn('By model', 'model-row');
    expect(opus).toHaveTextContent('opus 5-5');
    expect(opus).toHaveTextContent('99.5%');
    expect(opus).toHaveTextContent('39.0M');
    expect(opus).toHaveTextContent('$9,912.00');
    expect(haiku).toHaveTextContent('haiku 5-5');
    expect(haiku).toHaveTextContent('0.5%');
  });

  it('merges spellings of one model into a single row', async () => {
    ipcMock.getModelBreakdown.mockResolvedValue([
      model('claude-fable-5-1', 520, 10),
      model('anthropic/claude-fable-5.1', 1, 10),
      model('claude-fable-5-1-reversed', 4, 10),
    ]);
    render(<BreakdownTab />);
    await screen.findByText('By model');
    expect(rowsIn('By model', 'model-row').map((r) => r.querySelector('[data-testid=model-name]')?.textContent)).toEqual([
      'fable 5-1',
      'fable 5-1 (reversed)',
    ]);
    expect(screen.getByText('Last 30 days · 2 models · 3 projects')).toBeInTheDocument();
  });

  it('the model bars follow the measure switch', async () => {
    render(<BreakdownTab />);
    await screen.findByText('By model');
    expect(rowsIn('By model', 'model-row')[0]).toHaveTextContent('99.5%');

    fireEvent.click(screen.getByRole('button', { name: 'Tokens' }));
    const [first] = rowsIn('By model', 'model-row');
    expect(first).toHaveTextContent('opus 5-5');
    expect(first).toHaveTextContent('58.2%');
    expect(within(first).getByTestId('share-bar-fill').style.width).toMatch(/^58\.2/);
    // The headline follows the measure too.
    expect(screen.getByText('67.0M tokens')).toBeInTheDocument();
  });

  it('lists folded models under Other and opens them', async () => {
    ipcMock.getModelBreakdown.mockResolvedValue(
      [6000, 3000, 500, 200, 100, 90, 60, 30, 20].map((cost, i) => model(`relay-model-${i}`, cost, 1000)),
    );
    render(<BreakdownTab />);
    await screen.findByText('By model');
    expect(rowsIn('By model', 'model-row')).toHaveLength(6);

    const other = screen.getByRole('button', { name: /^Other \(3\)/ });
    expect(other).toHaveAttribute('aria-expanded', 'false');
    expect(other).toHaveTextContent('$110.00');
    fireEvent.click(other);
    expect(other).toHaveAttribute('aria-expanded', 'true');
    expect(rowsIn('By model', 'model-row')).toHaveLength(9);
    expect(screen.getByText('relay-model-8')).toBeInTheDocument();
  });

  it('shows ten projects, then all on request', async () => {
    ipcMock.getProjectBreakdown.mockResolvedValue(threeHundredProjects);
    render(<BreakdownTab />);
    expect(await screen.findAllByTestId('project-row')).toHaveLength(10);
    fireEvent.click(screen.getByRole('button', { name: 'Show all 300' }));
    expect(screen.getAllByTestId('project-row')).toHaveLength(300);
    fireEvent.click(screen.getByRole('button', { name: 'Show top 10' }));
    expect(screen.getAllByTestId('project-row')).toHaveLength(10);
  });

  it('filters projects by name', async () => {
    ipcMock.getProjectBreakdown.mockResolvedValue(threeHundredProjects);
    render(<BreakdownTab />);
    await screen.findAllByTestId('project-row');
    fireEvent.change(screen.getByRole('searchbox', { name: 'Filter projects' }), { target: { value: 'ATL' } });
    // Matches come from all 300, not only the ten on screen.
    const rows = screen.getAllByTestId('project-row');
    expect(rows.map((r) => r.querySelector('[data-testid=project-name]')?.textContent)).toEqual([
      'atlas-7', 'atlas-57', 'atlas-107', 'atlas-157', 'atlas-207', 'atlas-257',
    ]);
    expect(screen.queryByRole('button', { name: /Show all/ })).toBeNull();

    fireEvent.change(screen.getByRole('searchbox', { name: 'Filter projects' }), { target: { value: 'zzz' } });
    expect(screen.queryAllByTestId('project-row')).toHaveLength(0);
    expect(screen.getByText('No project matches “zzz”')).toBeInTheDocument();
  });

  it('offers no filter for a short list', async () => {
    render(<BreakdownTab />);
    await screen.findAllByTestId('project-row');
    expect(screen.queryByRole('searchbox', { name: 'Filter projects' })).toBeNull();
    expect(screen.queryByRole('button', { name: /Show all/ })).toBeNull();
  });

  it('gives each project its name, sessions and spend, sized against the largest', async () => {
    render(<BreakdownTab />);
    const [atlas, harbor] = await screen.findAllByTestId('project-row');
    expect(atlas).toHaveTextContent('atlas-web');
    expect(atlas).toHaveTextContent('1,234');
    expect(atlas).toHaveTextContent('$6,000.00');
    expect(within(atlas).getByTestId('share-bar-fill').style.width).toBe('100%');
    expect(within(harbor).getByTestId('share-bar-fill').style.width).toBe('50%');
    expect(within(atlas).getByTestId('project-name')).toHaveAttribute('title', 'atlas-web');
  });

  it('labels the no-directory bucket headless', async () => {
    render(<BreakdownTab />);
    const rows = await screen.findAllByTestId('project-row');
    expect(within(rows[2]).getByTestId('project-name')).toHaveTextContent('headless');
  });

  it('shows the cache figures in one row', async () => {
    render(<BreakdownTab />);
    const cache = await screen.findByRole('region', { name: 'Cache' });
    expect(within(cache).getByText('88.6%')).toBeInTheDocument();
    expect(within(cache).getByText('2.4B')).toBeInTheDocument();
    expect(within(cache).getByText('310.0M')).toBeInTheDocument();
    expect(within(cache).getByText('$18,250')).toBeInTheDocument();
  });

  it('one section failing does not blank the others', async () => {
    ipcMock.getCacheStats.mockRejectedValue(new Error('boom'));
    render(<BreakdownTab />);
    expect(await screen.findByText('By model')).toBeInTheDocument();
    expect(await screen.findByText("Couldn't load cache stats")).toBeInTheDocument();
    expect(screen.getAllByTestId('project-row')).toHaveLength(3);

    ipcMock.getCacheStats.mockResolvedValue(CACHE);
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(await screen.findByText('88.6%')).toBeInTheDocument();
  });

  it('shows a guiding empty state with no history', async () => {
    ipcMock.getModelBreakdown.mockResolvedValue([]);
    ipcMock.getProjectBreakdown.mockResolvedValue([]);
    ipcMock.getCacheStats.mockResolvedValue({
      total_cache_read_tokens: 0,
      total_cache_creation_tokens: 0,
      estimated_savings_usd: 0,
      hit_ratio: 0,
    });
    render(<BreakdownTab />);
    expect(await screen.findByText('Nothing in this range')).toBeInTheDocument();
    expect(screen.queryByText('By model')).toBeNull();
    expect(document.body.textContent).not.toMatch(/NaN|Infinity/);
    // The range is still there to change.
    expect(screen.getByRole('group', { name: 'Range' })).toBeInTheDocument();
  });

  it('keeps showing what it has while a refresh is on its way', async () => {
    render(<BreakdownTab />);
    await screen.findByText('By model');
    let release!: (rows: ModelStats[]) => void;
    ipcMock.getModelBreakdown.mockReturnValueOnce(new Promise<ModelStats[]>((r) => (release = r)));
    await act(async () => {
      useAppStore.setState((s) => ({ sessionDataVersion: s.sessionDataVersion + 1 }));
    });
    expect(rowsIn('By model', 'model-row')).toHaveLength(2);
    expect(screen.queryByRole('status', { name: 'Loading' })).toBeNull();
    await act(async () => release([model('claude-opus-5-5', 1, 1)]));
    expect(rowsIn('By model', 'model-row')).toHaveLength(1);
  });
});
