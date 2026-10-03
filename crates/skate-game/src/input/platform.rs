//! Platform device transport lives in the skate-platform crate; this module
//! keeps the crate-local paths used by callers and tests stable.
pub(crate) use skate_platform::input::{
    CapabilityCache, DeviceError, DevicePacket, poll, poll_cached,
};
