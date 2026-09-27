//! Mainnet broadcast gate (disabled).
//!
//! Network comes from the connected wallet / `--network` flag. No env unlock or
//! typed confirmation is required.

use phechan_bitcoin::Network;

/// Kept for CLI flag compatibility (`--confirm "BROADCAST MAINNET"` is ignored).
pub const MAINNET_PHRASE: &str = "BROADCAST MAINNET";

/// Always allows broadcast — wallet/network selection is the only gate.
pub fn require_mainnet_broadcast_gate(_args: &[String], _network: Network) -> Result<(), String> {
    Ok(())
}
