//! Runes co-spend awareness (not a Runes etch product).

mod cospend;
mod disclosure;

pub use cospend::{
    cospend_advice, encode_cospend_marker, looks_like_test_cospend_marker, RUNESTONE_MAGIC,
};
pub use disclosure::{
    fee_input_allowed, format_disclosure, rune_cospend_blocked, LabeledUtxo,
};

/// Asset kinds that may sit on a UTXO Phechan is about to spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UtxoAssetHint {
    Plain,
    Inscription,
    Rune,
    InscriptionAndRune,
    Unknown,
}
