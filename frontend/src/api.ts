// Typed client for sherwood-server. Every failure surfaces as ApiError with the
// server's { code, message, correlation_id } envelope.

export type Mode = "paper" | "live";

export interface Health {
  status: string;
  mode: Mode;
  kill_switch: boolean;
  uptime_secs: number;
}

export interface ControlView {
  mode: Mode;
  kill_switch: boolean;
}

export interface Position {
  symbol: string;
  quantity: string;
  avg_cost: string | null;
}

export interface PortfolioView {
  cash: string;
  realized_pnl: string;
  open_positions: number;
  positions: Position[];
}

export interface AuditEvent {
  seq: number;
  at: string;
  kind: string;
  data: unknown;
  prev_hash: string;
  hash: string;
}

export interface ActivityView {
  recent: AuditEvent[];
  fills: number;
}

export interface AuditVerifyView {
  ok: boolean;
  entries: number | null;
  broken_at: number | null;
}

export type ApprovalState = "pending" | "approved" | "denied" | "expired";
export type ApprovalMode = "auto" | "manual";

export interface Approval {
  id: string;
  state: ApprovalState;
  tool: string;
  order: {
    symbol: string;
    side: "buy" | "sell";
    quantity: string;
    limit_price: string | null;
    reason: string;
  };
  created_at: string;
  decided_at: string | null;
  decision_reason: string | null;
}

export interface ApprovalsView {
  mode: ApprovalMode;
  pending: number;
  approvals: Approval[];
}

export interface SessionView {
  orders_used: number;
  orders_cap: number;
  notional_used: string;
  notional_cap: string;
  elapsed_secs: number;
  duration_cap_secs: number;
  breached: boolean;
}

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
    readonly correlationId?: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

async function req<T>(
  path: string,
  token: string,
  init?: RequestInit,
): Promise<T> {
  let resp: Response;
  try {
    resp = await fetch(path, {
      ...init,
      headers: {
        ...(init?.body ? { "content-type": "application/json" } : {}),
        authorization: `Bearer ${token}`,
        ...init?.headers,
      },
    });
  } catch (e) {
    throw new ApiError(0, "network", `cannot reach the server (${String(e)})`);
  }

  const text = await resp.text();
  const body: unknown = text ? JSON.parse(text) : null;

  if (!resp.ok) {
    const env = body as {
      code?: string;
      message?: string;
      correlation_id?: string;
    } | null;
    throw new ApiError(
      resp.status,
      env?.code ?? "error",
      env?.message ?? resp.statusText,
      env?.correlation_id,
    );
  }
  return body as T;
}

export const api = {
  health: (t: string) => req<Health>("/v1/health", t),
  control: (t: string) => req<ControlView>("/v1/control", t),
  portfolio: (t: string) => req<PortfolioView>("/v1/portfolio", t),
  activity: (t: string, limit = 25) =>
    req<ActivityView>(`/v1/activity?limit=${limit}`, t),
  auditVerify: (t: string) => req<AuditVerifyView>("/v1/audit/verify", t),
  setKill: (t: string, engage: boolean, reauth: string) =>
    req<ControlView>("/v1/kill", t, {
      method: "POST",
      body: JSON.stringify({ engage, reauth }),
    }),
  setMode: (t: string, mode: Mode, reauth: string) =>
    req<ControlView>("/v1/mode", t, {
      method: "POST",
      body: JSON.stringify({ mode, reauth }),
    }),
  approvals: (t: string) => req<ApprovalsView>("/v1/approvals", t),
  session: (t: string) => req<SessionView>("/v1/session", t),
  resetSession: (t: string, reauth: string) =>
    req<SessionView>("/v1/session/reset", t, {
      method: "POST",
      body: JSON.stringify({ reauth }),
    }),
  decideApproval: (
    t: string,
    id: string,
    decision: "approve" | "deny",
    reason?: string,
  ) =>
    req<Approval>(`/v1/approvals/${encodeURIComponent(id)}`, t, {
      method: "POST",
      body: JSON.stringify({ decision, reason }),
    }),
};

// `/v1/health` needs no auth, but sending the token anyway is harmless and lets
// one code path serve every call.
export function fmtMoney(s: string): string {
  const n = Number(s);
  if (!Number.isFinite(n)) return s;
  return n.toLocaleString(undefined, {
    minimumFractionDigits: 2,
    maximumFractionDigits: 6,
  });
}
