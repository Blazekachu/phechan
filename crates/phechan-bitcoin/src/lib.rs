//! Bitcoin primitives helpers for Phechan.

mod esplora;
mod ord;
mod rpc;
mod taproot_commit;
mod vanity;

pub use esplora::{broadcast_tx as esplora_broadcast_tx, find_vout_in_esplora_tx, get_tx_json, EsploraError};
pub use ord::{OrdClient, OrdConfig, OrdError, OrdOutputInfo};
pub use rpc::{find_vout_for_address, BitcoindRpc, RpcConfig, RpcError};
pub use taproot_commit::{build_commit_output, CommitOutput, TaprootCommitError};
pub use vanity::{
    final_locktime_window, grind_locktime_affixes, grind_locktime_affixes_final,
    grind_locktime_prefix, locktime_is_final, with_lock_time, with_sequence, GrindableField,
    DO_NOT_MUTATE_AFTER_SIGN, LOCKTIME_THRESHOLD,
};

use bitcoin::Network as BtcNetwork;

/// Networks Phechan can target for build/preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    Regtest,
    Signet,
    Testnet,
    Mainnet,
}

impl Network {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "regtest" => Some(Self::Regtest),
            "signet" => Some(Self::Signet),
            "testnet" | "testnet3" => Some(Self::Testnet),
            "mainnet" | "bitcoin" => Some(Self::Mainnet),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Regtest => "regtest",
            Self::Signet => "signet",
            Self::Testnet => "testnet",
            Self::Mainnet => "mainnet",
        }
    }

    pub fn to_bitcoin(self) -> BtcNetwork {
        match self {
            Self::Regtest => BtcNetwork::Regtest,
            Self::Signet => BtcNetwork::Signet,
            Self::Testnet => BtcNetwork::Testnet,
            Self::Mainnet => BtcNetwork::Bitcoin,
        }
    }
}
