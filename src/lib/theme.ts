import { create } from 'zustand';

export type ThemePreference = 'light' | 'dark' | 'auto';
export type ResolvedTheme = 'light' | 'dark';

// Also referenced as a literal in index.html's pre-mount script — keep in sync.
const STORAGE_KEY = 'theme-preference';

export function readStoredPreference(): ThemePreference {
  if (typeof localStorage === 'undefined') return 'light';
  const raw = localStorage.getItem(STORAGE_KEY);
  // Legacy value 'cream' was the previous name for the light theme; migrate
  // silently so existing users keep their preference across the rename.
  if (raw === 'cream') return 'light';
  return raw === 'light' || raw === 'dark' || raw === 'auto' ? raw : 'light';
}

export function resolveTheme(pref: ThemePreference, prefersDark: boolean): ResolvedTheme {
  if (pref === 'auto') return prefersDark ? 'dark' : 'light';
  return pref;
}

interface ThemeStore {
  themePreference: ThemePreference;
  setThemePreference: (pref: ThemePreference) => void;
}

export const useThemeStore = create<ThemeStore>((set) => ({
  themePreference: readStoredPreference(),
  setThemePreference: (pref) => {
    try {
      localStorage.setItem(STORAGE_KEY, pref);
    } catch {
      // Safari private mode / quota-exceeded — preference still applies for
      // this session, just isn't persisted across launches. Surfacing an
      // error to the user for a theme preference would be over-reactive.
    }
    set({ themePreference: pref });
  },
}));
