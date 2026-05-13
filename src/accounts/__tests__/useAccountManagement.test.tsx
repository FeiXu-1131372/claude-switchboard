import { renderHook, act, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';

// vi.hoisted() runs before vi.mock() factories, so these references
// are available to the (hoisted) factory bodies below.
const { mockSubscribers, ipcMock, storeMocks } = vi.hoisted(() => ({
  mockSubscribers: {} as Record<string, (payload: any) => void>,
  ipcMock: {
    forceRefresh: vi.fn().mockResolvedValue(undefined),
    detectRunningClaudeCode: vi.fn().mockResolvedValue({ cli_processes: 0, vscode_with_extension: [] }),
    swapToAccount: vi.fn().mockResolvedValue({
      new_active_slot: 2,
      running: { cli_processes: 0, vscode_with_extension: [] },
    }),
    removeAccount: vi.fn().mockResolvedValue(undefined),
    startOauthFlow: vi.fn().mockResolvedValue('https://example.com/oauth'),
  },
  storeMocks: {
    refreshAccounts: vi.fn().mockResolvedValue(undefined),
    setPendingSwapReport: vi.fn(),
  },
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((name: string, cb: (e: any) => void) => {
    mockSubscribers[name] = (payload) => cb({ payload });
    return Promise.resolve(() => { delete mockSubscribers[name]; });
  }),
}));

vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('../../lib/ipc', () => ({ ipc: ipcMock }));

const ACCOUNTS = [
  { slot: 1, email: 'a@x.com', is_active: true, org_uuid: 'org1', account_uuid: 'u1' },
  { slot: 2, email: 'b@y.com', is_active: false, org_uuid: 'org1', account_uuid: 'u2' },
];

vi.mock('../../lib/store', () => ({
  useAppStore: (sel: (s: any) => any) =>
    sel({
      accounts: ACCOUNTS,
      settings: { thresholds: [75, 90] },
      refreshAccounts: storeMocks.refreshAccounts,
      setPendingSwapReport: storeMocks.setPendingSwapReport,
    }),
}));

import { useAccountManagement } from '../useAccountManagement';

describe('useAccountManagement', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    Object.keys(mockSubscribers).forEach((k) => delete mockSubscribers[k]);
  });

  it('exposes accounts and current active', () => {
    const { result } = renderHook(() => useAccountManagement());
    expect(result.current.accounts).toHaveLength(2);
    expect(result.current.currentActive?.slot).toBe(1);
  });

  it('opens and closes the chooser', () => {
    const { result } = renderHook(() => useAccountManagement());
    expect(result.current.chooserOpen).toBe(false);
    act(() => result.current.openChooser());
    expect(result.current.chooserOpen).toBe(true);
    act(() => result.current.closeChooser());
    expect(result.current.chooserOpen).toBe(false);
  });

  it('requestSwap sets pending state', async () => {
    const { result } = renderHook(() => useAccountManagement());
    await act(async () => { await result.current.requestSwap(ACCOUNTS[1] as any); });
    expect(result.current.pending?.target.slot).toBe(2);
  });

  it('requestSwap is a no-op for the active account', async () => {
    const { result } = renderHook(() => useAccountManagement());
    await act(async () => { await result.current.requestSwap(ACCOUNTS[0] as any); });
    expect(result.current.pending).toBe(null);
  });

  it('confirmSwap calls ipc.swapToAccount and clears pending on success', async () => {
    const { result } = renderHook(() => useAccountManagement());
    await act(async () => { await result.current.requestSwap(ACCOUNTS[1] as any); });
    await act(async () => { await result.current.confirmSwap(); });
    expect(ipcMock.swapToAccount).toHaveBeenCalledWith(2);
    expect(result.current.pending).toBe(null);
  });

  it('handleReauth opens browser and records the pending slot', async () => {
    const { result } = renderHook(() => useAccountManagement());
    await act(async () => { await result.current.handleReauth(ACCOUNTS[1] as any); });
    expect(ipcMock.startOauthFlow).toHaveBeenCalled();
    expect(result.current.reauthSlot).toBe(2);
  });

  it('reauth listener clears reauthSlot on oauth_complete', async () => {
    const { result } = renderHook(() => useAccountManagement());
    await act(async () => { await result.current.handleReauth(ACCOUNTS[1] as any); });
    await waitFor(() => expect(mockSubscribers['oauth_complete']).toBeDefined());
    act(() => { mockSubscribers['oauth_complete'](2); });
    await waitFor(() => expect(result.current.reauthSlot).toBe(null));
  });

  it('handleRefreshAll calls forceRefresh("all") and sets refreshing true then false', async () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => useAccountManagement());
    await act(async () => { await result.current.handleRefreshAll(); });
    expect(ipcMock.forceRefresh).toHaveBeenCalledWith('all');
    expect(result.current.refreshing).toBe(true);
    await act(async () => { await vi.runAllTimersAsync(); });
    expect(result.current.refreshing).toBe(false);
    vi.useRealTimers();
  });
});
