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
}

impl EndDialogScreen {
    /// Our families write no result rows (b925): a pass shows the small dialog, a failure the failed one.
    pub fn movie(&self) -> Movie {
        self.dialog.movie(0, false)
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
}

impl EndDialogHost {
    /// Text natives answer the localised text as a literal (`#` + text): the movies show it as is (laptop run
    /// 2026-11-07 showed `#` + id as the raw id).
    pub fn call(&self, vm: &mut Vm, language: &BTreeMap<String, String>, object: &str, method: &str, args: &[Value])
        -> Result<Option<Value>, String> {
        let _ = vm;
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
            ("HUDComponents", "Results_GetNumPlayers") => Value::Number(0.0),
            _ => return Ok(None),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natives_follow_the_slots() {
        let host = EndDialogHost { dialog: EndDialog { passed: true, ..Default::default() }, title: None };
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
