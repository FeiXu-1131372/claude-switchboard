import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { SettingsModal } from '../SettingsModal';
import { useAppStore } from '../../../lib/store';

vi.mock('../../../settings/SettingsPanel', () => ({
  SettingsPanel: () => <div data-testid="settings-panel-content">panel</div>,
}));

describe('SettingsModal', () => {
  beforeEach(() => {
    useAppStore.getState().resetModalStack();
  });

  it('renders SettingsPanel content and Settings title', () => {
    render(<SettingsModal onDismiss={() => {}} />);
    expect(screen.getByTestId('settings-panel-content')).toBeInTheDocument();
    expect(screen.getByText(/^settings$/i)).toBeInTheDocument();
  });

  it('calls onDismiss when the title-bar close button is clicked', () => {
    const fn = vi.fn();
    render(<SettingsModal onDismiss={fn} />);
    fireEvent.click(screen.getByRole('button', { name: /close/i }));
    expect(fn).toHaveBeenCalledTimes(1);
  });
});
