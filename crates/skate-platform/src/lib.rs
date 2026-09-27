//! Platform abstraction: raw gamepad transport, detached process spawning,
//! helper executable naming and crash reporting. Windows keeps the original
//! XInput/creation-flags behavior; other platforms use gilrs and no-ops.
pub mod crash;
pub mod exe;
pub mod input;
pub mod process;
