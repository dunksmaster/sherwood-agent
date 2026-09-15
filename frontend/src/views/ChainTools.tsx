import { useState } from "react";
import {
  api,
  ApiError,
  type DexSimulateOutcome,
  type ReconcileView,
  type RouteView,
} from "../api.ts";

/**
 * These three cards call operator-role endpoints. The dashboard has no
 * per-role UI variation anywhere else (Controls.tsx doesn't hide its admin
 * actions from a viewer token either) — a wrong-role token just gets a 403
 * back, shown the same way any other API error is. Hiding these by role
 * would need the client to know its own role first, and nothing here
 * exposes that (there's no `/v1/whoami`); relying on the server's own check
 * is the existing pattern, not a new one.
 */

function errMsg(e: unknown): string {
  return e instanceof ApiError ? `${e.code}: ${e.message}` : String(e);
}

export function RouteCard({ token }: { token: string }) {
  const [notional, setNotional] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [result, setResult] = useState<RouteView | null>(null);

  async function check() {
    setBusy(true);
    setErr(null);
    setResult(null);
    try {
      setResult(await api.route(token, notional));
    } catch (e) {
      setErr(errMsg(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="card">
      <h2>Router</h2>
      <p className="muted" style={{ fontSize: 12, marginTop: 0 }}>
        Which venue (AMM or RFQ) an order of this notional would use. Pure
        decision — no RPC, no calldata, nothing sent.
      </p>
      <label className="muted" htmlFor="route-notional">
        Notional
      </label>
      <input
        id="route-notional"
        value={notional}
        onChange={(e) => setNotional(e.target.value)}
        placeholder="e.g. 500"
      />
      <button
        style={{ marginTop: 12 }}
        disabled={busy || !notional.trim()}
        onClick={() => void check()}
      >
        Check venue
      </button>
      {err && <p className="err">{err}</p>}
      {result && (
        <div className="kv" style={{ marginTop: 12 }}>
          <span className="k">Venue</span>
          <span className="mono">
            {result.venue.toUpperCase()}
            <span className="muted"> — {result.reason}</span>
          </span>
        </div>
      )}
    </div>
  );
}

export function ReconcileCard({ token }: { token: string }) {
  const [txHash, setTxHash] = useState("");
  const [symbol, setSymbol] = useState("");
  const [side, setSide] = useState<"buy" | "sell">("buy");
  const [qty, setQty] = useState("");
  const [price, setPrice] = useState("");
  const [fee, setFee] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [result, setResult] = useState<ReconcileView | null>(null);

  const canSubmit = txHash.trim() && symbol.trim() && qty.trim() && price.trim();

  async function run() {
    setBusy(true);
    setErr(null);
    setResult(null);
    try {
      setResult(
        await api.reconcile(token, {
          tx_hash: txHash.trim(),
          symbol: symbol.trim(),
          side,
          qty,
          price,
          fee: fee.trim() || undefined,
        }),
      );
    } catch (e) {
      setErr(errMsg(e));
    } finally {
      setBusy(false);
    }
  }

  const statusBadge = (s: ReconcileView["status"]) => {
    const cls =
      s === "confirmed" ? "badge ok" : s === "reverted" ? "badge kill" : "badge";
    return <span className={cls}>{s}</span>;
  };

  return (
    <div className="card">
      <h2>Reconcile</h2>
      <p className="muted" style={{ fontSize: 12, marginTop: 0 }}>
        Read a receipt for a tx hash you already broadcast with your own
        tooling. Never sends anything.
      </p>
      <label className="muted" htmlFor="rec-tx">
        Transaction hash
      </label>
      <input
        id="rec-tx"
        value={txHash}
        onChange={(e) => setTxHash(e.target.value)}
        placeholder="0x…"
      />
      <div className="row" style={{ marginTop: 8 }}>
        <input
          value={symbol}
          onChange={(e) => setSymbol(e.target.value)}
          placeholder="symbol"
          style={{ flex: 1 }}
        />
        <select
          value={side}
          onChange={(e) => setSide(e.target.value as "buy" | "sell")}
        >
          <option value="buy">buy</option>
          <option value="sell">sell</option>
        </select>
      </div>
      <div className="row" style={{ marginTop: 8 }}>
        <input
          value={qty}
          onChange={(e) => setQty(e.target.value)}
          placeholder="qty"
          style={{ flex: 1 }}
        />
        <input
          value={price}
          onChange={(e) => setPrice(e.target.value)}
          placeholder="price"
          style={{ flex: 1 }}
        />
        <input
          value={fee}
          onChange={(e) => setFee(e.target.value)}
          placeholder="fee (optional)"
          style={{ flex: 1 }}
        />
      </div>
      <button style={{ marginTop: 12 }} disabled={busy || !canSubmit} onClick={() => void run()}>
        Reconcile
      </button>
      {err && <p className="err">{err}</p>}
      {result && (
        <div style={{ marginTop: 12 }}>
          <div className="kv">
            <span className="k">Status</span>
            {statusBadge(result.status)}
          </div>
          {result.block_number != null && (
            <div className="kv">
              <span className="k">Block</span>
              <span className="mono">
                {result.block_number} · gas {result.gas_used}
              </span>
            </div>
          )}
          <div className="kv">
            <span className="k">Recorded</span>
            <span className="mono">{result.recorded ? "yes" : "no"}</span>
          </div>
          {result.note && (
            <p className="muted" style={{ fontSize: 12 }}>
              {result.note}
            </p>
          )}
        </div>
      )}
    </div>
  );
}

export function DexSimulateCard({ token }: { token: string }) {
  const [from, setFrom] = useState("");
  const [tokenSym, setTokenSym] = useState("");
  const [amountInRaw, setAmountInRaw] = useState("");
  const [denom, setDenom] = useState("");
  const [slippageBps, setSlippageBps] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [result, setResult] = useState<DexSimulateOutcome | null>(null);

  const canSubmit = from.trim() && tokenSym.trim() && amountInRaw.trim();

  async function run() {
    setBusy(true);
    setErr(null);
    setResult(null);
    try {
      setResult(
        await api.dexSimulate(token, {
          from: from.trim(),
          token: tokenSym.trim(),
          amount_in_raw: amountInRaw.trim(),
          denom: denom.trim() || undefined,
          slippage_bps: slippageBps.trim() ? Number(slippageBps) : undefined,
        }),
      );
    } catch (e) {
      setErr(errMsg(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="card">
      <h2>DEX simulate</h2>
      <p className="muted" style={{ fontSize: 12, marginTop: 0 }}>
        <span className="mono">eth_call</span>-simulates a swap against the
        live chain. Signs and sends nothing.
      </p>
      <label className="muted" htmlFor="dex-from">
        From address
      </label>
      <input
        id="dex-from"
        value={from}
        onChange={(e) => setFrom(e.target.value)}
        placeholder="0x…"
      />
      <div className="row" style={{ marginTop: 8 }}>
        <input
          value={tokenSym}
          onChange={(e) => setTokenSym(e.target.value)}
          placeholder="token (symbol or address)"
          style={{ flex: 1 }}
        />
        <input
          value={amountInRaw}
          onChange={(e) => setAmountInRaw(e.target.value)}
          placeholder="amount in (raw base units)"
          style={{ flex: 1 }}
        />
      </div>
      <div className="row" style={{ marginTop: 8 }}>
        <input
          value={denom}
          onChange={(e) => setDenom(e.target.value)}
          placeholder="denom (default USDG)"
          style={{ flex: 1 }}
        />
        <input
          value={slippageBps}
          onChange={(e) => setSlippageBps(e.target.value)}
          placeholder="slippage bps (default 50)"
          style={{ flex: 1 }}
        />
      </div>
      <button style={{ marginTop: 12 }} disabled={busy || !canSubmit} onClick={() => void run()}>
        Simulate swap
      </button>
      {err && <p className="err">{err}</p>}
      {result && (
        <div style={{ marginTop: 12 }}>
          <div className="kv">
            <span className="k">Result</span>
            <span className={`badge ${result.ok ? "ok" : "kill"}`}>
              {result.ok ? "succeeded" : "reverted"}
            </span>
          </div>
          <div className="kv">
            <span className="k">Pool</span>
            <span className="mono">
              {result.token_symbol}/{result.denom_symbol} · fee{" "}
              {result.pool_fee} · tick {result.pool_tick_spacing}
            </span>
          </div>
          <div className="kv">
            <span className="k">Min out</span>
            <span className="mono">{result.amount_out_minimum}</span>
          </div>
          <p
            className="muted mono"
            style={{ fontSize: 12, wordBreak: "break-all" }}
            title={result.calldata_hex}
          >
            {result.detail}
          </p>
        </div>
      )}
    </div>
  );
}
