import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { WelcomeToSwitchboard } from "../WelcomeToSwitchboard";

describe("WelcomeToSwitchboard", () => {
  it("renders the welcome heading and migration summary", () => {
    render(
      <WelcomeToSwitchboard
        outcome={{
          legacy_data_dir_found: true,
          files_copied: 3,
          legacy_process_quit: true,
          legacy_autostart_removed: true,
        }}
        onClose={() => {}}
      />,
    );
    expect(screen.getByText(/Welcome to Claude Switchboard/i)).toBeInTheDocument();
    expect(screen.getByText(/3 files migrated/i)).toBeInTheDocument();
  });

  it("calls onClose when the dismiss button is clicked", () => {
    let closed = false;
    render(
      <WelcomeToSwitchboard
        outcome={{
          legacy_data_dir_found: true,
          files_copied: 3,
          legacy_process_quit: true,
          legacy_autostart_removed: false,
        }}
        onClose={() => {
          closed = true;
        }}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /got it/i }));
    expect(closed).toBe(true);
  });

  it("does not render when no legacy data was found (fresh install)", () => {
    const { container } = render(
      <WelcomeToSwitchboard
        outcome={{
          legacy_data_dir_found: false,
          files_copied: 0,
          legacy_process_quit: false,
          legacy_autostart_removed: false,
        }}
        onClose={() => {}}
      />,
    );
    expect(container.firstChild).toBeNull();
  });
});
