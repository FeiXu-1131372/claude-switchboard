import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { SwapConfirmCard } from '../SwapConfirmCard';

const TARGET = {
  slot: 1, account_uuid: 'uu', email: 'bob@y.com', subscription_type: 'pro',
  is_active: false, org_uuid: null, cached_usage: null, last_error: null,
} as any;

const RUNNING = { cli_processes: 0, vscode_with_extension: [] };

describe('SwapConfirmCard', () => {
  it('renders top bar back button in fullpane mode', () => {
    render(
      <SwapConfirmCard
        current={null}
        target={TARGET}
        running={RUNNING}
        busy={false}
        errorMessage={null}
        onConfirm={() => {}}
        onCancel={() => {}}
        presentation="fullpane"
      />,
    );
    // Fullpane has THREE Cancel-named affordances: top ← Cancel button,
    // top-right <X> with aria-label="Cancel", and footer Cancel button.
    expect(screen.getAllByRole('button', { name: /cancel/i }).length).toBe(3);
  });

  it('drops the top bar in modal mode', () => {
    render(
      <SwapConfirmCard
        current={null}
        target={TARGET}
        running={RUNNING}
        busy={false}
        errorMessage={null}
        onConfirm={() => {}}
        onCancel={() => {}}
        presentation="modal"
      />,
    );
    // Footer Cancel still present; no second "Cancel" affordance in the top bar.
    const cancels = screen.getAllByRole('button', { name: /cancel/i });
    expect(cancels.length).toBe(1);
  });

  it('always renders the footer Switch button', () => {
    render(
      <SwapConfirmCard
        current={null}
        target={TARGET}
        running={RUNNING}
        busy={false}
        errorMessage={null}
        onConfirm={() => {}}
        onCancel={() => {}}
        presentation="modal"
      />,
    );
    expect(screen.getByRole('button', { name: /switch/i })).toBeInTheDocument();
  });

  it('defaults presentation to modal', () => {
    render(
      <SwapConfirmCard
        current={null}
        target={TARGET}
        running={RUNNING}
        busy={false}
        errorMessage={null}
        onConfirm={() => {}}
        onCancel={() => {}}
      />,
    );
    expect(screen.getAllByRole('button', { name: /cancel/i }).length).toBe(1);
  });
});
