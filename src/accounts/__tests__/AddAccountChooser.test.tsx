import { render, screen } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { AddAccountChooser } from '../AddAccountChooser';

vi.mock('../../lib/ipc', () => ({
  ipc: {
    addAccountFromClaudeCode: vi.fn(),
  },
}));

vi.mock('../../lib/store', () => ({
  useAppStore: (sel: (s: any) => any) =>
    sel({
      accounts: [],
      refreshAccounts: vi.fn(),
    }),
}));

describe('AddAccountChooser', () => {
  it('renders the Cancel button in fullpane mode (default fallback caller)', () => {
    render(<AddAccountChooser presentation="fullpane" onClose={() => {}} />);
    expect(screen.getByRole('button', { name: /cancel/i })).toBeInTheDocument();
  });

  it('does NOT render the footer Cancel button in modal mode', () => {
    render(<AddAccountChooser presentation="modal" onClose={() => {}} />);
    expect(screen.queryByRole('button', { name: /^cancel$/i })).toBeNull();
  });

  it('renders the h2 heading in fullpane mode', () => {
    render(<AddAccountChooser presentation="fullpane" onClose={() => {}} />);
    expect(screen.getByRole('heading', { name: /add account/i })).toBeInTheDocument();
  });

  it('does NOT render the h2 heading in modal mode (parent ModalShell provides title)', () => {
    render(<AddAccountChooser presentation="modal" onClose={() => {}} />);
    expect(screen.queryByRole('heading', { name: /add account/i })).toBeNull();
  });

  it('defaults presentation to modal', () => {
    render(<AddAccountChooser onClose={() => {}} />);
    expect(screen.queryByRole('button', { name: /^cancel$/i })).toBeNull();
  });
});
