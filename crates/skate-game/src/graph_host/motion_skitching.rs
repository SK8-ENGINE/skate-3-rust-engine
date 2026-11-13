//! The skitch motion graph nodes (b58, b60, b61; `anim_skitching/default` 8289D550).
//! Behaviours, conditions and gaps: docs/hails-additions/26h-living-world-traffic.md#skitch-graph-nodes
use skate_core::point_graph::PointGraph;
use skate_data::collections::Collections;

/// `anim_skitching/default` (data-driven; the stock values as defaults).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Settings {
    pub absorb_curve: PointGraph<8>,
    /// The record floats +80..+100 in layout order.
    pub floats: [f32; 6],
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            absorb_curve: PointGraph { x: [0.0, 2.8339, 5.57, 7.4593, 8.8274, 9.9349, 11.9218, 14.9837], y: [0.0, 0.0, 0.1286, 0.3429, 0.5571, 0.7643, 0.9214, 1.0] },
            floats: [0.1, 0.25, 0.85, 0.05, 0.83, 0.121],
        }
    }
}

/// The fields at +80..+100 in layout order.
const FLOAT_FIELDS: [&str; 6] = ["Hash_8738E31E171E2A87", "Hash_B2D3A764E1D86D13", "Hash_F146CFA3B03529A5", "Hash_1C7DF104E24101FF", "LongSkitchIntoReachTime", "Hash_6E6EFE7CF9BEF4B8"];

impl Settings {
    /// The record's values where present, else the stock defaults.
    pub(crate) fn load(data: &Collections) -> Self {
        let mut s = Self::default();
        if let Ok(w) = data.words::<20>("anim_skitching", "default", "Hash_1C04030E5B00A8EA") {
            s.absorb_curve = PointGraph { x: std::array::from_fn(|i| f32::from_bits(w[4 + i])), y: std::array::from_fn(|i| f32::from_bits(w[12 + i])) };
        }
        for (i, name) in FLOAT_FIELDS.iter().enumerate() {
            if let Ok(v) = data.float("anim_skitching", "default", name) {
                s.floats[i] = v;
            }
        }
        s
    }

    /// Crouch = ground+280 + rec+100.
    pub(crate) fn crouch(&self, grab_height: f32) -> f32 {
        grab_height + self.floats[5]
    }

    pub(crate) fn absorb_target(&self, closing: f32) -> f32 {
        self.absorb_curve.evaluate(closing)
    }
}

/// SkitchingBehaviour's per-activation instance (16 bytes; the absorb value at +12).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SkitchingState {
    pub absorb: f32,
}

impl SkitchingState {
    /// "absorbspeed" moves toward the curve's value by at most 0.04 per update.
    pub(crate) fn update(&mut self, settings: &Settings, closing: f32) -> f32 {
        let target = settings.absorb_target(closing);
        self.absorb += (target - self.absorb).clamp(-0.04, 0.04);
        self.absorb
    }
}

/// EnterSkitchingBehaviour's instance (`+16`, -1 until the first update).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EnterState {
    pub reach: f32,
}

impl Default for EnterState {
    fn default() -> Self {
        Self { reach: -1.0 }
    }
}

impl EnterState {
    /// "reachspeed" from the time to the spline and the current tree's time / length.
    pub(crate) fn update(&mut self, s: &Settings, time_to_skitch: f32, time: f32, length: f32) -> f32 {
        let rem = if length > 0.0 { 1.0 - time / length } else { 0.0 };
        let x = if rem > 0.0 { time_to_skitch / rem } else { length };
        let (lo, hi) = (s.floats[0], s.floats[4]);
        let mut n = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
        if self.reach >= 0.0 {
            n = n.clamp(self.reach - 0.1, self.reach + 0.1);
        }
        self.reach = n;
        1.0 - n
    }
}

/// "ShimmySpeed" from the shimmy rate (`82BBC858`).
pub(crate) fn shimmy_speed(s: &Settings, rate: f32) -> f32 {
    let (lo, hi) = (s.floats[1], s.floats[2]);
    let a = rate.abs().clamp(lo, hi);
    if a > lo { (a - lo) / (hi - lo) } else { 0.0 }
}

/// IsSkitchShimmying's result for a shimmy rate (`82BBC1A0`): 1 left, 2 right, 0 none.
pub(crate) fn shimmy_direction(rate: f32, flipped: bool) -> u32 {
    let v = if flipped { -rate } else { rate };
    if v > 0.25 {
        1
    } else if v < -0.25 {
        2
    } else {
        0
    }
}

/// SkitchingPosition's result (`82BBC068`): hands 1 -> 1 + m, 2 -> 2 - m, else 3.
pub(crate) fn skitching_position(hands: u32, mirrored: bool) -> u32 {
    let m = u32::from(mirrored);
    match hands {
        1 => 1 + m,
        2 => 2 - m,
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absorb_follows_the_curve_at_most_four_hundredths_per_update() {
        let s = Settings::default();
        let mut st = SkitchingState::default();
        assert_eq!(st.update(&s, 14.9837), 0.04);
        for _ in 0..30 {
            st.update(&s, 14.9837);
        }
        assert!((st.absorb - 1.0).abs() < 1e-5);
        assert_eq!(s.absorb_target(2.0), 0.0, "no absorb below the curve's first rise");
        assert!((s.crouch(0.1) - 0.221).abs() < 1e-6, "rec+100 is 0.121");
    }

    #[test]
    fn reach_and_shimmy_speeds() {
        let s = Settings::default();
        let mut e = EnterState::default();
        // Half way through the clip, 0.2 s to the spline: X = 0.4, n = (0.4 - 0.1) / 0.73.
        let r = e.update(&s, 0.2, 0.5, 1.0);
        assert!((r - (1.0 - 0.3 / 0.73)).abs() < 1e-5, "{r}");
        // Next update far away: n moves at most 0.1.
        let n0 = e.reach;
        e.update(&s, 5.0, 0.5, 1.0);
        assert!((e.reach - (n0 + 0.1)).abs() < 1e-5);
        assert_eq!(shimmy_speed(&s, 0.1), 0.0);
        assert!((shimmy_speed(&s, -0.55) - 0.5).abs() < 1e-5);
        assert_eq!(shimmy_speed(&s, 2.0), 1.0);
    }

    #[test]
    fn shimmy_and_position_results() {
        assert_eq!((shimmy_direction(0.3, false), shimmy_direction(-0.3, false), shimmy_direction(0.1, false), shimmy_direction(0.3, true)), (1, 2, 0, 2));
        assert_eq!((skitching_position(1, false), skitching_position(2, false), skitching_position(3, false), skitching_position(1, true)), (1, 2, 3, 2));
    }
}
