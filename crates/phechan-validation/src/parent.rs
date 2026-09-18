//! Parent+child reveal validation using sat-flow + asset disclosure rules.

use phechan_runes::{rune_cospend_blocked, format_disclosure, LabeledUtxo, UtxoAssetHint};
use phechan_sat::{verify_parent_return, ParentPlacementPolicy};

use crate::ValidationReport;

pub struct ParentChildValidationInput<'a> {
    pub policy: ParentPlacementPolicy,
    pub parent_input_index: usize,
    pub parent_sat_offset: u64,
    pub vault_vout: usize,
    pub input_values: &'a [u64],
    pub output_values: &'a [u64],
    pub labeled_inputs: &'a [LabeledUtxo],
    pub has_validated_runestone: bool,
    pub allow_asset_bearing_fees: bool,
}

/// Validate parent return + asset disclosure constraints for a parent-child layout.
pub fn validate_parent_child_layout(input: ParentChildValidationInput<'_>) -> ValidationReport {
    let mut report = ValidationReport::default();
    report.asset_disclosure = format_disclosure(input.labeled_inputs);
    report.runes_ok = Some(true);
    report.relay_ok = Some(true);

    for u in input.labeled_inputs {
        if rune_cospend_blocked(u.hint, input.has_validated_runestone) {
            report.errors.push(format!(
                "{}:{} is rune-bearing but no validated runestone — refusing silent burn",
                u.txid_hex, u.vout
            ));
            report.runes_ok = Some(false);
        }
        if matches!(
            u.hint,
            UtxoAssetHint::Inscription
                | UtxoAssetHint::Rune
                | UtxoAssetHint::InscriptionAndRune
                | UtxoAssetHint::Unknown
        ) && !input.allow_asset_bearing_fees
        {
            // Parent input is expected to be inscription-bearing; only flag non-parent
            // fee candidates. Callers mark fee inputs via allow flag or we only warn.
            report.warnings.push(format!(
                "{}:{} is asset-bearing ({:?}) — ensure it is not used as fee accidentally",
                u.txid_hex, u.vout, u.hint
            ));
        }
    }

    match verify_parent_return(
        input.policy,
        input.parent_input_index,
        input.parent_sat_offset,
        input.vault_vout,
        input.input_values,
        input.output_values,
    ) {
        Ok(()) => {
            report.ordinals_ok = Some(true);
            report.warnings.push(format!(
                "parent sat @ input {} offset {} returns to vault vout {}",
                input.parent_input_index, input.parent_sat_offset, input.vault_vout
            ));
        }
        Err(e) => {
            report.ordinals_ok = Some(false);
            report.errors.push(format!("parent sat-flow: {e}"));
        }
    }

    if report.errors.is_empty() {
        report.consensus_ok = Some(true);
    } else {
        report.consensus_ok = Some(false);
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parent_utxo() -> LabeledUtxo {
        LabeledUtxo {
            txid_hex: "ab".into(),
            vout: 0,
            value: 1000,
            hint: UtxoAssetHint::Inscription,
            inscription_ids: vec![
                "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1fi0".into(),
            ],
            rune_summary: None,
        }
    }

    #[test]
    fn fee_tail_regression_blocks_broadcast() {
        let labeled = [parent_utxo()];
        let report = validate_parent_child_layout(ParentChildValidationInput {
            policy: ParentPlacementPolicy::Custom,
            parent_input_index: 1,
            parent_sat_offset: 0,
            vault_vout: 1,
            input_values: &[5000, 1000],
            output_values: &[5000],
            labeled_inputs: &labeled,
            has_validated_runestone: false,
            allow_asset_bearing_fees: true,
        });
        assert!(!report.allows_broadcast());
        assert!(report.errors.iter().any(|e| e.contains("fee") || e.contains("parent")));
    }

    #[test]
    fn fifo_layout_ok() {
        let labeled = [parent_utxo()];
        let report = validate_parent_child_layout(ParentChildValidationInput {
            policy: ParentPlacementPolicy::FirstInFirstOut,
            parent_input_index: 0,
            parent_sat_offset: 0,
            vault_vout: 0,
            input_values: &[1000, 5000],
            output_values: &[1000, 4800],
            labeled_inputs: &labeled,
            has_validated_runestone: false,
            allow_asset_bearing_fees: true,
        });
        assert!(report.allows_broadcast(), "{:?}", report.errors);
    }

    #[test]
    fn rune_parent_without_runestone_blocked() {
        let mut u = parent_utxo();
        u.hint = UtxoAssetHint::InscriptionAndRune;
        u.rune_summary = Some("FAKE•RUNE".into());
        let report = validate_parent_child_layout(ParentChildValidationInput {
            policy: ParentPlacementPolicy::FirstInFirstOut,
            parent_input_index: 0,
            parent_sat_offset: 0,
            vault_vout: 0,
            input_values: &[1000, 5000],
            output_values: &[1000, 4800],
            labeled_inputs: &[u],
            has_validated_runestone: false,
            allow_asset_bearing_fees: true,
        });
        assert!(!report.allows_broadcast());
        assert_eq!(report.runes_ok, Some(false));
    }
}
