//! Follow the leader's distance meter (HUD element `distance`, vtable 0x8230622C, movie `hud2/huddistance`).
//! Details: docs/hails-additions/42-challenges.md#the-distance-meter

/// Retail's values as the defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeterTuning {
    /// The text turns red above this share of the max distance (0x821EE79C).
    pub red_fraction: f32,
    /// HUD ticks between refreshes (826648D0: every 5th tick).
    pub refresh_ticks: u32,
    pub tick_hz: f32,
}

impl Default for MeterTuning {
    fn default() -> Self {
        Self { red_fraction: 0.6, refresh_ticks: 5, tick_hz: 60.0 }
    }
}

/// `DistanceMeter_GetData` item 1 (826647E8): 0 = red, 1 = white.
pub fn flag(current: f32, max: f32, t: &MeterTuning) -> i32 {
    if max >= 0.0 && current / max > t.red_fraction { 0 } else { 1 }
}

/// The meter's own clock: the distance it shows changes only on a refresh.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DistanceMeter {
    pub max: f32,
    pub shown: f32,
    ticks: u32,
    carry: f32,
}

impl DistanceMeter {
    /// `SetupDistanceHUDData(name, max, current)` (82850320): shows `current` at once.
    pub fn setup(max: f32, current: f32) -> Self {
        Self { max, shown: current, ticks: 0, carry: 0.0 }
    }

    /// Advance by `seconds` of HUD ticks; true when a refresh took `current`.
    pub fn advance(&mut self, seconds: f32, current: f32, t: &MeterTuning) -> bool {
        let period = 1.0 / t.tick_hz.max(1.0);
        self.carry = (self.carry + seconds.max(0.0)).min(period * 30.0);
        let mut refreshed = false;
        while self.carry >= period {
            self.carry -= period;
            self.ticks += 1;
            if self.ticks >= t.refresh_ticks.max(1) {
                self.ticks = 0;
                self.shown = current;
                refreshed = true;
            }
        }
        refreshed
    }

    pub fn flag(&self, t: &MeterTuning) -> i32 {
        flag(self.shown, self.max, t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn red_above_sixty_percent_of_the_max() {
        let t = MeterTuning::default();
        assert_eq!(flag(60.0, 100.0, &t), 1);
        assert_eq!(flag(60.5, 100.0, &t), 0);
        assert_eq!(flag(0.0, 0.0, &t), 1, "0 / 0 is not above");
        assert_eq!(flag(5.0, 0.0, &t), 0);
        assert_eq!(flag(90.0, -1.0, &t), 1, "a negative max never turns red");
    }

    #[test]
    fn refreshes_every_fifth_tick_at_any_frame_rate() {
        let t = MeterTuning::default();
        for fps in [30.0f32, 60.0, 144.0] {
            let mut m = DistanceMeter::setup(50.0, 10.0);
            let mut refreshes = 0;
            for frame in 0..(fps as usize) {
                if m.advance(1.0 / fps, 10.0 + frame as f32, &t) {
                    refreshes += 1;
                }
            }
            assert!((11..=12).contains(&refreshes), "{fps} fps: {refreshes}");
        }
        let mut m = DistanceMeter::setup(50.0, 10.0);
        assert!(!m.advance(4.0 / 60.0, 40.0, &t));
        assert_eq!((m.shown, m.flag(&t)), (10.0, 1));
        assert!(m.advance(1.0 / 60.0, 40.0, &t));
        assert_eq!((m.shown, m.flag(&t)), (40.0, 0));
    }
}
