import { fireEvent, render, screen } from '@testing-library/react';
import { expect, it, vi } from 'vitest';

const state = vi.hoisted(() => ({
  usage: null,
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

it('lets a first token-only account be selected before any usage exists', () => {
  render(<CompactPopover />);
  fireEvent.click(screen.getByRole('button', { name: 'Choose an account' }));
  expect(screen.getByText('Manage imported accounts')).toBeInTheDocument();
});
