//! Race AI racers (research b969, b972, b987, b992): the line-up (ids, characters, start slots), the per-racer AI state
//! the race graph drives (pin, leg, path started), the leg test on lines and the ETA rubber band. Engine-independent:
//! the game spawns the riders and feeds positions and ETAs.
//! Details: docs/hails-additions/42-challenges.md#change-race-ai-racers-challengesrace_airs

/// Rubber band and speed shape tunables with the retail values as defaults (b969); data per race, a mod can replace
/// them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandTuning {
    /// ETA gap (s) where the band starts (factor 0 at or under it).
    pub ramp_start: f32,
    /// ETA gap (s) where the band is full (factor 1 at or over it).
    pub ramp_end: f32,
    /// k1 of the speed shape (82470830).
    pub shape_gain: f32,
    /// k2 of the speed shape: how much the band moves the speed.
    pub band_gain: f32,
}

impl Default for BandTuning {
    fn default() -> Self {
        Self { ramp_start: 2.0, ramp_end: 5.0, shape_gain: 0.3, band_gain: 0.125 }
    }
}

/// The race band (8246F0F0, races and challenge type 0x14): `d = ETA(player) - ETA(this racer)`; `|d|` ramps from 0 at
/// `ramp_start` to 1 at `ramp_end`; negative (slow down) when the racer is ahead (`d > 0`), positive when it is behind.
/// `player_flag` (the local player's byte +0x3B, meaning not decoded) caps the factor at -1.
pub fn eta_band(player_eta: f32, racer_eta: f32, player_flag: bool, t: &BandTuning) -> f32 {
    let d = player_eta - racer_eta;
    let span = (t.ramp_end - t.ramp_start).max(f32::EPSILON);
    let magnitude = ((d.abs() - t.ramp_start) / span).clamp(0.0, 1.0);
    let factor = if d > 0.0 { -magnitude } else { magnitude };
    if player_flag { factor.min(-1.0) } else { factor }
}

/// The speed shape (82470830): `s + k1 * s * shape + k2 * s * factor`; the caller clamps to its speed limits.
pub fn shaped_speed(speed: f32, shape: f32, factor: f32, t: &BandTuning) -> f32 {
    speed + t.shape_gain * speed * shape + t.band_gain * speed * factor
}

/// Most racers a race holds, the player included (race_shared asserts it).
pub const MAX_COMPETITORS: usize = 6;

/// One competitor: id 0 is the player (no character); AI racer k races as `character` and starts at slot k (b992).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Competitor {
    pub id: u32,
    /// The ai_characters row; None for the player.
    pub character: Option<String>,
}

/// The single player line-up (82566740 + setup_code, b987 / b992): the player is id 0; AI ids 1.. take the row's
/// competitors after a forward Fisher-Yates (`j = rng() % (i + 1)`, i from 1) when the row shuffles (every career
/// race). Extra competitors past the limit are dropped.
pub fn lineup(characters: &[String], shuffle: bool, mut rng: impl FnMut() -> u32) -> Vec<Competitor> {
    let mut list: Vec<String> = characters.iter().take(MAX_COMPETITORS - 1).cloned().collect();
    if shuffle {
        for i in 1..list.len() {
            let j = rng() as usize % (i + 1);
            list.swap(i, j);
        }
    }
    std::iter::once(Competitor { id: 0, character: None })
        .chain(list.into_iter().enumerate().map(|(k, c)| Competitor { id: k as u32 + 1, character: Some(c) }))
        .collect()
}

/// One AI racer's state as the race graph drives it (b978 §3-4): pinned (PinAI / UnpinAI), the leg for the line test
/// (StartLapPaths; -1 = no leg test), whether its path is running (StartPath) and whether it finished.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RacerAi {
    pub id: u32,
    pub pinned: bool,
    pub leg: i32,
    pub running: bool,
    pub finished: bool,
}

impl RacerAi {
    pub fn new(id: u32) -> Self {
        Self { id, pinned: false, leg: -1, running: false, finished: false }
    }

    /// setup_heat: StartLapPaths(id, 1) for racers with gates (one-based leg, as the graph passes it).
    pub fn start_lap_paths(&mut self, leg: i32) {
        self.leg = leg;
    }

    /// run_heat enter: UnpinAI, then StartPath.
    pub fn go(&mut self) {
        self.pinned = false;
        self.running = true;
    }

    /// The finish split: PinAI and FinishPlayer.
    pub fn finish(&mut self) {
        self.pinned = true;
        self.running = false;
        self.finished = true;
    }

    /// Whether it moves: a pinned or finished racer stands still.
    pub fn moving(&self) -> bool {
        self.running && !self.pinned
    }
}

/// The leg part of the line test (82456970, b972): with a leg set, a line whose flag bit `9 + leg` is set is not used
/// on that leg; leg -1 skips the test. No retail line sets those bits (b972 §2); kept for mods.
pub fn line_allowed_on_leg(line_flags: u32, leg: i32) -> bool {
    leg < 0 || 9 + leg >= 32 || line_flags & (1 << (9 + leg)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_band_slows_a_racer_ahead_and_speeds_one_behind_between_two_and_five_seconds() {
        let t = BandTuning::default();
        assert_eq!(eta_band(10.0, 9.0, false, &t), 0.0, "1 s gap: no band");
        assert!((eta_band(13.5, 10.0, false, &t) + 0.5).abs() < 1e-6, "racer 3.5 s ahead: -0.5");
        assert_eq!(eta_band(10.0, 20.0, false, &t), 1.0, "racer 10 s behind: full speed up");
        assert_eq!(eta_band(10.0, 20.0, true, &t), -1.0, "the player flag caps at -1");
        assert!((shaped_speed(8.0, 0.0, 1.0, &t) - 9.0).abs() < 1e-6);
        assert!((shaped_speed(8.0, 0.5, -1.0, &t) - (8.0 + 1.2 - 1.0)).abs() < 1e-6);
    }

    #[test]
    fn the_player_is_id_zero_and_the_ai_racers_take_the_shuffled_list() {
        let names: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let fixed = lineup(&names, false, || 0);
        assert_eq!(fixed.iter().map(|c| (c.id, c.character.as_deref())).collect::<Vec<_>>(),
            [(0, None), (1, Some("a")), (2, Some("b")), (3, Some("c"))]);
        // i = 1: j = 0 -> b a c; i = 2: j = 0 -> c a b.
        let shuffled = lineup(&names, true, || 0);
        assert_eq!(shuffled.iter().filter_map(|c| c.character.as_deref()).collect::<Vec<_>>(), ["c", "a", "b"]);
        let many: Vec<String> = (0..9).map(|i| i.to_string()).collect();
        assert_eq!(lineup(&many, false, || 0).len(), MAX_COMPETITORS);
    }

    #[test]
    fn a_racer_moves_only_after_go_and_stops_at_its_finish() {
        let mut r = RacerAi::new(2);
        r.pinned = true;
        r.start_lap_paths(1);
        assert!(!r.moving());
        r.go();
        assert!(r.moving() && r.leg == 1);
        r.finish();
        assert!(!r.moving() && r.finished);
    }

    #[test]
    fn a_set_leg_bit_keeps_a_line_off_that_leg_only() {
        assert!(line_allowed_on_leg(0x7, 1));
        assert!(!line_allowed_on_leg(0x7 | 1 << 10, 1));
        assert!(line_allowed_on_leg(0x7 | 1 << 10, 2));
        assert!(line_allowed_on_leg(u32::MAX, -1));
    }
}
