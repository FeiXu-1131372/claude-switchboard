import { render, screen } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { AuthPanel } from '../AuthPanel';

vi.mock('../../lib/store', () => ({
  useAppStore: (sel: (s: any) => any) => sel({ refreshAccounts: vi.fn() }),
}));

describe('AuthPanel presentation', () => {
  it('renders close-window button in fullpane mode (default)', () => {
    render(<AuthPanel />);
    expect(screen.getByRole('button', { name: /close/i })).toBeInTheDocument();
  });

  it('does NOT render close-window button in modal mode', () => {
    render(<AuthPanel presentation="modal" />);
    expect(screen.queryByRole('button', { name: /close/i })).toBeNull();
  });

  it('renders Connect to Claude heading in both modes', () => {
    const { rerender } = render(<AuthPanel presentation="fullpane" />);
    expect(screen.getByRole('heading', { name: /connect to claude/i })).toBeInTheDocument();
    rerender(<AuthPanel presentation="modal" />);
    expect(screen.getByRole('heading', { name: /connect to claude/i })).toBeInTheDocument();
  });

  it('renders the Back button in modal mode when onBack is provided', () => {
    // The chooser → OAuth flow inside the modal needs a way back to the
    // chooser tile list. Dropping the close-window chrome must not also
    // drop the Back affordance.
    render(<AuthPanel presentation="modal" onBack={() => {}} />);
    expect(screen.getByRole('button', { name: /back/i })).toBeInTheDocument();
  });

  it('does NOT render Back when onBack is omitted', () => {
    render(<AuthPanel presentation="modal" />);
    expect(screen.queryByRole('button', { name: /back/i })).toBeNull();
  });
});
