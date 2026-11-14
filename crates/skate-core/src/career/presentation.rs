//! The reward presentation after a challenge: summary, milestone cards, teammate cards, then the deferred events.
//! Order and rules: docs/hails-additions/43-career.md#change-the-reward-presentation-careerpresentationrs (research b893, b899).

use super::Pending;

/// The screens the presentation opens (retail FE ids 64 / 65 / 66).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Summary,
    Milestone,
    Teammate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Out {
    /// Presentation start message (0x7949DAC1 with 1).
    Started,
    Open(Screen, u32),
    Close(Screen),
    /// The summary's other rows are spent (the summary screen's exit).
    ClearOthers,
    /// The milestone and teammate counts are spent (the end of the card chain).
    ClearCards,
    /// Play the deferred presentation events, oldest first.
    PlayDeferred,
    /// End messages (0x7949DAC1 with 0, 0x478AA499).
    Done,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    /// A pass was paid; waits for the challenge runner to go idle.
    Waiting,
    Summary,
    Milestone(u32),
    Teammate(u32),
}

/// The reward summary's per result values (summary +0x3C cash owned, +0x40 cash killed, +0x48 / +0x4C board sales
/// owned / killed; +0x44 online sales stay 0 offline).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub cash_owned: u64,
    pub cash_killed: u64,
    pub sales_owned: u64,
    pub sales_killed: u64,
}

impl Totals {
    pub fn add(&mut self, killed: bool, money: u64, sales: u64) {
        if killed {
            self.cash_killed += money;
            self.sales_killed += sales;
        } else {
            self.cash_owned += money;
            self.sales_owned += sales;
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Presentation {
    pub phase: Phase,
    /// Cards left for the next arrival (set when the chain could not run, 827CCFA0).
    pub cards_on_arrival: bool,
}

impl Presentation {
    /// A challenge result: offline passes wait for the summary; online passes and failures show nothing.
    pub fn result(&mut self, passed: bool, online: bool) {
        if passed && !online {
            self.phase = Phase::Waiting;
        }
    }

    /// Per frame while nothing runs: opens the summary, or with nothing to sum up releases the deferred events and
    /// keeps the cards for later.
    pub fn runner_idle(&mut self, has_summary: bool) -> Vec<Out> {
        if self.phase != Phase::Waiting {
            return Vec::new();
        }
        if has_summary {
            self.phase = Phase::Summary;
            return vec![Out::Started, Out::Open(Screen::Summary, 0)];
        }
        self.phase = Phase::Idle;
        vec![Out::PlayDeferred, Out::Done]
    }

    /// The accept press on the open screen.
    pub fn accept(&mut self, cards: Pending) -> Vec<Out> {
        let mut out = Vec::new();
        match self.phase {
            Phase::Summary => {
                out.extend([Out::Close(Screen::Summary), Out::ClearOthers]);
                if cards.milestones > 0 || cards.teammates > 0 {
                    self.cards(cards, &mut out);
                } else {
                    self.release(&mut out);
                }
            }
            Phase::Milestone(i) => {
                out.push(Out::Close(Screen::Milestone));
                if i + 1 < cards.milestones {
                    self.phase = Phase::Milestone(i + 1);
                    out.push(Out::Open(Screen::Milestone, i + 1));
                } else if cards.teammates > 0 {
                    self.phase = Phase::Teammate(0);
                    out.push(Out::Open(Screen::Teammate, 0));
                } else {
                    self.release(&mut out);
                }
            }
            Phase::Teammate(i) => {
                out.push(Out::Close(Screen::Teammate));
                if i + 1 < cards.teammates {
                    self.phase = Phase::Teammate(i + 1);
                    out.push(Out::Open(Screen::Teammate, i + 1));
                } else {
                    self.release(&mut out);
                }
            }
            Phase::Idle | Phase::Waiting => {}
        }
        out
    }

    /// The player left with cards still to show (the 827CCFA0 role; its callers are not known yet).
    pub fn left(&mut self, with_cards: bool) {
        self.cards_on_arrival = with_cards;
    }

    /// The next location message: runs the card chain that could not run before (827CF960).
    pub fn arrived(&mut self, cards: Pending) -> Vec<Out> {
        let mut out = Vec::new();
        if std::mem::take(&mut self.cards_on_arrival) && self.phase == Phase::Idle
            && (cards.milestones > 0 || cards.teammates > 0) {
            self.cards(cards, &mut out);
        }
        out
    }

    fn cards(&mut self, cards: Pending, out: &mut Vec<Out>) {
        out.push(Out::Started);
        if cards.milestones > 0 {
            self.phase = Phase::Milestone(0);
            out.push(Out::Open(Screen::Milestone, 0));
        } else {
            self.phase = Phase::Teammate(0);
            out.push(Out::Open(Screen::Teammate, 0));
        }
    }

    fn release(&mut self, out: &mut Vec<Out>) {
        self.phase = Phase::Idle;
        out.extend([Out::ClearCards, Out::PlayDeferred, Out::Done]);
    }
}

/// One reward row or card: title text id, image, description text id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Card {
    pub title: String,
    pub image: String,
    pub description: String,
}

/// A reward tab (progression_reward_tabs) and the unlock kinds it lists.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tab {
    pub id: String,
    pub title: String,
    pub list: bool,
    pub kinds: Vec<u32>,
}

/// What the summary and milestone screens show (FERewardSummary, research b873 §4, b899 §1).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SummaryView {
    /// Tabs holding at least one of this result's rewards, in tab order, with their rows.
    pub tabs: Vec<(Tab, Vec<Card>)>,
    /// Milestone cards (the summary's big list).
    pub milestones: Vec<Card>,
    pub totals: Totals,
    pub total_cash: u64,
    pub total_sales: u64,
    pub prev_milestone: f64,
    pub next_milestone: f64,
}

impl SummaryView {
    /// `tabs` sorted by their order; `kind` gives a reward id's unlock kind, `card` its texts.
    pub fn build(tabs: &[Tab], others: &[String], big: &[String], kind: impl Fn(&str) -> Option<u32>,
                 card: impl Fn(&str) -> Card) -> Self {
        let tabs = tabs
            .iter()
            .filter_map(|t| {
                let rows: Vec<Card> = others.iter()
                    .filter(|r| kind(r).is_some_and(|k| t.kinds.contains(&k)))
                    .map(|r| card(r))
                    .collect();
                (!rows.is_empty()).then(|| (t.clone(), rows))
            })
            .collect();
        Self { tabs, milestones: big.iter().map(|r| card(r)).collect(), ..Default::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cards(milestones: u32, teammates: u32) -> Pending {
        Pending { milestones, teammates }
    }

    #[test]
    fn a_pass_shows_summary_then_milestones_then_teammates_then_releases() {
        let mut p = Presentation::default();
        let c = cards(2, 1);
        p.result(true, false);
        assert!(p.accept(c).is_empty(), "nothing open while the challenge still runs");
        assert_eq!(p.runner_idle(true), [Out::Started, Out::Open(Screen::Summary, 0)]);
        assert_eq!(p.accept(c), [Out::Close(Screen::Summary), Out::ClearOthers, Out::Started,
            Out::Open(Screen::Milestone, 0)]);
        assert_eq!(p.accept(c), [Out::Close(Screen::Milestone), Out::Open(Screen::Milestone, 1)]);
        assert_eq!(p.accept(c), [Out::Close(Screen::Milestone), Out::Open(Screen::Teammate, 0)]);
        assert_eq!(p.accept(c), [Out::Close(Screen::Teammate), Out::ClearCards, Out::PlayDeferred, Out::Done]);
        assert_eq!(p.phase, Phase::Idle);
    }

    #[test]
    fn no_summary_content_releases_the_deferred_and_keeps_the_cards() {
        let mut p = Presentation::default();
        p.result(true, false);
        let out = p.runner_idle(false);
        assert_eq!(out, [Out::PlayDeferred, Out::Done]);
        assert!(!out.contains(&Out::ClearCards));
    }

    #[test]
    fn a_summary_without_cards_releases_on_accept() {
        let mut p = Presentation::default();
        p.result(true, false);
        p.runner_idle(true);
        assert_eq!(p.accept(cards(0, 0)), [Out::Close(Screen::Summary), Out::ClearOthers, Out::ClearCards,
            Out::PlayDeferred, Out::Done]);
    }

    #[test]
    fn failures_and_online_passes_show_nothing() {
        let mut p = Presentation::default();
        p.result(false, false);
        assert!(p.runner_idle(true).is_empty());
        p.result(true, true);
        assert!(p.runner_idle(true).is_empty());
    }

    #[test]
    fn the_summary_lists_tabs_with_rewards_only() {
        let tab = |id: &str, kinds: &[u32]| Tab { id: id.into(), title: format!("ID_{id}"), list: true, kinds: kinds.to_vec() };
        let tabs = [tab("characters", &[1]), tab("gear", &[6, 5]), tab("locations", &[0])];
        let kind = |r: &str| match r { "shirt" => Some(5), "shoe" => Some(6), "park" => Some(0), _ => None };
        let card = |r: &str| Card { title: format!("T_{r}"), ..Default::default() };
        let v = SummaryView::build(&tabs, &["shoe".into(), "park".into(), "shirt".into(), "unknown".into()],
            &["milestone_01".into()], kind, card);
        let names: Vec<_> = v.tabs.iter().map(|(t, rows)| (t.id.as_str(), rows.len())).collect();
        assert_eq!(names, [("gear", 2), ("locations", 1)]);
        assert_eq!(v.tabs[0].1[0].title, "T_shoe");
        assert_eq!(v.milestones[0].title, "T_milestone_01");
    }

    #[test]
    fn cards_left_behind_show_on_the_next_arrival() {
        let mut p = Presentation::default();
        assert!(p.arrived(cards(1, 0)).is_empty(), "no cards were left");
        p.left(true);
        assert_eq!(p.arrived(cards(0, 1)), [Out::Started, Out::Open(Screen::Teammate, 0)]);
        assert!(!p.cards_on_arrival);
        assert_eq!(p.accept(cards(0, 1)), [Out::Close(Screen::Teammate), Out::ClearCards, Out::PlayDeferred,
            Out::Done]);
    }
}
