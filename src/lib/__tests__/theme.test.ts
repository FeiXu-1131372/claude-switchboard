import { describe, it, expect, beforeEach } from 'vitest';
import { useThemeStore, resolveTheme, readStoredPreference } from '../theme';

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
