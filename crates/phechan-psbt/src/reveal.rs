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
}

/// Parent+child reveal: input0=parent, input1=commit; out0=vault (parent return), out1=child dest.
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
}

/// Build unsigned reveal PSBT: spend commit via tapscript to one destination output.
pub fn build_reveal_psbt(params: RevealPsbtParams) -> Result<Psbt, PsbtBuildError> {
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
        output: vec![TxOut {
            value: params.destination_value,
            script_pubkey: params.destination_script_pubkey,
        }],
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
        output: vec![
            TxOut {
                value: params.vault_value,
                script_pubkey: params.vault_script_pubkey,
            },
            TxOut {
                value: params.child_value,
                script_pubkey: params.child_script_pubkey,
            },
        ],
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
