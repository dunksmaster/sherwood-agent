//! The [`EvmClient`] trait (one required method, `request`) plus read helpers
//! built on it, and [`HttpClient`], the `reqwest` implementation.
//!
//! Every helper here is a read. There is deliberately no `send_raw_transaction`,
//! no signer, no nonce management — that is a later crate.

use crate::abi::{self, strip0x};
use crate::{ChainError, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

/// A JSON-RPC endpoint we can send read requests to.
///
/// Implementors provide [`EvmClient::request`]; the rest are provided methods.
#[async_trait]
pub trait EvmClient: Send + Sync {
    /// Issue one JSON-RPC call and return its `result` value. A JSON-RPC error
    /// object becomes [`ChainError::Reverted`] when it carries revert `data`,
    /// otherwise [`ChainError::Rpc`].
    async fn request(&self, method: &str, params: Value) -> Result<Value>;

    /// `eth_chainId`.
    async fn chain_id(&self) -> Result<u64> {
        parse_quantity(&self.request("eth_chainId", json!([])).await?)
    }

    /// `eth_blockNumber`.
    async fn block_number(&self) -> Result<u64> {
        parse_quantity(&self.request("eth_blockNumber", json!([])).await?)
    }

    /// `web3_clientVersion` — informational.
    async fn client_version(&self) -> Result<String> {
        Ok(self
            .request("web3_clientVersion", json!([]))
            .await?
            .as_str()
            .unwrap_or_default()
            .to_owned())
    }

    /// `eth_call` at `latest`. Returns the raw return bytes. A revert surfaces
    /// as [`ChainError::Reverted`], with `Error(string)` reasons decoded.
    async fn call(&self, to: &str, data: &str) -> Result<Vec<u8>> {
        self.call_inner(None, to, data).await
    }

    /// `eth_call` at `latest` with an explicit `from` (needed to simulate a
    /// transfer as if a particular holder sent it).
    async fn call_from(&self, from: &str, to: &str, data: &str) -> Result<Vec<u8>> {
        self.call_inner(Some(from), to, data).await
    }

    #[doc(hidden)]
    async fn call_inner(&self, from: Option<&str>, to: &str, data: &str) -> Result<Vec<u8>> {
        let mut tx = serde_json::Map::new();
        if let Some(f) = from {
            tx.insert("from".into(), json!(f));
        }
        tx.insert("to".into(), json!(to));
        tx.insert("data".into(), json!(data));
        let ret = self
            .request("eth_call", json!([Value::Object(tx), "latest"]))
            .await?;
        abi::from_hex(ret.as_str().unwrap_or("0x"))
    }

    /// `eth_getCode` at `latest`.
    async fn get_code(&self, address: &str) -> Result<Vec<u8>> {
        let ret = self
            .request("eth_getCode", json!([address, "latest"]))
            .await?;
        abi::from_hex(ret.as_str().unwrap_or("0x"))
    }

    /// `eth_getStorageAt` at `latest` — a single 32-byte slot.
    async fn get_storage_at(&self, address: &str, slot: &str) -> Result<[u8; 32]> {
        let ret = self
            .request("eth_getStorageAt", json!([address, slot, "latest"]))
            .await?;
        let raw = abi::from_hex(ret.as_str().unwrap_or("0x"))?;
        let mut w = [0u8; 32];
        if raw.len() == 32 {
            w.copy_from_slice(&raw);
        } else if raw.len() < 32 {
            w[32 - raw.len()..].copy_from_slice(&raw);
        } else {
            return Err(ChainError::Decode(format!(
                "storage slot returned {} bytes",
                raw.len()
            )));
        }
        Ok(w)
    }

    /// `eth_getLogs`. A wide range that the node refuses (too many results, or
    /// it timed out scanning) is bisected and retried — public RPCs commonly
    /// cap `eth_getLogs`, and a token's own history can still span the whole
    /// chain.
    async fn get_logs(&self, filter: &LogFilter) -> Result<Vec<RpcLog>> {
        self.get_logs_bisecting(filter, 0).await
    }

    #[doc(hidden)]
    async fn get_logs_bisecting(&self, filter: &LogFilter, depth: u32) -> Result<Vec<RpcLog>> {
        const MAX_SPLIT_DEPTH: u32 = 12; // up to 4096-way; plenty for any realistic range
        match self
            .request("eth_getLogs", json!([filter.to_params()]))
            .await
        {
            Ok(ret) => {
                let raw: Vec<RawLog> = serde_json::from_value(ret)
                    .map_err(|e| ChainError::Decode(format!("eth_getLogs result: {e}")))?;
                raw.into_iter().map(RawLog::into_log).collect()
            }
            Err(e)
                if depth < MAX_SPLIT_DEPTH
                    && filter.to_block > filter.from_block
                    && is_range_too_wide(&e) =>
            {
                let mid = filter.from_block + (filter.to_block - filter.from_block) / 2;
                let left = LogFilter {
                    to_block: mid,
                    ..filter.clone()
                };
                let right = LogFilter {
                    from_block: mid + 1,
                    ..filter.clone()
                };
                let mut merged = self.get_logs_bisecting(&left, depth + 1).await?;
                merged.extend(self.get_logs_bisecting(&right, depth + 1).await?);
                Ok(merged)
            }
            Err(e) => Err(e),
        }
    }
}

/// Whether an `eth_getLogs` error looks like "the range is too wide" (as
/// opposed to a real failure) — worth bisecting and retrying rather than
/// giving up.
fn is_range_too_wide(e: &ChainError) -> bool {
    let ChainError::Rpc { message, .. } = e else {
        return false;
    };
    let m = message.to_lowercase();
    m.contains("time") || m.contains("limit") || m.contains("exceed") || m.contains("too many")
}

/// Filter for [`EvmClient::get_logs`]. `topics` positions may be `None` (wildcard)
/// or `Some(hex)`.
#[derive(Debug, Clone, Default)]
pub struct LogFilter {
    pub address: Option<String>,
    pub topics: Vec<Option<String>>,
    pub from_block: u64,
    pub to_block: u64,
}

impl LogFilter {
    fn to_params(&self) -> Value {
        let mut m = serde_json::Map::new();
        if let Some(a) = &self.address {
            m.insert("address".into(), json!(a));
        }
        if !self.topics.is_empty() {
            let t: Vec<Value> = self
                .topics
                .iter()
                .map(|x| x.as_ref().map_or(Value::Null, |s| json!(s)))
                .collect();
            m.insert("topics".into(), Value::Array(t));
        }
        m.insert(
            "fromBlock".into(),
            json!(format!("0x{:x}", self.from_block)),
        );
        m.insert("toBlock".into(), json!(format!("0x{:x}", self.to_block)));
        Value::Object(m)
    }
}

/// One decoded log.
#[derive(Debug, Clone)]
pub struct RpcLog {
    pub address: String,
    pub topics: Vec<String>,
    pub data: String,
    pub block_number: u64,
}

impl RpcLog {
    /// The low 20 bytes of `topics[i]` as a `0x` address, if present.
    #[must_use]
    pub fn topic_address(&self, i: usize) -> Option<String> {
        let t = self.topics.get(i)?;
        let raw = strip0x(t);
        (raw.len() == 64).then(|| format!("0x{}", &raw[24..]))
    }
}

#[derive(Deserialize)]
struct RawLog {
    address: String,
    topics: Vec<String>,
    data: String,
    #[serde(rename = "blockNumber")]
    block_number: String,
}

impl RawLog {
    fn into_log(self) -> Result<RpcLog> {
        Ok(RpcLog {
            address: self.address,
            topics: self.topics,
            data: self.data,
            block_number: parse_quantity_str(&self.block_number)?,
        })
    }
}

fn parse_quantity(v: &Value) -> Result<u64> {
    parse_quantity_str(
        v.as_str()
            .ok_or_else(|| ChainError::Decode("expected a hex quantity string".into()))?,
    )
}

fn parse_quantity_str(s: &str) -> Result<u64> {
    u64::from_str_radix(strip0x(s), 16)
        .map_err(|e| ChainError::Decode(format!("bad hex quantity {s:?}: {e}")))
}

/// Decode a revert payload into a human reason: `Error(string)` (selector
/// `0x08c379a0`) is unwrapped to its message; anything else is left as `None`
/// (the caller still has the raw `data` and can match a custom-error selector).
#[must_use]
pub fn decode_revert_reason(data: &str) -> Option<String> {
    let raw = abi::from_hex(data).ok()?;
    if raw.len() >= 4 + 64 && raw[..4] == [0x08, 0xc3, 0x79, 0xa0] {
        return abi::decode_string(&raw[4..]).ok().filter(|s| !s.is_empty());
    }
    None
}

// ------------------------------------------------------------------------------

/// `reqwest`-backed [`EvmClient`]. One endpoint URL, a shared connection pool,
/// a per-request timeout.
pub struct HttpClient {
    url: String,
    http: reqwest::Client,
    id: std::sync::atomic::AtomicU64,
}

impl HttpClient {
    /// Build a client for `url` with `timeout`.
    pub fn new(url: impl Into<String>, timeout: std::time::Duration) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| ChainError::Transport(e.to_string()))?;
        Ok(Self {
            url: url.into(),
            http,
            id: std::sync::atomic::AtomicU64::new(0),
        })
    }
}

/// Public RPCs throttle bursts of reads (Robinhood Chain's does). Back off and
/// retry a `429` this many times before giving up.
const MAX_429_RETRIES: u32 = 5;

#[async_trait]
impl EvmClient for HttpClient {
    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.id.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let body = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });

        let mut attempt = 0;
        let resp = loop {
            let resp = self
                .http
                .post(&self.url)
                .json(&body)
                .send()
                .await
                .map_err(|e| ChainError::Transport(e.to_string()))?;
            if resp.status().as_u16() == 429 && attempt < MAX_429_RETRIES {
                tokio::time::sleep(std::time::Duration::from_millis(400 * 2u64.pow(attempt))).await;
                attempt += 1;
                continue;
            }
            break resp;
        };
        if !resp.status().is_success() {
            return Err(ChainError::Transport(format!("HTTP {}", resp.status())));
        }
        let v: Value = resp
            .json()
            .await
            .map_err(|e| ChainError::Decode(format!("response body: {e}")))?;

        if let Some(err) = v.get("error") {
            let code = err.get("code").and_then(Value::as_i64).unwrap_or(0);
            let message = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned();
            let data = err
                .get("data")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .filter(|d| d.starts_with("0x"));
            if data.is_some() || message.to_lowercase().contains("revert") {
                let reason = data.as_deref().and_then(decode_revert_reason);
                return Err(ChainError::Reverted { reason, data });
            }
            return Err(ChainError::Rpc { code, message });
        }

        v.get("result")
            .cloned()
            .ok_or_else(|| ChainError::Decode("response had neither result nor error".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::MockRpc;

    #[tokio::test]
    async fn chain_id_and_block_number_parse_hex_quantities() {
        let rpc = MockRpc::new(vec![Ok(json!("0x1237")), Ok(json!("0x33be85d"))]);
        assert_eq!(rpc.chain_id().await.unwrap(), 4663);
        assert_eq!(rpc.block_number().await.unwrap(), 0x033b_e85d);
    }

    #[tokio::test]
    async fn call_returns_raw_bytes() {
        let rpc = MockRpc::new(vec![Ok(json!(
            "0x0000000000000000000000000000000000000000000000000000000000000012"
        ))]);
        let ret = rpc.call("0xabc", "0x313ce567").await.unwrap();
        assert_eq!(ret.len(), 32);
        assert_eq!(ret[31], 0x12);
    }

    #[tokio::test]
    async fn get_logs_decodes_and_exposes_topic_addresses() {
        let rpc = MockRpc::new(vec![Ok(json!([{
            "address": "0xtoken",
            "topics": [
                "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
                "0x0000000000000000000000001111111111111111111111111111111111111111",
                "0x0000000000000000000000002222222222222222222222222222222222222222"
            ],
            "data": "0x00",
            "blockNumber": "0x10"
        }]))]);
        let logs = rpc.get_logs(&LogFilter::default()).await.unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].block_number, 16);
        assert_eq!(
            logs[0].topic_address(1).unwrap(),
            "0x1111111111111111111111111111111111111111"
        );
    }

    #[test]
    fn revert_reason_decodes_error_string() {
        // abi.encodeWithSignature("Error(string)", "HOOD: blocked")
        let payload = "0x08c379a0\
            0000000000000000000000000000000000000000000000000000000000000020\
            000000000000000000000000000000000000000000000000000000000000000d\
            484f4f443a20626c6f636b6564000000000000000000000000000000000000000000";
        assert_eq!(
            decode_revert_reason(payload).as_deref(),
            Some("HOOD: blocked")
        );
        assert_eq!(decode_revert_reason("0xe450d38c").as_deref(), None);
    }
}
