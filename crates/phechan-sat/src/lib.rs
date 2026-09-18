//! Sat-flow simulation and parent placement policies.

mod flow;
mod parent;

pub use flow::{simulate_sat_flow, SatFlowError, SatFlowResult, SatLocation};
pub use parent::{verify_parent_return, ParentPlacementError, ParentPlacementPolicy};
