//! Unsigned PSBT that creates a Taproot commit output.

use bitcoin::absolute::LockTime;
use bitcoin::key::XOnlyPublicKey;
use bitcoin::psbt::Psbt;
use bitcoin::transaction::{OutPoint, Sequence, TxIn, TxOut, Version};
use bitcoin::{Amount, ScriptBuf, Transaction};

#[derive(Debug)]
pub enum PsbtBuildError {
    Message(String),
}

impl std::fmt::Display for PsbtBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Message(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for PsbtBuildError {}

/// One funding/carrier input for a commit PSBT.
pub struct CommitFundingInput {
    pub outpoint: OutPoint,
    pub value: Amount,
    pub script_pubkey: ScriptBuf,
    /// Nested P2SH-P2WPKH redeem (payment).
    pub redeem_script: Option<ScriptBuf>,
    /// P2TR key-path (ordinals / parent carrier) — required for Xverse.
    pub tap_internal_key: Option<XOnlyPublicKey>,
}

pub struct CommitPsbtParams {
    pub funding_outpoint: OutPoint,
    pub funding_value: Amount,
    pub funding_script_pubkey: ScriptBuf,
    /// Required for nested P2SH-P2WPKH payment inputs (Xverse). Redeem = P2WPKH script.
    pub funding_redeem_script: Option<ScriptBuf>,
    pub commit_script_pubkey: ScriptBuf,
    pub commit_value: Amount,
    pub change_script_pubkey: Option<ScriptBuf>,
    pub change_value: Amount,
}

/// Build an unsigned commit transaction PSBT (1 funding input → commit [+ optional change]).
pub fn build_commit_psbt(params: CommitPsbtParams) -> Result<Psbt, PsbtBuildError> {
    build_commit_psbt_multi(
        vec![CommitFundingInput {
            outpoint: params.funding_outpoint,
            value: params.funding_value,
            script_pubkey: params.funding_script_pubkey,
            redeem_script: params.funding_redeem_script,
            tap_internal_key: None,
        }],
        params.commit_script_pubkey,
        params.commit_value,
        params.change_script_pubkey,
        params.change_value,
    )
}

/// Multi-input commit fund: e.g. vin0 = parent carrier, vin1 = payment top-up.
/// Commit is always vout0 so parent sat at offset 0 lands in the commit (FI/FO).
pub fn build_commit_psbt_multi(
    inputs: Vec<CommitFundingInput>,
    commit_script_pubkey: ScriptBuf,
    commit_value: Amount,
    change_script_pubkey: Option<ScriptBuf>,
    change_value: Amount,
) -> Result<Psbt, PsbtBuildError> {
    if inputs.is_empty() {
        return Err(PsbtBuildError::Message(
            "commit fund needs at least one input".into(),
        ));
    }

    let mut outputs = vec![TxOut {
        value: commit_value,
        script_pubkey: commit_script_pubkey,
    }];
    if let Some(change_spk) = change_script_pubkey {
        if change_value.to_sat() > 0 {
            outputs.push(TxOut {
                value: change_value,
                script_pubkey: change_spk,
            });
        }
    }

    let tx_inputs: Vec<TxIn> = inputs
        .iter()
        .map(|i| TxIn {
            previous_output: i.outpoint,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: bitcoin::Witness::new(),
        })
        .collect();

    let tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: tx_inputs,
        output: outputs,
    };

    let mut psbt = Psbt::from_unsigned_tx(tx)
        .map_err(|e| PsbtBuildError::Message(format!("psbt: {e}")))?;

    for (i, inp) in inputs.into_iter().enumerate() {
        psbt.inputs[i].witness_utxo = Some(TxOut {
            value: inp.value,
            script_pubkey: inp.script_pubkey.clone(),
        });
        if let Some(redeem) = inp.redeem_script {
            psbt.inputs[i].redeem_script = Some(redeem);
        }
        if let Some(tik) = inp.tap_internal_key {
            psbt.inputs[i].tap_internal_key = Some(tik);
        }
    }
    Ok(psbt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::hashes::Hash;
    use bitcoin::key::PublicKey;
    use bitcoin::Txid;

    fn dummy_txid(n: u8) -> Txid {
        Txid::from_byte_array([n; 32])
    }

    fn p2wpkh_spk() -> ScriptBuf {
        let pk = PublicKey::from_slice(&[
            0x02, 0x79, 0xbe, 0x66, 0x7e, 0xf9, 0xdc, 0xbb, 0xac, 0x55, 0xa0, 0x62, 0x95, 0xce,
            0x87, 0x0b, 0x07, 0x02, 0x9b, 0xfc, 0xdb, 0x2d, 0xce, 0x28, 0xd9, 0x59, 0xf2, 0x81,
            0x5b, 0x16, 0xf8, 0x17, 0x98,
        ])
        .unwrap();
        ScriptBuf::new_p2wpkh(&pk.wpubkey_hash().unwrap())
    }

    #[test]
    fn commit_psbt_has_witness_utxo() {
        let funding_spk = p2wpkh_spk();
        let commit_spk = p2wpkh_spk();
        let psbt = build_commit_psbt(CommitPsbtParams {
            funding_outpoint: OutPoint {
                txid: dummy_txid(1),
                vout: 0,
            },
            funding_value: Amount::from_sat(10_000),
            funding_script_pubkey: funding_spk,
            funding_redeem_script: None,
            commit_script_pubkey: commit_spk,
            commit_value: Amount::from_sat(5_000),
            change_script_pubkey: None,
            change_value: Amount::ZERO,
        })
        .unwrap();
        assert!(psbt.inputs[0].witness_utxo.is_some());
    }

    #[test]
    fn multi_input_carrier_first() {
        // Valid x-only from secp256k1 generator
        let secp = bitcoin::secp256k1::Secp256k1::new();
        let kp = bitcoin::secp256k1::Keypair::from_seckey_slice(&secp, &[4u8; 32]).unwrap();
        let (xonly, _) = kp.x_only_public_key();
        let carrier_spk = ScriptBuf::new_p2tr(&secp, xonly, None);
        let pay_spk = p2wpkh_spk();
        let commit_spk = carrier_spk.clone();
        let psbt = build_commit_psbt_multi(
            vec![
                CommitFundingInput {
                    outpoint: OutPoint {
                        txid: dummy_txid(1),
                        vout: 0,
                    },
                    value: Amount::from_sat(546),
                    script_pubkey: carrier_spk,
                    redeem_script: None,
                    tap_internal_key: Some(xonly),
                },
                CommitFundingInput {
                    outpoint: OutPoint {
                        txid: dummy_txid(2),
                        vout: 1,
                    },
                    value: Amount::from_sat(10_000),
                    script_pubkey: pay_spk,
                    redeem_script: None,
                    tap_internal_key: None,
                },
            ],
            commit_spk,
            Amount::from_sat(1_000),
            None,
            Amount::ZERO,
        )
        .unwrap();
        assert_eq!(psbt.inputs.len(), 2);
        assert!(psbt.inputs[0].tap_internal_key.is_some());
        assert_eq!(psbt.unsigned_tx.output[0].value.to_sat(), 1_000);
    }
}
