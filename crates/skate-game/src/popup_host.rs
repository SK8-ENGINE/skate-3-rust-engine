//! The popup movies' C++ side: `Popup` natives (825A9F40) and popup focus for menu callbacks (82D09F28).
//! Details: docs/hails-additions/31-retail-menus.md#popup-host

use crate::apt_vm::Value;
use skate_core::fe_popup::PopupRequest;

/// Popups mods raised or took down (`sdk.menus.popup`) and mods that stopped, for the stage to apply.
#[derive(bevy::prelude::Resource, Default)]
pub(crate) struct PopupMods {
    pub requests: Vec<(String, String, Option<PopupRequest>)>,
    pub cleared: Vec<Option<String>>,
    /// Engine popups raised outside a menu screen (the replay's quit warning): requests with their callback id
    /// (`engine:...`), and the choices that came back (callback id, choice index).
    pub engine_requests: Vec<PopupRequest>,
    pub engine_choices: Vec<(String, i32)>,
    /// Whether the retail menu stage is up to show them (else the caller decides without a popup).
    pub stage_ready: bool,
    /// The teleporter prompt's title while it shows (teleporters.rs), drawn by the action overlay movie
    /// (`actionoverlay2`, state 2); `signup_ready`: the stage opened that movie (else the caller draws its own line).
    pub signup_title: Option<String>,
    pub signup_ready: bool,
}

/// Callback ids of mod popups: `mod:<mod id>:<key>`.
pub(crate) fn mod_callback(owner: &str, key: &str) -> String {
    format!("mod:{owner}:{key}")
}

impl PopupMods {
    pub(crate) fn set(&mut self, owner: &str, key: &str, options: Option<skate_mods::menus::PopupOptions>) {
        let request = options.map(|o| PopupRequest {
            title: o.title.unwrap_or_else(|| PopupRequest::default().title),
            description: o.description.unwrap_or_else(|| PopupRequest::default().description),
            options: o.options.unwrap_or_default(),
            kind: if o.kind.as_deref() == Some("small") { skate_core::fe_popup::PopupKind::Small } else { Default::default() },
            priority: o.priority.unwrap_or(0),
            cancelable: o.cancelable,
            callback: mod_callback(owner, key),
            ..Default::default()
        });
        self.requests.push((owner.to_owned(), key.to_owned(), request));
    }
    /// A mod stopped (`Some`) or every mod (`None`): its popups leave without an event.
    pub(crate) fn clear_owner(&mut self, owner: Option<&str>) {
        self.requests.retain(|(o, ..)| owner.is_some_and(|w| w != o));
        self.cleared.push(owner.map(str::to_owned));
    }
}

#[derive(Clone, Debug, Default)]
pub struct PopupHost {
    /// The shown popup (a copy of the list's last entry, like host 29's +60 buffer), None without one.
    pub shown: Option<PopupRequest>,
    /// Choices the movie made this frame (host vt+64 runs in the stage after the movie update).
    pub choices: Vec<i32>,
    /// +49: input taken until the next show (OnMenuSelect clears it).
    pub input_ready: bool,
}

impl PopupHost {
    /// Natives for `Popup.*`, and the focus-screen `Game` callbacks while a popup is shown.
    pub fn call(&mut self, object: &str, method: &str, args: &[Value]) -> Option<Value> {
        let shown = self.shown.as_ref()?;
        let index = |i: usize| args.get(i).map_or(0, crate::apt_vm::to_integer);
        Some(match (object, method) {
            ("Popup", "GetTitle") => Value::Text(shown.title.clone()),
            ("Popup", "GetDescription") => Value::Text(shown.description.clone()),
            ("Popup", "GetNumOptions") => Value::Number(shown.options.len() as f64),
            // 0x825AA170 has no bounds check; past the count we answer "".
            ("Popup", "GetOption") => Value::Text(shown.options.get(index(0).max(0) as usize).cloned().unwrap_or_default()),
            ("Popup", "IsPopupCancelable") => Value::Bool(shown.cancelable),
            ("Popup", "GetDefaultSelection") => Value::Number(f64::from(shown.default_selection)),
            // Host vt+72 0x8281DD70: every item enabled.
            ("Game", "OnMenuGetItemEnabled") => Value::Bool(true),
            ("Game", "OnMenuHilight") => Value::Undefined,
            // 0x825A4F00: input blocked until the next show, then host vt+64 with the item index (255 = cancel).
            ("Game", "OnMenuSelect") => {
                self.input_ready = false;
                self.choices.push(index(1));
                Value::Undefined
            }
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_popups_carry_their_owner_and_leave_with_the_mod() {
        let mut mods = PopupMods::default();
        let options = skate_mods::menus::PopupOptions {
            description: Some("#Keep going?".into()),
            options: Some(vec!["ID_COMMON_YES".into(), "ID_COMMON_NO".into()]),
            kind: Some("small".into()),
            cancelable: true,
            ..Default::default()
        };
        mods.set("m", "ask", Some(options));
        let request = mods.requests[0].2.clone().unwrap();
        assert_eq!(request.callback, "mod:m:ask");
        assert_eq!(request.kind, skate_core::fe_popup::PopupKind::Small);
        assert!(request.cancelable && request.auto_remove);
        assert_eq!(request.title, "#TODO", "retail's default where the mod sets none");
        mods.set("n", "x", None);
        mods.clear_owner(Some("m"));
        assert_eq!(mods.requests.len(), 1, "the stopped mod's queued popup is dropped");
        assert_eq!(mods.cleared, vec![Some("m".to_owned())]);
        let mut list = skate_core::fe_popup::PopupList::default();
        list.push(request);
        assert_eq!(list.clear_owned("mod:m:").len(), 1);
    }

    #[test]
    fn natives_answer_from_the_shown_popup_and_choices_are_taken() {
        let mut host = PopupHost::default();
        assert!(host.call("Popup", "GetTitle", &[]).is_none(), "no popup, no answer");
        assert!(host.call("Game", "OnMenuSelect", &[]).is_none(), "the screen below gets it");
        host.shown = Some(PopupRequest {
            description: "D".into(),
            options: vec!["ID_COMMON_YES".into(), "ID_COMMON_NO".into()],
            ..Default::default()
        });
        host.input_ready = true;
        assert_eq!(host.call("Popup", "GetNumOptions", &[]), Some(Value::Number(2.0)));
        assert_eq!(host.call("Popup", "GetOption", &[Value::Number(1.0)]), Some(Value::Text("ID_COMMON_NO".into())));
        assert_eq!(host.call("Popup", "IsPopupCancelable", &[]), Some(Value::Bool(false)));
        assert_eq!(host.call("Game", "OnMenuGetItemEnabled", &[Value::Number(0.0), Value::Number(1.0)]), Some(Value::Bool(true)));
        host.call("Game", "OnMenuSelect", &[Value::Number(0.0), Value::Number(1.0), Value::Text("ID_COMMON_NO".into())]);
        assert_eq!(host.choices, vec![1]);
        assert!(!host.input_ready);
    }
}
