import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ImportTokenPanel } from '../ImportTokenPanel';
import type { AccountListEntry } from '../../lib/generated/bindings';

const mocks = vi.hoisted(() => ({
  importLongLivedToken: vi.fn(),
  refreshAccounts: vi.fn(),
}));
vi.mock('../../lib/ipc', () => ({ ipc: mocks }));
vi.mock('../../lib/store', () => ({
  useAppStore: (select: (state: { refreshAccounts: typeof mocks.refreshAccounts }) => unknown) =>
    select({ refreshAccounts: mocks.refreshAccounts }),
}));
const token = 'sk-ant-oat01-synthetic_token_for_tests_only';

describe('ImportTokenPanel', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mocks.importLongLivedToken.mockResolvedValue(3);
    mocks.refreshAccounts.mockResolvedValue(undefined);
  });

  it('needs only a masked token and does not require identity or expiry fields', async () => {
    const success = vi.fn();
    render(<ImportTokenPanel onBack={vi.fn()} onSuccess={success} />);
    const input = screen.getByLabelText('Long-lived token');
    expect(input).toHaveAttribute('type', 'password');
    expect(input).toHaveAttribute('autocomplete', 'off');
    expect(screen.getByRole('button', { name: 'Add account' })).toBeDisabled();
    fireEvent.change(input, { target: { value: ` ${token} ` } });
    fireEvent.click(screen.getByRole('button', { name: 'Add account' }));
    await waitFor(() => expect(success).toHaveBeenCalledOnce());
    expect(mocks.importLongLivedToken).toHaveBeenCalledWith(token, null, null);
    expect(input).toHaveValue('');
    expect(mocks.refreshAccounts).toHaveBeenCalledOnce();
  });

  it('accepts an optional label and prevents another submission while saving', async () => {
    let finish!: (slot: number) => void;
    mocks.importLongLivedToken.mockImplementationOnce(() => new Promise<number>((resolve) => { finish = resolve; }));
    const busy = vi.fn();
    render(<ImportTokenPanel onBack={vi.fn()} onSuccess={vi.fn()} onBusyChange={busy} />);
    fireEvent.change(screen.getByLabelText('Long-lived token'), { target: { value: token } });
    fireEvent.change(screen.getByLabelText('Label (optional)'), { target: { value: ' Work ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add account' }));
    expect(screen.getByRole('button', { name: 'Importing…' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Back' })).toBeDisabled();
    expect(mocks.importLongLivedToken).toHaveBeenCalledWith(token, 'Work', null);
    finish(3);
    await waitFor(() => expect(busy).toHaveBeenLastCalledWith(false));
    expect(busy).toHaveBeenCalledWith(true);
  });

  it('clears the input and never renders a token echoed in an error', async () => {
    mocks.importLongLivedToken.mockRejectedValueOnce(new Error(`Rejected ${token}`));
    render(<ImportTokenPanel onBack={vi.fn()} onSuccess={vi.fn()} />);
    const input = screen.getByLabelText('Long-lived token');
    fireEvent.change(input, { target: { value: token } });
    fireEvent.click(screen.getByRole('button', { name: 'Add account' }));
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('[redacted token]'));
    expect(screen.getByRole('alert').textContent).not.toContain(token);
    expect(input).toHaveValue('');
    expect(mocks.refreshAccounts).not.toHaveBeenCalled();
  });

  it('going back does not import anything', () => {
    const back = vi.fn();
    render(<ImportTokenPanel onBack={back} onSuccess={vi.fn()} />);
    fireEvent.change(screen.getByLabelText('Long-lived token'), { target: { value: token } });
    fireEvent.click(screen.getByRole('button', { name: 'Back' }));
    expect(back).toHaveBeenCalledOnce();
    expect(mocks.importLongLivedToken).not.toHaveBeenCalled();
  });

  it('replaces only the selected inactive token account', async () => {
    const success = vi.fn();
    const account = { slot: 7, email: 'Shared', source: 'LongLivedToken', is_active: false } as AccountListEntry;
    render(<ImportTokenPanel account={account} onBack={vi.fn()} onSuccess={success} />);
    fireEvent.change(screen.getByLabelText('Long-lived token'), { target: { value: token } });
    fireEvent.click(screen.getByRole('button', { name: 'Replace token' }));
    await waitFor(() => expect(success).toHaveBeenCalledOnce());
    expect(mocks.importLongLivedToken).toHaveBeenCalledWith(token, 'Shared', 7);
  });
});
