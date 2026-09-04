import { useState } from "react";
import {
  api,
  ApiError,
  type Approval,
  type ApprovalsView,
  fmtMoney,
} from "../api.ts";

function stateBadge(s: Approval["state"]) {
  const cls =
    s === "approved" ? "badge ok" : s === "pending" ? "badge" : "badge kill";
  return (
    <span className={cls} style={{ marginLeft: 8 }}>
      {s}
    </span>
  );
}

function Card({
  a,
  token,
  onDone,
}: {
  a: Approval;
  token: string;
  onDone: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const pending = a.state === "pending";

  async function decide(decision: "approve" | "deny") {
    setBusy(true);
    setErr(null);
    try {
      await api.decideApproval(token, a.id, decision);
      onDone();
    } catch (e) {
      setErr(e instanceof ApiError ? `${e.code}: ${e.message}` : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div
      className="kv"
      style={{ flexDirection: "column", alignItems: "stretch", gap: 6 }}
    >
      <div className="row" style={{ justifyContent: "space-between" }}>
        <span className="mono">
          {a.order.side.toUpperCase()} {a.order.quantity} {a.order.symbol}
          {a.order.limit_price ? ` @ ${fmtMoney(a.order.limit_price)}` : ""}
        </span>
        {stateBadge(a.state)}
      </div>
      <span className="muted" style={{ fontSize: 12 }}>
        {a.order.reason} · {new Date(a.created_at).toLocaleTimeString()}
        {a.decision_reason ? ` · "${a.decision_reason}"` : ""}
      </span>
      {pending && (
        <div className="row" style={{ marginTop: 4 }}>
          <button disabled={busy} onClick={() => void decide("approve")}>
            Approve
          </button>
          <button
            className="danger"
            disabled={busy}
            onClick={() => void decide("deny")}
          >
            Deny
          </button>
        </div>
      )}
      {err && <p className="err">{err}</p>}
    </div>
  );
}

export function ApprovalsCard({
  data,
  error,
  token,
  onDecided,
}: {
  data: ApprovalsView | null;
  error: ApiError | null;
  token: string;
  onDecided: () => void;
}) {
  return (
    <div className="card">
      <h2>
        Approvals
        {data && (
          <span className="badge" style={{ marginLeft: 8 }}>
            {data.mode}
            {data.pending > 0 ? ` · ${data.pending} pending` : ""}
          </span>
        )}
      </h2>
      {error && error.status !== 401 && <p className="err">{error.message}</p>}
      {data?.mode === "auto" && (
        <p className="muted">
          Auto mode — the risk gate decides, nothing waits here. Set{" "}
          <span className="mono">approval_mode = "manual"</span> to review orders.
        </p>
      )}
      {data && data.approvals.length === 0 && data.mode === "manual" && (
        <p className="muted">No orders yet.</p>
      )}
      {data?.approvals.map((a) => (
        <Card key={a.id} a={a} token={token} onDone={onDecided} />
      ))}
    </div>
  );
}
