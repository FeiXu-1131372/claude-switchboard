import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';

const ipcMock = vi.hoisted(() => ({
  forceRefresh: vi.fn(),
}));
vi.mock('../lib/ipc', () => ({ ipc: ipcMock }));

const appState = vi.hoisted(() => ({
  stale: false,
  sessionDataVersion: 0,
  toggleViewMode: vi.fn(),
}));
vi.mock('../lib/store', () => {
  const useAppStore: any = (sel: any) => sel(appState);
  useAppStore.getState = () => appState;
  return { useAppStore };
});

const windowMock = vi.hoisted(() => ({ getCurrentWindow: vi.fn() }));
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: windowMock.getCurrentWindow,
  LogicalPosition: class {},
}));
vi.mock('@tauri-apps/api/event', () => ({ emit: vi.fn(), listen: vi.fn() }));

// Each tab is replaced by a stub that renders its own name: these tests are
// about the shell around the tabs, not what a tab shows.
vi.mock('./SessionsTab', () => ({ SessionsTab: () => <div>SessionsTab stub</div>, WINDOW_DAYS: 7 }));
vi.mock('./TrendsTab', () => ({ TrendsTab: () => <div>TrendsTab stub</div> }));
vi.mock('./BreakdownTab', () => ({ BreakdownTab: () => <div>BreakdownTab stub</div> }));
vi.mock('../providers/ProvidersTab', () => ({ ProvidersTab: () => <div>ProvidersTab stub</div> }));
vi.mock('../sessions/SessionsBrowserTab', () => ({
  SessionsBrowserTab: () => <div>SessionsBrowserTab stub</div>,
}));
vi.mock('../accounts/AccountsSidebar', () => ({ AccountsSidebar: () => <aside>Accounts</aside> }));
vi.mock('../components/modals/SettingsModal', () => ({
  SettingsModal: () => <div role="dialog">Settings stub</div>,
}));

import { ExpandedReport } from './ExpandedReport';
import { useReportStore } from './reportState';

beforeEach(() => {
  // jsdom has no ResizeObserver; the tab underline measures with one.
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
  // …and its Range cannot measure text, which the underline also does.
  Range.prototype.getBoundingClientRect = () =>
    ({ x: 0, y: 0, left: 0, top: 0, right: 0, bottom: 0, width: 0, height: 0, toJSON() {} }) as DOMRect;
  // …and no PointerEvent, so a pointer press would carry no `button`.
  if (!window.PointerEvent) {
    window.PointerEvent = class extends MouseEvent {
      pointerId = 1;
    } as unknown as typeof PointerEvent;
  }
  ipcMock.forceRefresh.mockReset().mockResolvedValue(null);
  appState.toggleViewMode.mockReset();
  appState.stale = false;
  windowMock.getCurrentWindow.mockReset().mockImplementation(() => {
    throw new Error('outside Tauri');
  });
});

describe('ExpandedReport shell', () => {
  it('opens on Sessions and shows every destination', () => {
    render(<ExpandedReport />);
    expect(screen.getByRole('tab', { name: 'Sessions' })).toHaveAttribute('aria-selected', 'true');
    expect(screen.getByText('SessionsBrowserTab stub')).toBeInTheDocument();
    expect(screen.getAllByRole('tab').map((t) => t.textContent)).toEqual([
      'Sessions',
      'Cost',
      'Trends',
      'Breakdown',
      'Providers',
    ]);
  });

  it('Breakdown stands where Models, Projects and Cache were', () => {
    render(<ExpandedReport />);
    fireEvent.click(screen.getByRole('tab', { name: 'Breakdown' }));
    expect(screen.getByText('BreakdownTab stub')).toBeInTheDocument();
    for (const gone of ['Models', 'Projects', 'Cache', 'Heatmap']) {
      expect(screen.queryByRole('tab', { name: gone })).toBeNull();
    }
  });

  it('shows the chosen tab', () => {
    render(<ExpandedReport />);
    fireEvent.click(screen.getByRole('tab', { name: 'Trends' }));
    expect(screen.getByRole('tab', { name: 'Trends' })).toHaveAttribute('aria-selected', 'true');
    expect(screen.getByText('TrendsTab stub')).toBeInTheDocument();
    expect(screen.queryByText('SessionsBrowserTab stub')).toBeNull();
  });

  it('Refresh asks the backend for the active account', async () => {
    render(<ExpandedReport />);
    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));
    await waitFor(() => expect(ipcMock.forceRefresh).toHaveBeenCalledWith('active'));
  });

  it('Settings opens the settings dialog', () => {
    render(<ExpandedReport />);
    expect(screen.queryByRole('dialog')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    expect(screen.getByRole('dialog')).toBeInTheDocument();
  });

  it('Collapse details returns to the popover', () => {
    render(<ExpandedReport />);
    fireEvent.click(screen.getByRole('button', { name: 'Collapse details' }));
    expect(appState.toggleViewMode).toHaveBeenCalledTimes(1);
  });

  it('comes back to the tab you left', () => {
    useReportStore.setState({ activeTab: 'trends' });
    render(<ExpandedReport />);
    expect(screen.getByRole('tab', { name: 'Trends' })).toHaveAttribute('aria-selected', 'true');
    expect(screen.getByText('TrendsTab stub')).toBeInTheDocument();
  });

  it('remembers the tab you choose', () => {
    const first = render(<ExpandedReport />);
    fireEvent.click(screen.getByRole('tab', { name: 'Cost' }));
    first.unmount();
    render(<ExpandedReport />);
    expect(screen.getByRole('tab', { name: 'Cost' })).toHaveAttribute('aria-selected', 'true');
  });

  it('falls back to the first tab for an unknown saved id', () => {
    useReportStore.setState({ activeTab: 'nope' });
    render(<ExpandedReport />);
    expect(screen.getByRole('tab', { name: 'Sessions' })).toHaveAttribute('aria-selected', 'true');
    expect(screen.getByText('SessionsBrowserTab stub')).toBeInTheDocument();
  });

  it('only the selected tab is in the tab order', () => {
    render(<ExpandedReport />);
    const tabs = screen.getAllByRole('tab');
    expect(tabs.filter((t) => t.tabIndex === 0)).toHaveLength(1);
    expect(tabs.find((t) => t.tabIndex === 0)).toHaveTextContent('Sessions');
    expect(tabs.filter((t) => t.tabIndex === -1)).toHaveLength(tabs.length - 1);
  });

  it('arrow keys move and select, wrapping at the ends', () => {
    render(<ExpandedReport />);
    const sessions = screen.getByRole('tab', { name: 'Sessions' });
    sessions.focus();
    fireEvent.keyDown(sessions, { key: 'ArrowLeft' });
    const providers = screen.getByRole('tab', { name: 'Providers' });
    expect(providers).toHaveAttribute('aria-selected', 'true');
    expect(providers).toHaveFocus();
    expect(screen.getByText('ProvidersTab stub')).toBeInTheDocument();

    fireEvent.keyDown(providers, { key: 'ArrowRight' });
    expect(screen.getByRole('tab', { name: 'Sessions' })).toHaveAttribute('aria-selected', 'true');
    expect(screen.getByRole('tab', { name: 'Sessions' })).toHaveFocus();

    fireEvent.keyDown(screen.getByRole('tab', { name: 'Sessions' }), { key: 'ArrowRight' });
    expect(screen.getByRole('tab', { name: 'Cost' })).toHaveFocus();
  });

  it('Home and End jump to the first and last tab', () => {
    render(<ExpandedReport />);
    const sessions = screen.getByRole('tab', { name: 'Sessions' });
    sessions.focus();
    fireEvent.keyDown(sessions, { key: 'End' });
    expect(screen.getByRole('tab', { name: 'Providers' })).toHaveFocus();
    expect(screen.getByRole('tab', { name: 'Providers' })).toHaveAttribute('aria-selected', 'true');
    fireEvent.keyDown(screen.getByRole('tab', { name: 'Providers' }), { key: 'Home' });
    expect(screen.getByRole('tab', { name: 'Sessions' })).toHaveFocus();
    expect(screen.getByRole('tab', { name: 'Sessions' })).toHaveAttribute('aria-selected', 'true');
  });

  it('other keys are left alone', () => {
    render(<ExpandedReport />);
    const sessions = screen.getByRole('tab', { name: 'Sessions' });
    sessions.focus();
    fireEvent.keyDown(sessions, { key: 'a' });
    expect(sessions).toHaveAttribute('aria-selected', 'true');
  });

  it('the panel is labelled by its tab', () => {
    render(<ExpandedReport />);
    const panel = screen.getByRole('tabpanel');
    expect(panel).toHaveAttribute('aria-labelledby', 'report-tab-browse');
    expect(panel).toHaveAttribute('tabindex', '0');
    expect(screen.getByRole('tab', { name: 'Sessions' })).toHaveAttribute('id', 'report-tab-browse');
    expect(screen.getByRole('tab', { name: 'Sessions' })).toHaveAttribute('aria-controls', panel.id);

    fireEvent.click(screen.getByRole('tab', { name: 'Trends' }));
    expect(screen.getByRole('tabpanel')).toHaveAttribute('aria-labelledby', 'report-tab-trends');
  });

  it('still opens and switches tabs when storage is blocked', () => {
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    try {
      render(<ExpandedReport />);
      fireEvent.click(screen.getByRole('tab', { name: 'Trends' }));
      expect(screen.getByText('TrendsTab stub')).toBeInTheDocument();
    } finally {
      vi.restoreAllMocks();
    }
  });

  it('returns each tab to where it was scrolled', async () => {
    render(<ExpandedReport />);
    const panel = screen.getByTestId('report-scroll');
    fireEvent.click(screen.getByRole('tab', { name: 'Cost' }));
    panel.scrollTop = 240;
    fireEvent.scroll(panel);
    await waitFor(() => expect(useReportStore.getState().scrollTop.cost).toBe(240));

    fireEvent.click(screen.getByRole('tab', { name: 'Trends' }));
    expect(panel.scrollTop).toBe(0);
    fireEvent.click(screen.getByRole('tab', { name: 'Cost' }));
    expect(panel.scrollTop).toBe(240);
  });

  it('a pointer press on the header starts a window drag, but not on a tab or a button', () => {
    render(<ExpandedReport />);
    fireEvent.pointerDown(screen.getByRole('tab', { name: 'Cost' }), { button: 0 });
    fireEvent.pointerDown(screen.getByRole('button', { name: 'Refresh' }), { button: 0 });
    expect(windowMock.getCurrentWindow).not.toHaveBeenCalled();

    fireEvent.pointerDown(screen.getByRole('banner'), { button: 0 });
    expect(windowMock.getCurrentWindow).toHaveBeenCalledTimes(1);
  });
});
