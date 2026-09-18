//! Minimal bitcoind JSON-RPC client for regtest workflows.

use serde_json::{json, Value};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct RpcConfig {
    pub url: String,
    pub user: String,
    pub password: String,
}

impl RpcConfig {
    /// Defaults for local phechan regtest-stack (`F:\bitcoin-regtest`).
    pub fn from_env() -> Self {
        Self {
            url: std::env::var("PHECHAN_RPC_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:18444/wallet/phechan_plain".into()),
            user: std::env::var("PHECHAN_RPC_USER").unwrap_or_else(|_| "ord".into()),
            password: std::env::var("PHECHAN_RPC_PASS")
                .unwrap_or_else(|_| "regtest-local-dev".into()),
        }
    }
}

#[derive(Debug)]
pub enum RpcError {
    Http(String),
    Json(String),
    Rpc { code: i64, message: String },
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "rpc http: {e}"),
            Self::Json(e) => write!(f, "rpc json: {e}"),
            Self::Rpc { code, message } => write!(f, "rpc {code}: {message}"),
        }
    }
}

impl std::error::Error for RpcError {}

#[derive(Debug, Clone)]
pub struct BitcoindRpc {
    cfg: RpcConfig,
}

impl BitcoindRpc {
    pub fn new(cfg: RpcConfig) -> Self {
        Self { cfg }
    }

    pub fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let body = json!({
            "jsonrpc": "1.0",
            "id": "phechan",
            "method": method,
            "params": params,
        });
        let resp = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(60))
            .build()
            .post(&self.cfg.url)
            .set(
                "Authorization",
                &basic_auth(&self.cfg.user, &self.cfg.password),
            )
            .send_json(body);

        let value: Value = match resp {
            Ok(r) => r.into_json().map_err(|e| RpcError::Json(e.to_string()))?,
            Err(ureq::Error::Status(_code, r)) => r
                .into_json()
                .map_err(|e| RpcError::Json(format!("http error body: {e}")))?,
            Err(e) => return Err(RpcError::Http(e.to_string())),
        };
        if let Some(err) = value.get("error").filter(|e| !e.is_null()) {
            return Err(RpcError::Rpc {
                code: err.get("code").and_then(|c| c.as_i64()).unwrap_or(-1),
                message: err
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
            });
        }
        value
            .get("result")
            .cloned()
            .ok_or_else(|| RpcError::Json("missing result".into()))
    }

    pub fn get_block_count(&self) -> Result<u64, RpcError> {
        Ok(self.call("getblockcount", json!([]))?.as_u64().unwrap_or(0))
    }

    /// `(blocks, mediantime)` for final locktime vanity grinding.
    pub fn get_tip_for_locktime(&self) -> Result<(u32, u32), RpcError> {
        let info = self.call("getblockchaininfo", json!([]))?;
        let blocks = info
            .get("blocks")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let mediantime = info
            .get("mediantime")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        Ok((blocks, mediantime))
    }

    pub fn send_to_address(&self, address: &str, btc: f64) -> Result<String, RpcError> {
        let txid = self.call("sendtoaddress", json!([address, btc]))?;
        txid.as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| RpcError::Json("sendtoaddress not string".into()))
    }

    pub fn send_raw_transaction(&self, hex: &str) -> Result<String, RpcError> {
        let txid = self.call("sendrawtransaction", json!([hex]))?;
        txid.as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| RpcError::Json("sendrawtransaction not string".into()))
    }

    pub fn get_raw_transaction_verbose(&self, txid: &str) -> Result<Value, RpcError> {
        self.call("getrawtransaction", json!([txid, true]))
    }

    pub fn get_new_address_bech32m(&self) -> Result<String, RpcError> {
        let v = self.call("getnewaddress", json!(["", "bech32m"]))?;
        v.as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| RpcError::Json("getnewaddress not string".into()))
    }

    pub fn generate_to_address(&self, nblocks: u32, address: &str) -> Result<Value, RpcError> {
        self.call("generatetoaddress", json!([nblocks, address]))
    }

    pub fn list_unspent(&self) -> Result<Value, RpcError> {
        self.call("listunspent", json!([]))
    }

    pub fn wallet_process_psbt(&self, psbt_base64: &str) -> Result<Value, RpcError> {
        self.call("walletprocesspsbt", json!([psbt_base64]))
    }

    pub fn finalize_psbt(&self, psbt_base64: &str) -> Result<Value, RpcError> {
        self.call("finalizepsbt", json!([psbt_base64]))
    }

    pub fn decode_psbt(&self, psbt_base64: &str) -> Result<Value, RpcError> {
        self.call("decodepsbt", json!([psbt_base64]))
    }

    pub fn decode_raw_transaction(&self, hex: &str) -> Result<Value, RpcError> {
        self.call("decoderawtransaction", json!([hex]))
    }
}

fn basic_auth(user: &str, pass: &str) -> String {
    use std::io::Write;
    let mut out = Vec::new();
    write!(&mut out, "{user}:{pass}").ok();
    format!("Basic {}", base64_encode(&out))
}

fn base64_encode(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let mut n = (chunk[0] as u32) << 16;
        if chunk.len() > 1 {
            n |= (chunk[1] as u32) << 8;
        }
        if chunk.len() > 2 {
            n |= chunk[2] as u32;
        }
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Find vout index paying `address` in a verbose tx JSON.
pub fn find_vout_for_address(verbose_tx: &Value, address: &str) -> Option<(u32, u64)> {
    let vouts = verbose_tx.get("vout")?.as_array()?;
    for v in vouts {
        let spk = v.get("scriptPubKey")?;
        let addrs = spk
            .get("address")
            .and_then(|a| a.as_str())
            .map(|s| vec![s.to_string()])
            .or_else(|| {
                spk.get("addresses")
                    .and_then(|a| a.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect()
                    })
            })
            .unwrap_or_default();
        if addrs.iter().any(|a| a == address) {
            let n = v.get("n")?.as_u64()? as u32;
            let value_btc = v.get("value")?.as_f64()?;
            let sats = (value_btc * 100_000_000.0).round() as u64;
            return Some((n, sats));
        }
    }
    None
}
