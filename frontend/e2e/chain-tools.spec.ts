import { expect, test } from "@playwright/test";
import { login, mockApi } from "./mocks.ts";

test("router card posts a notional and shows the venue choice", async ({ page }) => {
  await mockApi(page);
  await page.route("**/v1/route", async (r) => {
    const body = r.request().postDataJSON() as { notional: string };
    expect(body.notional).toBe("500");
    await r.fulfill({ json: { venue: "amm", reason: "no RFQ threshold configured" } });
  });
  await login(page);
  await page.getByPlaceholder("e.g. 500").fill("500");
  await page.getByRole("button", { name: "Check venue" }).click();
  await expect(page.getByText("AMM — no RFQ threshold configured")).toBeVisible();
});

test("reconcile card submits the form and renders a confirmed result", async ({ page }) => {
  await mockApi(page);
  await page.route("**/v1/reconcile", async (r) => {
    const body = r.request().postDataJSON() as { tx_hash: string; side: string };
    expect(body.tx_hash).toBe("0xabc");
    expect(body.side).toBe("buy");
    await r.fulfill({
      json: {
        status: "confirmed",
        block_number: 123,
        gas_used: 21000,
        recorded: true,
        note: null,
      },
    });
  });
  await login(page);
  await page.getByPlaceholder("0x…").first().fill("0xabc");
  await page.getByPlaceholder("symbol", { exact: true }).fill("NVDA");
  await page.getByPlaceholder("qty").fill("1");
  await page.getByPlaceholder("price").fill("200");
  await page.getByRole("button", { name: "Reconcile" }).click();
  await expect(page.getByText("confirmed")).toBeVisible();
  await expect(page.getByText("123", { exact: false })).toBeVisible();
});

test("dex-simulate card submits the form and renders the outcome", async ({ page }) => {
  await mockApi(page);
  await page.route("**/v1/dex/simulate", async (r) => {
    await r.fulfill({
      json: {
        token_symbol: "NVDA",
        denom_symbol: "USDG",
        pool_fee: 3000,
        pool_tick_spacing: 60,
        pool_liquidity: "123456",
        amount_out_minimum: "995000",
        calldata_hex: "0xdead",
        ok: true,
        detail: "eth_call succeeded",
      },
    });
  });
  await login(page);
  await page.getByPlaceholder("0x…").nth(1).fill("0xfrom");
  await page.getByPlaceholder("token (symbol or address)").fill("NVDA");
  await page.getByPlaceholder("amount in (raw base units)").fill("1000000");
  await page.getByRole("button", { name: "Simulate swap" }).click();
  await expect(page.getByText("succeeded", { exact: true })).toBeVisible();
  await expect(page.getByText("eth_call succeeded")).toBeVisible();
});

test("a 404 (feature not configured) surfaces as an error, not a crash", async ({ page }) => {
  await mockApi(page);
  await page.route("**/v1/route", (r) =>
    r.fulfill({ status: 404, json: { code: "not_found", message: "no router configured" } }),
  );
  await login(page);
  await page.getByPlaceholder("e.g. 500").fill("500");
  await page.getByRole("button", { name: "Check venue" }).click();
  await expect(page.getByText("not_found: no router configured")).toBeVisible();
});
