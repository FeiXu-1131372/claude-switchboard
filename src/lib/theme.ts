import { create } from 'zustand';

export type ThemePreference = 'cream' | 'dark' | 'auto';
export type ResolvedTheme = 'cream' | 'dark';

const STORAGE_KEY = 'theme-preference';

function readStoredPreference(): ThemePreference {
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
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem(STORAGE_KEY, pref);
    }
    set({ themePreference: pref });
  },
}));
