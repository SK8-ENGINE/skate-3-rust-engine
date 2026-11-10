//! The challenge end dialog (FE screen 4) between the runs and the menu stage, and its Mission / HUDComponents natives
//! (research b920, b923, b925). Rules: skate-core `challenges::end_dialog`.
//! Details: docs/hails-additions/42-challenges.md#change-the-end-dialog-challengesend_dialogrs-research-b920-b923
use crate::apt_vm::{Value, Vm};
use std::collections::BTreeMap;
use bevy::prelude::Resource;
use skate_core::challenges::end_dialog::{default_texts, Action, EndDialog, Movie, NONE};

#[derive(Resource, Default)]
pub(crate) struct EndDialogScreen {
    pub dialog: EndDialog,
    /// The run asked for the dialog and it shows (or waits for the stage).
    pub open: bool,
    /// The running challenge's title text id (Mission.GetTitle).
    pub title: Option<String>,
    /// The option the player picked, for the run to take.
    pub choice: Option<Action>,
    /// The stage has both small dialog movies; without them the run continues at once.
    pub available: bool,
    /// The results table (a race's RACE_TIMES, skate-core `challenges::results`); None for families without one.
    pub results: Option<skate_core::challenges::results::ResultsView>,
}

impl EndDialogScreen {
    /// A failure shows the failed dialog; a pass the table with two or more result rows, else the small one (b920).
    pub fn movie(&self) -> Movie {
        self.dialog.movie(self.results.as_ref().map_or(0, |r| r.rows.len()), false)
    }

    /// A face button on the open dialog: the slot's action, or None for an empty slot.
    pub fn press(&mut self, slot: usize) -> Option<Action> {
        let action = self.dialog.press(slot);
        if action == Action::Nothing {
            return None;
        }
        self.choice = Some(action);
        self.open = false;
        self.dialog.close(action == Action::ChangeDifficulty);
        Some(action)
    }
}

/// The dialog's natives while it shows; None for anything else.
#[derive(Clone, Default)]
pub struct EndDialogHost {
    pub dialog: EndDialog,
    pub title: Option<String>,
    pub results: Option<skate_core::challenges::results::ResultsView>,
}

impl EndDialogHost {
    /// Text natives answer the localised text as a literal (`#` + text): the movies show it as is (laptop run
    /// 2026-11-07 showed `#` + id as the raw id).
    pub fn call(&self, vm: &mut Vm, language: &BTreeMap<String, String>, object: &str, method: &str, args: &[Value])
        -> Result<Option<Value>, String> {
        if method.starts_with("Results_") {
            bevy::log::info!("END_DIALOG native {object}.{method}({args:?})");
        }
        // The table movie reads these class statics right after this call, while it loads (laptop race-results3b:
        // the natives run before the dialog shows).
        if method == "Results_GetNumPlayers" {
            results_enums(vm);
        }
        let literal = |id: &str| Value::Text(format!("#{}", crate::fe_language::template(language, id)));
        Ok(Some(match (object, method) {
            ("Mission", "GetNumEndMissionOptions") => Value::Number(self.dialog.count() as f64),
            ("Mission", "GetEndMissionOptionText") => {
                let slot = match args.first() {
                    Some(Value::Number(n)) if *n >= 0.0 => *n as usize,
                    _ => usize::MAX,
                };
                let option = self.dialog.option(slot);
                let text = if option == NONE { String::new() } else { default_texts().get(option as usize).cloned().unwrap_or_default() };
                literal(&text)
            }
            ("Mission", "GetWinState") => Value::Bool(self.dialog.passed),
            ("Mission", "Results_GetKilledIt") => Value::Bool(self.dialog.killed),
            ("Mission", "GetTitle") => literal(self.title.as_deref().unwrap_or("ID_FREESKATE_LOBBY")),
            // The results table (8265E8C0 / 8265E950 / 8265F598, research race-results-rows.md).
            ("HUDComponents", "Results_GetNumPlayers") => Value::Number(self.results.as_ref().map_or(0, |r| r.rows.len()) as f64),
            ("HUDComponents", "Results_GetLayoutType") => Value::Number(self.results.as_ref().map_or(0, |r| r.layout) as f64),
            ("HUDComponents", "Results_GetSelfRank") => Value::Number(self.results.as_ref().map_or(0, |r| r.self_rank()) as f64),
            ("HUDComponents", "Results_GetThrowdownType") | ("HUDComponents", "Results_GetLeaderboardType") => Value::Number(0.0),
            ("HUDComponents", "Results_GetLeaderboardColTitle") => {
                let c = match args.first() {
                    Some(Value::Number(n)) if *n >= 0.0 => *n as usize,
                    _ => usize::MAX,
                };
                literal(self.results.as_ref().and_then(|r| r.columns.get(c)).map_or("", String::as_str))
            }
            ("HUDComponents", "Results_GetLeaderboardDataBlock") => {
                use skate_core::challenges::results::Cell;
                let cells = self.results.as_ref().map(|r| r.data_block()).unwrap_or_default();
                // The movie reads one row even when there are none (b925: a placeholder).
                let cells = if cells.is_empty() { vec![Cell::Number(0.0), Cell::Text(String::new()), Cell::Number(0.0), Cell::Text(String::new()),
                    Cell::Text(String::new()), Cell::Bool(false), Cell::Number(0.0), Cell::Text(String::new())] } else { cells };
                let values = cells.into_iter().map(|c| match c {
                    Cell::Number(n) => Value::Number(n as f64),
                    Cell::Bool(b) => Value::Bool(b),
                    Cell::Text(t) if t.starts_with("ID_") => literal(&t),
                    // Already a literal (the player's `#name`): as is, else the movie shows the `#`.
                    Cell::Text(t) if t.starts_with('#') => Value::Text(t),
                    Cell::Text(t) => Value::Text(format!("#{t}")),
                });
                crate::hud_challenge::array(vm, values)?
            }
            _ => return Ok(None),
        }))
    }
}

/// The results table's class statics, which retail's native code sets and the movie only reads (828534B0 registers
/// the eResultsLBType values; research b925 / race-results-rows.md): `STRUCT_ITEMS` (8 data block items per row) and
/// `eResultsLBType_<name>` (0 PlaceNameInt .. 8, 15 PlaceNameStringTime).
pub fn results_enums(vm: &mut Vm) {
    const LAYOUTS: [&str; 9] = ["PlaceNameInt", "BestTrickFloat", "BestTrickInt", "DeathRace", "BestTrickNonElimFloat",
        "BestTrickNonElimInt", "PlaceNameScoreTime", "PlaceNameString", "TeamChallengeResults"];
    let class = match vm.get(vm.global, "endchallengeoptions") {
        Value::Object(class) => class,
        other => {
            bevy::log::info!("END_DIALOG results enums NOT set: _global.endchallengeoptions is {other:?}");
            return;
        }
    };
    let mut values: Vec<(String, f64)> = vec![("STRUCT_ITEMS".into(), 8.0), ("eResultsLBType_PlaceNameStringTime".into(), 15.0)];
    values.extend(LAYOUTS.iter().enumerate().map(|(i, n)| (format!("eResultsLBType_{n}"), i as f64)));
    for (key, n) in values {
        if let Err(e) = vm.set(class, key.clone(), Value::Number(n)) {
            bevy::log::warn!("END_DIALOG endchallengeoptions.{key}: {e}");
        }
    }
    bevy::log::info!("END_DIALOG results enums set on endchallengeoptions (STRUCT_ITEMS 8, DeathRace 3)");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natives_follow_the_slots() {
        let host = EndDialogHost { dialog: EndDialog { passed: true, ..Default::default() }, title: None, results: None };
        let mut vm = Vm::new();
        let language = BTreeMap::from([("ID_COMMON_RETRY".to_string(), "Retry".to_string())]);
        let call = |vm: &mut Vm, m: &str, args: &[Value]| host.call(vm, &language, "Mission", m, args).unwrap().unwrap();
        assert_eq!(call(&mut vm, "GetNumEndMissionOptions", &[]), Value::Number(2.0));
        assert_eq!(call(&mut vm, "GetEndMissionOptionText", &[Value::Number(1.0)]), Value::Text("#Retry".into()), "the localised text as a literal");
        assert_eq!(call(&mut vm, "GetWinState", &[]), Value::Bool(true));
        assert_eq!(host.call(&mut vm, &language, "Progression", "GetTitle", &[]).unwrap(), None);
    }

    #[test]
    fn a_press_closes_and_resets_unless_settings() {
        let mut screen = EndDialogScreen { open: true, dialog: EndDialog { passed: true, ..Default::default() }, ..Default::default() };
        assert_eq!(screen.press(2), None, "empty slot");
        assert!(screen.open);
        assert_eq!(screen.press(0), Some(Action::Continue));
        assert!(!screen.open);
        assert_eq!(screen.choice, Some(Action::Continue));
    }
}
