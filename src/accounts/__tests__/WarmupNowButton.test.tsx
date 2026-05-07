import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { WarmupNowButton } from "../WarmupNowButton";

describe("WarmupNowButton", () => {
  it("renders the label and is enabled when warm-up is on", () => {
    render(<WarmupNowButton enabled={true} onClick={() => {}} />);
    const btn = screen.getByRole("button", { name: /warm up now/i });
    expect(btn).not.toBeDisabled();
  });

  it("is disabled when warm-up is off", () => {
    render(<WarmupNowButton enabled={false} onClick={() => {}} />);
    expect(screen.getByRole("button", { name: /warm up now/i })).toBeDisabled();
  });

  it("calls onClick when clicked while enabled", () => {
    const fn = vi.fn();
    render(<WarmupNowButton enabled={true} onClick={fn} />);
    fireEvent.click(screen.getByRole("button", { name: /warm up now/i }));
    expect(fn).toHaveBeenCalledTimes(1);
  });
});
