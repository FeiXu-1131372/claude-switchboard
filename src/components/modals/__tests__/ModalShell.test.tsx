import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { ModalShell } from '../ModalShell';
import { useAppStore } from '../../../lib/store';

describe('ModalShell', () => {
  beforeEach(() => {
    useAppStore.getState().resetModalStack();
  });

  it('renders children and pushes id onto the stack on mount', () => {
    render(
      <ModalShell onDismiss={() => {}} id="m1">
        <p>hello</p>
      </ModalShell>,
    );
    expect(screen.getByText('hello')).toBeInTheDocument();
    expect(useAppStore.getState().modalStack).toEqual(['m1']);
  });

  it('pops id on unmount', () => {
    const { unmount } = render(
      <ModalShell onDismiss={() => {}} id="m1">
        <p>x</p>
      </ModalShell>,
    );
    unmount();
    expect(useAppStore.getState().modalStack).toEqual([]);
  });

  it('renders a title strip when title prop is provided', () => {
    render(
      <ModalShell onDismiss={() => {}} id="m1" title="Hi">
        <p>x</p>
      </ModalShell>,
    );
    expect(screen.getByText('Hi')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /close/i })).toBeInTheDocument();
  });

  it('does not render a title strip when title prop is omitted', () => {
    render(
      <ModalShell onDismiss={() => {}} id="m1">
        <p>x</p>
      </ModalShell>,
    );
    expect(screen.queryByRole('button', { name: /close/i })).toBeNull();
  });

  it('ESC dismisses when topmost', () => {
    const fn = vi.fn();
    render(
      <ModalShell onDismiss={fn} id="m1">
        <p>x</p>
      </ModalShell>,
    );
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it('ESC does NOT dismiss when not topmost', () => {
    const onA = vi.fn();
    const onB = vi.fn();
    render(
      <>
        <ModalShell onDismiss={onA} id="a"><p>a</p></ModalShell>
        <ModalShell onDismiss={onB} id="b"><p>b</p></ModalShell>
      </>,
    );
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onA).not.toHaveBeenCalled();
    expect(onB).toHaveBeenCalledTimes(1);
  });

  it('backdrop click dismisses when topmost', () => {
    const fn = vi.fn();
    render(
      <ModalShell onDismiss={fn} id="m1">
        <p>x</p>
      </ModalShell>,
    );
    fireEvent.click(screen.getByTestId('modal-backdrop'));
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it('content click does NOT dismiss', () => {
    const fn = vi.fn();
    render(
      <ModalShell onDismiss={fn} id="m1">
        <p data-testid="content">x</p>
      </ModalShell>,
    );
    fireEvent.click(screen.getByTestId('content'));
    expect(fn).not.toHaveBeenCalled();
  });

  it('title-bar close button calls onDismiss', () => {
    const fn = vi.fn();
    render(
      <ModalShell onDismiss={fn} id="m1" title="Hi">
        <p>x</p>
      </ModalShell>,
    );
    fireEvent.click(screen.getByRole('button', { name: /close/i }));
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it('dismissable=false: ESC, backdrop click, and title-X are all no-ops', () => {
    const fn = vi.fn();
    render(
      <ModalShell onDismiss={fn} id="m1" title="Hi" dismissable={false}>
        <p>x</p>
      </ModalShell>,
    );
    fireEvent.keyDown(window, { key: 'Escape' });
    fireEvent.click(screen.getByTestId('modal-backdrop'));
    expect(fn).not.toHaveBeenCalled();
    expect(screen.queryByRole('button', { name: /close/i })).toBeNull();
  });
});
