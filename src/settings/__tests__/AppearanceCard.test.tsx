import { fireEvent, render, screen, within } from '@testing-library/react';
import { beforeEach, describe, expect, it } from 'vitest';
import { PALETTES, useThemeStore } from '../../lib/theme';
import { AppearanceCard } from '../AppearanceCard';

describe('AppearanceCard', () => {
  beforeEach(() => {
    localStorage.clear();
    useThemeStore.setState({ themePreference: 'light', palette: 'terracotta' });
  });

  it('offers light, dark and follow-the-system', () => {
    render(<AppearanceCard />);
    const mode = screen.getByRole('radiogroup', { name: 'Mode' });
    expect(within(mode).getAllByRole('radio').map((r) => (r as HTMLInputElement).value)).toEqual([
      'light',
      'dark',
      'auto',
    ]);
    expect(within(mode).getByRole('radio', { name: 'Light' })).toBeChecked();
  });

  it('changes the mode', () => {
    render(<AppearanceCard />);
    fireEvent.click(screen.getByRole('radio', { name: 'Dark' }));
    expect(useThemeStore.getState().themePreference).toBe('dark');
    expect(screen.getByRole('radio', { name: 'Dark' })).toBeChecked();
  });

  it('offers every colour theme by name, with the current one chosen', () => {
    render(<AppearanceCard />);
    const colour = screen.getByRole('radiogroup', { name: 'Colour' });
    expect(within(colour).getAllByRole('radio')).toHaveLength(PALETTES.length);
    for (const p of PALETTES) {
      expect(within(colour).getByRole('radio', { name: p.label })).toBeInTheDocument();
    }
    expect(within(colour).getByRole('radio', { name: 'Terracotta' })).toBeChecked();
  });

  it('applies a colour theme as soon as it is chosen, and remembers it', () => {
    render(<AppearanceCard />);
    fireEvent.click(screen.getByRole('radio', { name: 'Tide' }));
    expect(useThemeStore.getState().palette).toBe('tide');
    expect(localStorage.getItem('theme-palette')).toBe('tide');
    expect(screen.getByRole('radio', { name: 'Tide' })).toBeChecked();
    expect(screen.getByRole('radio', { name: 'Terracotta' })).not.toBeChecked();
  });

  it('shows each colour theme in its own colour, whichever one is in use', () => {
    render(<AppearanceCard />);
    for (const p of PALETTES) {
      const swatch = screen.getByTestId(`swatch-${p.id}`);
      expect(swatch.style.background).toBe(`var(--swatch-${p.id})`);
    }
  });
});
