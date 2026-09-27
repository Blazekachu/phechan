//! Local key material for regtest/CI automation.
//!
//! Mainnet inscriptions use wallet self-custody (`--ordinals-pubkey-hex`): the
//! connected wallet's pubkey is baked into the tapscript. Phechan never holds
//! that private key — only the wallet owner can reveal or key-path recover.
//! Deterministic keystore keygen stays forbidden on mainnet.

mod regtest;

pub use regtest::{derive_regtest_key, KeystoreError, RegtestKey};

use phechan_bitcoin::Network;

/// Deterministic keystore is for regtest / signet / testnet automation only.
pub fn keygen_allowed(network: Network) -> bool {
    matches!(network, Network::Regtest | Network::Signet | Network::Testnet)
}
