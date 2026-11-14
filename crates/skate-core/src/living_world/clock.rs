//! World-step clock: the retail living-world update runs on a fixed 60 Hz world step (8285C928 -> 82859E70).
//! Evidence and conversion: docs/hails-additions/26b-living-world-population.md#world-step-clock

/// Retail world steps per game second (`sub_826BDB50` steps with `1/60`, `0x820849C8`) [code].
pub const RETAIL_TICK_HZ: f64 = 60.0;
/// At most this many ticks per `advance` (about 0.27 s of world time, so a long hitch does not
/// run minutes of census at once). Engine guard, not a retail value.
pub const MAX_TICKS_PER_ADVANCE: u32 = 16;

#[derive(Clone, Debug, PartialEq)]
pub struct ConsoleClock {
    /// World ticks per second (default [`RETAIL_TICK_HZ`] = 60).
    pub hz: f64,
    /// At most this many ticks per `advance` (a long hitch does not run minutes of census).
    pub max_ticks_per_advance: u32,
    accumulator: f64,
    ticks: u64,
}

impl Default for ConsoleClock {
    fn default() -> Self {
        Self::new(RETAIL_TICK_HZ)
    }
}

impl ConsoleClock {
    pub fn new(hz: f64) -> Self {
        Self { hz: hz.max(1.0), max_ticks_per_advance: MAX_TICKS_PER_ADVANCE, accumulator: 0.0, ticks: 0 }
    }

    /// Add `seconds` of game time; returns how many console ticks are due now.
    pub fn advance(&mut self, seconds: f64) -> u32 {
        if !seconds.is_finite() || seconds <= 0.0 {
            return 0;
        }
        let period = 1.0 / self.hz;
        self.accumulator += seconds;
        let mut due = 0;
        // A tiny epsilon keeps e.g. 2 x (1/120) == 1/60 from losing a tick to rounding.
        while self.accumulator + 1e-9 >= period && due < self.max_ticks_per_advance {
            self.accumulator -= period;
            due += 1;
        }
        if due == self.max_ticks_per_advance && self.accumulator >= period {
            self.accumulator %= period;
        }
        self.accumulator = self.accumulator.max(0.0);
        self.ticks += due as u64;
        due
    }

    /// Fraction (0..1) of the next tick already elapsed (render interpolation).
    pub fn overstep(&self) -> f64 {
        (self.accumulator * self.hz).clamp(0.0, 1.0)
    }

    /// Console ticks counted so far.
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    pub fn reset(&mut self) {
        self.accumulator = 0.0;
        self.ticks = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_rate_does_not_change_the_tick_count() {
        for engine_hz in [30.0, 60.0, 64.0, 120.0, 144.0, 240.0] {
            let mut clock = ConsoleClock::default();
            let steps = (engine_hz * 10.0) as u32; // 10 s
            let total: u32 = (0..steps).map(|_| clock.advance(1.0 / engine_hz)).sum();
            assert!((599..=600).contains(&total), "{engine_hz} Hz gave {total} ticks");
        }
    }

    #[test]
    fn a_hitch_is_capped() {
        let mut clock = ConsoleClock::default();
        assert_eq!(clock.advance(5.0), MAX_TICKS_PER_ADVANCE);
        assert_eq!(clock.advance(1.0 / 60.0), 1);
    }

    #[test]
    fn the_retail_tick_is_the_60_hz_world_step() {
        // sub_826BDB50 steps with 1/60 s (0x820849C8) and the census / skater manager run once
        // per call [code]; the lights' 8.0 s phase took 480 ticks [trace].
        assert_eq!(ConsoleClock::default().hz, 60.0);
        let mut clock = ConsoleClock::default();
        let ticks: u32 = (0..8 * 60).map(|_| clock.advance(1.0 / 60.0)).sum();
        assert_eq!(ticks, 480);
    }
}
