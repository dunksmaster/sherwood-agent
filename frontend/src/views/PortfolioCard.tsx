import { type ApiError, fmtMoney, type PortfolioView } from "../api.ts";

export function PortfolioCard({
  data,
  error,
}: {
  data: PortfolioView | null;
  error: ApiError | null;
}) {
  return (
    <div className="card">
      <h2>Portfolio</h2>
      {error?.status === 404 && (
        <p className="muted">
          No persisted state. Run <span className="mono">sherwood run</span> with a{" "}
          <span className="mono">state_path</span>.
        </p>
      )}
      {error && error.status !== 404 && <p className="err">{error.message}</p>}
      {data && (
        <>
          <div className="kv">
            <span className="k">Cash</span>
            <span className="mono">{fmtMoney(data.cash)}</span>
          </div>
          <div className="kv">
            <span className="k">Realized P&amp;L</span>
            <span className="mono">{fmtMoney(data.realized_pnl)}</span>
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
