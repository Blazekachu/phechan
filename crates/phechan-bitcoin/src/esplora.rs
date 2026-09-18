//! Public Esplora / mempool.space broadcast + tx lookup (sort-utxo / runes-etch path).
//!
//! Local bitcoind often keeps `minrelaytxfee` at 1 sat/vB, while signet public
//! relays accept fractional rates (e.g. 0.69). Prefer Esplora for signet/testnet
//! broadcast so Phechan matches those tools.

use crate::Network;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug)]
pub enum EsploraError {
    Http(String),
    Message(String),
}

impl std::fmt::Display for EsploraError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "esplora http: {e}"),
            Self::Message(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for EsploraError {}

fn providers(network: Network) -> &'static [&'static str] {
    // Order matches sort-utxo compose: emzy → memepool → mempool.space → blockstream.
    // mempool.space often rate-limits residential IPs; memepool.space is the mirror.
    match network {
        Network::Signet => &[
            "https://mempool.emzy.de/signet/api",
            "https://memepool.space/signet/api",
            "https://mempool.space/signet/api",
            "https://blockstream.info/signet/api",
        ],
        Network::Testnet => &[
            "https://mempool.emzy.de/testnet/api",
            "https://memepool.space/testnet/api",
            "https://mempool.space/testnet/api",
            "https://blockstream.info/testnet/api",
        ],
        Network::Mainnet => &[
            "https://mempool.emzy.de/api",
            "https://memepool.space/api",
            "https://mempool.space/api",
            "https://blockstream.info/api",
        ],
        Network::Regtest => &[],
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .build()
}

/// POST raw tx hex to Esplora. Returns txid string.
pub fn broadcast_tx(network: Network, tx_hex: &str) -> Result<String, EsploraError> {
    let hex = tx_hex.trim();
    if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(EsploraError::Message("invalid tx hex".into()));
    }
    let bases = providers(network);
    if bases.is_empty() {
        return Err(EsploraError::Message(
            "no public Esplora for this network — use local bitcoind".into(),
        ));
    }
    let mut errors: Vec<String> = Vec::new();
    for base in bases {
        let url = format!("{base}/tx");
        // Esplora expects raw hex body as text/plain (same as sort-utxo fetch POST).
        match agent()
            .post(&url)
            .set("Content-Type", "text/plain")
            .send_string(hex)
        {
            Ok(resp) => {
                let status = resp.status();
                let body = resp.into_string().unwrap_or_default();
                if (200..300).contains(&status) {
                    let txid = body.trim().to_string();
                    if txid.len() == 64 && txid.chars().all(|c| c.is_ascii_hexdigit()) {
                        return Ok(txid);
                    }
                    if !txid.is_empty() {
                        return Ok(txid);
                    }
                    errors.push(format!("{base} → {status} empty body"));
                } else {
                    let snip: String = body.chars().take(120).collect();
                    errors.push(format!("{base} → {status} {snip}"));
                }
            }
            Err(e) => errors.push(format!("{base} → {e}")),
        }
    }
    Err(EsploraError::Http(format!(
        "all Esplora providers failed: {}",
        errors.join(" | ")
    )))
}

/// GET `/tx/{txid}` JSON (Esplora schema) for commit vout lookup when local node
/// has not accepted a sub-minrelay tx into its mempool.
pub fn get_tx_json(network: Network, txid: &str) -> Result<Value, EsploraError> {
    let txid = txid.trim();
    if txid.len() != 64 || !txid.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(EsploraError::Message("invalid txid".into()));
    }
    let bases = providers(network);
    if bases.is_empty() {
        return Err(EsploraError::Message("no public Esplora for this network".into()));
    }
    let mut last = String::new();
    for base in bases {
        let url = format!("{base}/tx/{txid}");
        match agent().get(&url).call() {
            Ok(resp) => {
                let status = resp.status();
                if !(200..300).contains(&status) {
                    last = format!("{base} → {status}");
                    continue;
                }
                let v: Value = resp
                    .into_json()
                    .map_err(|e| EsploraError::Message(e.to_string()))?;
                return Ok(v);
            }
            Err(e) => last = format!("{base} → {e}"),
        }
    }
    Err(EsploraError::Http(format!(
        "all Esplora providers failed: {last}"
    )))
}

/// Find vout + value for `address` in an Esplora tx JSON (`vout[].scriptpubkey_address`).
pub fn find_vout_in_esplora_tx(tx: &Value, address: &str) -> Option<(u32, u64)> {
    let outs = tx.get("vout")?.as_array()?;
    for (i, o) in outs.iter().enumerate() {
        let addr = o
            .get("scriptpubkey_address")
            .and_then(|a| a.as_str())
            .unwrap_or("");
        if addr == address {
            let value = o.get("value").and_then(|v| v.as_u64())?;
            return Some((i as u32, value));
        }
    }
    None
}
