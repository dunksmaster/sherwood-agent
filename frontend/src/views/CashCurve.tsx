import { fmtMoney, type ActivityView, type ApiError, type AuditEvent, type PortfolioView } from "../api.ts";

interface FillPoint {
  at: string;
  cash: number;
}

/** Reconstructs a cash-over-time series from the fills in the current
 * activity window, anchored to the live portfolio cash. Not equity — no
 * mark-to-market — and only as deep as `/v1/activity`'s window, so this is a
 * recent-movement sparkline, not a full-history chart. */
function deriveCashSeries(fills: AuditEvent[], currentCash: number): FillPoint[] {
  const deltas: { at: string; delta: number }[] = [];
  for (const ev of fills) {
    if (ev.kind !== "fill") continue;
    const d = ev.data as Record<string, unknown> | null;
    if (!d || typeof d !== "object") continue;
    const side = d.side;
    const qty = Number(d.qty);
    const price = Number(d.price);
    const fee = Number(d.fee ?? 0);
    if (
      (side !== "buy" && side !== "sell") ||
      !Number.isFinite(qty) ||
      !Number.isFinite(price) ||
      !Number.isFinite(fee)
    ) {
      continue;
    }
    const notional = qty * price;
    deltas.push({ at: ev.at, delta: side === "sell" ? notional - fee : -(notional + fee) });
  }
  if (deltas.length === 0) return [];
  const total = deltas.reduce((sum, d) => sum + d.delta, 0);
  let running = currentCash - total;
  return deltas.map((d) => {
    running += d.delta;
    return { at: d.at, cash: running };
  });
}

const W = 560;
const H = 110;
const PAD = 8;

function buildPaths(points: FillPoint[]) {
  const values = points.map((p) => p.cash);
  const min = Math.min(...values);
  const max = Math.max(...values);
  const span = max - min || 1;
  const innerW = W - PAD * 2;
  const innerH = H - PAD * 2;
  const coords = points.map((p, i) => {
    const x = points.length === 1 ? PAD + innerW / 2 : PAD + (i / (points.length - 1)) * innerW;
    const y = PAD + innerH - ((p.cash - min) / span) * innerH;
    return [x, y] as const;
  });
  const line = coords.map(([x, y], i) => `${i === 0 ? "M" : "L"}${x.toFixed(1)},${y.toFixed(1)}`).join(" ");
  const floor = (H - PAD).toFixed(1);
  const area =
    `M${coords[0][0].toFixed(1)},${floor} ` +
    coords.map(([x, y]) => `L${x.toFixed(1)},${y.toFixed(1)}`).join(" ") +
    ` L${coords[coords.length - 1][0].toFixed(1)},${floor} Z`;
  return { line, area, min, max };
}

export function CashCurve({
  events,
  data,
  portfolio,
  error,
}: {
  events: AuditEvent[];
  data: ActivityView | null;
  portfolio: PortfolioView | null;
  error: ApiError | null;
}) {
  const rows = events.length > 0 ? events : (data?.recent ?? []);
  const cash = portfolio ? Number(portfolio.cash) : null;
  const points = cash != null && Number.isFinite(cash) ? deriveCashSeries(rows, cash) : [];

  return (
    <div className="card">
      <h2>Cash over time</h2>
      {error?.status === 404 && <p className="muted">No persisted state.</p>}
      {error && error.status !== 404 && <p className="err">{error.message}</p>}
      {!error && points.length < 2 && (
        <p className="muted">Not enough fills in the recent window to chart yet.</p>
      )}
      {points.length >= 2 &&
        (() => {
          const { line, area, min, max } = buildPaths(points);
          const up = points[points.length - 1].cash >= points[0].cash;
          const tone = up ? "var(--ok)" : "var(--danger)";
          return (
            <>
              <svg
                viewBox={`0 0 ${W} ${H}`}
                className="curve-svg"
                preserveAspectRatio="none"
                role="img"
                aria-label="Cash over time across recent fills"
              >
                <defs>
                  <linearGradient id="curveFill" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" stopColor={tone} stopOpacity="0.16" />
                    <stop offset="100%" stopColor={tone} stopOpacity="0" />
                  </linearGradient>
                </defs>
                <path d={area} fill="url(#curveFill)" stroke="none" />
                <path d={line} fill="none" stroke={tone} strokeWidth="1.5" />
              </svg>
              <div className="curve-range">
                <span className="mono muted">{fmtMoney(String(min))}</span>
                <span className="muted">recent fills, client-derived — not equity</span>
                <span className="mono muted">{fmtMoney(String(max))}</span>
              </div>
            </>
          );
        })()}
    </div>
  );
}
