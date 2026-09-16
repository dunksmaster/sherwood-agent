import { Link } from "react-router-dom";

/**
 * Public, static, no token required. Content mirrors README.md's own
 * framing rather than inventing new marketing copy — see the Overview and
 * Operator boundary sections there for the source wording this paraphrases.
 */
export function LandingPage() {
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
          An automated trading system for the Robinhood Agentic Trading MCP,
          with a hard risk gate in front of every order.
        </p>

        <div className="kv" style={{ marginTop: 16 }}>
          <span className="k">Status</span>
          <span className="badge paper">
            <span className="dot" aria-hidden="true" />
            PAPER TRADING
          </span>
        </div>
        <p className="muted" style={{ fontSize: 12 }}>
          No live venue is wired by default. Live requires the venue
          connected, the admin role, and an explicit toggle — see below.
        </p>

        <h2 style={{ marginTop: 20 }}>What it does</h2>
        <ul className="muted" style={{ paddingLeft: 18, fontSize: 13 }}>
          <li>
            Proposes trades on a schedule or in response to a monitored event,
            using deterministic rules, a language model, or both.
          </li>
          <li>
            Every proposal clears a risk gate and spend caps before anything
            reaches a venue.
          </li>
          <li>
            <span className="mono">manual</span> mode: you approve each order.{" "}
            <span className="mono">auto</span> mode: it executes within
            configured limits.
          </li>
          <li>
            Every decision, order, and fill is written to a tamper-evident
            audit log you can verify.
          </li>
        </ul>

        <h2 style={{ marginTop: 20 }}>What it will never do on your behalf</h2>
        <ul className="muted" style={{ paddingLeft: 18, fontSize: 13 }}>
          <li>Open or authenticate a trading account for you.</li>
          <li>Accept any venue's customer agreement or disclosures.</li>
          <li>Hold your credentials, or place a live order without you enabling live mode.</li>
        </ul>

        <Link to="/login">
          <button style={{ marginTop: 20, width: "100%" }}>Connect</button>
        </Link>
      </div>
    </div>
  );
}
