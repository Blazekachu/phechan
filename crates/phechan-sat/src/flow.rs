//! Ordinal sat-flow simulation (input sats → outputs, remainder to fees).

/// Where a sat lands after a transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SatLocation {
    Output { vout: usize },
    Fee,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SatFlowResult {
    pub input_values: Vec<u64>,
    pub output_values: Vec<u64>,
    pub fee: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SatFlowError {
    OutputsExceedInputs { input_sum: u64, output_sum: u64 },
}

impl std::fmt::Display for SatFlowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutputsExceedInputs {
                input_sum,
                output_sum,
            } => write!(
                f,
                "outputs ({output_sum}) exceed inputs ({input_sum})"
            ),
        }
    }
}

impl std::error::Error for SatFlowError {}

/// Simulate ordinal sat assignment: sats are ordered by input index then offset;
/// outputs consume that stream in order; leftover sats are fees.
pub fn simulate_sat_flow(
    input_values: &[u64],
    output_values: &[u64],
) -> Result<SatFlowResult, SatFlowError> {
    let input_sum: u64 = input_values.iter().sum();
    let output_sum: u64 = output_values.iter().sum();
    if output_sum > input_sum {
        return Err(SatFlowError::OutputsExceedInputs {
            input_sum,
            output_sum,
        });
    }
    Ok(SatFlowResult {
        input_values: input_values.to_vec(),
        output_values: output_values.to_vec(),
        fee: input_sum - output_sum,
    })
}

impl SatFlowResult {
    /// Global offset of the first sat of `input_index`.
    pub fn input_start_offset(&self, input_index: usize) -> Option<u64> {
        if input_index >= self.input_values.len() {
            return None;
        }
        Some(self.input_values[..input_index].iter().sum())
    }

    /// Absolute offset of a sat within the transaction's input stream.
    pub fn absolute_offset(&self, input_index: usize, sat_offset: u64) -> Option<u64> {
        let start = self.input_start_offset(input_index)?;
        let value = *self.input_values.get(input_index)?;
        if sat_offset >= value {
            return None;
        }
        Some(start + sat_offset)
    }

    /// Where the sat at absolute `offset` lands.
    pub fn location_of(&self, absolute_offset: u64) -> SatLocation {
        let mut cursor = 0u64;
        for (vout, &value) in self.output_values.iter().enumerate() {
            let end = cursor + value;
            if absolute_offset < end {
                return SatLocation::Output { vout };
            }
            cursor = end;
        }
        SatLocation::Fee
    }

    pub fn location_of_input_sat(&self, input_index: usize, sat_offset: u64) -> Option<SatLocation> {
        let abs = self.absolute_offset(input_index, sat_offset)?;
        Some(self.location_of(abs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_assignment() {
        let flow = simulate_sat_flow(&[1000, 2000], &[1000, 1500]).unwrap();
        assert_eq!(flow.fee, 500);
        assert_eq!(
            flow.location_of_input_sat(0, 0),
            Some(SatLocation::Output { vout: 0 })
        );
        assert_eq!(
            flow.location_of_input_sat(1, 0),
            Some(SatLocation::Output { vout: 1 })
        );
        assert_eq!(flow.location_of(2500), SatLocation::Fee);
    }

    #[test]
    fn parent_second_input_can_fall_into_fee() {
        // Funding first (5000), parent second (1000). Single output takes only funding
        // sats → parent range 5000..6000 becomes fee (fee-tail class).
        let flow = simulate_sat_flow(&[5000, 1000], &[5000]).unwrap();
        assert_eq!(flow.fee, 1000);
        assert_eq!(
            flow.location_of_input_sat(1, 0),
            Some(SatLocation::Fee)
        );
    }

    #[test]
    fn parent_first_in_first_out_safe() {
        let flow = simulate_sat_flow(&[1000, 5000], &[1000, 4800]).unwrap();
        assert_eq!(
            flow.location_of_input_sat(0, 0),
            Some(SatLocation::Output { vout: 0 })
        );
    }
}
