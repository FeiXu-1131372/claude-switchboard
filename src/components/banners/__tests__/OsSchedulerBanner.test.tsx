import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { OsSchedulerBanner } from "../OsSchedulerBanner";

describe("OsSchedulerBanner", () => {
  it("renders nothing when OS-level is registered", () => {
    const { container } = render(
      <OsSchedulerBanner registered={true} onEnable={() => {}} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders the warning text and Enable link when not registered", () => {
    render(
      <OsSchedulerBanner registered={false} onEnable={() => {}} />,
    );
    expect(
      screen.getByText(/Schedules only fire while the app is open/i),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /enable os-level/i })).toBeInTheDocument();
  });

  it("calls onEnable when the link is clicked", () => {
    const fn = vi.fn();
    render(<OsSchedulerBanner registered={false} onEnable={fn} />);
    fireEvent.click(screen.getByRole("button", { name: /enable os-level/i }));
    expect(fn).toHaveBeenCalledTimes(1);
  });
});
