//! The sign-up poster's natives (FE screen 0x1B page 0, movie state 20; research b932 §2). Rules: skate-core
//! `challenges::poster`. Details: docs/hails-additions/42-challenges.md#change-the-sign-up-poster-challengesposterrs
use crate::apt_vm::{Value, Vm};
use bevy::prelude::Resource;
use crate::fe_language::{format_string, template, Arg, NumberFormat};
use skate_core::challenges::poster::{Outcome, Poster, Title};
use std::collections::BTreeMap;

/// The poster between the sign-up and the menu stage: the sign-up opens it, the stage shows it and takes A / B, the
/// sign-up takes the outcome.
#[derive(Resource, Default)]
pub(crate) struct PosterScreen {
    pub poster: Option<Poster>,
    pub outcome: Option<Outcome>,
    /// The stage has the poster movie and the screens switch is on; without it the sign-up starts at once.
    pub available: bool,
}

impl PosterScreen {
    /// A face button on the shown poster (2 = A, 3 = B); closes it on Start / Cancel.
    pub fn press(&mut self, key: u8) -> Option<Outcome> {
        let outcome = self.poster.as_mut()?.press(key);
        if outcome == Outcome::Ignored {
            return None;
        }
        self.poster = None;
        self.outcome = Some(outcome.clone());
        Some(outcome)
    }
}

/// The open poster; Mission.* natives are answered only by name, so the end dialog's stay its own.
#[derive(Clone, Default)]
pub struct PosterHost {
    pub poster: Poster,
}

/// The localised text as a literal (`#` + text): the movies show text natives as is (laptop run 2026-11-07 showed `#`
/// + id as the raw id).
fn text(id: Option<&str>, language: &BTreeMap<String, String>) -> Value {
    Value::Text(id.map(|t| format!("#{}", template(language, t))).unwrap_or_default())
}

/// A title localised with its argument, printed with "%d" / "%i" first (825C2018 / 8264D130; the substitution itself is
/// inferred, b947), or the id for the movie to resolve.
fn title(t: Option<&Title>, language: &BTreeMap<String, String>, numbers: &NumberFormat) -> Value {
    match t {
        Some(Title { id, arg: Some(n) }) => Value::Text(format!("#{}", format_string(numbers, template(language, id), &[Arg::Text(n.to_string())]))),
        Some(Title { id, arg: None }) => text(Some(id), language),
        None => Value::Text(String::new()),
    }
}

fn number(args: &[Value], i: usize) -> Option<f64> {
    match args.get(i) {
        Some(Value::Number(n)) => Some(*n),
        _ => None,
    }
}

fn flag(args: &[Value], i: usize) -> bool {
    match args.get(i) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => *n != 0.0,
        _ => false,
    }
}

impl PosterHost {
    pub fn call(&mut self, vm: &mut Vm, language: &BTreeMap<String, String>, numbers: &NumberFormat, object: &str, method: &str,
        args: &[Value]) -> Result<Option<Value>, String> {
        let view = &self.poster.view;
        let objectives = |killed: bool| if killed { &view.objectives_killed } else { &view.objectives_owned };
        Ok(Some(match (object, method) {
            ("SignUpPoster", "GetSignUpTitle") | ("HUDComponents", "SignUp_GetChallengeTitle") => text(view.title_id.as_deref(), language),
            ("SignUpPoster", "GetSignUpDescription") => title(view.description.as_ref(), language, numbers),
            ("SignUpPoster", "CanSignUpToChallenge") => Value::Bool(view.can_sign_up),
            ("SignUpPoster", "GetCICMenuOptions") => {
                let items = self.poster.option_texts().into_iter().map(|t| if t.starts_with('#') { Value::Text(t) } else { text(Some(&t), language) }).collect();
                Value::Object(vm.array(items)?)
            }
            ("SignUpPoster", "SetCICMenuOptionIndex") => {
                self.poster.set_index(number(args, 0).filter(|n| *n >= 0.0).unwrap_or(0.0) as usize);
                Value::Undefined
            }
            ("SignUpPoster", "GetCICMenuOptionIndex") => Value::Number(self.poster.index() as f64),
            ("SignUpPoster", "IsCompetitorChallenge") => Value::Bool(false),
            ("SignUpPoster", "GetCompetitors") => Value::Object(vm.array(Vec::new())?),
            // Arg 0 (isInMap) is false on this screen.
            ("Mission", "IsDescriptionEnabled") => Value::Bool(view.description.as_ref().is_some_and(|d| !d.id.is_empty())),
            ("Mission", "IsObjectivesEnabled") => Value::Bool(view.objectives_enabled),
            ("Mission", "IsChallengeKillable") => Value::Bool(view.killable),
            ("Mission", "IsChallengeOwned") => Value::Bool(view.owned),
            ("Mission", "IsChallengeKilled") => Value::Bool(view.killed),
            ("Mission", "GetBoardSalesReward") => Value::Number(if flag(args, 1) { view.rewards.1 } else { view.rewards.0 } as f64),
            ("Mission", "GetCoopBoardSaleBonusReward") => Value::Number(view.rewards.2 as f64),
            ("Mission", "AreRewardsShownForChallenge") => Value::Bool(view.rewards_shown),
            ("Mission", "ShowCareerTeammates") => Value::Bool(view.teammates),
            ("Mission", "IsCompetitorsEnabled") => Value::Bool(false),
            ("Mission", "HasUnlockedEnoughTeammatesForChallenge") => Value::Bool(view.teammates),
            ("HUDComponents", "SignUp_GetNumObjectives") => Value::Number(objectives(flag(args, 0)).len() as f64),
            ("HUDComponents", "SignUp_GetObjectiveTitle") => {
                let i = number(args, 0).filter(|n| *n >= 0.0).map_or(usize::MAX, |n| n as usize);
                title(objectives(flag(args, 1)).get(i), language, numbers)
            }
            _ => return Ok(None),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skate_core::challenges::poster::PosterView;

    #[test]
    fn natives_answer_from_the_view() {
        let view = PosterView {
            challenge_id: "photo_dwtn_01".into(),
            title_id: Some("ID_PHOTO_DWTN_01".into()),
            can_sign_up: true,
            coop: true,
            rewards: (100, 250, 50),
            objectives_owned: vec![Title { id: "ID_OBJ_A".into(), arg: None }, Title { id: "ID_OBJECTIVE_BEAT_SCORE".into(), arg: Some(1000) }],
            ..PosterView::default()
        };
        let mut host = PosterHost { poster: Poster::new(view) };
        let mut vm = Vm::new();
        let language = BTreeMap::from([("ID_OBJECTIVE_BEAT_SCORE".to_string(), "Beat {0:i}".to_string())]);
        let numbers = NumberFormat::default();
        let mut call = |o: &str, m: &str, args: &[Value]| host.call(&mut vm, &language, &numbers, o, m, args).unwrap().unwrap();
        assert_eq!(call("SignUpPoster", "GetSignUpTitle", &[]), Value::Text("#ID_PHOTO_DWTN_01".into()), "an id without text stays the id");
        assert_eq!(call("Mission", "GetBoardSalesReward", &[Value::Bool(false), Value::Bool(true)]), Value::Number(250.0));
        assert_eq!(call("HUDComponents", "SignUp_GetNumObjectives", &[Value::Bool(false)]), Value::Number(2.0));
        assert_eq!(call("HUDComponents", "SignUp_GetObjectiveTitle", &[Value::Number(1.0), Value::Bool(false)]), Value::Text("#Beat 1000".into()));
        assert_eq!(call("HUDComponents", "SignUp_GetObjectiveTitle", &[Value::Number(0.0), Value::Bool(false)]), Value::Text("#ID_OBJ_A".into()));
        call("SignUpPoster", "SetCICMenuOptionIndex", &[Value::Number(1.0)]);
        assert_eq!(call("SignUpPoster", "GetCICMenuOptionIndex", &[]), Value::Number(2.0), "entry 1 without teammates is co-op");
        assert!(host.call(&mut vm, &language, &numbers, "Mission", "GetNumEndMissionOptions", &[]).unwrap().is_none(), "the end dialog's natives stay its own");
    }
}
