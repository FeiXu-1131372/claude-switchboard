import { readFileSync } from 'node:fs';
import path from 'node:path';
import { afterEach, describe, it, expect, beforeEach, vi } from 'vitest';
import {
  PALETTES,
  applyTheme,
  readStoredPalette,
  readStoredPreference,
  resolveTheme,
  useThemeStore,
} from '../theme';

describe('resolveTheme', () => {
  it('returns the preference when explicit', () => {
    expect(resolveTheme('light', /* prefersDark */ true)).toBe('light');
    expect(resolveTheme('dark', /* prefersDark */ false)).toBe('dark');
  });

  it('follows the OS for auto', () => {
    expect(resolveTheme('auto', /* prefersDark */ true)).toBe('dark');
    expect(resolveTheme('auto', /* prefersDark */ false)).toBe('light');
  });
});

describe('useThemeStore', () => {
  beforeEach(() => {
    localStorage.clear();
    useThemeStore.setState({ themePreference: 'light' });
  });

  it('defaults to light', () => {
    expect(useThemeStore.getState().themePreference).toBe('light');
  });

  it('updates the preference', () => {
    useThemeStore.getState().setThemePreference('dark');
    expect(useThemeStore.getState().themePreference).toBe('dark');
  });
});

describe('readStoredPreference', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('returns light when localStorage is empty', () => {
    expect(readStoredPreference()).toBe('light');
  });

  it('migrates the legacy "cream" value to "light"', () => {
    localStorage.setItem('theme-preference', 'cream');
    expect(readStoredPreference()).toBe('light');
  });

  it('falls back to light for an unrecognized stored value', () => {
    localStorage.setItem('theme-preference', 'sepia');
    expect(readStoredPreference()).toBe('light');
  });
});

describe('colour themes', () => {
  beforeEach(() => {
    localStorage.clear();
    useThemeStore.setState({ themePreference: 'light', palette: 'terracotta' });
  });
  afterEach(() => vi.restoreAllMocks());

  it('lists the themes, the original first', () => {
    expect(PALETTES.map((p) => p.id)).toEqual(['terracotta', 'graphite', 'tide', 'moss', 'plum']);
    expect(PALETTES.every((p) => p.label.length > 0)).toBe(true);
  });

  it('starts on the original theme', () => {
    expect(readStoredPalette()).toBe('terracotta');
  });

  it('reads back a saved theme', () => {
    localStorage.setItem('theme-palette', 'moss');
    expect(readStoredPalette()).toBe('moss');
  });

  it('falls back to the original for a theme this build does not have', () => {
    localStorage.setItem('theme-palette', 'neon');
    expect(readStoredPalette()).toBe('terracotta');
  });

  it('falls back to the original when storage is blocked', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    expect(readStoredPalette()).toBe('terracotta');
  });

  it('saves the chosen theme', () => {
    useThemeStore.getState().setPalette('plum');
    expect(useThemeStore.getState().palette).toBe('plum');
    expect(localStorage.getItem('theme-palette')).toBe('plum');
  });

  it('still applies the chosen theme when it cannot be saved', () => {
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    useThemeStore.getState().setPalette('tide');
    expect(useThemeStore.getState().palette).toBe('tide');
  });
});

describe('applyTheme', () => {
  it('puts the mode and the colour theme on the body', () => {
    applyTheme('dark', 'tide', false);
    expect(document.body.dataset.theme).toBe('dark');
    expect(document.body.dataset.palette).toBe('tide');
  });

  it('resolves auto against the system', () => {
    applyTheme('auto', 'terracotta', true);
    expect(document.body.dataset.theme).toBe('dark');
    applyTheme('auto', 'terracotta', false);
    expect(document.body.dataset.theme).toBe('light');
    expect(document.body.dataset.palette).toBe('terracotta');
  });
});

describe('the pre-mount script', () => {
  // index.html sets both attributes before React loads, so the first paint is
  // already in the saved theme. It cannot import this module, so it repeats
  // the storage keys as literals.
  const html = readFileSync(path.resolve(__dirname, '../../../index.html'), 'utf8');

  it('reads the same storage keys as the store', () => {
    expect(html).toContain("localStorage.getItem('theme-preference')");
    expect(html).toContain("localStorage.getItem('theme-palette')");
  });

  it('sets the colour theme on the body', () => {
    expect(html).toMatch(/document\.body\.dataset\.palette\s*=/);
  });
});
