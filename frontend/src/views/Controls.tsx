import { useState } from "react";
import { api, ApiError, type Health } from "../api.ts";

/**
 * Admin controls: the kill switch and the PAPER/LIVE toggle. Both require the
 * admin token again (server-side re-auth), so we prompt for it inline rather
 * than reuse the session token.
 */
export function Controls({
  token,
  health,
  onChanged,
}: {
  token: string;
  health: Health | null;
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
      <h2>Controls — admin</h2>
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
      </div>
      {msg && <p className="err">{msg}</p>}
      <p className="muted" style={{ marginTop: 12, fontSize: 12 }}>
        LIVE is refused unless the server was started with{" "}
        <span className="mono">allow_live = true</span>. The bundled runner is
        paper-only regardless.
      </p>
    </div>
  );
}
