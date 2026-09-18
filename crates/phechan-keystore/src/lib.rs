//! Local key material for regtest/CI.

mod regtest;

pub use regtest::{derive_regtest_key, KeystoreError, RegtestKey};

use phechan_bitcoin::Network;

/// Refuse mainnet keygen in this crate until an explicit audited design exists.
pub fn keygen_allowed(network: Network) -> bool {
    matches!(network, Network::Regtest | Network::Signet | Network::Testnet)
}
