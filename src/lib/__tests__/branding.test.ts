import { describe, it, expect } from "vitest";
import * as branding from "../branding";

describe("branding (frontend mirror of Rust branding.rs)", () => {
  it("exports the new product name and identifiers", () => {
    expect(branding.PRODUCT_NAME).toBe("Claude Switchboard");
    expect(branding.TAURI_BUNDLE_ID).toBe("com.claude-switchboard.app");
    expect(branding.GITHUB_REPO_PATH).toBe(
      "FeiXu-1131372/claude-switchboard",
    );
  });

  it("exports the legacy product name (used in migration UI copy)", () => {
    expect(branding.LEGACY_PRODUCT_NAME).toBe("Claude Limits");
  });
});
