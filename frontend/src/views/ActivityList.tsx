import type {
  ActivityView,
  ApiError,
  AuditEvent,
  AuditVerifyView,
} from "../api.ts";

/** Which of the fixed accent classes a `kind` gets — purely a visual grouping
 * so the eye can scan the feed without reading every row, not a source of
 * truth for anything. Kind strings are whatever `StoreSubscriber::handle`
 * (crates/store/src/lib.rs) writes: "fill", "gate_reject", "decision",
 * "run_end", plus reconcile_cmd's own "fill" with `source: "reconcile"`. */
function kindClass(kind: string): string {
  if (kind === "fill") return "ev-dot-ok";
  if (kind === "gate_reject") return "ev-dot-danger";
  if (kind === "run_end") return "ev-dot-muted";
  return "ev-dot-accent"; // "decision" and anything unrecognized
}

/** `ev.data` is `unknown` on the wire — these read only the fields each
 * `kind` is actually written with, tolerating anything else (an
 * unrecognized kind, a missing field) by falling back to nothing extra
 * rather than throwing. */
function detail(ev: AuditEvent): string | null {
  const d = ev.data as Record<string, unknown> | null | undefined;
  if (!d || typeof d !== "object") return null;
  const str = (k: string): string | null =>
    typeof d[k] === "string" ? (d[k] as string) : null;

  switch (ev.kind) {
    case "fill": {
      const side = str("side");
      const qty = str("qty");
      const symbol = str("symbol");
      const price = str("price");
      if (!side || !qty || !symbol) return null;
      const via = d.source === "reconcile" ? " · reconciled" : "";
      return `${side.toUpperCase()} ${qty} ${symbol}${price ? ` @ ${price}` : ""}${via}`;
    }
    case "gate_reject": {
      const symbol = str("symbol");
      const reason = str("reason");
      return symbol ? `${symbol}${reason ? ` · ${reason}` : ""}` : reason;
    }
    case "decision": {
      const decision = str("decision");
      const price = str("price");
      return decision ? `${decision}${price ? ` @ ${price}` : ""}` : null;
    }
    case "run_end": {
      const label = str("label");
      const cash = str("cash");
      return label ? `${label}${cash ? ` · cash ${cash}` : ""}` : null;
    }
    default:
      return null;
  }
}

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
            {[...rows].reverse().map((ev) => {
              const d = detail(ev);
              return (
                <div className="ev" key={ev.seq}>
                  <span className="row" style={{ gap: 8, minWidth: 0 }}>
                    <span className={`ev-dot ${kindClass(ev.kind)}`} aria-hidden="true" />
                    <span className="mono">{ev.kind}</span>
                    {d && (
                      <span className="mono muted" style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                        {d}
                      </span>
                    )}
                  </span>
                  <span className="muted mono" style={{ flexShrink: 0 }}>
                    {new Date(ev.at).toLocaleTimeString()}
                  </span>
                </div>
              );
            })}
          </div>
        </>
      )}
    </div>
  );
}
