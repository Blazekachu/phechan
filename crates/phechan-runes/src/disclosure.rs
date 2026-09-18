//! UTXO asset labeling and human disclosure lines.

use crate::UtxoAssetHint;

/// A UTXO with optional asset metadata for disclosure (indexer may be offline → Unknown).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabeledUtxo {
    pub txid_hex: String,
    pub vout: u32,
    pub value: u64,
    pub hint: UtxoAssetHint,
    pub inscription_ids: Vec<String>,
    pub rune_summary: Option<String>,
}

/// Format disclosure lines for selected UTXOs.
pub fn format_disclosure(utxos: &[LabeledUtxo]) -> Vec<String> {
    utxos
        .iter()
        .map(|u| {
            let outpoint = format!("{}:{}", u.txid_hex, u.vout);
            match u.hint {
                UtxoAssetHint::Plain => {
                    format!("{outpoint} value={} — plain (no known inscription/rune)", u.value)
                }
                UtxoAssetHint::Inscription => format!(
                    "{outpoint} value={} — carries inscription(s): {}",
                    u.value,
                    if u.inscription_ids.is_empty() {
                        "(ids unknown)".into()
                    } else {
                        u.inscription_ids.join(", ")
                    }
                ),
                UtxoAssetHint::Rune => format!(
                    "{outpoint} value={} — carries runes: {}",
                    u.value,
                    u.rune_summary.as_deref().unwrap_or("(balances unknown)")
                ),
                UtxoAssetHint::InscriptionAndRune => format!(
                    "{outpoint} value={} — carries inscription(s): {}; runes: {}",
                    u.value,
                    if u.inscription_ids.is_empty() {
                        "(ids unknown)".into()
                    } else {
                        u.inscription_ids.join(", ")
                    },
                    u.rune_summary.as_deref().unwrap_or("(balances unknown)")
                ),
                UtxoAssetHint::Unknown => format!(
                    "{outpoint} value={} — UNKNOWN assets (indexer unavailable); spend may burn inscriptions/runes",
                    u.value
                ),
            }
        })
        .collect()
}

/// Whether this UTXO may be used as a fee/funding input without explicit override.
pub fn fee_input_allowed(hint: UtxoAssetHint, allow_asset_bearing_fees: bool) -> bool {
    match hint {
        UtxoAssetHint::Plain => true,
        UtxoAssetHint::Unknown => allow_asset_bearing_fees,
        UtxoAssetHint::Inscription
        | UtxoAssetHint::Rune
        | UtxoAssetHint::InscriptionAndRune => allow_asset_bearing_fees,
    }
}

/// Spending a rune-bearing UTXO without a validated runestone is blocked by default.
pub fn rune_cospend_blocked(hint: UtxoAssetHint, has_validated_runestone: bool) -> bool {
    let has_runes = matches!(
        hint,
        UtxoAssetHint::Rune | UtxoAssetHint::InscriptionAndRune
    );
    has_runes && !has_validated_runestone
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disclosure_mentions_unknown_risk() {
        let lines = format_disclosure(&[LabeledUtxo {
            txid_hex: "aa".into(),
            vout: 0,
            value: 1000,
            hint: UtxoAssetHint::Unknown,
            inscription_ids: vec![],
            rune_summary: None,
        }]);
        assert!(lines[0].contains("UNKNOWN"));
    }

    #[test]
    fn fee_input_prefers_plain() {
        assert!(fee_input_allowed(UtxoAssetHint::Plain, false));
        assert!(!fee_input_allowed(UtxoAssetHint::Inscription, false));
        assert!(fee_input_allowed(UtxoAssetHint::Inscription, true));
    }

    #[test]
    fn rune_without_runestone_blocked() {
        assert!(rune_cospend_blocked(UtxoAssetHint::Rune, false));
        assert!(!rune_cospend_blocked(UtxoAssetHint::Rune, true));
        assert!(!rune_cospend_blocked(UtxoAssetHint::Plain, false));
    }
}
