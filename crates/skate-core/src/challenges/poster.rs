//! The sign-up poster (FE screen 0x1B page 0, template 23 SignUp_Poster; research b932, b935, b939): what it lists
//! and what A / B do on it. The lot list (page 1) is the same screen; a chained start (picked from the lot list) never
//! shows the single poster. Details: docs/hails-additions/42-challenges.md#change-the-sign-up-poster-challengesposterrs

/// Movie states (`challenge_signup`): the single poster and the lot list (b935).
pub const STATE_SIGN_UP: i32 = 20;
pub const STATE_LOT_LIST: i32 = 10;

/// The answer of GetCICMenuOptions when the team gate fails (82222090).
pub const NO_OPTION: &str = "#NO OPTION";

/// One CIC menu entry (825EE890 / 825EEA68 / 825EEB50); its index is fixed: 0 start, 1 teammates, 2 co-op.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PosterOption {
    Start,
    PickTeammates,
    SignUpCoop,
}

impl PosterOption {
    pub fn text_id(self) -> &'static str {
        match self {
            Self::Start => "ID_ONLINE_HUB_START_CHALLENGE",
            Self::PickTeammates => "ID_COMMON_PICK_TEAMMATES",
            Self::SignUpCoop => "ID_COMMON_SIGNUP_COOP",
        }
    }
}

/// A text id with its one format argument (the description's "%d", an objective's score).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Title {
    pub id: String,
    pub arg: Option<i64>,
}

/// The own the spot objective title (8264D130, type 9): beat the group's point requirement.
pub const BEAT_SCORE: &str = "ID_OBJECTIVE_BEAT_SCORE";

/// What the poster shows for one challenge; filled from the challenge export and the player's records.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PosterView {
    pub challenge_id: String,
    pub title_id: Option<String>,
    /// The Description field with its Int8 argument (825C2018; research b947).
    pub description: Option<Title>,
    /// IsObjectivesEnabled (Bool 0x858D73CD6085C8EC, default false).
    pub objectives_enabled: bool,
    pub killable: bool,
    pub owned: bool,
    pub killed: bool,
    /// Board sales for owning / killing it and the co-op bonus.
    pub rewards: (i32, i32, i32),
    pub rewards_shown: bool,
    pub objectives_owned: Vec<Title>,
    pub objectives_killed: Vec<Title>,
    /// The team gate (825EE7A8); false lists no option and blocks the start.
    pub can_sign_up: bool,
    /// T: career teammates usable for this challenge.
    pub teammates: bool,
    /// C: the challenge can be played co-op.
    pub coop: bool,
}

/// What a key press asks the caller to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Close the poster and start the challenge (825EE188 -> 82D52B60).
    Start(String),
    /// Close the poster and cancel the pending sign-up (82D52C98).
    Cancel,
    /// Nothing (a locked poster, a key it ignores, or an option we do not offer yet).
    Ignored,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Poster {
    pub view: PosterView,
    index: usize,
    /// Set by the first accepted A or B (S+0x48): later keys are ignored until it closes.
    locked: bool,
}

impl Poster {
    pub fn new(view: PosterView) -> Self {
        Self { view, index: 0, locked: false }
    }

    /// The CIC menu entries (825C1E88): start first, then teammates or co-op, co-op third when both.
    pub fn options(&self) -> Vec<PosterOption> {
        if !self.view.can_sign_up {
            return Vec::new();
        }
        let mut out = vec![PosterOption::Start];
        if self.view.teammates {
            out.push(PosterOption::PickTeammates);
        }
        if self.view.coop {
            out.push(PosterOption::SignUpCoop);
        }
        out
    }

    /// GetCICMenuOptions' text ids; `#NO OPTION` alone when nothing is offered.
    pub fn option_texts(&self) -> Vec<String> {
        let options = self.options();
        if options.is_empty() {
            return vec![NO_OPTION.into()];
        }
        options.into_iter().map(|o| o.text_id().into()).collect()
    }

    /// SetCICMenuOptionIndex (825C1E30): entry 1 without teammates is the co-op entry, so it stores 2.
    pub fn set_index(&mut self, i: usize) {
        self.index = if i == 1 && !self.view.teammates { 2 } else { i };
    }

    pub fn index(&self) -> usize {
        self.index
    }

    /// 825ED410 for the single poster: 2 = A, 3 = B.
    pub fn press(&mut self, key: u8) -> Outcome {
        if self.locked {
            return Outcome::Ignored;
        }
        match key {
            2 if self.index == 0 && self.view.can_sign_up => {
                self.locked = true;
                Outcome::Start(self.view.challenge_id.clone())
            }
            3 => {
                self.locked = true;
                Outcome::Cancel
            }
            _ => Outcome::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn poster(teammates: bool, coop: bool) -> Poster {
        Poster::new(PosterView { challenge_id: "photo_dwtn_01".into(), can_sign_up: true, teammates, coop, ..PosterView::default() })
    }

    #[test]
    fn options_follow_the_team_and_coop_flags() {
        assert_eq!(poster(false, false).option_texts(), ["ID_ONLINE_HUB_START_CHALLENGE"]);
        assert_eq!(poster(true, true).options(), [PosterOption::Start, PosterOption::PickTeammates, PosterOption::SignUpCoop]);
        assert_eq!(poster(false, true).options(), [PosterOption::Start, PosterOption::SignUpCoop]);
        let mut closed = poster(true, true);
        closed.view.can_sign_up = false;
        assert_eq!(closed.option_texts(), [NO_OPTION]);
        assert_eq!(closed.press(2), Outcome::Ignored, "the team gate blocks the start");
    }

    #[test]
    fn entry_one_without_teammates_is_coop() {
        let mut p = poster(false, true);
        p.set_index(1);
        assert_eq!(p.index(), 2);
        assert_eq!(p.press(2), Outcome::Ignored, "co-op is not offered locally");
        let mut p = poster(true, true);
        p.set_index(1);
        assert_eq!(p.index(), 1);
    }

    #[test]
    fn a_starts_b_cancels_and_the_first_press_locks() {
        let mut p = poster(false, false);
        assert_eq!(p.press(5), Outcome::Ignored);
        assert_eq!(p.press(2), Outcome::Start("photo_dwtn_01".into()));
        assert_eq!(p.press(3), Outcome::Ignored, "locked after the accept");
        let mut p = poster(false, false);
        assert_eq!(p.press(3), Outcome::Cancel);
        assert_eq!(p.press(2), Outcome::Ignored);
    }
}
