//! Deterministic regtest key derivation (not BIP39).

use bitcoin::key::{Keypair, XOnlyPublicKey};
use bitcoin::secp256k1::{Secp256k1, SecretKey};
use phechan_bitcoin::Network;
use sha2::{Digest, Sha256};

use crate::keygen_allowed;

#[derive(Debug)]
pub enum KeystoreError {
    MainnetForbidden,
    InvalidSecret,
}

impl std::fmt::Display for KeystoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MainnetForbidden => write!(f, "keystore keygen forbidden on mainnet"),
            Self::InvalidSecret => write!(f, "derived secret key invalid"),
        }
    }
}

impl std::error::Error for KeystoreError {}

#[derive(Debug, Clone)]
pub struct RegtestKey {
    pub secret: SecretKey,
    pub xonly: XOnlyPublicKey,
    pub keypair: Keypair,
}

/// Derive a deterministic key from `phechan-regtest/` || label via SHA256.
/// Only allowed on non-mainnet networks.
pub fn derive_regtest_key(network: Network, seed_label: &str) -> Result<RegtestKey, KeystoreError> {
    if !keygen_allowed(network) {
        return Err(KeystoreError::MainnetForbidden);
    }
    let mut hasher = Sha256::new();
    hasher.update(b"phechan-regtest/");
    hasher.update(seed_label.as_bytes());
    let hash = hasher.finalize();
    let secp = Secp256k1::new();
    let secret = SecretKey::from_slice(&hash).map_err(|_| KeystoreError::InvalidSecret)?;
    let keypair = Keypair::from_secret_key(&secp, &secret);
    let (xonly, _parity) = XOnlyPublicKey::from_keypair(&keypair);
    Ok(RegtestKey {
        secret,
        xonly,
        keypair,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_label_same_xonly() {
        let a = derive_regtest_key(Network::Regtest, "alice").unwrap();
        let b = derive_regtest_key(Network::Regtest, "alice").unwrap();
        assert_eq!(a.xonly.serialize(), b.xonly.serialize());
    }

    #[test]
    fn mainnet_forbidden() {
        assert!(matches!(
            derive_regtest_key(Network::Mainnet, "alice"),
            Err(KeystoreError::MainnetForbidden)
        ));
    }
}
