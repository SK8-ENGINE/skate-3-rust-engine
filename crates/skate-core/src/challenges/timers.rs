//! Retail challenge timers (`ChallengeTimer`, research b832): a countdown holds trunc(seconds * 60) ticks, loses one
//! tick per update and expires when it reaches zero or below. Ticks run at a fixed 60 Hz, independent of frame rate.

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
            *left -= 1;
            if *left <= 0 {
                expired.push(*id);
                false
            } else {
                true
            }
        });
        expired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_half_second_countdown_expires_on_its_thirtieth_tick() {
        let mut timers = Timers::default();
        let id = timers.add_countdown(0.5);
        for _ in 0..29 {
            assert!(timers.tick().is_empty());
        }
        assert_eq!(timers.tick(), vec![id]);
        assert_eq!(timers.remaining(id), None);
    }

    #[test]
    fn ticks_truncate() {
        assert_eq!(Timers::ticks(0.999 / 60.0), 0);
        assert_eq!(Timers::ticks(1.5), 90);
    }
}
