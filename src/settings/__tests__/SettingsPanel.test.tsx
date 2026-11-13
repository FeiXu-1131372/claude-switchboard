import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Settings } from '../../lib/types';

const ipcMock = vi.hoisted(() => ({
  updateSettings: vi.fn(),
  getWarmupConsentGranted: vi.fn(() => Promise.resolve(false)),
  osSchedulerIsRegistered: vi.fn(() => Promise.resolve(false)),
  listAvailableTerminals: vi.fn(() => Promise.resolve([])),
}));
vi.mock('../../lib/ipc', () => ({ ipc: ipcMock }));

const autostart = vi.hoisted(() => ({
  enable: vi.fn(() => Promise.resolve()),
  disable: vi.fn(() => Promise.resolve()),
  isEnabled: vi.fn(() => Promise.resolve(false)),
}));
vi.mock('@tauri-apps/plugin-autostart', () => autostart);

// Their own behaviour is tested elsewhere; here they would only bring IPC.
vi.mock('../AppearanceCard', () => ({ AppearanceCard: () => null }));
vi.mock('../StatuslineBridgeCard', () => ({ StatuslineBridgeCard: () => null }));
vi.mock('../WarmupSettings', () => ({ WarmupSettings: () => null }));
vi.mock('../AccountRotationSettings', () => ({ AccountRotationSettings: () => null }));

import { SettingsPanel, SAVE_DELAY_MS } from '../SettingsPanel';
import { useAppStore } from '../../lib/store';

const SETTINGS: Settings = {
  polling_interval_secs: 300,
  stagger_gap_secs: 20,
  thresholds: [75, 90],
  theme: 'light',
  launch_at_login: false,
  crash_reports: false,
  preferred_auth_source: null,
  terminal: null,
  account_rotation: null,
};

beforeEach(() => {
  vi.useFakeTimers();
  ipcMock.updateSettings.mockReset().mockResolvedValue(null);
  autostart.enable.mockClear();
  useAppStore.setState({ settings: SETTINGS, accounts: [], usage: null });
});

afterEach(() => {
  vi.useRealTimers();
});

/**
 * Settings used to wait for a Save button at the bottom of a long scroll,
 * while Appearance, live usage and warm-up applied at once, so a change
 * made at the top was lost on Back. Now every setting saves on its own.
 */
describe('SettingsPanel saving', () => {
  it('has no Save button', () => {
    render(<SettingsPanel />);
    expect(screen.queryByRole('button', { name: /^save/i })).toBeNull();
  });

  it('saves a toggle on its own and turns launch at login on', async () => {
    render(<SettingsPanel />);
    fireEvent.click(screen.getByRole('switch', { name: /launch at login/i }));
    await act(() => vi.advanceTimersByTimeAsync(SAVE_DELAY_MS));

    expect(ipcMock.updateSettings).toHaveBeenCalledTimes(1);
    expect(ipcMock.updateSettings).toHaveBeenCalledWith({ ...SETTINGS, launch_at_login: true });
    expect(autostart.enable).toHaveBeenCalledTimes(1);
  });

  it('writes a slider drag once, when it settles', async () => {
    render(<SettingsPanel />);
    const slider = screen.getByLabelText('Stagger gap');
    for (const v of [25, 30, 35, 40]) {
      fireEvent.change(slider, { target: { value: String(v) } });
      await act(() => vi.advanceTimersByTimeAsync(SAVE_DELAY_MS / 4));
    }
    expect(ipcMock.updateSettings).not.toHaveBeenCalled();

    await act(() => vi.advanceTimersByTimeAsync(SAVE_DELAY_MS));
    expect(ipcMock.updateSettings).toHaveBeenCalledTimes(1);
    expect(ipcMock.updateSettings).toHaveBeenCalledWith({ ...SETTINGS, stagger_gap_secs: 40 });
  });

  it('does not drop an edit made just before leaving', async () => {
    const { unmount } = render(<SettingsPanel />);
    fireEvent.change(screen.getByLabelText('Warning at'), { target: { value: '60' } });
    unmount();
    await act(() => vi.advanceTimersByTimeAsync(0));

    expect(ipcMock.updateSettings).toHaveBeenCalledWith({ ...SETTINGS, thresholds: [60, 90] });
  });

  it('shows a failed save outside the scroll, and Retry writes it again', async () => {
    ipcMock.updateSettings.mockRejectedValueOnce(new Error('disk full'));
    render(<SettingsPanel />);
    fireEvent.change(screen.getByLabelText('Critical at'), { target: { value: '95' } });
    await act(() => vi.advanceTimersByTimeAsync(SAVE_DELAY_MS));

    const alert = screen.getByRole('alert');
    expect(alert).toHaveTextContent("Couldn't save settings: disk full");
    expect(alert.closest('.scroll-area')).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await act(() => vi.advanceTimersByTimeAsync(0));
    expect(ipcMock.updateSettings).toHaveBeenCalledTimes(2);
    expect(ipcMock.updateSettings).toHaveBeenLastCalledWith({ ...SETTINGS, thresholds: [75, 95] });
    expect(screen.queryByRole('alert')).toBeNull();
  });
});
