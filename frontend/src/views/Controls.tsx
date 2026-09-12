import { useState } from "react";
import { api, ApiError, fmtMoney, type Health, type SessionView } from "../api.ts";

/**
 * Admin controls: the kill switch and the PAPER/LIVE toggle. Both require the
 * admin token again (server-side re-auth), so we prompt for it inline rather
 * than reuse the session token.
 */
function budgetLine(s: SessionView): string {
  const parts: string[] = [];
  if (s.orders_cap > 0) parts.push(`${s.orders_used}/${s.orders_cap} orders`);
  if (Number(s.notional_cap) > 0)
    parts.push(`${fmtMoney(s.notional_used)}/${fmtMoney(s.notional_cap)} notional`);
  if (s.duration_cap_secs > 0)
    parts.push(`${s.elapsed_secs}s/${s.duration_cap_secs}s`);
  return parts.length ? parts.join(" · ") : "no caps configured";
}

export function Controls({
  token,
  health,
  session,
  onChanged,
}: {
  token: string;
  health: Health | null;
  session: SessionView | null;
  onChanged: () => void;
}) {
  const [reauth, setReauth] = useState("");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  async function act(fn: () => Promise<unknown>) {
    setBusy(true);
    setMsg(null);
    try {
      await fn();
      setReauth("");
      onChanged();
    } catch (e) {
      setMsg(e instanceof ApiError ? `${e.code}: ${e.message}` : String(e));
    } finally {
      setBusy(false);
    }
  }

  const killed = health?.kill_switch ?? false;
  const mode = health?.mode ?? "paper";
  const canAct = reauth.length > 0 && !busy;

  return (
    <div className="card">
      <h2>
        <svg viewBox="0 0 16 16" className="h2-icon" aria-hidden="true">
          <path
            fill="currentColor"
            d="M8 1a1 1 0 0 1 1 1v.1a5.5 5.5 0 0 1 1.66.69l.07-.07a1 1 0 1 1 1.41 1.41l-.07.07c.31.5.55 1.06.69 1.66H13a1 1 0 1 1 0 2h-.1a5.5 5.5 0 0 1-.69 1.66l.07.07a1 1 0 1 1-1.41 1.41l-.07-.07a5.5 5.5 0 0 1-1.66.69V13a1 1 0 1 1-2 0v-.1a5.5 5.5 0 0 1-1.66-.69l-.07.07a1 1 0 1 1-1.41-1.41l.07-.07A5.5 5.5 0 0 1 3.1 9H3a1 1 0 1 1 0-2h.1c.14-.6.38-1.16.69-1.66l-.07-.07a1 1 0 0 1 1.41-1.41l.07.07A5.5 5.5 0 0 1 7 3.1V3a1 1 0 0 1 1-1Zm0 4.5A2.5 2.5 0 1 0 8 11a2.5 2.5 0 0 0 0-5Z"
          />
        </svg>
        Controls — admin
      </h2>
      <label className="muted" htmlFor="reauth">
        Admin token (required to confirm)
      </label>
      <input
        id="reauth"
        type="password"
        value={reauth}
        autoComplete="off"
        onChange={(e) => setReauth(e.target.value)}
        placeholder="paste the admin token"
      />
      <div className="row" style={{ marginTop: 12 }}>
        <button
          className="danger"
          disabled={!canAct}
          onClick={() => void act(() => api.setKill(token, !killed, reauth))}
        >
          {killed ? "Release kill switch" : "Engage kill switch"}
        </button>
        <button
          disabled={!canAct}
          onClick={() =>
            void act(() =>
              api.setMode(token, mode === "paper" ? "live" : "paper", reauth),
            )
          }
        >
          Switch to {mode === "paper" ? "LIVE" : "PAPER"}
        </button>
        <button
          disabled={!canAct}
          onClick={() => void act(() => api.reloadConfig(token, reauth))}
          title="Re-read config.toml — risk caps, tool allowlist, approval mode"
        >
          Reload config
        </button>
      </div>
      {msg && <p className="err">{msg}</p>}
      <p className="muted" style={{ marginTop: 12, fontSize: 12 }}>
        LIVE is refused unless the server was started with{" "}
        <span className="mono">allow_live = true</span>. The bundled runner is
        paper-only regardless.
      </p>

      {session && (
        <div className="kv" style={{ marginTop: 12 }}>
          <span className="k">
            Session budget
            {session.breached && stateBadgeBreached()}
          </span>
          <span className="row">
            <span className="mono muted" style={{ fontSize: 12 }}>
              {budgetLine(session)}
            </span>
            <button
              disabled={!canAct}
              onClick={() => void act(() => api.resetSession(token, reauth))}
            >
              Reset
            </button>
          </span>
        </div>
      )}
    </div>
  );
}

function stateBadgeBreached() {
  return (
    <span className="badge kill" style={{ marginLeft: 8 }}>
      breached
    </span>
  );
}
