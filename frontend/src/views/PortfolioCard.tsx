import { type ApiError, fmtMoney, type PortfolioView } from "../api.ts";

function pnlClass(s: string): string {
  const n = Number(s);
  if (!Number.isFinite(n) || n === 0) return "";
  return n > 0 ? "pnl-up" : "pnl-down";
}

export function PortfolioCard({
  data,
  error,
}: {
  data: PortfolioView | null;
  error: ApiError | null;
}) {
  return (
    <div className="card">
      <h2>
        <svg viewBox="0 0 16 16" className="h2-icon" aria-hidden="true">
          <path
            fill="currentColor"
            d="M2 3.5A1.5 1.5 0 0 1 3.5 2h9A1.5 1.5 0 0 1 14 3.5v9a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 12.5v-9ZM4.5 11a.5.5 0 0 0 .5.5h6a.5.5 0 0 0 0-1H5a.5.5 0 0 0-.5.5Zm.75-5.75a.75.75 0 0 1 1.06 0L8 6.94l1.69-1.69a.75.75 0 1 1 1.06 1.06L8.53 8.53a.75.75 0 0 1-1.06 0L5.25 6.31a.75.75 0 0 1 0-1.06Z"
          />
        </svg>
        Portfolio
      </h2>
      {error?.status === 404 && (
        <p className="muted">
          No persisted state. Run <span className="mono">sherwood run</span> with a{" "}
          <span className="mono">state_path</span>.
        </p>
      )}
      {error && error.status !== 404 && <p className="err">{error.message}</p>}
      {data && (
        <>
          <div className="stat">
            <span className="stat-label">Cash</span>
            <span className="stat-value mono">{fmtMoney(data.cash)}</span>
            <span className={`stat-sub mono ${pnlClass(data.realized_pnl)}`}>
              {Number(data.realized_pnl) >= 0 ? "+" : ""}
              {fmtMoney(data.realized_pnl)} realized
            </span>
          </div>
          <div className="kv">
            <span className="k">Open positions</span>
            <span className="mono">{data.open_positions}</span>
          </div>
          {data.positions.map((p) => (
            <div className="kv" key={p.symbol}>
              <span className="k">{p.symbol}</span>
              <span className="mono">
                {p.quantity}
                {p.avg_cost ? ` @ ${fmtMoney(p.avg_cost)}` : ""}
              </span>
            </div>
          ))}
        </>
      )}
    </div>
  );
}
