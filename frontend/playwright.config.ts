import { defineConfig, devices } from "@playwright/test";

/**
 * E2E suite for the dashboard only — every `/v1/*` call is mocked via
 * `page.route` (see `e2e/mocks.ts`). No `sherwood-server` process is started;
 * this proves the UI renders and behaves correctly against the API contract
 * in `../src/api.ts`, not that the real server does. Backend behavior is
 * covered by `crates/server`'s own Rust tests.
 */
export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  reporter: process.env.CI ? "github" : "list",
  use: {
    baseURL: "http://localhost:5173",
    trace: "on-first-retry",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command: "npm run dev",
    url: "http://localhost:5173",
    reuseExistingServer: !process.env.CI,
  },
});
