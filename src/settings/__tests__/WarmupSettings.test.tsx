import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { WarmupSettings } from "../WarmupSettings";
import { ipc } from '../../lib/ipc';

vi.mock('../../lib/ipc', () => ({ ipc: { getWarmupState: vi.fn() } }));

describe("WarmupSettings", () => {
  it("renders the consent state and OS-scheduler state", () => {
    render(
      <WarmupSettings
        consentGranted={true}
        osSchedulerRegistered={true}
        onRevoke={() => {}}
        onRegisterOs={() => {}}
        onUnregisterOs={() => {}}
      />,
    );
    expect(screen.getByText(/global consent: granted/i)).toBeInTheDocument();
    expect(screen.getByRole('switch', { name: /run in background/i })).toBeChecked();
  });

  it("calls onRevoke when the revoke button is clicked", async () => {
    const fn = vi.fn();
    render(
      <WarmupSettings
        consentGranted={true}
        osSchedulerRegistered={false}
        onRevoke={fn}
        onRegisterOs={() => {}}
        onUnregisterOs={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /revoke/i }));
    expect(fn).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.getByRole('button', { name: /revoke/i })).not.toBeDisabled());
  });

  it("hides the revoke button when consent is not granted", () => {
    render(
      <WarmupSettings
        consentGranted={false}
        osSchedulerRegistered={false}
        onRevoke={() => {}}
        onRegisterOs={() => {}}
        onUnregisterOs={() => {}}
      />,
    );
    expect(screen.queryByRole("button", { name: /revoke/i })).toBeNull();
  });

  it('shows background registration failures instead of rejecting silently', async () => {
    const register = vi.fn().mockRejectedValue(new Error('Windows task registration failed'));
    render(<WarmupSettings consentGranted osSchedulerRegistered={false} onRegisterOs={register} onUnregisterOs={() => {}} onRevoke={() => {}} />);
    fireEvent.click(screen.getByRole('switch', { name: /run in background/i }));
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('Windows task registration failed'));
  });

  it('shows next check, last success and actionable per-account errors', async () => {
    vi.mocked(ipc.getWarmupState).mockResolvedValue({
      warmup_enabled: true, schedule: { type: 'Every5h', anchor: { hour: 4, minute: 0 } },
      last_warmup_at: 1794272400, last_success_at: 1794254400, next_scheduled_at: 1794290400,
      last_result: 'needs_reauth', last_error: 'Sign in to this account again.',
    });
    render(<WarmupSettings consentGranted osSchedulerRegistered accounts={[{ account_uuid: 'fixture', email: 'Fixture account' }]} onRegisterOs={() => {}} onUnregisterOs={() => {}} onRevoke={() => {}} />);
    await waitFor(() => expect(screen.getByText('Fixture account')).toBeInTheDocument());
    expect(screen.getByText(/Next scheduled check:/)).not.toHaveTextContent('off');
    expect(screen.getByText(/Last successful warm-up:/)).not.toHaveTextContent('None recorded');
    expect(screen.getByText('Sign in to this account again.')).toBeInTheDocument();
  });
});
