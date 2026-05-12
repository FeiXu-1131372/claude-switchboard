import { describe, it, expect, beforeEach } from 'vitest';
import { useThemeStore, resolveTheme, readStoredPreference } from '../theme';

describe('resolveTheme', () => {
  it('returns the preference when explicit', () => {
    expect(resolveTheme('cream', /* prefersDark */ true)).toBe('cream');
    expect(resolveTheme('dark', /* prefersDark */ false)).toBe('dark');
  });

  it('follows the OS for auto', () => {
    expect(resolveTheme('auto', /* prefersDark */ true)).toBe('dark');
    expect(resolveTheme('auto', /* prefersDark */ false)).toBe('cream');
  });
});

describe('useThemeStore', () => {
  beforeEach(() => {
    localStorage.clear();
    useThemeStore.setState({ themePreference: 'cream' });
  });

  it('defaults to cream', () => {
    expect(useThemeStore.getState().themePreference).toBe('cream');
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

  it('returns cream when localStorage is empty', () => {
    expect(readStoredPreference()).toBe('cream');
  });

  it('falls back to cream for an unrecognized stored value', () => {
    localStorage.setItem('theme-preference', 'light');
    expect(readStoredPreference()).toBe('cream');
  });
});
