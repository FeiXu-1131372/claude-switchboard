import { create } from 'zustand';

export type ThemePreference = 'light' | 'dark' | 'auto';
export type ResolvedTheme = 'light' | 'dark';

/** The colour themes. A theme is a set of surfaces, text colours and an
 * accent; each has a light and a dark version, so it is chosen independently
 * of the mode above. The first is the original and the default: it is the
 * token file's base, and the others are blocks in `tokens.css` keyed by `id`. */
export const PALETTES = [
  { id: 'terracotta', label: 'Terracotta' },
  { id: 'graphite', label: 'Graphite' },
  { id: 'tide', label: 'Tide' },
  { id: 'moss', label: 'Moss' },
  { id: 'plum', label: 'Plum' },
] as const;

export type Palette = (typeof PALETTES)[number]['id'];

const DEFAULT_PALETTE: Palette = PALETTES[0].id;

// Both keys are also referenced as literals in index.html's pre-mount script —
// keep in sync (theme.test.ts checks).
const STORAGE_KEY = 'theme-preference';
const PALETTE_KEY = 'theme-palette';

/** The saved colour theme. Nothing saved, a theme this build does not have,
 * or storage that cannot be read all mean the original. */
export function readStoredPalette(): Palette {
  try {
    const raw = localStorage.getItem(PALETTE_KEY);
    return PALETTES.find((p) => p.id === raw)?.id ?? DEFAULT_PALETTE;
  } catch {
    return DEFAULT_PALETTE;
  }
}

export function readStoredPreference(): ThemePreference {
  if (typeof localStorage === 'undefined') return 'light';
  let raw: string | null;
  try {
    raw = localStorage.getItem(STORAGE_KEY);
  } catch {
    // Storage blocked. This runs while the module loads, so throwing here
    // would stop the whole app from mounting over a colour preference.
    return 'light';
  }
  // Legacy value 'cream' was the previous name for the light theme; migrate
  // silently so existing users keep their preference across the rename.
  if (raw === 'cream') return 'light';
  return raw === 'light' || raw === 'dark' || raw === 'auto' ? raw : 'light';
}

export function resolveTheme(pref: ThemePreference, prefersDark: boolean): ResolvedTheme {
  if (pref === 'auto') return prefersDark ? 'dark' : 'light';
  return pref;
}

/** Paint the app in a mode and a colour theme. Every colour token hangs off
 * these two attributes on `<body>`, so setting them repaints everything. */
export function applyTheme(pref: ThemePreference, palette: Palette, prefersDark: boolean) {
  document.body.dataset.theme = resolveTheme(pref, prefersDark);
  document.body.dataset.palette = palette;
}

interface ThemeStore {
  themePreference: ThemePreference;
  setThemePreference: (pref: ThemePreference) => void;
  palette: Palette;
  setPalette: (palette: Palette) => void;
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
  palette: readStoredPalette(),
  setPalette: (palette) => {
    try {
      localStorage.setItem(PALETTE_KEY, palette);
    } catch {
      // As above: applied for this session, not remembered.
    }
    set({ palette });
  },
}));
