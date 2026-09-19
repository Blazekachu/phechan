//! Structural validation for inscription reveal transactions.

use bitcoin::Transaction;

use crate::ValidationReport;

/// Conservative P2TR dust approximation for policy checks (not consensus).
const P2TR_DUST_SATS: u64 = 330;

/// Validate a reveal transaction structurally for a known text body.
pub fn validate_inscription_reveal(tx: &Transaction, expected_body: &[u8]) -> ValidationReport {
    let mut report = ValidationReport::default();
    report.runes_ok = Some(true); // no runestone expected in phase-1 text path
    report.asset_disclosure.push(
        "phase1: inscription-only path; rune/parent disclosure lands in phase 2".into(),
    );

    if tx.input.is_empty() {
        report.errors.push("reveal tx has no inputs".into());
    }
    if tx.output.is_empty() {
        report.errors.push("reveal tx has no outputs".into());
    }

    let mut found_ord = false;
    let mut found_body = expected_body.is_empty();
    // Ordinal content is pushed in ≤520-byte chunks — full body may not be contiguous.
    let body_probe: &[u8] = if expected_body.len() > 520 {
        &expected_body[..520]
    } else {
        expected_body
    };
    for input in &tx.input {
        for item in input.witness.iter() {
            if item.windows(3).any(|w| w == b"ord") {
                found_ord = true;
            }
            if !found_body
                && !body_probe.is_empty()
                && item.windows(body_probe.len()).any(|w| w == body_probe)
            {
                found_body = true;
            }
        }
    }

    if !found_ord {
        report
            .errors
            .push("witness missing ord inscription envelope marker".into());
        report.ordinals_ok = Some(false);
    } else if !found_body {
        report
            .errors
            .push("witness missing expected inscription body".into());
        report.ordinals_ok = Some(false);
    } else {
        report.ordinals_ok = Some(true);
    }

    for (i, out) in tx.output.iter().enumerate() {
        // OP_RETURN / nulldata is allowed at value 0 (inscription message output).
        if out.script_pubkey.is_op_return() {
            continue;
        }
        if out.value.to_sat() == 0 {
            report.errors.push(format!("output {i} has zero value"));
        } else if out.value.to_sat() < P2TR_DUST_SATS {
            report.warnings.push(format!(
                "output {i} value {} may be dust under relay policy (threshold {P2TR_DUST_SATS})",
                out.value.to_sat()
            ));
            report.relay_ok = Some(false);
        }
    }
    if report.relay_ok.is_none() && report.errors.is_empty() {
        report.relay_ok = Some(true);
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
    use bitcoin::absolute::LockTime;
    use bitcoin::transaction::{OutPoint, Sequence, TxIn, TxOut, Version};
    use bitcoin::{Amount, ScriptBuf, Txid, Witness};
    use bitcoin::hashes::Hash;

    fn bare_tx_with_witness(witness_blob: Vec<u8>, value: u64) -> Transaction {
        let mut witness = Witness::new();
        witness.push([]); // sig placeholder
        witness.push(witness_blob);
        witness.push([]); // control placeholder
        Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint {
                    txid: Txid::from_byte_array([2u8; 32]),
                    vout: 0,
                },
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness,
            }],
            output: vec![TxOut {
                value: Amount::from_sat(value),
                script_pubkey: ScriptBuf::new(),
            }],
        }
    }

    #[test]
    fn missing_envelope_errors() {
        let tx = bare_tx_with_witness(b"nope".to_vec(), 1000);
        let report = validate_inscription_reveal(&tx, b"Hello");
        assert!(!report.allows_broadcast());
        assert_eq!(report.ordinals_ok, Some(false));
    }

    #[test]
    fn happy_path_allows_broadcast() {
        let mut script = Vec::new();
        script.extend_from_slice(&[0x00, 0x63]);
        script.extend_from_slice(b"ord");
        script.extend_from_slice(b"Hello");
        script.push(0x68);
        let tx = bare_tx_with_witness(script, 1000);
        let report = validate_inscription_reveal(&tx, b"Hello");
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(report.allows_broadcast());
    }
}
