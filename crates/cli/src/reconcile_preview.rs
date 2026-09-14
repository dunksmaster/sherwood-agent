//! Wires `sherwood-reconcile` into `sherwood_server::state::Reconciler` for
//! `sherwood serve` (`POST /v1/reconcile`, v0.2.10). Same boundary as the CLI
//! command it mirrors ([`crate::reconcile_cmd`]): reads a receipt only,
//! never sends anything, has no signer.

use async_trait::async_trait;
use sherwood_chain::HttpClient;
use sherwood_core::Side;
use sherwood_reconcile::{wait_and_reconcile, ExpectedTrade, Outcome};
use sherwood_server::state::{ReconcileOutcome, ReconcileRequest, Reconciler};
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_secs(3);
const TIMEOUT: Duration = Duration::from_secs(120);

pub struct ChainReconciler {
    pub rpc_url: String,
}

#[async_trait]
impl Reconciler for ChainReconciler {
    async fn reconcile(&self, req: ReconcileRequest) -> Result<ReconcileOutcome, String> {
        let side = match req.side.to_ascii_lowercase().as_str() {
            "buy" => Side::Buy,
            "sell" => Side::Sell,
            other => return Err(format!("side must be \"buy\" or \"sell\", got {other:?}")),
        };
        let client = HttpClient::new(self.rpc_url.clone(), Duration::from_secs(30))
            .map_err(|e| format!("connecting to {}: {e}", self.rpc_url))?;

        let expected = ExpectedTrade {
            symbol: req.symbol,
            side,
            qty: req.qty,
            price: req.price,
            fee: req.fee.unwrap_or_default(),
        };
        let outcome = wait_and_reconcile(&client, &req.tx_hash, &expected, POLL_INTERVAL, TIMEOUT)
            .await
            .map_err(|e| e.to_string())?;

        Ok(match outcome {
            Outcome::NotFound => ReconcileOutcome::NotFound,
            Outcome::Reverted(r) => ReconcileOutcome::Reverted {
                block_number: r.block_number,
                gas_used: r.gas_used,
            },
            Outcome::Confirmed { receipt, fill } => ReconcileOutcome::Confirmed {
                block_number: receipt.block_number,
                gas_used: receipt.gas_used,
                fill,
            },
        })
    }
}
