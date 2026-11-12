import { render, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const appState = vi.hoisted(() => ({
  init: () => Promise.resolve(),
  accounts: [{ slot: 1 }] as unknown[],
  viewMode: 'expanded' as 'expanded' | 'compact',
}));
vi.mock('./lib/store', () => {
  const useAppStore: any = (sel: any) => sel(appState);
  useAppStore.getState = () => appState;
  return { useAppStore };
});
vi.mock('./lib/theme', () => ({
  applyTheme: vi.fn(),
  useThemeStore: (sel: any) => sel({ themePreference: 'light', palette: 'terracotta' }),
}));
vi.mock('./lib/updateEvents', () => ({ attachUpdateListeners: () => Promise.resolve(() => {}) }));
vi.mock('./popover/CompactPopover', () => ({ CompactPopover: () => <div>compact stub</div> }));
vi.mock('./report/ExpandedReport', () => ({ ExpandedReport: () => <div>expanded stub</div> }));
vi.mock('./settings/AuthPanel', () => ({ AuthPanel: () => <div>auth stub</div> }));

import { App } from './App';

const WINDOWS_UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Edg/130.0';

beforeEach(() => {
  window.matchMedia = ((query: string) => ({
    matches: false,
    media: query,
    addEventListener: () => {},
    removeEventListener: () => {},
    // The older pair, which the animation library still calls.
    addListener: () => {},
    removeListener: () => {},
  })) as unknown as typeof window.matchMedia;
  vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(WINDOWS_UA);
});

afterEach(() => {
  vi.restoreAllMocks();
  appState.viewMode = 'expanded';
});

describe('the Windows window wrapper', () => {
  /**
   * The wrapper animates the DOM because resizing a Windows window every frame
   * is laggy. It asks for 960x640 (CSS px), but the OS can give the window less
   * (mixed-DPI monitors, snapping, a manual resize), and a fixed-size box in a
   * smaller window cuts the report's right and bottom edges off. macOS has no
   * wrapper and just reflows. The box may never be larger than the window.
   */
  it('never grows past the window it is in', async () => {
    const { container } = render(<App />);
    const box = await waitFor(() => {
      const el = container.querySelector<HTMLElement>('.win-animated-container');
      expect(el).not.toBeNull();
      return el!;
    });
    expect(box.style.maxWidth).toBe('100%');
    expect(box.style.maxHeight).toBe('100%');
  });

  it('still asks for the full report size when the window has room', async () => {
    const { container } = render(<App />);
    const box = await waitFor(() => {
      const el = container.querySelector<HTMLElement>('.win-animated-container');
      expect(el).not.toBeNull();
      return el!;
    });
    expect(box.style.width).toBe('960px');
    expect(box.style.height).toBe('640px');
  });
});
