import { Card } from '../components/ui/Card';
import { PALETTES, useThemeStore, type ThemePreference } from '../lib/theme';

const MODES: readonly { value: ThemePreference; label: string }[] = [
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
  { value: 'auto', label: 'Auto (follow system)' },
];

const LEGEND =
  'text-[length:var(--text-micro)] uppercase tracking-[var(--tracking-label)] text-[color:var(--color-text-muted)]';

/**
 * How the app looks: light or dark, and which colour theme.
 *
 * The two are separate choices. A colour theme is a whole palette (surfaces,
 * text and one accent) with a light and a dark version, so any theme works in
 * any mode. Either choice applies at once; the window behind the dialog is
 * the preview.
 */
export function AppearanceCard() {
  const themePreference = useThemeStore((s) => s.themePreference);
  const setThemePreference = useThemeStore((s) => s.setThemePreference);
  const palette = useThemeStore((s) => s.palette);
  const setPalette = useThemeStore((s) => s.setPalette);

  return (
    <Card className="flex flex-col gap-[var(--space-md)] p-[var(--space-md)]">
      <div role="radiogroup" aria-label="Mode" className="flex flex-col gap-[var(--space-xs)]">
        <span aria-hidden className={LEGEND}>
          Mode
        </span>
        {MODES.map((mode) => (
          <label
            key={mode.value}
            className="flex cursor-pointer items-center gap-[var(--space-sm)] py-[var(--space-2xs)]"
          >
            <input
              type="radio"
              name="theme-preference"
              value={mode.value}
              checked={themePreference === mode.value}
              onChange={() => setThemePreference(mode.value)}
              className="accent-[color:var(--color-accent)]"
            />
            <span className="text-[length:var(--text-body)] text-[color:var(--color-text)]">{mode.label}</span>
          </label>
        ))}
      </div>

      <div
        role="radiogroup"
        aria-label="Colour"
        className="flex flex-col gap-[var(--space-sm)] border-t border-[var(--color-border-subtle)] pt-[var(--space-md)]"
      >
        <span aria-hidden className={LEGEND}>
          Colour
        </span>
        <div className="flex flex-wrap gap-[var(--space-sm)]">
          {PALETTES.map((p) => {
            const chosen = palette === p.id;
            return (
              <label
                key={p.id}
                className={[
                  'flex cursor-pointer items-center gap-[var(--space-sm)]',
                  'rounded-[var(--radius-pill)] border py-[var(--space-xs)] pl-[var(--space-xs)] pr-[var(--space-md)]',
                  'transition-[background,border-color] duration-[var(--duration-fast)]',
                  // The native radio is hidden, so the chip shows its focus.
                  'has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-[var(--color-border-focus)] has-[:focus-visible]:outline-offset-2',
                  chosen
                    ? 'border-[var(--color-accent)] bg-[var(--color-accent-dim)]'
                    : 'border-[var(--color-border)] hover:border-[var(--color-border-hover)]',
                ].join(' ')}
              >
                <input
                  type="radio"
                  name="theme-palette"
                  value={p.id}
                  checked={chosen}
                  onChange={() => setPalette(p.id)}
                  className="sr-only"
                />
                {/* Each theme's own accent, not the one in use: a fixed swatch
                    token, since the accent token follows the current theme. */}
                <span
                  aria-hidden
                  data-testid={`swatch-${p.id}`}
                  className="size-[var(--space-lg)] rounded-full border border-[var(--color-border-subtle)]"
                  style={{ background: `var(--swatch-${p.id})` }}
                />
                <span className="text-[length:var(--text-label)] text-[color:var(--color-text)]">{p.label}</span>
              </label>
            );
          })}
        </div>
      </div>
    </Card>
  );
}
