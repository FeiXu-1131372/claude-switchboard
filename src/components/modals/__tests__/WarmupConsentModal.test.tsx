import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { WarmupConsentModal } from "../WarmupConsentModal";
import { useAppStore } from "../../../lib/store";

describe("WarmupConsentModal", () => {
  beforeEach(() => {
    useAppStore.getState().resetModalStack();
  });

  it("renders the consent text and two action buttons", () => {
    render(<WarmupConsentModal onAccept={() => {}} onDismiss={() => {}} />);
    expect(
      screen.getByText(/Warm-up sends messages on your behalf/i),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /enable warm-up/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /don't enable/i }),
    ).toBeInTheDocument();
  });

  it("calls onAccept when 'Enable warm-up' is clicked", () => {
    const onAccept = vi.fn();
    render(<WarmupConsentModal onAccept={onAccept} onDismiss={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: /enable warm-up/i }));
    expect(onAccept).toHaveBeenCalledTimes(1);
  });

  it("calls onDismiss when 'Don't enable' is clicked", () => {
    const onDismiss = vi.fn();
    render(<WarmupConsentModal onAccept={() => {}} onDismiss={onDismiss} />);
    fireEvent.click(screen.getByRole("button", { name: /don't enable/i }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("ESC does NOT dismiss (consent is forced)", () => {
    const onDismiss = vi.fn();
    render(<WarmupConsentModal onAccept={() => {}} onDismiss={onDismiss} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("backdrop click does NOT dismiss (consent is forced)", () => {
    const onDismiss = vi.fn();
    render(<WarmupConsentModal onAccept={() => {}} onDismiss={onDismiss} />);
    fireEvent.click(screen.getByTestId("modal-backdrop"));
    expect(onDismiss).not.toHaveBeenCalled();
  });
});
