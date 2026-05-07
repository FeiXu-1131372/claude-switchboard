import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { WarmupSettings } from "../WarmupSettings";

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
    expect(screen.getByText(/os-level scheduler: registered/i)).toBeInTheDocument();
  });

  it("calls onRevoke when the revoke button is clicked", () => {
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
});
