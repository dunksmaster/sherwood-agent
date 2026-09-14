import { expect, test } from "@playwright/test";
import { login, mockApi } from "./mocks.ts";

test("admin actions are disabled until a reauth token is entered", async ({ page }) => {
  await mockApi(page);
  await login(page);
  await expect(page.getByRole("button", { name: "Engage kill switch" })).toBeDisabled();
  await page.getByPlaceholder("paste the admin token").fill("admin-reauth");
  await expect(page.getByRole("button", { name: "Engage kill switch" })).toBeEnabled();
});

test("engaging the kill switch calls POST /v1/kill and updates the badge", async ({ page }) => {
  await mockApi(page);
  let sawEngage = false;
  await page.route("**/v1/kill", async (r) => {
    const body = r.request().postDataJSON() as { engage: boolean; reauth: string };
    sawEngage = body.engage === true && body.reauth === "admin-reauth";
    await r.fulfill({ json: { mode: "paper", kill_switch: true } });
  });
  await page.route("**/v1/health", (r) =>
    r.fulfill({ json: { status: "ok", mode: "paper", kill_switch: sawEngage, uptime_secs: 1 } }),
  );
  await login(page);
  await page.getByPlaceholder("paste the admin token").fill("admin-reauth");
  await page.getByRole("button", { name: "Engage kill switch" }).click();
  await expect(page.getByText("KILL SWITCH ENGAGED")).toBeVisible();
  expect(sawEngage).toBe(true);
});

// Regresses the exact scenario hit interactively this session: attempting to
// arm LIVE with `[server] allow_live = false` in config.toml.
test("switching to LIVE surfaces the server's forbidden message when allow_live is false", async ({
  page,
}) => {
  await mockApi(page);
  await page.route("**/v1/mode", (r) =>
    r.fulfill({
      status: 403,
      json: { code: "forbidden", message: "live mode is disabled in config (`[server] allow_live = false`)" },
    }),
  );
  await login(page);
  await page.getByPlaceholder("paste the admin token").fill("admin-reauth");
  await page.getByRole("button", { name: "Switch to LIVE" }).click();
  await expect(page.getByText("forbidden: live mode is disabled in config")).toBeVisible();
  await expect(page.getByText("PAPER", { exact: true })).toBeVisible();
});
