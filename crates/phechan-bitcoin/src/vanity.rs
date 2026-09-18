//! TXID vanity grinding helpers.
//!
//! Only mutate fields that still affect TXID **before** signatures are applied.
//! Never grind after signing.
//!
//! Absolute `nLockTime` must stay **final** for immediate broadcast:
//! - height mode (`lt < 500_000_000`): `lt <= tip_height`
//! - time mode (`lt >= 500_000_000`): `lt <= mediantime`
//! Prefer time mode — search space is large enough for vanity without producing
//! `rpc -26: non-final` on signet/mainnet.

use bitcoin::absolute::LockTime;
use bitcoin::transaction::{Sequence, Transaction};

/// BIP-65 / consensus: locktimes at or above this are unix timestamps.
pub const LOCKTIME_THRESHOLD: u32 = 500_000_000;

/// Fields that may be varied to grind TXID **pre-sign**.
#[derive(Debug, Clone, Copy)]
pub enum GrindableField {
    /// `nLockTime` when sequences allow (non-final input sequence).
    LockTime,
    /// Input `nSequence` (within RBF/policy constraints you accept).
    Sequence { input_index: usize },
}

/// Documented non-grindable post-sign surfaces.
pub const DO_NOT_MUTATE_AFTER_SIGN: &[&str] = &[
    "scriptSig / witness (signatures)",
    "output scriptPubKey / values committed by SIGHASH_ALL",
    "input outpoints committed by sighash",
];

/// Apply a candidate locktime for grinding (caller must re-sign afterward).
pub fn with_lock_time(mut tx: Transaction, lock_time: u32) -> Transaction {
    tx.lock_time = LockTime::from_consensus(lock_time);
    tx
}

/// Apply a candidate sequence on one input (caller must re-sign afterward).
pub fn with_sequence(mut tx: Transaction, input_index: usize, sequence: u32) -> Option<Transaction> {
    let inp = tx.input.get_mut(input_index)?;
    inp.sequence = Sequence(sequence);
    Some(tx)
}

/// Search locktime range `[start, start+max_tries)` for a txid matching hex prefix/suffix.
/// Prefer [`grind_locktime_affixes_final`] for broadcastable txs.
pub fn grind_locktime_affixes(
    base: &Transaction,
    prefix_hex: &str,
    suffix_hex: &str,
    start: u32,
    max_tries: u32,
) -> Option<(u32, String)> {
    let prefix = prefix_hex.to_ascii_lowercase();
    let suffix = suffix_hex.to_ascii_lowercase();
    for i in 0..max_tries {
        let lt = start.wrapping_add(i);
        let tx = with_lock_time(base.clone(), lt);
        let txid = tx.compute_txid().to_string();
        if (prefix.is_empty() || txid.starts_with(&prefix))
            && (suffix.is_empty() || txid.ends_with(&suffix))
        {
            return Some((lt, txid));
        }
    }
    None
}

/// Grind only locktimes that are **immediately final** given chain tip.
///
/// Uses unix-timestamp locktimes in `(LOCKTIME_THRESHOLD ..= mediantime]` when
/// `mediantime >= LOCKTIME_THRESHOLD`, otherwise height locktimes in `[0 ..= tip_height]`.
pub fn grind_locktime_affixes_final(
    base: &Transaction,
    prefix_hex: &str,
    suffix_hex: &str,
    tip_height: u32,
    mediantime: u32,
    max_tries: u32,
) -> Option<(u32, String)> {
    let (start, end) = final_locktime_window(tip_height, mediantime, max_tries)?;
    let span = end.saturating_sub(start).saturating_add(1);
    grind_locktime_affixes(base, prefix_hex, suffix_hex, start, span.min(max_tries).max(1))
}

/// Inclusive `[start, end]` locktime window that Core will accept as final now.
pub fn final_locktime_window(
    tip_height: u32,
    mediantime: u32,
    max_tries: u32,
) -> Option<(u32, u32)> {
    if mediantime >= LOCKTIME_THRESHOLD {
        let end = mediantime;
        let room = end.saturating_sub(LOCKTIME_THRESHOLD);
        let span = max_tries.saturating_sub(1).min(room);
        let start = end.saturating_sub(span);
        Some((start, end))
    } else if tip_height > 0 {
        let end = tip_height;
        let span = max_tries.saturating_sub(1).min(end);
        let start = end.saturating_sub(span);
        Some((start, end))
    } else {
        Some((0, 0))
    }
}

/// True if `lock_time` is final at this tip (same rules as mempool acceptance).
pub fn locktime_is_final(lock_time: u32, tip_height: u32, mediantime: u32) -> bool {
    if lock_time == 0 {
        return true;
    }
    if lock_time < LOCKTIME_THRESHOLD {
        lock_time <= tip_height
    } else {
        lock_time <= mediantime
    }
}

/// Search locktime range for a txid hex prefix. Returns matching lock_time or None.
pub fn grind_locktime_prefix(
    base: &Transaction,
    prefix_hex: &str,
    start: u32,
    max_tries: u32,
) -> Option<(u32, String)> {
    grind_locktime_affixes(base, prefix_hex, "", start, max_tries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::absolute::LockTime;
    use bitcoin::hashes::Hash;
    use bitcoin::transaction::{OutPoint, TxIn, TxOut, Version};
    use bitcoin::{Amount, ScriptBuf, Sequence, Txid};

    fn sample_tx() -> Transaction {
        Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint {
                    txid: Txid::from_byte_array([1u8; 32]),
                    vout: 0,
                },
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: bitcoin::Witness::new(),
            }],
            output: vec![TxOut {
                value: Amount::from_sat(1000),
                script_pubkey: ScriptBuf::new_op_return(b"x"),
            }],
        }
    }

    #[test]
    fn grind_finds_trivial_empty_prefix() {
        let tx = sample_tx();
        let hit = grind_locktime_prefix(&tx, "", 0, 1).unwrap();
        assert!(!hit.1.is_empty());
    }

    #[test]
    fn final_window_prefers_timestamp_mode() {
        let (start, end) = final_locktime_window(322_533, 1_789_662_655, 5_000_000).unwrap();
        assert!(start >= LOCKTIME_THRESHOLD);
        assert_eq!(end, 1_789_662_655);
        assert!(end - start <= 5_000_000);
        assert!(locktime_is_final(end, 322_533, 1_789_662_655));
        assert!(!locktime_is_final(2_614_257, 322_533, 1_789_662_655));
    }

    #[test]
    fn grind_final_stays_final() {
        let tx = sample_tx();
        let tip = 1000u32;
        let mt = LOCKTIME_THRESHOLD + 10_000;
        let (lt, _) = grind_locktime_affixes_final(&tx, "", "", tip, mt, 100).unwrap();
        assert!(locktime_is_final(lt, tip, mt));
    }

    #[test]
    fn do_not_mutate_list_mentions_witness() {
        assert!(DO_NOT_MUTATE_AFTER_SIGN.iter().any(|s| s.contains("witness")));
    }
}
