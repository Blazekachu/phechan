//! Regtest Taproot script-path signing and finalization.

use bitcoin::key::Keypair;
use bitcoin::psbt::Psbt;
use bitcoin::secp256k1::{Message, Secp256k1};
use bitcoin::sighash::{Prevouts, SighashCache, TapSighashType};
use bitcoin::taproot::{LeafVersion, Signature as TaprootSignature};
use bitcoin::{ScriptBuf, Transaction, TxOut, Witness};

use crate::PsbtBuildError;

/// Sign input 0 of a reveal PSBT with Taproot script-path spend.
pub fn sign_reveal_script_path(
    psbt: Psbt,
    keypair: &Keypair,
    leaf_script: &ScriptBuf,
) -> Result<Psbt, PsbtBuildError> {
    sign_reveal_script_path_at(psbt, 0, keypair, leaf_script)
}

/// Sign input `index` with Taproot script-path spend (SIGHASH_ALL / Default).
pub fn sign_reveal_script_path_at(
    mut psbt: Psbt,
    index: usize,
    keypair: &Keypair,
    leaf_script: &ScriptBuf,
) -> Result<Psbt, PsbtBuildError> {
    if index >= psbt.inputs.len() {
        return Err(PsbtBuildError::Message("input index out of range".into()));
    }

    let (control_block, (script, leaf_version)) = psbt.inputs[index]
        .tap_scripts
        .iter()
        .next()
        .map(|(cb, v)| (cb.clone(), v.clone()))
        .ok_or_else(|| PsbtBuildError::Message("missing tap_scripts".into()))?;

    if &script != leaf_script || leaf_version != LeafVersion::TapScript {
        return Err(PsbtBuildError::Message("leaf script mismatch".into()));
    }

    let mut prevouts: Vec<TxOut> = Vec::with_capacity(psbt.inputs.len());
    for (i, inp) in psbt.inputs.iter().enumerate() {
        let utxo = inp.witness_utxo.clone().ok_or_else(|| {
            PsbtBuildError::Message(format!("missing witness_utxo on input {i}"))
        })?;
        prevouts.push(utxo);
    }

    let leaf_hash = leaf_script.tapscript_leaf_hash();
    let secp = Secp256k1::new();
    let unsigned_tx = psbt.unsigned_tx.clone();
    let mut cache = SighashCache::new(&unsigned_tx);
    let sighash = cache
        .taproot_script_spend_signature_hash(
            index,
            &Prevouts::All(&prevouts),
            leaf_hash,
            TapSighashType::Default,
        )
        .map_err(|e| PsbtBuildError::Message(format!("sighash: {e}")))?;

    let msg = Message::from_digest_slice(sighash.as_ref())
        .map_err(|e| PsbtBuildError::Message(format!("message: {e}")))?;
    let sig = secp.sign_schnorr_no_aux_rand(&msg, keypair);
    let tap_sig = TaprootSignature {
        signature: sig,
        sighash_type: TapSighashType::Default,
    };

    let mut witness = Witness::new();
    witness.push(tap_sig.to_vec());
    witness.push(leaf_script.as_bytes());
    witness.push(control_block.serialize());
    psbt.inputs[index].final_script_witness = Some(witness);
    Ok(psbt)
}

/// Build the non-witness TXID template used for pre-sign vanity grinding.
///
/// For nested P2SH-P2WPKH, the final TXID includes the redeem push in `scriptSig`
/// (witness does not). Grinding `unsigned_tx` with empty scriptSig predicts the
/// wrong TXID — same bug runes-etch / sort-utxo fixed via `serializeForTxid`.
pub fn txid_grind_template(psbt: &Psbt) -> Result<Transaction, PsbtBuildError> {
    use bitcoin::script::{Builder, PushBytesBuf};

    let mut tx = psbt.unsigned_tx.clone();
    for (i, input) in psbt.inputs.iter().enumerate() {
        if let Some(redeem) = &input.redeem_script {
            let p2sh = input
                .witness_utxo
                .as_ref()
                .map(|u| u.script_pubkey.is_p2sh())
                .unwrap_or(true);
            if p2sh {
                let push = PushBytesBuf::try_from(redeem.to_bytes()).map_err(|_| {
                    PsbtBuildError::Message(format!(
                        "input {i}: redeemScript too large for TXID template"
                    ))
                })?;
                tx.input[i].script_sig = Builder::new().push_slice(&*push).into_script();
            }
        }
    }
    Ok(tx)
}

/// Build a P2WPKH witness `[sig, pubkey]` from PSBT `partial_sigs`.
///
/// Xverse / sats-connect often returns signed-but-unfinalized PSBTs (like bitcoinjs
/// before `finalizeAllInputs`). Core reports empty/wrong-sized witness as
/// `Witness program hash mismatch`.
fn witness_from_partial_sigs(
    input: &bitcoin::psbt::Input,
) -> Option<Witness> {
    use bitcoin::hashes::Hash;

    if input.partial_sigs.is_empty() {
        return None;
    }

    // Prefer the pubkey that matches nested redeem (OP_0 <wpkh>) or native P2WPKH spk.
    let want_wpkh = input.redeem_script.as_ref().and_then(|r| {
        let b = r.as_bytes();
        // 0x00 0x14 <20-byte-hash>
        if b.len() == 22 && b[0] == 0x00 && b[1] == 0x14 {
            bitcoin::WPubkeyHash::from_slice(&b[2..]).ok()
        } else {
            None
        }
    }).or_else(|| {
        input.witness_utxo.as_ref().and_then(|u| {
            if u.script_pubkey.is_p2wpkh() {
                bitcoin::WPubkeyHash::from_slice(&u.script_pubkey.as_bytes()[2..]).ok()
            } else {
                None
            }
        })
    });

    let (pk, sig) = if let Some(wpkh) = want_wpkh {
        input
            .partial_sigs
            .iter()
            .find(|(pk, _)| pk.wpubkey_hash().ok().as_ref() == Some(&wpkh))
            .or_else(|| input.partial_sigs.iter().next())?
    } else {
        input.partial_sigs.iter().next()?
    };

    let mut wit = Witness::new();
    wit.push(sig.to_vec());
    wit.push(pk.to_bytes());
    Some(wit)
}

/// Build a P2TR key-path witness from `tap_key_sig` (BIP-371).
///
/// Xverse often signs parent ordinals inputs with `PSBT_IN_TAP_KEY_SIG` and does
/// not set `final_script_witness` — same role as bitcoinjs `finalizeInput`.
fn witness_from_tap_key_sig(input: &bitcoin::psbt::Input) -> Option<Witness> {
    let sig = input.tap_key_sig.as_ref()?;
    Some(Witness::p2tr_key_spend(sig))
}

/// Build a Taproot **script-path** witness from `tap_script_sigs` + `tap_scripts`.
///
/// Wallets may leave script-path reveals unfinalized (sig map only). Prefer this
/// over `tap_key_sig` whenever `tap_scripts` is present — key-path on a commit
/// leaf (often NUMS internal key) yields `Invalid Schnorr signature` at broadcast.
fn witness_from_tap_script_sigs(input: &bitcoin::psbt::Input) -> Option<Witness> {
    let (control_block, (script, leaf_version)) = input.tap_scripts.iter().next()?;
    if *leaf_version != LeafVersion::TapScript {
        return None;
    }
    let leaf_hash = script.tapscript_leaf_hash();
    let sig = input
        .tap_script_sigs
        .iter()
        .find(|((_, lh), _)| *lh == leaf_hash)
        .map(|(_, s)| s)
        .or_else(|| input.tap_script_sigs.values().next())?;
    let mut wit = Witness::new();
    wit.push(sig.to_vec());
    wit.push(script.as_bytes());
    wit.push(control_block.serialize());
    Some(wit)
}

fn witness_looks_like_key_path(w: &Witness) -> bool {
    // Key-path: <sig> or <sig> <annex>. Script-path: <sig> <script> <control>.
    w.len() <= 2
}

/// Extract a finalized transaction from a PSBT.
///
/// Copies `final_script_sig` + `final_script_witness`. If the wallet left only
/// `partial_sigs` (Xverse), builds the P2WPKH witness — same role as bitcoinjs
/// `finalizeAllInputs`. For nested P2SH-P2WPKH, synthesizes the BIP-141 redeem
/// push from `redeem_script` when `final_script_sig` is empty. Also finalizes
/// Taproot key-path spends from `tap_key_sig` (parent input of parent-child reveal)
/// and script-path from `tap_script_sigs` (commit input).
pub fn finalize_to_tx(psbt: &Psbt) -> Result<Transaction, PsbtBuildError> {
    use bitcoin::script::{Builder, PushBytesBuf};

    let mut tx = psbt.unsigned_tx.clone();
    for (i, input) in psbt.inputs.iter().enumerate() {
        if let Some(sig) = &input.final_script_sig {
            if !sig.is_empty() {
                tx.input[i].script_sig = sig.clone();
            }
        }

        // Nested segwit: scriptSig must push the redeem script (p2wpkh program).
        if tx.input[i].script_sig.is_empty() {
            if let Some(redeem) = &input.redeem_script {
                let p2sh = input
                    .witness_utxo
                    .as_ref()
                    .map(|u| u.script_pubkey.is_p2sh())
                    .unwrap_or(false);
                if p2sh {
                    let push = PushBytesBuf::try_from(redeem.to_bytes()).map_err(|_| {
                        PsbtBuildError::Message(format!("input {i}: redeemScript too large to push"))
                    })?;
                    tx.input[i].script_sig = Builder::new().push_slice(&*push).into_script();
                }
            }
        }

        let has_tap_scripts = !input.tap_scripts.is_empty();
        let witness = match &input.final_script_witness {
            // Wallet sometimes attaches a 1-stack key-path witness on a script-path
            // commit input — reject that and rebuild from tap_script_sigs.
            Some(w)
                if !w.is_empty()
                    && !(has_tap_scripts && witness_looks_like_key_path(w)) =>
            {
                Some(w.clone())
            }
            _ if has_tap_scripts => witness_from_tap_script_sigs(input)
                .or_else(|| witness_from_partial_sigs(input)),
            _ => witness_from_partial_sigs(input).or_else(|| witness_from_tap_key_sig(input)),
        };

        if let Some(witness) = witness {
            tx.input[i].witness = witness;
        } else if tx.input[i].script_sig.is_empty() {
            return Err(PsbtBuildError::Message(format!(
                "input {i} not finalized (missing final_script_witness / tap_script_sigs / tap_key_sig / partial_sigs / final_script_sig)"
            )));
        }

        // Segwit spends need a non-empty witness (nested P2SH-P2WPKH included).
        if let Some(utxo) = &input.witness_utxo {
            let needs_witness = utxo.script_pubkey.is_witness_program()
                || (utxo.script_pubkey.is_p2sh() && input.redeem_script.is_some());
            if needs_witness && tx.input[i].witness.is_empty() {
                return Err(PsbtBuildError::Message(format!(
                    "input {i}: segwit input missing witness (wallet returned partial_sigs unset / unfinalized?)"
                )));
            }
            if utxo.script_pubkey.is_p2sh() && tx.input[i].script_sig.is_empty() {
                return Err(PsbtBuildError::Message(format!(
                    "input {i}: P2SH/nested segwit missing redeem scriptSig (wallet stripped redeemScript?)"
                )));
            }
        }
    }
    Ok(tx)
}

/// Ensure nested P2SH inputs have `redeem_script` set from a compressed/x-only payment pubkey.
pub fn ensure_nested_redeem_from_pubkey(
    psbt: &mut Psbt,
    pubkey_hex: &str,
) -> Result<(), PsbtBuildError> {
    use bitcoin::key::PublicKey;
    use bitcoin::ScriptBuf;

    let mut raw = hex::decode(pubkey_hex.trim())
        .map_err(|e| PsbtBuildError::Message(format!("funding pubkey hex: {e}")))?;
    if raw.len() == 32 {
        let mut compressed = Vec::with_capacity(33);
        compressed.push(0x02);
        compressed.extend_from_slice(&raw);
        raw = compressed;
    }
    let pk = PublicKey::from_slice(&raw)
        .map_err(|e| PsbtBuildError::Message(format!("funding pubkey: {e}")))?;
    let wpkh = pk
        .wpubkey_hash()
        .map_err(|e| PsbtBuildError::Message(format!("funding pubkey wpkh: {e}")))?;
    let redeem = ScriptBuf::new_p2wpkh(&wpkh);
    let expected_p2sh = ScriptBuf::new_p2sh(&redeem.script_hash());

    for (i, input) in psbt.inputs.iter_mut().enumerate() {
        let Some(utxo) = input.witness_utxo.as_ref() else {
            continue;
        };
        if !utxo.script_pubkey.is_p2sh() {
            continue;
        }
        if utxo.script_pubkey != expected_p2sh {
            return Err(PsbtBuildError::Message(format!(
                "input {i}: funding pubkey does not match P2SH scriptPubKey"
            )));
        }
        if input.redeem_script.is_none() {
            input.redeem_script = Some(redeem.clone());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::hashes::Hash;
    use bitcoin::{Amount, Txid};
    use phechan_bitcoin::{build_commit_output, Network};
    use phechan_keystore::derive_regtest_key;
    use phechan_ordinals::build_inscription_tapscript;

    use crate::{build_reveal_psbt, RevealPsbtParams};

    #[test]
    fn sign_finalize_reveal_has_witness() {
        let key = derive_regtest_key(Network::Regtest, "sign-test").unwrap();
        let xonly = key.xonly.serialize();
        let leaf = build_inscription_tapscript(&xonly, b"hi");
        let commit = build_commit_output(Network::Regtest, &xonly, leaf.clone()).unwrap();

        let fake_txid = Txid::from_byte_array([1u8; 32]);
        let dest = commit.address.script_pubkey();
        let commit_value = Amount::from_sat(10_000);
        let fee = Amount::from_sat(500);
        let psbt = build_reveal_psbt(RevealPsbtParams {
            commit_txid: fake_txid,
            commit_vout: 0,
            commit_value,
            commit_script_pubkey: commit.script_pubkey.clone(),
            destination_script_pubkey: dest,
            destination_value: commit_value.checked_sub(fee).expect("value"),
            leaf_script: commit.leaf_script.clone(),
            spend_info: commit.spend_info.clone(),
            op_return: None,
            change_script_pubkey: None,
            change_value: Amount::ZERO,
        })
        .unwrap();

        let signed = sign_reveal_script_path(psbt, &key.keypair, &commit.leaf_script).unwrap();
        let tx = finalize_to_tx(&signed).unwrap();
        assert!(!tx.input[0].witness.is_empty());
        assert_eq!(tx.output.len(), 1);
    }

    #[test]
    fn sign_finalize_reveal_with_op_return() {
        let key = derive_regtest_key(Network::Regtest, "sign-opr").unwrap();
        let xonly = key.xonly.serialize();
        let leaf = build_inscription_tapscript(&xonly, b"hi");
        let commit = build_commit_output(Network::Regtest, &xonly, leaf.clone()).unwrap();

        let fake_txid = Txid::from_byte_array([1u8; 32]);
        let dest = commit.address.script_pubkey();
        let psbt = build_reveal_psbt(RevealPsbtParams {
            commit_txid: fake_txid,
            commit_vout: 0,
            commit_value: Amount::from_sat(10_000),
            commit_script_pubkey: commit.script_pubkey.clone(),
            destination_script_pubkey: dest,
            destination_value: Amount::from_sat(9_500),
            leaf_script: commit.leaf_script.clone(),
            spend_info: commit.spend_info.clone(),
            op_return: Some(b"msg".to_vec()),
            change_script_pubkey: None,
            change_value: Amount::ZERO,
        })
        .unwrap();

        let signed = sign_reveal_script_path(psbt, &key.keypair, &commit.leaf_script).unwrap();
        let tx = finalize_to_tx(&signed).unwrap();
        assert!(!tx.input[0].witness.is_empty());
        assert_eq!(tx.output.len(), 2);
        assert!(tx.output[1].script_pubkey.is_op_return());
        assert_eq!(tx.output[1].value.to_sat(), 0);
    }

    #[test]
    fn nested_p2sh_synthesize_script_sig_from_redeem() {
        use bitcoin::key::PublicKey;
        use bitcoin::psbt::Psbt;
        use bitcoin::transaction::{OutPoint, Sequence, TxIn, TxOut, Version};
        use bitcoin::{absolute::LockTime, Amount, ScriptBuf, Transaction, Witness};

        let pk = PublicKey::from_slice(&[
            0x02, 0x79, 0xbe, 0x66, 0x7e, 0xf9, 0xdc, 0xbb, 0xac, 0x55, 0xa0, 0x62, 0x95, 0xce,
            0x87, 0x0b, 0x07, 0x02, 0x9b, 0xfc, 0xdb, 0x2d, 0xce, 0x28, 0xd9, 0x59, 0xf2, 0x81,
            0x5b, 0x16, 0xf8, 0x17, 0x98,
        ])
        .unwrap();
        let wpkh = pk.wpubkey_hash().unwrap();
        let redeem = ScriptBuf::new_p2wpkh(&wpkh);
        let p2sh = ScriptBuf::new_p2sh(&redeem.script_hash());

        let tx = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint {
                    txid: Txid::from_byte_array([2u8; 32]),
                    vout: 0,
                },
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::new(),
            }],
            output: vec![TxOut {
                value: Amount::from_sat(1000),
                script_pubkey: ScriptBuf::new_p2wpkh(&wpkh),
            }],
        };
        let mut psbt = Psbt::from_unsigned_tx(tx).unwrap();
        psbt.inputs[0].witness_utxo = Some(TxOut {
            value: Amount::from_sat(2000),
            script_pubkey: p2sh,
        });
        psbt.inputs[0].redeem_script = Some(redeem.clone());
        // Wallet finalized witness only (no final_script_sig) — our bug case
        let mut wit = Witness::new();
        wit.push([0u8; 71]);
        wit.push(pk.to_bytes());
        psbt.inputs[0].final_script_witness = Some(wit);

        let out = finalize_to_tx(&psbt).unwrap();
        assert!(
            !out.input[0].script_sig.is_empty(),
            "nested P2SH must have redeem push in scriptSig"
        );
        assert!(!out.input[0].witness.is_empty());
    }

    #[test]
    fn nested_p2sh_finalizes_from_partial_sigs_like_xverse() {
        use bitcoin::ecdsa::Signature as EcdsaSig;
        use bitcoin::key::PublicKey;
        use bitcoin::psbt::Psbt;
        use bitcoin::secp256k1::ecdsa::Signature as SecpSig;
        use bitcoin::sighash::EcdsaSighashType;
        use bitcoin::transaction::{OutPoint, Sequence, TxIn, TxOut, Version};
        use bitcoin::{absolute::LockTime, Amount, ScriptBuf, Transaction};

        let pk = PublicKey::from_slice(&[
            0x02, 0x79, 0xbe, 0x66, 0x7e, 0xf9, 0xdc, 0xbb, 0xac, 0x55, 0xa0, 0x62, 0x95, 0xce,
            0x87, 0x0b, 0x07, 0x02, 0x9b, 0xfc, 0xdb, 0x2d, 0xce, 0x28, 0xd9, 0x59, 0xf2, 0x81,
            0x5b, 0x16, 0xf8, 0x17, 0x98,
        ])
        .unwrap();
        let wpkh = pk.wpubkey_hash().unwrap();
        let redeem = ScriptBuf::new_p2wpkh(&wpkh);
        let p2sh = ScriptBuf::new_p2sh(&redeem.script_hash());

        let tx = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint {
                    txid: Txid::from_byte_array([3u8; 32]),
                    vout: 0,
                },
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::new(),
            }],
            output: vec![TxOut {
                value: Amount::from_sat(1000),
                script_pubkey: ScriptBuf::new_p2wpkh(&wpkh),
            }],
        };
        let mut psbt = Psbt::from_unsigned_tx(tx).unwrap();
        psbt.inputs[0].witness_utxo = Some(TxOut {
            value: Amount::from_sat(2000),
            script_pubkey: p2sh,
        });
        psbt.inputs[0].redeem_script = Some(redeem);
        // Xverse-style: partial_sigs only, no final_* fields
        let fake_sig = EcdsaSig {
            signature: SecpSig::from_compact(&[1u8; 64]).unwrap(),
            sighash_type: EcdsaSighashType::All,
        };
        psbt.inputs[0].partial_sigs.insert(pk, fake_sig);

        let out = finalize_to_tx(&psbt).unwrap();
        assert_eq!(out.input[0].witness.len(), 2, "P2WPKH witness must be [sig, pubkey]");
        assert!(!out.input[0].script_sig.is_empty(), "nested needs redeem scriptSig");
    }

    #[test]
    fn nested_txid_grind_template_matches_finalize() {
        use bitcoin::ecdsa::Signature as EcdsaSig;
        use bitcoin::key::PublicKey;
        use bitcoin::psbt::Psbt;
        use bitcoin::secp256k1::ecdsa::Signature as SecpSig;
        use bitcoin::sighash::EcdsaSighashType;
        use bitcoin::transaction::{OutPoint, Sequence, TxIn, TxOut, Version};
        use bitcoin::{absolute::LockTime, Amount, ScriptBuf, Transaction};
        use phechan_bitcoin::with_lock_time;

        let pk = PublicKey::from_slice(&[
            0x02, 0x79, 0xbe, 0x66, 0x7e, 0xf9, 0xdc, 0xbb, 0xac, 0x55, 0xa0, 0x62, 0x95, 0xce,
            0x87, 0x0b, 0x07, 0x02, 0x9b, 0xfc, 0xdb, 0x2d, 0xce, 0x28, 0xd9, 0x59, 0xf2, 0x81,
            0x5b, 0x16, 0xf8, 0x17, 0x98,
        ])
        .unwrap();
        let wpkh = pk.wpubkey_hash().unwrap();
        let redeem = ScriptBuf::new_p2wpkh(&wpkh);
        let p2sh = ScriptBuf::new_p2sh(&redeem.script_hash());

        let tx = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint {
                    txid: Txid::from_byte_array([4u8; 32]),
                    vout: 0,
                },
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::new(),
            }],
            output: vec![TxOut {
                value: Amount::from_sat(1000),
                script_pubkey: ScriptBuf::new_p2wpkh(&wpkh),
            }],
        };
        let mut psbt = Psbt::from_unsigned_tx(tx).unwrap();
        psbt.inputs[0].witness_utxo = Some(TxOut {
            value: Amount::from_sat(2000),
            script_pubkey: p2sh,
        });
        psbt.inputs[0].redeem_script = Some(redeem);

        let lt = 1_700_000_000u32;
        let predicted = with_lock_time(txid_grind_template(&psbt).unwrap(), lt)
            .compute_txid()
            .to_string();
        // Empty-scriptSig unsigned id must differ (the bug we hit on nested commit vanity)
        assert_ne!(
            with_lock_time(psbt.unsigned_tx.clone(), lt)
                .compute_txid()
                .to_string(),
            predicted
        );

        psbt.unsigned_tx = with_lock_time(psbt.unsigned_tx.clone(), lt);
        let fake_sig = EcdsaSig {
            signature: SecpSig::from_compact(&[1u8; 64]).unwrap(),
            sighash_type: EcdsaSighashType::All,
        };
        psbt.inputs[0].partial_sigs.insert(pk, fake_sig);
        let out = finalize_to_tx(&psbt).unwrap();
        assert_eq!(out.compute_txid().to_string(), predicted);
    }
}
