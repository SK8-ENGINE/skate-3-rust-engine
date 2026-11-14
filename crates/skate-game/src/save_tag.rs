//! The autosave layer's save tag (the corner indicator while the game writes its save; research b948 / b957 S4,
//! `825DF4F8`, `82522310`, hide after `[82063B08]` = 3.0 s): every background save write holds a [`Writing`] guard; the
//! menu stage shows `_global.Save_EnableTag` while any is in flight and hides it once none is and the tag has been up
//! at least [`SaveTag::min_seconds`]. When saves happen is not changed here (b957 S1-S3 are open).

use std::sync::atomic::{AtomicUsize, Ordering};

static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

/// Held by a save write from before its thread starts until the file is written.
pub(crate) struct Writing(());

impl Writing {
    pub(crate) fn begin() -> Self {
        IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
        Writing(())
    }
}

impl Drop for Writing {
    fn drop(&mut self) {
        IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A save write is in flight.
pub(crate) fn saving() -> bool {
    IN_FLIGHT.load(Ordering::SeqCst) > 0
}

/// The tag's state (retail's listener: open on write start, the 3 s stopwatch, hide when done and past it).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SaveTag {
    pub shown: bool,
    pub since: f32,
    /// The least time the tag stays up (data; retail 3.0 s).
    pub min_seconds: f32,
}

impl Default for SaveTag {
    fn default() -> Self {
        Self { shown: false, since: 0.0, min_seconds: 3.0 }
    }
}

impl SaveTag {
    /// One frame: `Some(true)` = show the tag, `Some(false)` = hide it, None = no change.
    pub(crate) fn step(&mut self, saving: bool, dt: f32) -> Option<bool> {
        if !self.shown {
            if saving {
                (self.shown, self.since) = (true, 0.0);
                return Some(true);
            }
            return None;
        }
        self.since += dt;
        if !saving && self.since >= self.min_seconds {
            self.shown = false;
            return Some(false);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tag_shows_on_a_write_and_stays_up_three_seconds() {
        let mut tag = SaveTag::default();
        assert_eq!(tag.step(false, 0.1), None);
        assert_eq!(tag.step(true, 0.1), Some(true));
        assert_eq!(tag.step(false, 1.0), None, "written fast, but up less than 3 s");
        assert_eq!(tag.step(false, 1.0), None);
        assert_eq!(tag.step(false, 1.1), Some(false));
        assert_eq!(tag.step(true, 0.1), Some(true));
        assert_eq!(tag.step(true, 5.0), None, "a long write keeps it up");
        assert_eq!(tag.step(false, 0.1), Some(false));
    }

    #[test]
    fn a_guard_counts_while_it_lives() {
        let w = Writing::begin();
        assert!(saving());
        drop(w);
    }
}
