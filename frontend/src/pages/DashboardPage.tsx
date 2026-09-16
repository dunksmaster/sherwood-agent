import { useCallback, useState } from "react";
import { api, type AuditEvent } from "../api.ts";
import { usePoll } from "../hooks/usePoll.ts";
import { useAuditStream } from "../hooks/useAuditStream.ts";
import { StatusBar } from "../views/StatusBar.tsx";
import { PortfolioCard } from "../views/PortfolioCard.tsx";
import { CashCurve } from "../views/CashCurve.tsx";
import { ActivityList } from "../views/ActivityList.tsx";
import { ApprovalsCard } from "../views/ApprovalsCard.tsx";
import { Controls } from "../views/Controls.tsx";
import { DexSimulateCard, ReconcileCard, RouteCard } from "../views/ChainTools.tsx";

const POLL_MS = 4000;
const MAX_EVENTS = 200;

export function DashboardPage({ token, onLogout }: { token: string; onLogout: () => void }) {
  const onUnauth = useCallback(() => onLogout(), [onLogout]);

  const health = usePoll(() => api.health(token), POLL_MS, onUnauth);
  const portfolio = usePoll(() => api.portfolio(token), POLL_MS, onUnauth);
  // The audit rows come over SSE; this poll only needs the fill count now, so
  // it can be slow. It also doubles as the fallback if the stream is down.
  const activity = usePoll(() => api.activity(token, 25), POLL_MS * 3, onUnauth);
  const audit = usePoll(() => api.auditVerify(token), POLL_MS * 4, onUnauth);
  // Approvals are rare but time-sensitive when they appear — poll fast.
  const approvals = usePoll(() => api.approvals(token), 2000, onUnauth);
  const session = usePoll(() => api.session(token), POLL_MS, onUnauth);

  const [events, setEvents] = useState<AuditEvent[]>([]);
  const onBatch = useCallback((rows: AuditEvent[]) => {
    setEvents((prev) => {
      const seen = new Set(prev.map((e) => e.seq));
      const merged = [...prev, ...rows.filter((r) => !seen.has(r.seq))];
      merged.sort((a, b) => a.seq - b.seq);
      return merged.slice(-MAX_EVENTS);
    });
  }, []);
  useAuditStream(token, onBatch, onUnauth);

  const refreshControls = useCallback(() => {
    health.refresh();
    session.refresh();
  }, [health, session]);

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
        <div className="area-portfolio">
          <PortfolioCard data={portfolio.data} error={portfolio.error} />
        </div>
        <div className="area-approvals">
          <ApprovalsCard
            data={approvals.data}
            error={approvals.error}
            token={token}
            onDecided={approvals.refresh}
          />
        </div>
        <div className="area-curve">
          <CashCurve
            events={events}
            data={activity.data}
            portfolio={portfolio.data}
            error={activity.error ?? portfolio.error}
          />
        </div>
        <div className="area-activity">
          <ActivityList
            events={events}
            data={activity.data}
            error={activity.error}
            audit={audit.data}
          />
        </div>
        <div className="area-route">
          <RouteCard token={token} />
        </div>
        <div className="area-reconcile">
          <ReconcileCard token={token} />
        </div>
        <div className="area-dexsim">
          <DexSimulateCard token={token} />
        </div>
        <div className="area-controls">
          <Controls
            token={token}
            health={health.data}
            session={session.data}
            onChanged={refreshControls}
          />
        </div>
      </div>
    </div>
  );
}
