//! Optional ord HTTP indexer client for UTXO asset labels.

use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct OrdConfig {
    pub base_url: String,
}

impl OrdConfig {
    pub fn from_env() -> Self {
        Self {
            base_url: std::env::var("PHECHAN_ORD_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:80".into())
                .trim_end_matches('/')
                .to_string(),
        }
    }
}

#[derive(Debug)]
pub enum OrdError {
    Http(String),
    Json(String),
    NotFound,
}

impl std::fmt::Display for OrdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "ord http: {e}"),
            Self::Json(e) => write!(f, "ord json: {e}"),
            Self::NotFound => write!(f, "ord output not found"),
        }
    }
}

impl std::error::Error for OrdError {}

#[derive(Debug, Clone, Default)]
pub struct OrdOutputInfo {
    pub inscription_ids: Vec<String>,
    pub rune_summary: Option<String>,
    pub found: bool,
}

#[derive(Debug, Clone)]
pub struct OrdClient {
    cfg: OrdConfig,
}

impl OrdClient {
    pub fn new(cfg: OrdConfig) -> Self {
        Self { cfg }
    }

    pub fn from_env() -> Self {
        Self::new(OrdConfig::from_env())
    }

    /// GET output labels — tries classic `/output/` then recursive `/r/utxo/` (ordinals.com).
    pub fn output(&self, txid: &str, vout: u32) -> Result<OrdOutputInfo, OrdError> {
        let outpoint = format!("{txid}:{vout}");
        match self.get_json(&format!("{}/output/{outpoint}", self.cfg.base_url)) {
            Ok(value) => Ok(parse_ord_output(&value)),
            Err(OrdError::NotFound) => Err(OrdError::NotFound),
            Err(_) => {
                // Public ordinals.com (and modern ord) expose UTXO JSON under /r/utxo/
                let value = self.get_json(&format!("{}/r/utxo/{outpoint}", self.cfg.base_url))?;
                Ok(parse_ord_output(&value))
            }
        }
    }

    /// Same as [`output`] but never errors — Unknown-friendly for listing.
    /// If local ord is down, also tries https://ordinals.com/r/utxo (mainnet public).
    pub fn output_best_effort(&self, txid: &str, vout: u32) -> OrdOutputInfo {
        match self.output(txid, vout) {
            Ok(info) => info,
            Err(OrdError::NotFound) => OrdOutputInfo {
                found: false,
                ..Default::default()
            },
            Err(_) => {
                if self.cfg.base_url.contains("ordinals.com") {
                    return OrdOutputInfo::default();
                }
                let public = Self::new(OrdConfig {
                    base_url: "https://ordinals.com".into(),
                });
                match public.output(txid, vout) {
                    Ok(info) => info,
                    Err(OrdError::NotFound) => OrdOutputInfo {
                        found: false,
                        ..Default::default()
                    },
                    Err(_) => OrdOutputInfo::default(),
                }
            }
        }
    }

    fn get_json(&self, url: &str) -> Result<Value, OrdError> {
        let resp = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(8))
            .build()
            .get(url)
            .set("Accept", "application/json")
            .call();

        match resp {
            Ok(r) => r.into_json().map_err(|e| OrdError::Json(e.to_string())),
            Err(ureq::Error::Status(404, _)) => Err(OrdError::NotFound),
            Err(ureq::Error::Status(_, r)) => {
                let body = r.into_string().unwrap_or_default();
                Err(OrdError::Http(body))
            }
            Err(e) => Err(OrdError::Http(e.to_string())),
        }
    }
}

fn parse_ord_output(v: &Value) -> OrdOutputInfo {
    let mut inscription_ids = Vec::new();
    if let Some(arr) = v.get("inscriptions").and_then(|x| x.as_array()) {
        for item in arr {
            if let Some(s) = item.as_str() {
                inscription_ids.push(s.to_string());
            } else if let Some(id) = item.get("id").and_then(|x| x.as_str()) {
                inscription_ids.push(id.to_string());
            }
        }
    }

    let rune_summary = v
        .get("runes")
        .and_then(|r| {
            if r.is_null() {
                None
            } else if let Some(s) = r.as_str() {
                Some(s.to_string())
            } else {
                Some(r.to_string())
            }
        })
        .filter(|s| s != "null" && s != "{}" && s != "[]");

    OrdOutputInfo {
        inscription_ids,
        rune_summary,
        found: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_inscriptions_array_of_strings() {
        let v = json!({
            "inscriptions": ["abci0", "defi1"],
            "runes": null
        });
        let info = parse_ord_output(&v);
        assert_eq!(info.inscription_ids, vec!["abci0", "defi1"]);
        assert!(info.found);
        assert!(info.rune_summary.is_none());
    }

    #[test]
    fn parse_runes_object() {
        let v = json!({
            "inscriptions": [],
            "runes": { "A•B": { "amount": 1 } }
        });
        let info = parse_ord_output(&v);
        assert!(info.rune_summary.unwrap().contains("A•B"));
    }
}
