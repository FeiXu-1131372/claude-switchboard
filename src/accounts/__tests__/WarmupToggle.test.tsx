import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { WarmupToggle } from "../WarmupToggle";

describe("WarmupToggle", () => {
  it("renders an off-state toggle when disabled", () => {
    render(<WarmupToggle enabled={false} onToggle={() => {}} />);
    const t = screen.getByRole("switch");
    expect(t).toHaveAttribute("aria-checked", "false");
  });

  it("renders an on-state toggle when enabled", () => {
    render(<WarmupToggle enabled={true} onToggle={() => {}} />);
    expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "true");
  });

  it("invokes onToggle with the inverse value on click", () => {
    const fn = vi.fn();
    render(<WarmupToggle enabled={false} onToggle={fn} />);
    fireEvent.click(screen.getByRole("switch"));
    expect(fn).toHaveBeenCalledWith(true);
  });
});
