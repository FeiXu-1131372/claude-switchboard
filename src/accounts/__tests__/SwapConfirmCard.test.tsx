import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { SwapConfirmCard } from '../SwapConfirmCard';
import { ModalShell } from '../../components/modals/ModalShell';

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

  /**
   * With Windows text size above 100%, WebView2 zooms the page and the
   * window holds fewer CSS pixels than the 380 the popover asks for. The
   * modal card then hit its max height and scrolled as one block, pushing
   * the Switch button below the fold. Only the details may scroll.
   */
  it('keeps the Switch button in view when the modal is short', () => {
    render(
      <ModalShell id="swap-confirm" onDismiss={() => {}} title="Confirm switch">
        <SwapConfirmCard
          current={null}
          target={TARGET}
          running={RUNNING}
          busy={false}
          errorMessage={null}
          onConfirm={() => {}}
          onCancel={() => {}}
        />
      </ModalShell>,
    );
    const switchButton = screen.getByRole('button', { name: /switch account/i });
    const details = screen.getByText('What happens').closest<HTMLElement>('.overflow-y-auto')!;
    const swapCard = details.parentElement!;
    const modalCard = swapCard.parentElement!;

    expect(details).not.toContainElement(switchButton);
    // The card shrinks to the modal so its details scroll, not the modal...
    expect(swapCard).toHaveClass('min-h-0', 'flex-col');
    // ...which has to be a column for that shrink to happen.
    expect(modalCard).toHaveClass('flex', 'flex-col', 'max-h-full');
  });
});
