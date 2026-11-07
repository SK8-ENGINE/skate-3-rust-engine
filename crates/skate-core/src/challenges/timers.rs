//! Retail challenge timers (`ChallengeTimer`, research b832): a countdown holds trunc(seconds * 60) ticks. Each update
//! first expires a countdown at zero or below, else takes one tick off (82D57960, research b958), so an N-tick countdown
//! expires on update N + 1. Ticks run at a fixed 60 Hz, independent of frame rate.

pub const TICKS_PER_SECOND: f32 = 60.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TimerId(pub u32);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Timers {
    next: u32,
    countdowns: Vec<(TimerId, i32)>,
}

impl Timers {
    pub fn ticks(seconds: f32) -> i32 {
        (seconds * TICKS_PER_SECOND) as i32
    }

    pub fn add_countdown(&mut self, seconds: f32) -> TimerId {
        self.next += 1;
        let id = TimerId(self.next);
        self.countdowns.push((id, Self::ticks(seconds)));
        id
    }

    pub fn remove(&mut self, id: TimerId) {
        self.countdowns.retain(|(t, _)| *t != id);
    }

    pub fn remaining(&self, id: TimerId) -> Option<i32> {
        self.countdowns.iter().find(|(t, _)| *t == id).map(|(_, left)| *left)
    }

    /// One 60 Hz tick: returns the timers that expired, in the order they were added, and removes them.
    pub fn tick(&mut self) -> Vec<TimerId> {
        let mut expired = Vec::new();
        self.countdowns.retain_mut(|(id, left)| {
            if *left <= 0 {
                expired.push(*id);
                return false;
            }
            *left -= 1;
            true
        });
        expired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_half_second_countdown_expires_on_its_thirty_first_tick() {
        let mut timers = Timers::default();
        let id = timers.add_countdown(0.5);
        for _ in 0..30 {
            assert!(timers.tick().is_empty());
        }
        assert_eq!(timers.remaining(id), Some(0), "at zero it still waits one update (82D57960)");
        assert_eq!(timers.tick(), vec![id]);
        assert_eq!(timers.remaining(id), None);
        let zero = timers.add_countdown(0.0);
        assert_eq!(timers.tick(), vec![zero], "a zero-tick countdown expires on the next update");
    }

    #[test]
    fn ticks_truncate() {
        assert_eq!(Timers::ticks(0.999 / 60.0), 0);
        assert_eq!(Timers::ticks(1.5), 90);
    }
}

/// Below this many seconds the HUD timer draws red, flashes and ticks (retail 0x821963E4, research b845).
pub const LOW_TIME_SECONDS: f32 = 10.0;

/// The HUD timer element's text and colour flag for `ticks` left (retail 82663940, research b845): the time rounded
/// up to whole seconds, at least 0, as `M:SS` from a minute up and bare seconds below (the digit padding below a
/// minute is not proven); the flag is 0 (red) under [`LOW_TIME_SECONDS`].
pub fn hud_text(ticks: i32) -> (String, i32) {
    let seconds = ticks as f32 / TICKS_PER_SECOND;
    let whole = seconds.max(0.0).ceil() as i32;
    let text = if whole >= 60 { format!("{}:{:02}", whole / 60, whole % 60) } else { whole.to_string() };
    (text, i32::from(seconds >= LOW_TIME_SECONDS))
}

#[cfg(test)]
mod hud_text_tests {
    use super::*;

    #[test]
    fn the_hud_time_rounds_up_and_turns_red_under_ten_seconds() {
        assert_eq!(hud_text(600), ("10".into(), 1));
        assert_eq!(hud_text(599), ("10".into(), 0));
        assert_eq!(hud_text(61), ("2".into(), 0));
        assert_eq!(hud_text(-5), ("0".into(), 0));
        assert_eq!(hud_text(90 * 60), ("1:30".into(), 1));
    }
}
