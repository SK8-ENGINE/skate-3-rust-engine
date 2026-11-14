//! The skitch hold step and release (82D49D70, 82D49580, tail 82D4AFB8, reset 82D47318; b51 to b53).
//! Details: docs/hails-additions/26h-living-world-traffic.md#skitch-hold-step

/// The release bits of state+1344.
pub mod bits {
    pub const RATCHET: u8 = 0x08;
    pub const RELEASED: u8 = 0x10;
    pub const PUSH_OFF: u8 = 0x20;
    pub const PULL_IN: u8 = 0x40;
}

/// `physics_state_skitching/default` values (mod-overridable).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoldSettings {
    /// `1F5F8C47EF1C7C97` (0.02) and `4CC8C0D741C121D4` (0.1): rest thresholds.
    pub rest_rate: f32,
    pub rest_offset: f32,
    /// `8FEF0AD38542CECD` (0.2) and `CFBBDDD08082CF69` (2.0 s): ratchet re-arm.
    pub rearm_offset: f32,
    pub rearm_time: f32,
    /// `FF6B65AF360C6707` (0.25) and `EA885312130505B0` (0.03 per tick): the posed hand's follower.
    pub follow_gain: f32,
    pub follow_step: f32,
    /// `CC257F85657759A1` (0.5): the hold timer 1020 at entry; `659537728CE30B02` (0.5): the re-grab block 1016.
    pub hold_time: f32,
    pub regrab_block: f32,
    /// The release frames (literals: impulse on frame 1, the state ends after frame 3).
    pub release_frames: u32,
}

impl Default for HoldSettings {
    fn default() -> Self {
        Self { rest_rate: 0.02, rest_offset: 0.1, rearm_offset: 0.2, rearm_time: 2.0, follow_gain: 0.25, follow_step: 0.03, hold_time: 0.5, regrab_block: 0.5, release_frames: 3 }
    }
}

/// The hold / release part of the skitch state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HoldState {
    /// 904 target, 908 posed hand, 912 its step, 964 the time since the ratchet cleared.
    pub target: f32,
    pub posed: f32,
    pub posed_step: f32,
    pub idle: f32,
    /// state+1344.
    pub bits: u8,
    /// 1020 (hold timer), 1016 (re-grab block, published as the ground timer), 1320 (frames in sub-mode 4).
    pub hold_timer: f32,
    pub regrab_block: f32,
    pub release_frames: u32,
}

/// Which one-frame release impulse the tail queues.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Impulse {
    PullIn,
    PushOff,
}

impl HoldState {
    /// `82D47318`'s part: 1344 keeps only bit 0x04, the hold timer restarts, nothing released.
    pub fn reset(&mut self, s: &HoldSettings) {
        self.bits &= 0x04;
        self.hold_timer = s.hold_time;
        self.release_frames = 0;
    }

    /// `82D49D70`. `along` = 836, `rate` = 840, `half` = 928, `limit` = 932, `stick` = 924.
    pub fn step(&mut self, along: f32, rate: f32, half_limit: f32, stick: f32, s: &HoldSettings, dt: f32) {
        let offset = (along - self.posed).abs();
        let stick = if stick.abs() <= 0.01 { 0.0 } else { stick };
        if self.bits & bits::RATCHET != 0 {
            let c = along.clamp(-half_limit, half_limit);
            self.target = if stick > 0.5 { self.target.max(c) } else if stick < -0.5 { self.target.min(c) } else { c };
            if rate.abs() < s.rest_rate && offset < s.rest_offset {
                self.bits &= !bits::RATCHET;
            }
            self.idle = 0.0;
        } else {
            self.idle += dt;
            if offset > s.rearm_offset || self.idle > s.rearm_time {
                self.bits |= bits::RATCHET;
            }
        }
        let want = s.follow_gain * (self.target - self.posed);
        self.posed_step = want.clamp(self.posed_step - s.follow_step, self.posed_step + s.follow_step);
        self.posed += self.posed_step;
    }

    /// The release bits `82D49580` writes after the sub-mode step (`from` -> `to`, `switched` = its switch bit).
    pub fn mode_bits(&mut self, from: u8, to: u8, switched: bool) {
        match to {
            0 => self.bits &= !(bits::PULL_IN | bits::PUSH_OFF),
            1 => self.bits |= bits::PULL_IN,
            2 | 3 => self.bits |= bits::PUSH_OFF,
            _ if from == 2 => self.bits &= !(bits::PULL_IN | bits::PUSH_OFF),
            _ => {}
        }
        if switched {
            self.bits |= bits::RATCHET;
        }
    }

    /// `82D4AFB8`. Returns the sub-mode to switch to (4) and the impulse to queue this frame.
    pub fn tail(&mut self, mode: u8, grab_input: bool, hands_off: bool, s: &HoldSettings, dt: f32) -> (u8, Option<Impulse>) {
        let mut mode = mode;
        if !grab_input {
            mode = 4;
        } else if matches!(mode, 1 | 2) && hands_off && self.hold_timer <= 0.0 {
            mode = 4;
            self.bits = (self.bits & !0x70) | bits::RELEASED;
        }
        if self.hold_timer > 0.0 {
            self.hold_timer -= dt;
        }
        let mut impulse = None;
        if mode == 4 {
            if self.regrab_block <= 0.0 {
                self.regrab_block = s.regrab_block;
            }
            self.release_frames += 1;
            if self.release_frames == 1 {
                impulse = if self.bits & bits::PULL_IN != 0 {
                    Some(Impulse::PullIn)
                } else if self.bits & bits::PUSH_OFF != 0 {
                    Some(Impulse::PushOff)
                } else {
                    None
                };
            }
            if self.release_frames > s.release_frames {
                self.bits |= bits::RELEASED;
            }
        } else {
            self.release_frames = 0;
        }
        (mode, impulse)
    }

    /// The state ends (1344 bit 0x10).
    pub fn released(&self) -> bool {
        self.bits & bits::RELEASED != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    #[test]
    fn the_posed_hand_eases_toward_the_target_in_capped_steps() {
        let s = HoldSettings::default();
        let mut h = HoldState { bits: bits::RATCHET, ..Default::default() };
        h.step(0.6, 1.0, 0.8, 0.0, &s, DT);
        assert_eq!(h.target, 0.6);
        assert!((h.posed - 0.03).abs() < 1e-6, "the step grows 0.03 per tick: {}", h.posed);
        for _ in 0..200 {
            h.step(0.6, 1.0, 0.8, 0.0, &s, DT);
        }
        assert!((h.posed - 0.6).abs() < 0.01);
    }

    #[test]
    fn the_stick_ratchets_the_target_and_rest_clears_the_bit() {
        let s = HoldSettings::default();
        let mut h = HoldState { bits: bits::RATCHET, target: 0.5, posed: 0.45, ..Default::default() };
        h.step(0.2, 1.0, 0.8, 0.9, &s, DT);
        assert_eq!(h.target, 0.5, "pushing right keeps the farther target");
        h.step(0.46, 0.0, 0.8, 0.0, &s, DT);
        assert_eq!(h.bits & bits::RATCHET, 0, "at rest near the posed hand");
    }

    #[test]
    fn release_queues_one_impulse_then_ends_after_three_frames() {
        let s = HoldSettings::default();
        let mut h = HoldState::default();
        h.reset(&s);
        h.mode_bits(0, 2, true);
        assert_eq!(h.bits & (bits::PUSH_OFF | bits::RATCHET), bits::PUSH_OFF | bits::RATCHET);
        let (m, i) = h.tail(2, false, false, &s, DT);
        assert_eq!((m, i), (4, Some(Impulse::PushOff)), "grab input let go: push off");
        assert!((h.regrab_block - 0.5).abs() < 1e-6);
        for _ in 0..2 {
            assert_eq!(h.tail(4, false, false, &s, DT), (4, None));
        }
        assert!(!h.released());
        h.tail(4, false, false, &s, DT);
        assert!(h.released());
        // 2 -> 4 by the sub-mode clears both impulse bits; 3 -> 4 keeps the push off.
        let mut a = HoldState { bits: bits::PUSH_OFF, ..Default::default() };
        a.mode_bits(2, 4, true);
        assert_eq!(a.bits & bits::PUSH_OFF, 0);
        let mut b = HoldState { bits: bits::PUSH_OFF, ..Default::default() };
        b.mode_bits(3, 4, false);
        assert_eq!(b.bits & bits::PUSH_OFF, bits::PUSH_OFF);
    }
}
