import { expect, test } from "@playwright/test";
import { ADMIN_TOKEN, login, mockApi } from "./mocks.ts";

test("shows the token gate before connecting", async ({ page }) => {
  await mockApi(page);
  await page.goto("/");
  await expect(page.getByText("sherwood")).toBeVisible();
  await expect(page.getByPlaceholder("bearer token")).toBeVisible();
  await expect(page.getByRole("button", { name: "Connect" })).toBeDisabled();
  await expect(page.getByText("Portfolio")).toHaveCount(0);
});

test("connects with a token and reaches the dashboard", async ({ page }) => {
  await mockApi(page);
  await login(page);
  await expect(page.getByText("control plane")).toBeVisible();
  await expect(page.getByText("PAPER", { exact: true })).toBeVisible();
});

test("survives a reload — the token is held in sessionStorage", async ({ page }) => {
  await mockApi(page);
  await login(page);
  await page.reload();
  await expect(page.getByText("Portfolio")).toBeVisible();
  await expect(page.getByPlaceholder("bearer token")).toHaveCount(0);
});

test("a 401 on the first poll drops back to the login gate", async ({ page }) => {
  await mockApi(page);
  await page.route("**/v1/health", (r) =>
    r.fulfill({ status: 401, json: { code: "unauthorized", message: "bad token" } }),
  );
  await page.goto("/");
  await page.getByPlaceholder("bearer token").fill(ADMIN_TOKEN);
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.getByPlaceholder("bearer token")).toBeVisible();
});
