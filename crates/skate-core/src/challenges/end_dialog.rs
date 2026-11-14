//! The challenge end dialog (FE screen 4; research b920, b923; own the spot never opens it, b929): four option
//! slots, the movie it shows and what each option does. Details: docs/hails-additions/42-challenges.md#change-the-end-dialog-challengesend_dialogrs-research-b920-b923

/// eEndChallengeOption ids (registrar 82853BA8).
pub const CONTINUE: u32 = 0;
pub const RETRY: u32 = 1;
pub const REMATCH: u32 = 2;
pub const QUIT: u32 = 8;
pub const CONTINUE_RETRY: u32 = 10;
pub const RETRY_QUIT: u32 = 11;
pub const NONE: u32 = 12;
pub const MORE_ACTIVITIES: u32 = 18;
pub const CHANGE_DIFFICULTY: u32 = 19;
pub const HIGHLIGHT_REEL: u32 = 9;
pub const NEXT: u32 = 13;
pub const PREVIOUS: u32 = 14;

/// The slots after a reset (825ECD18).
pub const DEFAULT_SLOTS: [u32; 4] = [CONTINUE_RETRY, RETRY_QUIT, NONE, NONE];

/// Text ids per option id (table 8221EB28), our data; a mod can replace or extend it.
pub fn default_texts() -> Vec<String> {
    ["ID_COMMON_CONTINUE", "ID_COMMON_RETRY", "ID_PARTYPLAY_REMATCH", "ID_MEDIA_VIEW_PHOTO_CAPS", "ID_REPLAY_EDITREPLAY",
     "ID_COMMON_SAVE_REPLAY_CAPS", "ID_ONLINE_SCORES_CAPS", "ID_DEMO_PHOTOCHALLENGE_VIEWPHOTO", "ID_COMMON_QUIT",
     "ID_COMMON_HIGHLIGHTREEL_CAPS", "ID_COMMON_UNKNOWN", "ID_COMMON_UNKNOWN", "", "ID_COMMON_NEXT", "ID_COMMON_PREVIOUS",
     "ID_LOGBOOK_NOT_COMPLETED", "ID_PARTYPLAY_EXIT_GAME", "ID_PARTYPLAY_CHANGE_SETTINGS", "ID_COMMON_MORE_ACTIVITIES",
     "ID_COMMON_CHANGE_DIFFICULTY"].map(String::from).to_vec()
}

/// The movie screen 4's init picks (825F0CE8), offline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Movie {
    /// Template 33 endchallengeoptions.
    Options,
    /// Template 34 endchallenge_small.
    Small,
    /// Template 36 challengefailed_small.
    Failed,
}

/// What an option does (825F13F8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Close; hudcontinue once the outro finished (Continue, Quit, MoreActivities).
    Continue,
    /// hudretry; after a pass, first the rematch confirm popup (217).
    Retry { confirm: bool },
    /// Push the difficulty screen (FE 45); the slots are kept.
    ChangeDifficulty,
    /// Next / Previous: the other results block (825F1768 / 825F17C0), the dialog stays and refills (SetState 6).
    Page,
    /// No effect (an empty slot or an option without an offline action).
    Nothing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndDialog {
    pub slots: [u32; 4],
    pub passed: bool,
    pub killed: bool,
}

impl Default for EndDialog {
    fn default() -> Self {
        Self { slots: DEFAULT_SLOTS, passed: false, killed: false }
    }
}

impl EndDialog {
    /// SetEndChallengeOption (82855548).
    pub fn set(&mut self, slot: usize, option: u32) {
        if let Some(s) = self.slots.get_mut(slot) {
            *s = option;
        }
    }

    /// The option a slot shows: 10 / 11 resolve by the passed flag (825ECF80).
    pub fn option(&self, slot: usize) -> u32 {
        match self.slots.get(slot).copied().unwrap_or(NONE) {
            CONTINUE_RETRY => if self.passed { CONTINUE } else { RETRY },
            RETRY_QUIT => if self.passed { RETRY } else { QUIT },
            other => other,
        }
    }

    /// GetNumEndMissionOptions (825BC6A0): the slots before the first empty one.
    pub fn count(&self) -> usize {
        self.slots.iter().take_while(|s| **s != NONE).count()
    }

    /// The movie: failed, the options table with two or more result rows (or the challenge's force flag), else small.
    pub fn movie(&self, result_rows: usize, force_table: bool) -> Movie {
        if !self.passed {
            Movie::Failed
        } else if result_rows >= 2 || force_table {
            Movie::Options
        } else {
            Movie::Small
        }
    }

    /// The pad key's option: A slot 0, B slot 1, X slot 2, Y slot 3 (825F12E8; no count check, empty slots do nothing).
    pub fn press(&self, slot: usize) -> Action {
        match self.option(slot) {
            CONTINUE | QUIT | MORE_ACTIVITIES => Action::Continue,
            RETRY | REMATCH => Action::Retry { confirm: self.passed },
            CHANGE_DIFFICULTY => Action::ChangeDifficulty,
            NEXT | PREVIOUS => Action::Page,
            _ => Action::Nothing,
        }
    }

    /// Screen 4 close (825F0F98): the slots reset unless the dialog was left for a settings screen.
    pub fn close(&mut self, to_settings: bool) {
        if !to_settings {
            self.slots = DEFAULT_SLOTS;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_slots_resolve_by_result() {
        let mut d = EndDialog { passed: true, ..Default::default() };
        assert_eq!((d.option(0), d.option(1), d.count()), (CONTINUE, RETRY, 2));
        assert_eq!(d.press(0), Action::Continue);
        assert_eq!(d.press(1), Action::Retry { confirm: true });
        assert_eq!(d.press(2), Action::Nothing);
        d.passed = false;
        assert_eq!((d.option(0), d.option(1)), (RETRY, QUIT));
        assert_eq!(d.press(0), Action::Retry { confirm: false });
        assert_eq!(d.press(1), Action::Continue, "Quit ends like Continue");
    }

    #[test]
    fn lot_activity_slots_and_reset() {
        let mut d = EndDialog { passed: false, ..Default::default() };
        d.set(2, MORE_ACTIVITIES);
        d.set(3, CHANGE_DIFFICULTY);
        assert_eq!(d.count(), 4);
        assert_eq!(d.press(3), Action::ChangeDifficulty);
        d.close(true);
        assert_eq!(d.slots[2], MORE_ACTIVITIES, "kept while away at a settings screen");
        d.close(false);
        assert_eq!(d.slots, DEFAULT_SLOTS);
    }

    #[test]
    fn movie_choice() {
        let d = EndDialog { passed: true, ..Default::default() };
        assert_eq!(d.movie(1, false), Movie::Small);
        assert_eq!(d.movie(2, false), Movie::Options);
        assert_eq!(d.movie(0, true), Movie::Options);
        assert_eq!(EndDialog::default().movie(5, true), Movie::Failed);
        assert_eq!(default_texts().len(), 20);
    }
}
