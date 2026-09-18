//! Parent vault batch state machine (Phase 6).
//! Spec: protocols/parent-vault/STATE_MACHINE.md

mod store;

pub use store::{load_vault, save_vault, vault_path};

use phechan_runes::{LabeledUtxo, UtxoAssetHint};
use phechan_sat::ParentPlacementPolicy;
use phechan_validation::{validate_parent_child_layout, ParentChildValidationInput};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VaultState {
    Unfunded,
    Ready,
    Reserving,
    Assembled,
    PartialSig,
    Broadcast,
    Settled,
    Quarantine,
    Closed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParentOutpoint {
    pub txid: String,
    pub vout: u32,
    pub value_sats: u64,
    pub parent_inscription_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputTemplate {
    /// Frozen after seal. Values in sats.
    pub output_values: Vec<u64>,
    pub vault_vout: usize,
    pub parent_input_index: usize,
    pub parent_sat_offset: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reservation {
    pub minter_id: String,
    pub funding_outpoint: String,
    pub reserved_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Batch {
    pub id: String,
    pub cap: usize,
    pub template: Option<OutputTemplate>,
    pub reservations: Vec<Reservation>,
    pub collected_sigs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vault {
    pub state: VaultState,
    pub parent: Option<ParentOutpoint>,
    pub batch: Option<Batch>,
    pub network: String,
}

impl Default for Vault {
    fn default() -> Self {
        Self {
            state: VaultState::Unfunded,
            parent: None,
            batch: None,
            network: "regtest".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultError {
    BadState { have: String, need: String },
    Message(String),
}

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadState { have, need } => write!(f, "invalid state {have}; need {need}"),
            Self::Message(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for VaultError {}

impl Vault {
    pub fn fund(&mut self, parent: ParentOutpoint) -> Result<(), VaultError> {
        if self.state != VaultState::Unfunded && self.state != VaultState::Ready {
            return Err(VaultError::BadState {
                have: format!("{:?}", self.state),
                need: "UNFUNDED|READY".into(),
            });
        }
        if parent.txid.len() != 64 {
            return Err(VaultError::Message("parent txid must be 64 hex".into()));
        }
        self.parent = Some(parent);
        self.state = VaultState::Ready;
        self.batch = None;
        Ok(())
    }

    pub fn open_batch(&mut self, id: String, cap: usize) -> Result<(), VaultError> {
        if self.state != VaultState::Ready {
            return Err(VaultError::BadState {
                have: format!("{:?}", self.state),
                need: "READY".into(),
            });
        }
        if self.parent.is_none() {
            return Err(VaultError::Message("no parent funded".into()));
        }
        if cap == 0 {
            return Err(VaultError::Message("cap must be > 0".into()));
        }
        self.batch = Some(Batch {
            id,
            cap,
            template: None,
            reservations: vec![],
            collected_sigs: 0,
        });
        self.state = VaultState::Reserving;
        Ok(())
    }

    pub fn reserve(&mut self, minter_id: String, funding_outpoint: String) -> Result<(), VaultError> {
        if self.state != VaultState::Reserving {
            return Err(VaultError::BadState {
                have: format!("{:?}", self.state),
                need: "RESERVING".into(),
            });
        }
        let batch = self.batch.as_mut().ok_or_else(|| VaultError::Message("no batch".into()))?;
        if batch.reservations.len() >= batch.cap {
            return Err(VaultError::Message("batch full".into()));
        }
        if batch
            .reservations
            .iter()
            .any(|r| r.funding_outpoint == funding_outpoint)
        {
            return Err(VaultError::Message("funding outpoint already reserved".into()));
        }
        batch.reservations.push(Reservation {
            minter_id,
            funding_outpoint,
            reserved_at_unix: now_unix(),
        });
        Ok(())
    }

    /// Freeze output template. After this, outputs must not change.
    pub fn seal(&mut self, template: OutputTemplate) -> Result<(), VaultError> {
        if self.state != VaultState::Reserving {
            return Err(VaultError::BadState {
                have: format!("{:?}", self.state),
                need: "RESERVING".into(),
            });
        }
        let parent = self
            .parent
            .as_ref()
            .ok_or_else(|| VaultError::Message("no parent".into()))?;
        let batch = self.batch.as_mut().ok_or_else(|| VaultError::Message("no batch".into()))?;

        // Parent return must validate against sealed template (FI/FO default values).
        let input_values = {
            let mut v = vec![0u64; template.parent_input_index + 1];
            v[template.parent_input_index] = parent.value_sats;
            // Pad remaining slots with placeholder funding totals from template sum
            let funding = template
                .output_values
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != template.vault_vout)
                .map(|(_, x)| *x)
                .sum::<u64>()
                + 500; // fee cushion placeholder
            if v.len() == 1 {
                v.push(funding);
            } else {
                for i in 0..v.len() {
                    if i != template.parent_input_index && v[i] == 0 {
                        v[i] = funding;
                    }
                }
            }
            v
        };

        let labeled = [LabeledUtxo {
            txid_hex: parent.txid.clone(),
            vout: parent.vout,
            value: parent.value_sats,
            hint: UtxoAssetHint::Inscription,
            inscription_ids: vec![parent.parent_inscription_id.clone()],
            rune_summary: None,
        }];

        let report = validate_parent_child_layout(ParentChildValidationInput {
            policy: ParentPlacementPolicy::FirstInFirstOut,
            parent_input_index: template.parent_input_index,
            parent_sat_offset: template.parent_sat_offset,
            vault_vout: template.vault_vout,
            input_values: &input_values,
            output_values: &template.output_values,
            labeled_inputs: &labeled,
            has_validated_runestone: false,
            allow_asset_bearing_fees: true,
        });
        if !report.allows_broadcast() {
            return Err(VaultError::Message(format!(
                "parent-return validation failed: {}",
                report.errors.join("; ")
            )));
        }

        batch.template = Some(template);
        self.state = VaultState::Assembled;
        Ok(())
    }

    pub fn collect_sig(&mut self) -> Result<(), VaultError> {
        if self.state != VaultState::Assembled && self.state != VaultState::PartialSig {
            return Err(VaultError::BadState {
                have: format!("{:?}", self.state),
                need: "ASSEMBLED|PARTIAL_SIG".into(),
            });
        }
        let batch = self.batch.as_mut().ok_or_else(|| VaultError::Message("no batch".into()))?;
        if batch.template.is_none() {
            return Err(VaultError::Message("template not sealed".into()));
        }
        batch.collected_sigs += 1;
        self.state = VaultState::PartialSig;
        Ok(())
    }

    pub fn mark_broadcast(&mut self) -> Result<(), VaultError> {
        if self.state != VaultState::PartialSig {
            return Err(VaultError::BadState {
                have: format!("{:?}", self.state),
                need: "PARTIAL_SIG".into(),
            });
        }
        let batch = self.batch.as_ref().ok_or_else(|| VaultError::Message("no batch".into()))?;
        if batch.collected_sigs == 0 {
            return Err(VaultError::Message("no signatures collected".into()));
        }
        if batch.template.is_none() {
            return Err(VaultError::Message("missing sealed template".into()));
        }
        self.state = VaultState::Broadcast;
        Ok(())
    }

    pub fn settle(&mut self, new_parent: ParentOutpoint) -> Result<(), VaultError> {
        if self.state != VaultState::Broadcast {
            return Err(VaultError::BadState {
                have: format!("{:?}", self.state),
                need: "BROADCAST".into(),
            });
        }
        self.parent = Some(new_parent);
        self.batch = None;
        self.state = VaultState::Ready;
        Ok(())
    }

    pub fn abort_batch(&mut self) -> Result<(), VaultError> {
        match self.state {
            VaultState::Reserving | VaultState::Assembled | VaultState::PartialSig => {
                self.batch = None;
                self.state = VaultState::Ready;
                Ok(())
            }
            other => Err(VaultError::BadState {
                have: format!("{other:?}"),
                need: "RESERVING|ASSEMBLED|PARTIAL_SIG".into(),
            }),
        }
    }

    pub fn quarantine(&mut self) {
        self.state = VaultState::Quarantine;
    }

    pub fn close(&mut self) -> Result<(), VaultError> {
        if self.state != VaultState::Ready {
            return Err(VaultError::BadState {
                have: format!("{:?}", self.state),
                need: "READY".into(),
            });
        }
        self.state = VaultState::Closed;
        Ok(())
    }

    /// Anti-sniping binding: reservations + sealed template hash proxy.
    pub fn binding_summary(&self) -> String {
        match &self.batch {
            Some(b) => {
                let sealed = b.template.is_some();
                format!(
                    "batch={} state={:?} reserved={}/{} sealed={sealed} sigs={}",
                    b.id,
                    self.state,
                    b.reservations.len(),
                    b.cap,
                    b.collected_sigs
                )
            }
            None => format!("state={:?} no active batch", self.state),
        }
    }
}

fn now_unix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn funded() -> Vault {
        let mut v = Vault::default();
        v.fund(ParentOutpoint {
            txid: "ab".repeat(32),
            vout: 0,
            value_sats: 1000,
            parent_inscription_id: format!("{}i0", "cd".repeat(32)),
        })
        .unwrap();
        v
    }

    #[test]
    fn happy_path_to_ready_after_settle() {
        let mut v = funded();
        v.open_batch("b1".into(), 2).unwrap();
        v.reserve("m1".into(), format!("{}:0", "11".repeat(32)))
            .unwrap();
        v.seal(OutputTemplate {
            output_values: vec![1000, 4800],
            vault_vout: 0,
            parent_input_index: 0,
            parent_sat_offset: 0,
        })
        .unwrap();
        assert_eq!(v.state, VaultState::Assembled);
        v.collect_sig().unwrap();
        v.mark_broadcast().unwrap();
        v.settle(ParentOutpoint {
            txid: "ef".repeat(32),
            vout: 0,
            value_sats: 1000,
            parent_inscription_id: format!("{}i0", "cd".repeat(32)),
        })
        .unwrap();
        assert_eq!(v.state, VaultState::Ready);
        assert!(v.batch.is_none());
    }

    #[test]
    fn cannot_reserve_after_seal() {
        let mut v = funded();
        v.open_batch("b1".into(), 2).unwrap();
        v.seal(OutputTemplate {
            output_values: vec![1000, 4800],
            vault_vout: 0,
            parent_input_index: 0,
            parent_sat_offset: 0,
        })
        .unwrap();
        let err = v.reserve("m".into(), "aa:0".into()).unwrap_err();
        assert!(matches!(err, VaultError::BadState { .. }));
    }

    #[test]
    fn fee_tail_template_rejected_at_seal() {
        let mut v = funded();
        v.open_batch("b1".into(), 1).unwrap();
        let err = v
            .seal(OutputTemplate {
                output_values: vec![5000], // parent would fall in fee if mis-modeled
                vault_vout: 1,
                parent_input_index: 1,
                parent_sat_offset: 0,
            })
            .unwrap_err();
        assert!(matches!(err, VaultError::Message(_)));
    }

    #[test]
    fn duplicate_outpoint_blocked() {
        let mut v = funded();
        v.open_batch("b1".into(), 3).unwrap();
        let op = format!("{}:1", "22".repeat(32));
        v.reserve("a".into(), op.clone()).unwrap();
        assert!(v.reserve("b".into(), op).is_err());
    }
}
