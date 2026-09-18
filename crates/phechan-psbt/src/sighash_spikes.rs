//! Phase 5 sighash spike matrix (Taproot key-path).
//!
//! Proves what each sighash commits to by signing once, mutating the tx, and
//! checking whether the original signature still verifies under the new tx.

use bitcoin::absolute::LockTime;
use bitcoin::consensus::encode::serialize;
use bitcoin::hashes::Hash;
use bitcoin::key::{Keypair, XOnlyPublicKey};
use bitcoin::secp256k1::{Message, Secp256k1};
use bitcoin::sighash::{Prevouts, SighashCache, TapSighashType};
use bitcoin::taproot::Signature as TaprootSignature;
use bitcoin::transaction::{OutPoint, Sequence, TxIn, TxOut, Version};
use bitcoin::{Address, Amount, Network, ScriptBuf, Transaction, Txid, Witness};
use phechan_bitcoin::Network as PhechanNetwork;
use phechan_keystore::derive_regtest_key;

#[derive(Debug, Clone, Copy)]
struct SpikeCase {
    name: &'static str,
    sighash: TapSighashType,
}

fn keypair() -> (Keypair, XOnlyPublicKey, ScriptBuf) {
    let key = derive_regtest_key(PhechanNetwork::Regtest, "sighash-spike").unwrap();
    let secp = Secp256k1::new();
    let addr = Address::p2tr(&secp, key.xonly, None, Network::Regtest);
    (key.keypair, key.xonly, addr.script_pubkey())
}

fn base_tx(spk: ScriptBuf, value: Amount) -> (Transaction, TxOut) {
    let prevout = TxOut {
        value,
        script_pubkey: spk.clone(),
    };
    let tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint {
                txid: Txid::from_byte_array([9u8; 32]),
                vout: 0,
            },
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::new(),
        }],
        output: vec![
            TxOut {
                value: Amount::from_sat(value.to_sat() - 500),
                script_pubkey: spk.clone(), // placeholder "minter output"
            },
            TxOut {
                value: Amount::from_sat(200),
                script_pubkey: ScriptBuf::new_op_return(b"fee-marker"),
            },
        ],
    };
    (tx, prevout)
}

fn sign_keypath(
    tx: &Transaction,
    prevout: &TxOut,
    keypair: &Keypair,
    sighash_type: TapSighashType,
) -> TaprootSignature {
    let secp = Secp256k1::new();
    let mut cache = SighashCache::new(tx);
    let hash = match sighash_type {
        TapSighashType::AllPlusAnyoneCanPay | TapSighashType::SinglePlusAnyoneCanPay
        | TapSighashType::NonePlusAnyoneCanPay => cache
            .taproot_key_spend_signature_hash(0, &Prevouts::One(0, prevout.clone()), sighash_type)
            .expect("sighash acp"),
        _ => cache
            .taproot_key_spend_signature_hash(0, &Prevouts::All(&[prevout.clone()]), sighash_type)
            .expect("sighash all"),
    };
    let msg = Message::from_digest_slice(hash.as_ref()).expect("msg");
    let sig = secp.sign_schnorr_no_aux_rand(&msg, keypair);
    TaprootSignature {
        signature: sig,
        sighash_type,
    }
}

fn verify_keypath(
    tx: &Transaction,
    signed_prevout: &TxOut,
    xonly: &XOnlyPublicKey,
    tap_sig: &TaprootSignature,
) -> bool {
    let secp = Secp256k1::verification_only();
    let mut cache = SighashCache::new(tx);
    // ANYONECANPAY only needs the signed input's prevout; ALL needs every input's.
    let hash = match tap_sig.sighash_type {
        TapSighashType::AllPlusAnyoneCanPay
        | TapSighashType::SinglePlusAnyoneCanPay
        | TapSighashType::NonePlusAnyoneCanPay => cache.taproot_key_spend_signature_hash(
            0,
            &Prevouts::One(0, signed_prevout.clone()),
            tap_sig.sighash_type,
        ),
        _ => {
            // Build All prevouts: signed input first, dummy for any appended inputs.
            let mut prevouts: Vec<TxOut> = vec![signed_prevout.clone()];
            for _ in 1..tx.input.len() {
                prevouts.push(TxOut {
                    value: Amount::from_sat(0),
                    script_pubkey: ScriptBuf::new(),
                });
            }
            cache.taproot_key_spend_signature_hash(0, &Prevouts::All(&prevouts), tap_sig.sighash_type)
        }
    };
    let hash = match hash {
        Ok(h) => h,
        Err(_) => return false,
    };
    let msg = match Message::from_digest_slice(hash.as_ref()) {
        Ok(m) => m,
        Err(_) => return false,
    };
    secp.verify_schnorr(&tap_sig.signature, &msg, xonly).is_ok()
}

fn with_extra_input(mut tx: Transaction) -> Transaction {
    tx.input.push(TxIn {
        previous_output: OutPoint {
            txid: Txid::from_byte_array([11u8; 32]),
            vout: 1,
        },
        script_sig: ScriptBuf::new(),
        sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
        witness: Witness::new(),
    });
    tx
}

fn with_extra_output(mut tx: Transaction, spk: ScriptBuf) -> Transaction {
    // Append only — do not mutate matched output[0] (SINGLE commits to it).
    tx.output.push(TxOut {
        value: Amount::from_sat(100),
        script_pubkey: spk,
    });
    tx
}

fn with_reordered_outputs(mut tx: Transaction) -> Transaction {
    if tx.output.len() >= 2 {
        tx.output.swap(0, 1);
    }
    tx
}

fn with_fee_bump(mut tx: Transaction) -> Transaction {
    if tx.output[0].value.to_sat() > 1000 {
        tx.output[0].value = Amount::from_sat(tx.output[0].value.to_sat() - 200);
    }
    tx
}

fn with_malicious_output_swap(mut tx: Transaction, attacker_spk: ScriptBuf) -> Transaction {
    tx.output[0].script_pubkey = attacker_spk;
    tx
}

fn with_parent_input(mut tx: Transaction) -> Transaction {
    // Insert parent as input 0; original signed input shifts to index 1.
    // For ACP tests we keep verifying index 0 signature against original input —
    // so for "add parent" we append instead (coordinator adds parent without moving minter input).
    tx.input.push(TxIn {
        previous_output: OutPoint {
            txid: Txid::from_byte_array([77u8; 32]),
            vout: 0,
        },
        script_sig: ScriptBuf::new(),
        sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
        witness: Witness::new(),
    });
    tx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_matrix(case: SpikeCase) {
        let (kp, xonly, spk) = keypair();
        let value = Amount::from_sat(10_000);
        let (tx, prevout) = base_tx(spk.clone(), value);
        let sig = sign_keypath(&tx, &prevout, &kp, case.sighash);

        assert!(
            verify_keypath(&tx, &prevout, &xonly, &sig),
            "{}: baseline must verify",
            case.name
        );

        let attacker = ScriptBuf::new_op_return(b"steal");

        let add_in = with_extra_input(tx.clone());
        let add_in_ok = verify_keypath(&add_in, &prevout, &xonly, &sig);

        let add_out = with_extra_output(tx.clone(), spk.clone());
        let add_out_ok = verify_keypath(&add_out, &prevout, &xonly, &sig);

        let reorder = with_reordered_outputs(tx.clone());
        let reorder_ok = verify_keypath(&reorder, &prevout, &xonly, &sig);

        let fee = with_fee_bump(tx.clone());
        let fee_ok = verify_keypath(&fee, &prevout, &xonly, &sig);

        let parent = with_parent_input(tx.clone());
        let parent_ok = verify_keypath(&parent, &prevout, &xonly, &sig);

        let steal = with_malicious_output_swap(tx.clone(), attacker);
        let steal_ok = verify_keypath(&steal, &prevout, &xonly, &sig);

        // Invalid sig path: wrong prevout value must fail verify
        let wrong_prev = TxOut {
            value: Amount::from_sat(1),
            script_pubkey: prevout.script_pubkey.clone(),
        };
        let invalid_ok = verify_keypath(&tx, &wrong_prev, &xonly, &sig);

        println!(
            "spike[{}]: add_in={add_in_ok} add_out={add_out_ok} reorder={reorder_ok} fee={fee_ok} parent_add={parent_ok} steal_out={steal_ok} wrong_prevout={invalid_ok}",
            case.name
        );

        match case.sighash {
            TapSighashType::Default | TapSighashType::All => {
                assert!(!add_in_ok, "{}: ALL must fail when input added", case.name);
                assert!(!add_out_ok, "{}: ALL must fail when output added", case.name);
                assert!(!reorder_ok, "{}: ALL must fail on reorder", case.name);
                assert!(!fee_ok, "{}: ALL must fail on fee change", case.name);
                assert!(!parent_ok, "{}: ALL must fail when parent input added", case.name);
                assert!(!steal_ok, "{}: ALL must fail on output swap", case.name);
                assert!(!invalid_ok, "{}: ALL must fail wrong prevout", case.name);
            }
            TapSighashType::AllPlusAnyoneCanPay => {
                assert!(add_in_ok, "{}: ALL|ACP should allow extra inputs", case.name);
                assert!(parent_ok, "{}: ALL|ACP should allow parent input append", case.name);
                assert!(!add_out_ok, "{}: ALL|ACP must NOT allow extra outputs", case.name);
                assert!(!reorder_ok, "{}: ALL|ACP must NOT allow reorder", case.name);
                assert!(!fee_ok, "{}: ALL|ACP must NOT allow fee/output value change", case.name);
                assert!(!steal_ok, "{}: ALL|ACP must NOT allow output script swap", case.name);
                assert!(!invalid_ok, "{}: ALL|ACP must fail wrong prevout", case.name);
            }
            TapSighashType::SinglePlusAnyoneCanPay => {
                assert!(add_in_ok, "{}: SINGLE|ACP should allow extra inputs", case.name);
                assert!(parent_ok, "{}: SINGLE|ACP should allow parent input append", case.name);
                // Extra output at end: SINGLE only commits to output[input_index]=output[0]
                assert!(
                    add_out_ok,
                    "{}: SINGLE|ACP should allow additional outputs beyond matched index",
                    case.name
                );
                // Reorder swaps output 0 away → should fail
                assert!(!reorder_ok, "{}: SINGLE|ACP must fail if matched output moves", case.name);
                // Fee bump changes output[0] value → fail
                assert!(!fee_ok, "{}: SINGLE|ACP must fail if matched output value changes", case.name);
                assert!(!steal_ok, "{}: SINGLE|ACP must fail if matched output script swapped", case.name);
                assert!(!invalid_ok, "{}: SINGLE|ACP must fail wrong prevout", case.name);
            }
            _ => panic!("unexpected sighash in matrix"),
        }

        // Replacement folklore: RBF signaling is orthogonal — we only assert sig binding here.
        let _serialized = serialize(&tx);
        assert!(!_serialized.is_empty());
    }

    #[test]
    fn spike_sighash_all_default() {
        run_matrix(SpikeCase {
            name: "SIGHASH_ALL(Default)",
            sighash: TapSighashType::Default,
        });
    }

    #[test]
    fn spike_sighash_all_anyone_can_pay() {
        run_matrix(SpikeCase {
            name: "SIGHASH_ALL|ANYONECANPAY",
            sighash: TapSighashType::AllPlusAnyoneCanPay,
        });
    }

    #[test]
    fn spike_sighash_single_anyone_can_pay() {
        run_matrix(SpikeCase {
            name: "SIGHASH_SINGLE|ANYONECANPAY",
            sighash: TapSighashType::SinglePlusAnyoneCanPay,
        });
    }
}
