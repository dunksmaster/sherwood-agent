import type {
  ActivityView,
  ApiError,
  AuditEvent,
  AuditVerifyView,
} from "../api.ts";

export function ActivityList({
  events,
  data,
  error,
  audit,
}: {
  /** Live rows from the SSE stream. Falls back to `data.recent` if empty. */
  events: AuditEvent[];
  data: ActivityView | null;
  error: ApiError | null;
  audit: AuditVerifyView | null;
}) {
  const rows = events.length > 0 ? events : (data?.recent ?? []);
  return (
    <div className="card">
      <h2>
        Activity
        {audit &&
          (audit.ok ? (
            <span className="badge ok" style={{ marginLeft: 8 }}>
              chain ok{audit.entries != null ? ` · ${audit.entries}` : ""}
            </span>
          ) : (
            <span className="badge kill" style={{ marginLeft: 8 }}>
              chain BROKEN @ {audit.broken_at}
            </span>
          ))}
      </h2>
      {error?.status === 404 && <p className="muted">No persisted state.</p>}
      {error && error.status !== 404 && <p className="err">{error.message}</p>}
      {(data || rows.length > 0) && (
        <>
          <div className="kv">
            <span className="k">Fills recorded</span>
            <span className="mono">{data?.fills ?? "—"}</span>
          </div>
          <div className="activity">
            {rows.length === 0 && <p className="muted">Nothing yet.</p>}
            {[...rows].reverse().map((ev) => (
              <div className="ev" key={ev.seq}>
                <span className="mono">{ev.kind}</span>
                <span className="muted mono">
                  {new Date(ev.at).toLocaleTimeString()}
                </span>
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
