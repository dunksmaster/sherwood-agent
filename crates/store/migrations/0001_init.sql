-- sherwood-store v0.1 schema.
--
-- Conventions:
--   * timestamps are RFC 3339 UTC strings (TEXT)
--   * money and quantities are decimal strings (TEXT), never REAL
--   * the audit log is append-only by contract; there is no UPDATE or DELETE
--     path for it in the Store trait, and the hash chain makes tampering
--     detectable regardless
--
-- Tables for later steps (config_state, cursors, pending_approvals) are added
-- by the migration that accompanies the step that first writes them, so this
-- file only contains what v0.1 actually uses.

CREATE TABLE portfolio_snapshots (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    taken_at   TEXT NOT NULL,
    state_json TEXT NOT NULL
);

CREATE TABLE fills (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    order_id    TEXT NOT NULL,
    symbol      TEXT NOT NULL,
    address     TEXT,
    side        TEXT NOT NULL CHECK (side IN ('buy', 'sell')),
    qty         TEXT NOT NULL,
    price       TEXT NOT NULL,
    fee         TEXT NOT NULL,
    venue       TEXT NOT NULL,
    filled_at   TEXT NOT NULL,
    recorded_at TEXT NOT NULL
);

CREATE INDEX idx_fills_symbol ON fills (symbol);
CREATE INDEX idx_fills_filled_at ON fills (filled_at);

CREATE TABLE audit_log (
    seq       INTEGER PRIMARY KEY,   -- 1-based, contiguous, assigned by the store
    at        TEXT NOT NULL,
    kind      TEXT NOT NULL,
    data_json TEXT NOT NULL,         -- canonical (key-sorted) JSON
    prev_hash TEXT NOT NULL,         -- hex SHA-256 of the previous row's hash (genesis: 64 zeros)
    hash      TEXT NOT NULL          -- hex SHA-256(prev_hash || "\n" || seq || "\n" || at || "\n" || kind || "\n" || data_json)
);
