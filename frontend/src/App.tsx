import { useCallback, useState } from "react";
import { api } from "./api.ts";
import { useToken } from "./hooks/useToken.ts";
import { usePoll } from "./hooks/usePoll.ts";
import { StatusBar } from "./views/StatusBar.tsx";
import { PortfolioCard } from "./views/PortfolioCard.tsx";
import { ActivityList } from "./views/ActivityList.tsx";
import { Controls } from "./views/Controls.tsx";

const POLL_MS = 4000;

function Login({ onSubmit }: { onSubmit: (t: string) => void }) {
  const [v, setV] = useState("");
  return (
    <div className="wrap login">
      <h1>sherwood control plane</h1>
      <p className="muted">
        Paste an API token. It is held only for this tab.
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
        <button style={{ marginTop: 12 }} disabled={!v.trim()}>
          Connect
        </button>
      </form>
    </div>
  );
}

function Dashboard({ token, onLogout }: { token: string; onLogout: () => void }) {
  const onUnauth = useCallback(() => onLogout(), [onLogout]);

  const health = usePoll(() => api.health(token), POLL_MS, onUnauth);
  const portfolio = usePoll(() => api.portfolio(token), POLL_MS, onUnauth);
  const activity = usePoll(() => api.activity(token, 25), POLL_MS, onUnauth);
  const audit = usePoll(() => api.auditVerify(token), POLL_MS * 4, onUnauth);

  const refreshControls = useCallback(() => {
    health.refresh();
  }, [health]);

  return (
    <div className="wrap">
      <StatusBar health={health.data} />
      {health.error && health.error.status !== 401 && (
        <p className="err">
          {health.error.message}
          {health.error.correlationId ? ` (${health.error.correlationId})` : ""}
        </p>
      )}
      <div className="grid">
        <PortfolioCard data={portfolio.data} error={portfolio.error} />
        <ActivityList
          data={activity.data}
          error={activity.error}
          audit={audit.data}
        />
        <Controls
          token={token}
          health={health.data}
          onChanged={refreshControls}
        />
        <div className="card">
          <h2>Session</h2>
          <p className="muted">Token held in this tab only.</p>
          <button onClick={onLogout}>Disconnect</button>
        </div>
      </div>
    </div>
  );
}

export function App() {
  const { token, setToken, clear } = useToken();
  return token ? (
    <Dashboard token={token} onLogout={clear} />
  ) : (
    <Login onSubmit={setToken} />
  );
}
