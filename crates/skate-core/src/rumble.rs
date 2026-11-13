//! Controller rumble ported from the TU3 rumble controller (82694D38, 82695010, curve 8296D480); data-driven.
//! Details: docs/hails-additions/23-input-controllers.md#rumble
use std::collections::BTreeMap;

/// Retail ticks the rumble controller with a 1/60 rate scale (the console cadence).
pub const TICK_HZ: f32 = 60.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RumbleTuning {
    /// Decay = clamp(1 - rate * scale): 1/60 at 0x820849C8.
    pub rate_scale: f32,
    /// A channel goes once both motors are below this after its decay: 0.001 at 0x82063A48.
    pub drop_below: f32,
    /// Cap headroom over the strongest channel: 0.2 at 0x82099280.
    pub headroom: f32,
    /// Device curve, wireless pads (caps flag X_INPUT_CAPS_WIRELESS, `sub_8296D480`): motor 0 = a * level - b
    /// (1.2 / 0.02), motor 1 = c * level + d (1.05 / 0.05).
    pub pad_low: [f32; 2],
    pub pad_high: [f32; 2],
    /// Device curve, wired and other pads: motor 0 = e * level - b, motor 1 = e * level (0.7).
    pub other_scale: f32,
}

impl RumbleTuning {
    pub const RETAIL: Self = Self {
        rate_scale: 1.0 / 60.0,
        drop_below: 0.001,
        headroom: 0.2,
        pad_low: [1.2, 0.02],
        pad_high: [1.05, 0.05],
        other_scale: 0.7,
    };
}

impl Default for RumbleTuning {
    fn default() -> Self {
        Self::RETAIL
    }
}

/// One rumble start (cMsgRumble / cMsgRumbleChannel): motor levels 0..1 and a decay rate per second.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rumble {
    pub low: f32,
    pub high: f32,
    pub rate: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Slot {
    low: f32,
    high: f32,
    decay: f32,
}

/// One pad's rumble channels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PadRumble {
    channels: BTreeMap<u32, Slot>,
    /// The pad's vibration flag (pad +408, Game Settings > Vibration). Only starts check it: rumble already
    /// running fades out when it goes off.
    pub enabled: bool,
}

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

impl PadRumble {
    pub fn new(enabled: bool) -> Self {
        Self { channels: BTreeMap::new(), enabled }
    }

    /// Start (or restart) a channel; nothing while vibration is off.
    pub fn start(&mut self, channel: u32, rumble: Rumble, tuning: &RumbleTuning) {
        if !self.enabled {
            return;
        }
        let decay = clamp01(1.0 - rumble.rate * tuning.rate_scale);
        self.channels.insert(channel, Slot { low: clamp01(rumble.low), high: clamp01(rumble.high), decay });
    }

    /// One channel stops at once (mods; retail lets channels decay or resets the pad).
    pub fn stop(&mut self, channel: u32) {
        self.channels.remove(&channel);
    }

    /// cMsgRumbleReset: every channel stops.
    pub fn reset(&mut self) {
        self.channels.clear();
    }

    pub fn active(&self) -> bool {
        !self.channels.is_empty()
    }

    /// One retail tick: the mixed [low, high] levels (0..1) from the channels as they were, then each channel
    /// decays and is dropped once both motors are below the threshold.
    pub fn tick(&mut self, tuning: &RumbleTuning) -> [f32; 2] {
        let (mut sum, mut max) = ([0.0f32; 2], [0.0f32; 2]);
        self.channels.retain(|_, slot| {
            sum[0] += slot.low;
            sum[1] += slot.high;
            max[0] = max[0].max(slot.low);
            max[1] = max[1].max(slot.high);
            slot.low *= slot.decay;
            slot.high *= slot.decay;
            !(slot.low < tuning.drop_below && slot.high < tuning.drop_below)
        });
        std::array::from_fn(|m| {
            let cap = (1.0 - max[m]).mul_add(tuning.headroom, max[m]);
            clamp01(sum[m].max(0.0).min(cap))
        })
    }
}

/// Device curve: motor level to 16-bit speed (8296D480; wireless vs wired curve, retail order): docs/hails-additions/23-input-controllers.md#rumble-device-curve
pub fn motor_speed(motor: usize, level: f32, wireless: bool, tuning: &RumbleTuning) -> u16 {
    let v = match (wireless, motor) {
        (true, 0) => level.mul_add(tuning.pad_low[0], -tuning.pad_low[1]),
        (true, _) => level.mul_add(tuning.pad_high[0], tuning.pad_high[1]),
        (false, 0) => level.mul_add(tuning.other_scale, -tuning.pad_low[1]),
        (false, _) => level * tuning.other_scale,
    };
    let v = clamp01(v);
    if v == 0.0 { 0 } else { ((v * 65535.0) as i32).clamp(0, 65535) as u16 }
}

#[cfg(test)]
mod tests {
    use super::*;
    const T: RumbleTuning = RumbleTuning::RETAIL;

    #[test]
    fn a_start_decays_per_tick_and_drops_below_the_threshold() {
        let mut pad = PadRumble::new(true);
        pad.start(0, Rumble { low: 0.5, high: 0.5, rate: 5.0 }, &T);
        let first = pad.tick(&T);
        assert_eq!(first, [0.5, 0.5], "the first tick mixes the levels as started");
        let decay = 1.0 - 5.0 / 60.0;
        let second = pad.tick(&T);
        assert!((second[0] - 0.5 * decay).abs() < 1e-6);
        let mut ticks = 2;
        while pad.active() {
            pad.tick(&T);
            ticks += 1;
        }
        // 0.5 * (11/12)^n < 0.001 first at n = 72.
        assert_eq!(ticks, 72);
        assert_eq!(pad.tick(&T), [0.0, 0.0]);
    }

    #[test]
    fn channels_sum_up_to_the_capped_maximum_and_restart_overwrites() {
        let mut pad = PadRumble::new(true);
        pad.start(1, Rumble { low: 0.5, high: 0.1, rate: 0.0 }, &T);
        pad.start(2, Rumble { low: 0.5, high: 0.1, rate: 0.0 }, &T);
        // Sum 1.0, cap 0.5 + 0.5 * 0.2 = 0.6; high: sum 0.2, cap 0.1 + 0.9 * 0.2 = 0.28.
        let [low, high] = pad.tick(&T);
        assert!((low - 0.6).abs() < 1e-6 && (high - 0.2).abs() < 1e-6);
        pad.start(2, Rumble { low: 0.0, high: 0.0, rate: 0.0 }, &T);
        assert!((pad.tick(&T)[0] - 0.5).abs() < 1e-6, "same channel replaces its slot");
        pad.reset();
        assert!(!pad.active());
    }

    #[test]
    fn vibration_off_blocks_starts_but_running_rumble_fades_out() {
        let mut pad = PadRumble::new(false);
        pad.start(0, Rumble { low: 1.0, high: 1.0, rate: 1.0 }, &T);
        assert!(!pad.active());
        pad.enabled = true;
        pad.start(0, Rumble { low: 1.0, high: 1.0, rate: 1.0 }, &T);
        pad.enabled = false;
        assert!(pad.active(), "turning it off does not stop a running rumble");
        assert!(pad.tick(&T)[0] > 0.9);
    }

    #[test]
    fn device_curve_matches_retail_including_the_wireless_motor_1_floor() {
        assert_eq!(motor_speed(0, 0.0, true, &T), 0);
        assert_eq!(motor_speed(1, 0.0, true, &T), 3276);
        assert_eq!(motor_speed(0, 0.5, true, &T), ((0.5f32 * 1.2 - 0.02) * 65535.0) as u16);
        assert_eq!(motor_speed(1, 1.0, true, &T), 65535);
        assert_eq!(motor_speed(0, 1.0, false, &T), ((0.7f32 - 0.02) * 65535.0) as u16);
        assert_eq!(motor_speed(1, 0.0, false, &T), 0, "the wired curve has no floor");
        assert_eq!(motor_speed(0, 0.0, false, &T), 0);
    }
}
