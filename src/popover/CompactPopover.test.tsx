import { fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';

const state = vi.hoisted(() => ({
  usage: null as unknown,
  settings: null,
  stale: false,
  accounts: [{ slot: 1, email: 'Token account 1', is_active: false }],
  shownTick: 0,
  pendingSwapReport: null,
  refreshUsage: vi.fn().mockResolvedValue(undefined),
  dismissBanner: vi.fn(),
  toggleViewMode: vi.fn(),
  consumeSwapReport: vi.fn(),
}));
vi.mock('../lib/store', () => ({ useAppStore: (select: (value: typeof state) => unknown) => select(state) }));
vi.mock('../lib/ipc', () => ({ ipc: {
  resizeWindow: vi.fn().mockResolvedValue(undefined),
  forceRefresh: vi.fn().mockResolvedValue(undefined),
} }));
vi.mock('../lib/useActiveModel', () => ({ useActiveModel: () => null }));
vi.mock('../accounts/AccountsPanel', () => ({ AccountsPanel: () => <div>Manage imported accounts</div> }));
vi.mock('../settings/SettingsPanel', () => ({ SettingsPanel: () => null }));
vi.mock('../lib/window-chrome', () => ({ handleDragStart: vi.fn(), closeWindow: vi.fn() }));

import { CompactPopover } from './CompactPopover';
import { ipc } from '../lib/ipc';

/** A reading with the Opus and Sonnet split, or (Pro) with none. */
function reading(split: boolean) {
  return {
    snapshot: {
      five_hour: { utilization: 19, resets_at: null },
      seven_day: { utilization: 32, resets_at: null },
      seven_day_sonnet: split ? { utilization: 20, resets_at: null } : null,
      seven_day_opus: split ? { utilization: 40, resets_at: null } : null,
      seven_day_fable: { utilization: 44, resets_at: null },
      extra_usage: null,
      fetched_at: new Date().toISOString(),
    },
    account_id: 'uuid-1',
    account_email: 'a@example.com',
    last_error: null,
    burn_rate: null,
    auth_source: 'OAuth',
  };
}

afterEach(() => {
  state.usage = null;
  vi.mocked(ipc.resizeWindow).mockClear();
});

it('lets a first token-only account be selected before any usage exists', () => {
  render(<CompactPopover />);
  fireEvent.click(screen.getByRole('button', { name: 'Choose an account' }));
  expect(screen.getByText('Manage imported accounts')).toBeInTheDocument();
});

it('fills the window it is given, capped at the design size', () => {
  state.usage = reading(false);
  const { container } = render(<CompactPopover />);
  const shell = container.firstElementChild as HTMLElement;
  // A fixed width/height here overflowed the webview when WebView2 applied
  // the system text-size setting as page zoom, clipping the row badges.
  expect(shell.style.width).toBe('');
  expect(shell.style.height).toBe('');
  expect(shell.style.maxWidth).toBe('var(--popover-width)');
  expect(shell.style.maxHeight).toBe('var(--popover-height-minimal)');

  // Settings and accounts use the full compact height, still as a ceiling.
  fireEvent.click(screen.getByRole('button', { name: 'a@example.com' }));
  const fullShell = container.firstElementChild as HTMLElement;
  expect(fullShell.style.maxHeight).toBe('var(--popover-height)');
});

it('opens the taller window for the details and the short one for the glance', () => {
  state.usage = reading(true);
  render(<CompactPopover />);
  expect(vi.mocked(ipc.resizeWindow).mock.lastCall).toEqual(['compact-minimal']);
  fireEvent.click(screen.getByRole('button', { name: 'Details' }));
  expect(vi.mocked(ipc.resizeWindow).mock.lastCall).toEqual(['compact']);
});

it('returns to the short window when the account now shown has no details to open', () => {
  state.usage = reading(true);
  const { rerender } = render(<CompactPopover />);
  fireEvent.click(screen.getByRole('button', { name: 'Details' }));
  expect(vi.mocked(ipc.resizeWindow).mock.lastCall).toEqual(['compact']);

  // Switched to a Pro account: no model split, so nothing behind Details.
  state.usage = reading(false);
  rerender(<CompactPopover />);
  expect(screen.queryByRole('button', { name: 'Details' })).toBeNull();
  expect(vi.mocked(ipc.resizeWindow).mock.lastCall).toEqual(['compact-minimal']);
});
