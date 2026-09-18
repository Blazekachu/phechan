//! PSBT build/inspect/finalize helpers for inscription commit/reveal.

mod commit;
mod reveal;
#[cfg(test)]
mod sighash_spikes;
mod sign_regtest;

pub use commit::{
    build_commit_psbt, build_commit_psbt_multi, CommitFundingInput, CommitPsbtParams, PsbtBuildError,
};
pub use reveal::{
    build_parent_child_reveal_psbt, build_reveal_psbt, ParentChildRevealParams, RevealPsbtParams,
};
pub use sign_regtest::{
    ensure_nested_redeem_from_pubkey, finalize_to_tx, sign_reveal_script_path,
    sign_reveal_script_path_at, txid_grind_template,
};

/// Placeholder kept for older call sites.
pub fn psbt_engine_ready() -> bool {
    true
}
