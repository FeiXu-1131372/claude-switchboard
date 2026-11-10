import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { AccountRotationSettings } from '../AccountRotationSettings';
import type { AccountListEntry, RotationSettings } from '../../lib/generated/bindings';

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock('../../lib/ipc', () => ({ ipc: { getAccountRotationStatus: vi.fn().mockResolvedValue('Waiting for an available account.') } }));

const enabled: RotationSettings = {
  enabled: true, five_hour_threshold: 96, seven_day_threshold: 98, preferred_account_id: null,
};
const account = (id: string, active = false): AccountListEntry => ({
  slot: active ? 1 : 2, email: `${id}@example.com`, account_uuid: id,
  org_name: null, org_uuid: null, subscription_type: 'pro', source: 'OAuth',
  is_active: active, cached_usage: null, last_error: null,
});

describe('AccountRotationSettings', () => {
  it('keeps legacy settings disabled with the requested defaults', async () => {
    const onChange = vi.fn();
    render(<AccountRotationSettings value={null} accounts={[]} onChange={onChange} />);
    expect(screen.getByRole('switch')).not.toBeChecked();
    expect(screen.getByLabelText('5-hour rotation threshold')).toHaveValue('96');
    expect(screen.getByLabelText('Weekly rotation threshold')).toHaveValue('98');
    expect(screen.getByLabelText('Preferred next account')).toBeDisabled();
    expect(screen.getByText(/Add at least two accounts/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('switch'));
    expect(onChange).toHaveBeenCalledWith(enabled);
    await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('Waiting for an available account.'));
  });

  it('lets users adjust thresholds and choose a stable account identity', async () => {
    const onChange = vi.fn();
    render(<AccountRotationSettings value={enabled} accounts={[account('first', true), account('next')]} onChange={onChange} />);
    fireEvent.change(screen.getByLabelText('5-hour rotation threshold'), { target: { value: '94' } });
    expect(onChange).toHaveBeenLastCalledWith({ ...enabled, five_hour_threshold: 94 });
    fireEvent.change(screen.getByLabelText('Weekly rotation threshold'), { target: { value: '97' } });
    expect(onChange).toHaveBeenLastCalledWith({ ...enabled, seven_day_threshold: 97 });
    fireEvent.change(screen.getByLabelText('Preferred next account'), { target: { value: 'next' } });
    expect(onChange).toHaveBeenLastCalledWith({ ...enabled, preferred_account_id: 'next' });
    fireEvent.change(screen.getByLabelText('Preferred next account'), { target: { value: '' } });
    expect(onChange).toHaveBeenLastCalledWith(enabled);
    await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('Waiting for an available account.'));
  });

  it('explains fallback when the preferred account was deleted', async () => {
    render(<AccountRotationSettings value={{ ...enabled, preferred_account_id: 'deleted' }} accounts={[account('first')]} onChange={() => {}} />);
    expect(screen.getByLabelText('Preferred next account')).toHaveDisplayValue('Removed account — using automatic selection');
    await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('Waiting for an available account.'));
  });
});
