//! Mainnet broadcast dual-gate: env unlock + typed confirmation.

use std::env;
use std::io::{self, Write};

use phechan_bitcoin::Network;

use crate::args::flag_value;

pub const MAINNET_ENV: &str = "PHECHAN_ALLOW_MAINNET_BROADCAST";
pub const MAINNET_PHRASE: &str = "BROADCAST MAINNET";

/// Allow mainnet broadcast only when `PHECHAN_ALLOW_MAINNET_BROADCAST=1` and
/// confirmation is exactly `BROADCAST MAINNET` via `--confirm` (UI/non-interactive)
/// or an interactive stdin prompt (CLI).
pub fn require_mainnet_broadcast_gate(args: &[String], network: Network) -> Result<(), String> {
    if network != Network::Mainnet {
        return Ok(());
    }
    let allow = env::var(MAINNET_ENV).unwrap_or_default();
    if allow != "1" {
        return Err(format!(
            "mainnet locked; set {MAINNET_ENV}=1 and confirm with --confirm \"{MAINNET_PHRASE}\""
        ));
    }
    if let Some(phrase) = flag_value(args, "--confirm") {
        if phrase.trim() == MAINNET_PHRASE {
            return Ok(());
        }
        return Err(format!(
            "mainnet confirmation mismatch — pass --confirm \"{MAINNET_PHRASE}\" exactly"
        ));
    }
    eprint!("Type {MAINNET_PHRASE} to continue: ");
    io::stderr().flush().ok();
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    if line.trim() != MAINNET_PHRASE {
        return Err("mainnet confirmation mismatch".into());
    }
    Ok(())
}
