import { useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";

/**
 * `setToken` is passed down from the single `useToken()` call at the top of
 * the route tree (`router.tsx`) — this hook holds plain `useState`, not
 * context, so calling it again here would create a second, disconnected copy
 * of the token instead of sharing the one every other route reads.
 */
export function LoginPage({ setToken }: { setToken: (t: string) => void }) {
  const [v, setV] = useState("");
  const navigate = useNavigate();
  const location = useLocation();
  const from = (location.state as { from?: string } | null)?.from ?? "/app";

  function onSubmit(token: string) {
    setToken(token);
    navigate(from, { replace: true });
  }

  return (
    <div className="wrap login">
      <div className="card">
        <div className="brand">
          <span className="brand-mark" aria-hidden="true">
            &gt;
          </span>
          <h1>sherwood</h1>
        </div>
        <p className="muted">
          Paste an API token to connect. It is held only for this tab.
        </p>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (v.trim()) onSubmit(v.trim());
          }}
        >
          <input
            type="password"
            value={v}
            autoComplete="off"
            onChange={(e) => setV(e.target.value)}
            placeholder="bearer token"
          />
          <button style={{ marginTop: 12, width: "100%" }} disabled={!v.trim()}>
            Connect
          </button>
        </form>
      </div>
    </div>
  );
}
