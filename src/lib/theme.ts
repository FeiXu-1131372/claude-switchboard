import { create } from 'zustand';

export type ThemePreference = 'cream' | 'dark' | 'auto';
export type ResolvedTheme = 'cream' | 'dark';

// Also referenced as a literal in index.html's pre-mount script — keep in sync.
const STORAGE_KEY = 'theme-preference';

export function readStoredPreference(): ThemePreference {
  if (typeof localStorage === 'undefined') return 'cream';
  const raw = localStorage.getItem(STORAGE_KEY);
  return raw === 'cream' || raw === 'dark' || raw === 'auto' ? raw : 'cream';
}

export function resolveTheme(pref: ThemePreference, prefersDark: boolean): ResolvedTheme {
  if (pref === 'auto') return prefersDark ? 'dark' : 'cream';
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
