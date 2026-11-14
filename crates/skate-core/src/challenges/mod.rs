//! Retail challenges, engine independent: the result tiers, the challenge timers and the per-family state
//! machines re-implemented from the retail state graphs (docs/hails-additions/42-challenges.md).

pub mod api;
pub mod callout;
pub mod contest_run;
pub mod distance_meter;
pub mod dpo;
pub mod end_dialog;
pub mod film_run;
pub mod hom_run;
pub mod lots;
pub mod path_filter;
pub mod objectives;
pub mod poster;
pub mod predicate;
pub mod race;
pub mod race_ai;
pub mod race_run;
pub mod results;
pub mod tutorial;
pub mod ots;
pub mod timers;

/// Retail `EActivityCompletionType` (research b831): the value `CompleteChallenge` records.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Completion {
    Abandoned = 0,
    Owned = 1,
    Killed = 2,
}

/// Retail `eChallengeFlow` (research b831): how a run ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Flow {
    Exit = 0,
    Restart = 1,
    Win = 2,
    Lose = 3,
    Kill = 4,
}

/// Retail `eHighScoreTier`: the next target a challenge offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tier {
    Owned,
    Killed,
    PersonalBest,
}

/// A challenge's saved record: the tier only ever moves up (retail 82C1D878, research b828).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Record {
    pub completion: Option<Completion>,
    pub high_score: i32,
}

impl Record {
    pub fn complete(&mut self, completion: Completion) {
        let rank = |c: Option<Completion>| c.map_or(0, |c| c as u8);
        if completion != Completion::Abandoned && rank(Some(completion)) > rank(self.completion) {
            self.completion = Some(completion);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_only_moves_up() {
        let mut record = Record::default();
        record.complete(Completion::Abandoned);
        assert_eq!(record.completion, None);
        record.complete(Completion::Killed);
        record.complete(Completion::Owned);
        assert_eq!(record.completion, Some(Completion::Killed));
    }
}
