import { expect, test } from "@playwright/test";
import { login, mockApi } from "./mocks.ts";

test("renders the portfolio card from /v1/portfolio", async ({ page }) => {
  await mockApi(page);
  await login(page);
  await expect(page.locator(".stat-value")).toHaveText("983.612898");
  await expect(page.getByText("realized")).toBeVisible();
  await expect(page.getByText("Open positions")).toBeVisible();
});

test("renders the activity feed from /v1/activity", async ({ page }) => {
  await mockApi(page);
  await login(page);
  await expect(page.getByText("Fills recorded")).toBeVisible();
  await expect(page.getByText("chain ok")).toBeVisible();
  await expect(page.getByText(/BUY .* HMNI/)).toBeVisible();
  await expect(page.getByText(/SELL .* HMNI/)).toBeVisible();
});

test("renders the cash-over-time chart once there are 2+ fills", async ({ page }) => {
  await mockApi(page);
  await login(page);
  await expect(page.getByText("Cash over time")).toBeVisible();
  await expect(page.locator("svg.curve-svg")).toBeVisible();
  await expect(page.locator("svg.curve-svg path[fill^='url']")).toHaveCount(1);
  await expect(page.getByText("recent fills, client-derived — not equity")).toBeVisible();
});

test("cash-over-time shows an empty state with fewer than 2 fills", async ({ page }) => {
  await mockApi(page);
  await page.route("**/v1/activity**", (r) => r.fulfill({ json: { recent: [], fills: 0 } }));
  await login(page);
  await expect(page.getByText("Not enough fills in the recent window to chart yet.")).toBeVisible();
  await expect(page.locator("svg.curve-svg")).toHaveCount(0);
});

test("portfolio's 404 empty state matches the no-state-path message", async ({ page }) => {
  await mockApi(page);
  await page.route("**/v1/portfolio", (r) =>
    r.fulfill({ status: 404, json: { code: "not_found", message: "no state" } }),
  );
  await login(page);
  await expect(page.getByText("No persisted state. Run")).toBeVisible();
});
