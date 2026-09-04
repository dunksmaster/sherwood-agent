import type { ActivityView, ApiError, AuditVerifyView } from "../api.ts";

export function ActivityList({
  data,
  error,
  audit,
}: {
  data: ActivityView | null;
  error: ApiError | null;
  audit: AuditVerifyView | null;
}) {
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
      {data && (
        <>
          <div className="kv">
            <span className="k">Fills recorded</span>
            <span className="mono">{data.fills}</span>
          </div>
          <div className="activity">
            {data.recent.length === 0 && <p className="muted">Nothing yet.</p>}
            {[...data.recent].reverse().map((ev) => (
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
