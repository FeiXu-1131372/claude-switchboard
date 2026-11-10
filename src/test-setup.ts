import "@testing-library/jest-dom/vitest";
import { beforeEach } from "vitest";
import { clearTabDataCache } from "./lib/useTabData";
import { rehydrateReportStore } from "./report/reportState";

// The report view store and the tab data cache are module-level on purpose:
// they are what lets a tab come back the way it was left. Between tests that
// is a leak, so every test starts from a fresh start of the app.
beforeEach(() => {
  localStorage.clear();
  rehydrateReportStore();
  clearTabDataCache();
});
