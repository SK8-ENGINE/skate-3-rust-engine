//! Skate 3 TU3 `PhysicalPlayerHiLOD::Input` (`0x82DB4048`).
//!
//! The phase is split into direct field publication and an ordered runtime.
//! Separate native subsystems remain mandatory service calls; none have a
//! fallback implementation here.

mod publication;
mod motion_math;
mod runtime;
mod types;
mod air_output;
mod pose_output;

pub use runtime::{InputContinuation, InputPhaseError, InputPhaseServices, process_input, start_input, finish_input};
pub use types::*;
pub use air_output::AirOutputFields;
pub use pose_output::{AnimationOutputFields, ScoringOutputFields, SkeletonOutputFields};

#[cfg(test)]
mod tests;

