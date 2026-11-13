//! The central message HUD (`hud2/hudcentral`) on the scoring HUD's APT VM; mods: `sdk.ui.central_message`.
//! Retail post path 82663F38: docs/hails-additions/31-retail-menus.md#central-message-hud
use crate::{
    apt_movie::Movie,
    apt_vm::{Host, ObjectKind, Value, Vm},
};
use bevy::prelude::*;

/// Show one centre-screen line: a language id of the HUD movie (`ID_CHASE_TAZER_1`) or plain text. Retail's
/// defaults: style 0 (off-white), audio hook 0 (the default central message sound), shown for
/// [`DISPLAY_SECONDS`] (`seconds` overrides it, a mod's choice).
#[derive(Message, Clone, Debug, PartialEq)]
pub(crate) struct CentralMessage {
    pub text: String,
    pub style: i32,
    pub audio_hook: i32,
    pub seconds: Option<f32>,
}

impl CentralMessage {
    pub(crate) fn new(text: impl Into<String>) -> Self {
        Self { text: text.into(), style: 0, audio_hook: 0, seconds: None }
    }
}

/// How long a message stays before the movie's outro: retail's central HUD update (`82664550`) counts the shown
/// message's updates (`+0xBC`) and sends the screen event 5 (`SetState(5)`: `HideMessage`, the outro) at 180
/// (`0xB4`), i.e. 6 s at the console's 30 updates a second; time-based here, so any frame rate gives the same.
pub(crate) const DISPLAY_SECONDS: f32 = 180.0 / 30.0;

/// `ScreenManager` colours the movie reads (`source/helper/screenmanager`: `mColour_OffWhite` 0xFFFDEC,
/// `mColour_Red` 9904651); that movie runs on the front-end stage, not here.
const SCREEN_MANAGER_COLOURS: [(&str, f64); 2] = [("mColour_OffWhite", 16776940.0), ("mColour_Red", 9904651.0)];

pub struct Bindings {
    pub movie: Movie,
    text: String,
    style: i32,
    audio_hook: i32,
    /// `Audio.PlaySound` names the movie asked for since the last update (`fe` record names).
    pub sounds: Vec<String>,
    random: u32,
}

pub struct Runtime {
    pub vm: Vm,
    pub bindings: Bindings,
    controller: usize,
    /// The shown message's time on screen and its limit (retail `+0xBC` / `+0xC0`); `None` = nothing up.
    shown: Option<(f32, f32)>,
}

fn array(vm: &mut Vm, values: impl IntoIterator<Item = Value>) -> Result<Value, String> {
    let object = vm.object(ObjectKind::Plain);
    let mut n = 0;
    for value in values {
        vm.set(object, n.to_string(), value)?;
        n += 1;
    }
    vm.set(object, "length", Value::Number(n as f64))?;
    Ok(Value::Object(object))
}

impl Host for Bindings {
    fn property_changed(&mut self, vm: &mut Vm, object: usize, key: &str) -> Result<(), String> {
        if key == "text" || key == "autoSize" {
            self.movie.text_changed(vm, object)?;
        }
        if ["_xscale", "_yscale", "_rotation", "_width", "_height"].iter().any(|k| key.eq_ignore_ascii_case(k)) {
            self.movie.transform_changed(vm, object, key)?;
        }
        Ok(())
    }
    fn call(&mut self, vm: &mut Vm, object: usize, method: &str, args: Vec<Value>) -> Result<Value, String> {
        if self.movie.method(vm, object, method, &args)? {
            return Ok(Value::Undefined);
        }
        let native = match vm.objects.get(object).map(|o| &o.kind) {
            Some(ObjectKind::Native(name)) => name.as_str(),
            _ => "global",
        };
        match (native, method) {
            ("Math", "floor") => Ok(Value::Number(args.first().ok_or("Math.floor argument")?.number().floor())),
            ("Math", "random") => {
                // Presentation-only random source.
                self.random ^= self.random << 13;
                self.random ^= self.random >> 17;
                self.random ^= self.random << 5;
                Ok(Value::Number(self.random as f64 / 4294967296.0))
            }
            ("global", "ASSetPropFlags") => Ok(Value::Undefined),
            ("FELanguage", "GetLanguage") => Ok(Value::Text("english".into())),
            ("HUDComponents", "Central_GetMessageData") => {
                array(vm, [Value::Text(self.text.clone()), Value::Number(self.style as f64)])
            }
            ("HUDComponents", "Central_GetAudioHook") => Ok(Value::Number(self.audio_hook as f64)),
            ("Audio", "PlaySound") => {
                if let Some(Value::Text(name)) = args.first() {
                    self.sounds.push(name.clone());
                }
                Ok(Value::Undefined)
            }
            // The message clip's timeline calls it, but the name is not in the TU3 image: retail's Audio object has
            // no such method, and AS2 makes the call a no-op.
            ("Audio", "PlayGenCentralMsge") => Ok(Value::Undefined),
            // The screen registers with the front end's screen manager; the HUD has none to answer.
            ("ScreenManager", _) => Ok(Value::Undefined),
            _ => Err(format!("Unimplemented central HUD binding {native}.{method}")),
        }
    }
}

impl Runtime {
    /// `sounds`: the `fe` record names, registered as `FE_SOUND_<NAME>` like the front end's RegisterAudioEnum.
    pub fn load(json: &serde_json::Value, sounds: &[String]) -> Result<Self, String> {
        let mut vm = Vm::new();
        for name in ["MovieClip", "FELanguage", "HUDComponents", "Math", "Audio", "Mission", "ScreenManager"] {
            let object = vm.object(ObjectKind::Native(name.into()));
            let prototype = vm.object(ObjectKind::Plain);
            vm.set(object, "prototype", Value::Object(prototype))?;
            vm.set(vm.global, name, Value::Object(object))?;
        }
        let Value::Object(manager) = vm.get(vm.global, "ScreenManager") else {
            return Err("ScreenManager".into());
        };
        for (name, colour) in SCREEN_MANAGER_COLOURS {
            vm.set(manager, name, Value::Number(colour))?;
        }
        for name in sounds {
            vm.set(vm.global, format!("FE_SOUND_{}", name.to_ascii_uppercase()), Value::Text(name.clone()))?;
        }
        vm.set(vm.global, "Screen_EdgeOffset", Value::Number(0.0))?;
        let mut bindings = Bindings {
            movie: Movie::load(json)?,
            text: String::new(),
            style: 0,
            audio_hook: 0,
            sounds: Vec::new(),
            random: 0x9e3779b9,
        };
        let initial: Vec<_> = bindings
            .movie
            .characters
            .values()
            .flat_map(|c| &c.frames)
            .flat_map(|f| &f.controls)
            .filter(|c| c.type_name == "do_init_action")
            .map(|c| c.actions_offset)
            .collect();
        for offset in initial {
            let code = bindings.movie.actions[&offset.to_string()].clone();
            vm.run(&code, &mut bindings)?;
        }
        vm.begin_update();
        bindings.movie.initialize(&mut vm)?;
        let mut runtime = Self { vm, bindings, controller: usize::MAX, shown: None };
        runtime.drain()?;
        let Value::Object(controller) = runtime.vm.get(runtime.bindings.movie.root, "screen") else {
            return Err("Central HUD controller was not constructed".into());
        };
        runtime.controller = controller;
        runtime.vm.call_method(controller, "Show", vec![Value::Bool(true)], &mut runtime.bindings)?;
        runtime.drain()?;
        Ok(runtime)
    }

    fn drain(&mut self) -> Result<(), String> {
        let mut calls = 0;
        while let Some(crate::apt_movie::Pending { object, offset, .. }) = self.bindings.movie.next_action() {
            calls += 1;
            if calls > 4096 {
                return Err("Central HUD frame action limit".into());
            }
            let code = self.bindings.movie.actions.get(&offset.to_string()).ok_or("Missing central HUD action block")?.clone();
            self.vm.run_on(object, &code, &mut self.bindings)?;
        }
        Ok(())
    }

    /// Retail `82663F38` then the screen event 4 (`SetState(4)`). Retail stores the formatted, localised text
    /// (the id is looked up, `8290C8A8`), so a language id the movie knows goes in as its text ("ZAP!"); anything
    /// else as written. It restarts the timer.
    pub fn show(&mut self, message: &CentralMessage) -> Result<(), String> {
        let known = self.bindings.movie.text_assets.language.contains_key(&message.text);
        self.bindings.text = if known { self.bindings.movie.text_assets.localize(&message.text) } else { format!("#{}", message.text) };
        self.shown = Some((0.0, message.seconds.unwrap_or(DISPLAY_SECONDS)));
        self.bindings.style = message.style;
        self.bindings.audio_hook = message.audio_hook;
        self.vm.begin_update();
        self.vm.call_method(self.controller, "SetState", vec![Value::Number(4.0)], &mut self.bindings)?;
        self.drain()
    }

    /// The text the message clip shows now (`mScreen.mcMessageText...labelText.text`), for logs and tests.
    pub fn message_text(&self) -> &str {
        &self.bindings.text
    }

    /// Whether a message is up (shown and not yet hidden).
    pub fn showing(&self) -> bool {
        self.shown.is_some()
    }

    /// Retail's central HUD update (`82664550`): `dt` seconds more on screen; at the limit the screen event 5
    /// (`SetState(5)`: the movie's `HideMessage`, which plays the outro). True when it hid the message now.
    pub fn tick(&mut self, dt: f32) -> Result<bool, String> {
        let Some((t, limit)) = self.shown.as_mut() else { return Ok(false) };
        *t += dt;
        if *t < *limit {
            return Ok(false);
        }
        self.shown = None;
        self.vm.begin_update();
        self.vm.call_method(self.controller, "SetState", vec![Value::Number(5.0)], &mut self.bindings)?;
        self.drain()?;
        Ok(true)
    }

    /// One frame of the movie's timelines (intro / idle / outro of the message clip).
    pub fn update(&mut self) -> Result<(), String> {
        self.vm.begin_update();
        self.bindings.movie.advance(&mut self.vm)?;
        self.drain()?;
        self.vm.collect(self.bindings.movie.instances.keys().copied())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires the installed central HUD (SKATE3_ASSET_ROOT=<install>/assets)"]
    fn a_taze_line_shows_its_text_and_plays_the_default_sound() {
        let root = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").unwrap());
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("private/hud/runtime/hudcentral.json")).unwrap()).unwrap();
        let mut hud = Runtime::load(&json, &["central_message_default".to_owned()]).unwrap();
        assert_eq!(hud.bindings.movie.text_assets.localize("ID_CHASE_TAZER_1"), "ZAP!");
        hud.show(&CentralMessage::new("ID_CHASE_TAZER_1")).unwrap();
        // ShowMessage wrote the localised text into the message clip's label (mcText2.mcTextStringWhite.labelText.text).
        let zap = Value::Text("ZAP!".into());
        assert!(hud.vm.objects.iter().any(|o| o.fields.get("text") == Some(&zap)), "label text set");
        assert_eq!(hud.bindings.sounds, ["central_message_default"], "kAUDIOHOOK_DEFAULT is 0");
        for _ in 0..90 {
            hud.update().unwrap();
        }
        // Retail hides it after 180 central HUD updates (6 s at 30 a second, `82664550`): SetState(5), the outro.
        assert!(hud.showing());
        assert!(!hud.tick(DISPLAY_SECONDS - 0.5).unwrap(), "still up before 6 s");
        assert!(hud.tick(0.5).unwrap(), "hidden at 6 s");
        assert!(!hud.showing());
        for _ in 0..60 {
            hud.update().unwrap();
        }
        // Plain text from a mod stays as written.
        hud.show(&CentralMessage::new("Nice line")).unwrap();
        assert_eq!(hud.message_text(), "#Nice line");
    }
}
