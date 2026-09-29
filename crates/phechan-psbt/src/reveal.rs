//! Unsigned reveal PSBT spending the commit output via script path.

use bitcoin::absolute::LockTime;
use bitcoin::psbt::{Input as PsbtInput, Psbt};
use bitcoin::taproot::{LeafVersion, TaprootSpendInfo};
use bitcoin::transaction::{OutPoint, Sequence, TxIn, TxOut, Version};
use bitcoin::{Amount, ScriptBuf, Transaction, Txid};

use crate::PsbtBuildError;

pub struct RevealPsbtParams {
    pub commit_txid: Txid,
    pub commit_vout: u32,
    pub commit_value: Amount,
    pub commit_script_pubkey: ScriptBuf,
    pub destination_script_pubkey: ScriptBuf,
    pub destination_value: Amount,
    pub leaf_script: ScriptBuf,
    pub spend_info: TaprootSpendInfo,
    /// Optional OP_RETURN payload (after dest [+ change]). Max 80 bytes for standard relay.
    pub op_return: Option<Vec<u8>>,
    /// Optional change back to payment address when commit was over-funded for reveal fee.
    pub change_script_pubkey: Option<ScriptBuf>,
    pub change_value: Amount,
}

/// Parent+child reveal: input0=parent, input1=commit; out0=vault, out1=child [, out2=change].
pub struct ParentChildRevealParams {
    pub parent_txid: Txid,
    pub parent_vout: u32,
    pub parent_value: Amount,
    pub parent_script_pubkey: ScriptBuf,
    /// X-only internal key for parent P2TR (Xverse / Leather need this to sign).
    pub parent_tap_internal_key: Option<bitcoin::XOnlyPublicKey>,
    pub commit_txid: Txid,
    pub commit_vout: u32,
    pub commit_value: Amount,
    pub commit_script_pubkey: ScriptBuf,
    pub vault_script_pubkey: ScriptBuf,
    pub vault_value: Amount,
    pub child_script_pubkey: ScriptBuf,
    pub child_value: Amount,
    pub leaf_script: ScriptBuf,
    pub spend_info: TaprootSpendInfo,
    /// Optional change (vout2) — surplus commit sats after postage + miner fee.
    pub change_script_pubkey: Option<ScriptBuf>,
    pub change_value: Amount,
}

/// Build unsigned reveal PSBT: spend commit via tapscript to destination [+ change] [+ OP_RETURN].
pub fn build_reveal_psbt(params: RevealPsbtParams) -> Result<Psbt, PsbtBuildError> {
    let mut outputs = vec![TxOut {
        value: params.destination_value,
        script_pubkey: params.destination_script_pubkey,
    }];
    if params.change_value.to_sat() > 0 {
        let spk = params.change_script_pubkey.ok_or_else(|| {
            PsbtBuildError::Message("change_value set but change_script_pubkey missing".into())
        })?;
        outputs.push(TxOut {
            value: params.change_value,
            script_pubkey: spk,
        });
    }
    if let Some(data) = params.op_return {
        if data.len() > 80 {
            return Err(PsbtBuildError::Message(
                "OP_RETURN payload exceeds 80-byte standard relay limit".into(),
            ));
        }
        let push: &bitcoin::script::PushBytes = data.as_slice().try_into().map_err(|_| {
            PsbtBuildError::Message("OP_RETURN payload too large for push".into())
        })?;
        outputs.push(TxOut {
            value: Amount::ZERO,
            script_pubkey: ScriptBuf::new_op_return(push),
        });
    }

    let tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint {
                txid: params.commit_txid,
                vout: params.commit_vout,
            },
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: bitcoin::Witness::new(),
        }],
        output: outputs,
    };

    let mut psbt = Psbt::from_unsigned_tx(tx)
        .map_err(|e| PsbtBuildError::Message(format!("psbt: {e}")))?;

    let control_block = params
        .spend_info
        .control_block(&(params.leaf_script.clone(), LeafVersion::TapScript))
        .ok_or_else(|| PsbtBuildError::Message("missing control block for leaf".into()))?;

    let mut input = PsbtInput {
        witness_utxo: Some(TxOut {
            value: params.commit_value,
            script_pubkey: params.commit_script_pubkey,
        }),
        ..Default::default()
    };
    input.tap_scripts.insert(
        control_block.clone(),
        (params.leaf_script, LeafVersion::TapScript),
    );
    input.tap_internal_key = Some(params.spend_info.internal_key());
    psbt.inputs[0] = input;

    Ok(psbt)
}

/// Build FI/FO parent+child reveal PSBT (parent input first, vault output first).
pub fn build_parent_child_reveal_psbt(
    params: ParentChildRevealParams,
) -> Result<Psbt, PsbtBuildError> {
    let mut outputs = vec![
        TxOut {
            value: params.vault_value,
            script_pubkey: params.vault_script_pubkey,
        },
        TxOut {
            value: params.child_value,
            script_pubkey: params.child_script_pubkey,
        },
    ];
    if params.change_value.to_sat() > 0 {
        let spk = params.change_script_pubkey.ok_or_else(|| {
            PsbtBuildError::Message("change_value set but change_script_pubkey missing".into())
        })?;
        outputs.push(TxOut {
            value: params.change_value,
            script_pubkey: spk,
        });
    }

    let tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![
            TxIn {
                previous_output: OutPoint {
                    txid: params.parent_txid,
                    vout: params.parent_vout,
                },
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: bitcoin::Witness::new(),
            },
            TxIn {
                previous_output: OutPoint {
                    txid: params.commit_txid,
                    vout: params.commit_vout,
                },
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: bitcoin::Witness::new(),
            },
        ],
        output: outputs,
    };

    let mut psbt = Psbt::from_unsigned_tx(tx)
        .map_err(|e| PsbtBuildError::Message(format!("psbt: {e}")))?;

    // Parent input: wallet signs (key-path P2TR). Provide witness_utxo + tap_internal_key.
    psbt.inputs[0] = PsbtInput {
        witness_utxo: Some(TxOut {
            value: params.parent_value,
            script_pubkey: params.parent_script_pubkey,
        }),
        tap_internal_key: params.parent_tap_internal_key,
        ..Default::default()
    };

    let control_block = params
        .spend_info
        .control_block(&(params.leaf_script.clone(), LeafVersion::TapScript))
        .ok_or_else(|| PsbtBuildError::Message("missing control block for leaf".into()))?;

    let mut commit_in = PsbtInput {
        witness_utxo: Some(TxOut {
            value: params.commit_value,
            script_pubkey: params.commit_script_pubkey,
        }),
        ..Default::default()
    };
    commit_in
        .tap_scripts
        .insert(control_block, (params.leaf_script, LeafVersion::TapScript));
    commit_in.tap_internal_key = Some(params.spend_info.internal_key());
    psbt.inputs[1] = commit_in;

    Ok(psbt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::hashes::Hash;
    use bitcoin::key::Keypair;
    use bitcoin::secp256k1::Secp256k1;
    use bitcoin::taproot::TaprootBuilder;
    use bitcoin::XOnlyPublicKey;

    fn mock_commit() -> (ScriptBuf, TaprootSpendInfo, ScriptBuf) {
        let secp = Secp256k1::new();
        let kp = Keypair::from_seckey_slice(&secp, &[2u8; 32]).unwrap();
        let (xonly, _parity) = XOnlyPublicKey::from_keypair(&kp);
        let leaf = ScriptBuf::new_op_return(b"x");
        let builder = TaprootBuilder::new().add_leaf(0, leaf.clone()).unwrap();
        let spend = builder.finalize(&secp, xonly).unwrap();
        let spk = ScriptBuf::new_p2tr(&secp, spend.internal_key(), spend.merkle_root());
        (spk, spend, leaf)
    }

    #[test]
    fn parent_child_change_adds_vout2() {
        let (commit_spk, spend, leaf) = mock_commit();
        let dest = commit_spk.clone();
        let psbt = build_parent_child_reveal_psbt(ParentChildRevealParams {
            parent_txid: Txid::from_byte_array([1u8; 32]),
            parent_vout: 0,
            parent_value: Amount::from_sat(546),
            parent_script_pubkey: dest.clone(),
            parent_tap_internal_key: Some(spend.internal_key()),
            commit_txid: Txid::from_byte_array([2u8; 32]),
            commit_vout: 0,
            commit_value: Amount::from_sat(10_000),
            commit_script_pubkey: commit_spk,
            vault_script_pubkey: dest.clone(),
            vault_value: Amount::from_sat(546),
            child_script_pubkey: dest.clone(),
            child_value: Amount::from_sat(546),
            leaf_script: leaf,
            spend_info: spend,
            change_script_pubkey: Some(dest),
            change_value: Amount::from_sat(8_000),
        })
        .unwrap();
        assert_eq!(psbt.unsigned_tx.output.len(), 3);
        assert_eq!(psbt.unsigned_tx.output[2].value.to_sat(), 8_000);
    }
}
