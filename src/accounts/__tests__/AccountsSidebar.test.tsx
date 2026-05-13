import { render, screen } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { useAppStore } from '../../lib/store';

vi.mock('../useAccountManagement', () => ({
  useAccountManagement: () => ({
    accounts: [
      { slot: 1, email: 'a@x.com', is_active: true, account_uuid: 'u1', org_uuid: 'g1', cached_usage: null, last_error: null, subscription_type: 'max' },
      { slot: 2, email: 'b@y.com', is_active: false, account_uuid: 'u2', org_uuid: 'g1', cached_usage: null, last_error: null, subscription_type: 'pro' },
    ],
    orgGroups: new Map(),
    currentActive: { slot: 1, email: 'a@x.com', is_active: true, account_uuid: 'u1' },
    pending: null,
    swappingSlot: null,
    confirmError: null,
    refreshing: false,
    reauthSlot: null,
    chooserOpen: false,
    requestSwap: vi.fn(),
    confirmSwap: vi.fn(),
    cancelSwap: vi.fn(),
    handleReauth: vi.fn(),
    handleRemove: vi.fn(),
    handleRefreshAll: vi.fn(),
    openChooser: vi.fn(),
    closeChooser: vi.fn(),
  }),
}));

vi.mock('../../lib/store', async () => {
  const actual = await vi.importActual<typeof import('../../lib/store')>('../../lib/store');
  const state = {
    settings: { thresholds: [75, 90] },
    modalStack: [],
    pushModal: vi.fn(),
    popModal: vi.fn(),
    isTopmost: () => true,
    resetModalStack: vi.fn(),
  };
  const useAppStore: any = (sel: any) => sel(state);
  useAppStore.getState = () => state;
  return {
    ...actual,
    useAppStore,
  };
});

import { AccountsSidebar } from '../AccountsSidebar';

describe('AccountsSidebar', () => {
  beforeEach(() => {
    useAppStore.getState().resetModalStack?.();
  });

  it('renders an ACCOUNTS label', () => {
    render(<AccountsSidebar />);
    expect(screen.getByText(/^accounts$/i)).toBeInTheDocument();
  });

  it('renders a refresh-all button', () => {
    render(<AccountsSidebar />);
    expect(screen.getByRole('button', { name: /refresh all/i })).toBeInTheDocument();
  });

  it('renders one row per account', () => {
    render(<AccountsSidebar />);
    expect(screen.getByText('a@x.com')).toBeInTheDocument();
    expect(screen.getByText('b@y.com')).toBeInTheDocument();
  });

  it('renders the + Add account button', () => {
    render(<AccountsSidebar />);
    expect(screen.getByRole('button', { name: /\+ add account/i })).toBeInTheDocument();
  });
});
