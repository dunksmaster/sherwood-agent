import type { Page } from "@playwright/test";

export const ADMIN_TOKEN = "e2e-admin-token";

const health = { status: "ok", mode: "paper" as const, kill_switch: false, uptime_secs: 137 };

const portfolio = {
  cash: "983.612898",
  realized_pnl: "-16.33703",
  open_positions: 0,
  positions: [],
};

const fillEvent = (seq: number, side: "buy" | "sell", price: string) => ({
  seq,
  at: new Date(Date.UTC(2026, 8, 14, 9, 12, seq)).toISOString(),
  kind: "fill",
  data: { order_id: `o-${seq}`, symbol: "HMNI", side, qty: "4.5", price, fee: "0.05" },
  prev_hash: "a".repeat(64),
  hash: "b".repeat(64),
});

const activity = {
  recent: [fillEvent(1, "buy", "10.80"), fillEvent(2, "sell", "9.49")],
  fills: 2,
};

const auditVerify = { ok: true, entries: 9, broken_at: null };
const approvals = { mode: "auto" as const, pending: 0, approvals: [] };
const session = {
  orders_used: 0,
  orders_cap: 0,
  notional_used: "0",
  notional_cap: "0",
  elapsed_secs: 0,
  duration_cap_secs: 0,
  breached: false,
};

/** Stubs every `/v1/*` call the dashboard makes with steady-state paper data.
 * Individual tests override specific routes after calling this. */
export async function mockApi(page: Page): Promise<void> {
  await page.route("**/v1/health", (r) => r.fulfill({ json: health }));
  await page.route("**/v1/portfolio", (r) => r.fulfill({ json: portfolio }));
  await page.route("**/v1/activity**", (r) => r.fulfill({ json: activity }));
  await page.route("**/v1/audit/verify", (r) => r.fulfill({ json: auditVerify }));
  await page.route("**/v1/approvals", (r) => r.fulfill({ json: approvals }));
  await page.route("**/v1/session", (r) => r.fulfill({ json: session }));
  // The dashboard reads this as an SSE stream; an immediately-closed body is
  // a valid empty stream and the UI falls back to `activity.recent`.
  await page.route("**/v1/events", (r) =>
    r.fulfill({ status: 200, contentType: "text/event-stream", body: "" }),
  );
}

export async function login(page: Page): Promise<void> {
  await page.goto("/");
  await page.getByPlaceholder("bearer token").fill(ADMIN_TOKEN);
  await page.getByRole("button", { name: "Connect" }).click();
  await page.getByText("Portfolio").waitFor();
}
