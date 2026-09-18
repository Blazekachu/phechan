//! Layered validation: consensus / relay / ordinals / runes / assets.

mod inscription;
mod parent;

pub use inscription::validate_inscription_reveal;
pub use parent::{validate_parent_child_layout, ParentChildValidationInput};

use phechan_bitcoin::Network;

#[derive(Debug, Clone, Default)]
pub struct ValidationReport {
    pub consensus_ok: Option<bool>,
    pub relay_ok: Option<bool>,
    pub ordinals_ok: Option<bool>,
    pub runes_ok: Option<bool>,
    pub asset_disclosure: Vec<String>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ValidationReport {
    /// Phase 0: nothing is validated yet — never claim a pass suitable for mainnet.
    pub fn phase0_blocked(network: Network) -> Self {
        Self {
            consensus_ok: None,
            relay_ok: None,
            ordinals_ok: None,
            runes_ok: None,
            asset_disclosure: vec![],
            errors: vec![
                "validation engine not implemented (phase 0)".to_string(),
                format!("refusing broadcast readiness on {}", network.as_str()),
            ],
            warnings: vec![],
        }
    }

    pub fn allows_broadcast(&self) -> bool {
        self.errors.is_empty()
            && self.consensus_ok == Some(true)
            && self.relay_ok != Some(false)
            && self.ordinals_ok != Some(false)
            && self.runes_ok != Some(false)
    }
}
