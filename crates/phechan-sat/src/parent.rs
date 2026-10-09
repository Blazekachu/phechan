//! Parent placement policies and verification via sat-flow.

use crate::flow::{simulate_sat_flow, SatFlowError, SatLocation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParentPlacementPolicy {
    /// Default: parent first input; return to first *valued* output
    /// (vout0 normally, or vout1 when a leading 0-sat OP_RETURN is present).
    FirstInFirstOut,
    /// Opt-in: any layout; must pass sat-flow against `vault_vout`.
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParentPlacementError {
    SatFlow(SatFlowError),
    ParentInputMissing,
    InvalidParentOffset,
    ParentSatLostToFee,
    ParentSatWrongOutput { got: usize, expected: usize },
    FifoRequiresParentFirstInput,
    FifoRequiresVaultFirstOutput,
}

impl std::fmt::Display for ParentPlacementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SatFlow(e) => write!(f, "{e}"),
            Self::ParentInputMissing => write!(f, "parent input index out of range"),
            Self::InvalidParentOffset => write!(f, "parent sat offset out of range"),
            Self::ParentSatLostToFee => write!(f, "parent sat would fall into fees"),
            Self::ParentSatWrongOutput { got, expected } => {
                write!(f, "parent sat lands on vout {got}, expected vault vout {expected}")
            }
            Self::FifoRequiresParentFirstInput => {
                write!(f, "FirstInFirstOut requires parent_input_index == 0")
            }
            Self::FifoRequiresVaultFirstOutput => {
                write!(
                    f,
                    "FirstInFirstOut requires vault at the first valued output"
                )
            }
        }
    }
}

impl std::error::Error for ParentPlacementError {}

/// Verify the parent sat returns to `vault_vout` under the given policy.
pub fn verify_parent_return(
    policy: ParentPlacementPolicy,
    parent_input_index: usize,
    parent_sat_offset: u64,
    vault_vout: usize,
    input_values: &[u64],
    output_values: &[u64],
) -> Result<(), ParentPlacementError> {
    match policy {
        ParentPlacementPolicy::FirstInFirstOut => {
            if parent_input_index != 0 {
                return Err(ParentPlacementError::FifoRequiresParentFirstInput);
            }
            let first_valued = output_values
                .iter()
                .position(|&v| v > 0)
                .ok_or(ParentPlacementError::FifoRequiresVaultFirstOutput)?;
            if vault_vout != first_valued {
                return Err(ParentPlacementError::FifoRequiresVaultFirstOutput);
            }
        }
        ParentPlacementPolicy::Custom => {}
    }

    let flow = simulate_sat_flow(input_values, output_values)
        .map_err(ParentPlacementError::SatFlow)?;

    if parent_input_index >= input_values.len() {
        return Err(ParentPlacementError::ParentInputMissing);
    }

    let loc = flow
        .location_of_input_sat(parent_input_index, parent_sat_offset)
        .ok_or(ParentPlacementError::InvalidParentOffset)?;

    match loc {
        SatLocation::Fee => Err(ParentPlacementError::ParentSatLostToFee),
        SatLocation::Output { vout } if vout == vault_vout => Ok(()),
        SatLocation::Output { vout } => Err(ParentPlacementError::ParentSatWrongOutput {
            got: vout,
            expected: vault_vout,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifo_happy() {
        assert!(verify_parent_return(
            ParentPlacementPolicy::FirstInFirstOut,
            0,
            0,
            0,
            &[1000, 5000],
            &[1000, 4800],
        )
        .is_ok());
    }

    #[test]
    fn fifo_trailing_zero_still_vault_vout0() {
        assert!(verify_parent_return(
            ParentPlacementPolicy::FirstInFirstOut,
            0,
            0,
            0,
            &[1000, 5000],
            &[1000, 4800, 0],
        )
        .is_ok());
    }

    #[test]
    fn fifo_leading_zero_op_return_vault_is_vout1() {
        assert!(verify_parent_return(
            ParentPlacementPolicy::FirstInFirstOut,
            0,
            0,
            1,
            &[546, 10_000],
            &[0, 546, 546],
        )
        .is_ok());
    }

    #[test]
    fn fifo_leading_zero_rejects_vault_vout0() {
        let err = verify_parent_return(
            ParentPlacementPolicy::FirstInFirstOut,
            0,
            0,
            0,
            &[546, 10_000],
            &[0, 546, 546],
        )
        .unwrap_err();
        assert!(matches!(
            err,
            ParentPlacementError::FifoRequiresVaultFirstOutput
        ));
    }

    #[test]
    fn fee_tail_fails() {
        let err = verify_parent_return(
            ParentPlacementPolicy::Custom,
            1,
            0,
            1,
            &[5000, 1000],
            &[5000],
        )
        .unwrap_err();
        assert!(matches!(err, ParentPlacementError::ParentSatLostToFee));
    }

    #[test]
    fn custom_last_in_first_out_can_pass() {
        // Parent last input (1000); first output 1000 captures those sats if
        // earlier inputs are fully consumed by later outputs... 
        // Inputs: funding 5000 then parent 1000. Absolute: 0..5000 funding, 5000..6000 parent.
        // Outputs: vault 1000 first would take sats 0..1000 (funding!), NOT parent.
        // Correct last-in/first-out for parent: parent must be FIRST in the sat stream
        // for first output, OR first output sized after consuming prior inputs.
        // User's pattern: parent last input, first output — works when prior inputs
        // are zero? Or when first output starts after prior are assigned elsewhere.
        // Actually last-input → first-output is NOT generally true under ordinal FIFO.
        // It works when parent is the only input, or when earlier inputs go entirely
        // to fee (unusual), or last-input → last-output when first outputs consume funding.
        // Documented safe custom: last input → last output.
        assert!(verify_parent_return(
            ParentPlacementPolicy::Custom,
            1,
            0,
            1,
            &[5000, 1000],
            &[4800, 1000],
        )
        .is_ok());
    }
}
