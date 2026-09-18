//! Taproot commit output for an inscription leaf script.

use bitcoin::key::XOnlyPublicKey;
use bitcoin::secp256k1::Secp256k1;
use bitcoin::taproot::{TaprootBuilder, TaprootSpendInfo};
use bitcoin::{Address, ScriptBuf};

use crate::Network;

#[derive(Debug)]
pub enum TaprootCommitError {
    InvalidInternalKey,
    TaprootBuild(String),
    Address(String),
}

impl std::fmt::Display for TaprootCommitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInternalKey => write!(f, "invalid internal x-only key"),
            Self::TaprootBuild(e) => write!(f, "taproot build: {e}"),
            Self::Address(e) => write!(f, "address: {e}"),
        }
    }
}

impl std::error::Error for TaprootCommitError {}

#[derive(Debug, Clone)]
pub struct CommitOutput {
    pub script_pubkey: ScriptBuf,
    pub address: Address,
    pub spend_info: TaprootSpendInfo,
    pub leaf_script: ScriptBuf,
}

/// Build a P2TR output whose script tree commits to `leaf_script`.
pub fn build_commit_output(
    network: Network,
    internal_xonly: &[u8; 32],
    leaf_script: Vec<u8>,
) -> Result<CommitOutput, TaprootCommitError> {
    let secp = Secp256k1::new();
    let internal_key = XOnlyPublicKey::from_slice(internal_xonly)
        .map_err(|_| TaprootCommitError::InvalidInternalKey)?;
    let leaf = ScriptBuf::from_bytes(leaf_script);
    let builder = TaprootBuilder::new()
        .add_leaf(0, leaf.clone())
        .map_err(|e| TaprootCommitError::TaprootBuild(format!("{e:?}")))?;
    let spend_info = builder
        .finalize(&secp, internal_key)
        .map_err(|e| TaprootCommitError::TaprootBuild(format!("{e:?}")))?;
    let address = Address::p2tr(
        &secp,
        internal_key,
        spend_info.merkle_root(),
        network.to_bitcoin(),
    );
    let script_pubkey = address.script_pubkey();
    Ok(CommitOutput {
        script_pubkey,
        address,
        spend_info,
        leaf_script: leaf,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use phechan_ordinals::build_inscription_tapscript;

    #[test]
    fn commit_output_is_p2tr_regtest() {
        // Valid x-only pubkey (generator X coordinate).
        let key_hex = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
        let mut key = [0u8; 32];
        for i in 0..32 {
            key[i] = u8::from_str_radix(&key_hex[i * 2..i * 2 + 2], 16).unwrap();
        }
        let leaf = build_inscription_tapscript(&key, b"Hello, world!");
        let commit = build_commit_output(Network::Regtest, &key, leaf).unwrap();
        let addr = commit.address.to_string();
        assert!(
            addr.starts_with("bcrt1p"),
            "expected regtest p2tr address, got {addr}"
        );
    }
}
