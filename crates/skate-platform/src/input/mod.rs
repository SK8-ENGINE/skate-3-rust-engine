//! Raw device transport. Signed axes/trigger bytes reach the TU3 converter
//! without Bevy/gilrs deadzones or normalized-axis reconstruction.
use skate_core::input::xbox::XboxState;

pub struct DevicePacket {
    pub number: u32,
    pub state: XboxState,
    pub subtype: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceError {
    Disconnected,
    State(u32),
    Capabilities(u32),
}

/// Device identity is metadata; raw input is still sampled every host frame.
/// Refresh periodically as well as after errors, so hot swaps cannot leave a
/// subtype cached indefinitely even if the OS never exposes a disconnect.
#[derive(Default)]
pub struct CapabilityCache {
    value: Option<(u8, std::time::Instant)>,
}
impl CapabilityCache {
    pub fn invalidate(&mut self) {
        self.value = None;
    }
    fn get(
        &mut self,
        now: std::time::Instant,
        read: impl FnOnce() -> Result<u8, DeviceError>,
    ) -> Result<u8, DeviceError> {
        if let Some((subtype, expires)) = self.value {
            if now < expires {
                return Ok(subtype);
            }
        }
        self.value = None;
        let subtype = read()?;
        self.value = Some((subtype, now + std::time::Duration::from_secs(1)));
        Ok(subtype)
    }
}

#[cfg(windows)]
mod xinput;
#[cfg(not(windows))]
mod gilrs_raw;
#[cfg(not(windows))]
use gilrs_raw as imp;
#[cfg(windows)]
use xinput as imp;

pub fn poll_cached(index: usize, cache: &mut CapabilityCache) -> Result<DevicePacket, DeviceError> {
    assert!(index < 4);
    imp::poll(index as u32, cache)
}

// Preserve the uncached API for menu-only polling.
pub fn poll(index: usize) -> Result<DevicePacket, DeviceError> {
    poll_cached(index, &mut CapabilityCache::default())
}
