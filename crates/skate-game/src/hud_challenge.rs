//! The challenge HUD components (`hud2/hudtimer`, `hud2/dpo`, `hud2/hudobjective2`) and the own the spot banners
//! (`challenge/ownthespot`, `challenge/killedit`), each on its own APT VM next to the central message HUD.
//! The movies only read: their `HUDComponents.*` natives answer from [`ChallengeHudState`], which the challenge
//! runtime (challenges.rs) or a mod writes. Research b844 / b845; docs/hails-additions/42-challenges.md, "the
//! challenge HUD in play".
use crate::{
    apt_movie::Movie,
    apt_vm::{Host, ObjectKind, Value, Vm},
};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// The components a challenge HUD shows, in draw order (retail eHUDComponent names).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum Component {
    Timer,
    Objective,
    Dpo,
    OwnedIt,
    KilledIt,
}

impl Component {
    pub const ALL: [Component; 5] = [Component::Timer, Component::Objective, Component::Dpo, Component::OwnedIt, Component::KilledIt];

    /// The converted movie (`private/hud/runtime/<file>`, tools/prepare_hud.py).
    pub fn file(self) -> &'static str {
        match self {
            Component::Timer => "hudtimer.json",
            Component::Objective => "hudobjective2.json",
            Component::Dpo => "dpo.json",
            Component::OwnedIt => "ownthespot.json",
            Component::KilledIt => "killedit.json",
        }
    }

    /// The movie's refresh state while shown (b844: timer 3 `UpdateTimerHUD`, dpo 3 `Update`, objective 3
    /// `SetupObjectives`); the banners have none.
    fn refresh_state(self) -> Option<f64> {
        match self {
            Component::Timer | Component::Dpo | Component::Objective => Some(3.0),
            Component::OwnedIt | Component::KilledIt => None,
        }
    }
}

/// The front-end sound of each new second under 10 s on the challenge timer.
pub(crate) const LOW_TIME_SOUND: &str = "timer_end";

/// One DPO (the score / target box): the numbers as shown and the title text id (retail `DPOSetParams` /
/// `DPOSetTitle`, `DPO_GetParams` items 0, 1, 2 and 4).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct DpoSlot {
    pub numerator: String,
    pub denominator: String,
    pub title: String,
    pub highlight: i32,
}

/// One objective row (`Objective_GetObjective*`): state 0 fail, 1 success, 2 inactive, 3 active, 4 disabled; kind
/// 0 = a state row, otherwise a fraction row showing `value`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct ObjectiveRow {
    pub state: i32,
    pub kind: i32,
    pub title: String,
    pub value: String,
}

/// What the challenge HUD shows (retail's HUD elements `timer`, `dpo`, `objective` and the banner flags).
/// Serialisable: the authority's run writes it, peers and replays show the same.
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct ChallengeHudState {
    pub shown: Vec<Component>,
    pub timer_text: String,
    /// `Timer_GetData` item 1: the movie draws the time red when it is 0.
    pub timer_flag: i32,
    pub dpo: [DpoSlot; 2],
    pub two_dpos: bool,
    pub objectives: Vec<ObjectiveRow>,
}

impl ChallengeHudState {
    pub fn show(&mut self, component: Component, shown: bool) {
        self.shown.retain(|c| *c != component);
        if shown {
            self.shown.push(component);
        }
    }

    pub fn is_shown(&self, component: Component) -> bool {
        self.shown.contains(&component)
    }

    /// The part of the state a component reads, to tell whether it needs its refresh state.
    fn reads(&self, component: Component) -> String {
        match component {
            Component::Timer => format!("{}|{}", self.timer_text, self.timer_flag),
            Component::Dpo => format!("{:?}|{}", self.dpo, self.two_dpos),
            Component::Objective => format!("{:?}", self.objectives),
            Component::OwnedIt | Component::KilledIt => String::new(),
        }
    }
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

pub struct Bindings {
    pub movie: Movie,
    state: ChallengeHudState,
    /// `Audio.PlaySound` names the movie asked for since the last update (`fe` record names).
    pub sounds: Vec<String>,
    /// The screen controller the movie registered (`ScreenManager.OnLoaded(movie, controller)`).
    controller: Option<usize>,
    /// The last `Mission.SetKilledItShakeActivation` argument.
    pub shake: bool,
    random: u32,
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
        let row = |i: usize| self.state.objectives.get(args.get(i).map_or(0.0, |v| v.number()) as usize).cloned().unwrap_or_default();
        let s = &self.state;
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
            ("HUDComponents", "Timer_GetData") => array(vm, [Value::Text(s.timer_text.clone()), Value::Number(s.timer_flag as f64)]),
            ("HUDComponents", "DPO_IsUsingTwoDPOs") => Ok(Value::Bool(s.two_dpos)),
            ("HUDComponents", "DPO_GetParams") => {
                let slots = if s.two_dpos { &s.dpo[..] } else { &s.dpo[..1] };
                let items: Vec<Value> = slots
                    .iter()
                    .flat_map(|d| {
                        [Value::Text(d.numerator.clone()), Value::Text(d.denominator.clone()), Value::Text(d.title.clone()),
                            Value::Number(0.0), Value::Number(d.highlight as f64)]
                    })
                    .collect();
                array(vm, items)
            }
            ("HUDComponents", "Objective_GetMaxNumObjectives") => Ok(Value::Number(4.0)),
            ("HUDComponents", "Objective_GetNumObjectives") => Ok(Value::Number(s.objectives.len().min(4) as f64)),
            ("HUDComponents", "Objective_GetObjectiveState") => Ok(Value::Number(row(0).state as f64)),
            ("HUDComponents", "Objective_GetObjectiveType") => Ok(Value::Number(row(0).kind as f64)),
            ("HUDComponents", "Objective_GetObjectiveTitle") => Ok(Value::Text(row(0).title)),
            ("HUDComponents", "Objective_GetObjectiveValue") => Ok(Value::Text(row(0).value)),
            // Own the spot sets up its objective HUD without the activity line (b844: 3-argument SetupObjectiveHUDData).
            ("HUDComponents", "Objective_GetActivityTitle" | "Objective_GetActivityType") => Ok(Value::Text(String::new())),
            ("HUDComponents", "Objective_GetActivityTypeVisibility" | "Objective_GetActivityNameVisibility") => Ok(Value::Bool(false)),
            ("FELanguage", "GetLanguage") => Ok(Value::Text("english".into())),
            ("FELanguage", "GetLocalizedFormatStringCustom") => {
                let id = args.first().map(|v| v.text()).unwrap_or_default();
                Ok(Value::Text(self.movie.text_assets.localize(&id)))
            }
            ("Mission", "IsInSkateParkTutorial") => Ok(Value::Bool(false)),
            // The killed it banner's camera shake switch; the shake is not ported (doc 42 open questions).
            ("Mission", "SetKilledItShakeActivation") => {
                self.shake = args.first().is_some_and(|v| v.truth());
                Ok(Value::Undefined)
            }
            ("Audio", "PlaySound") => {
                if let Some(Value::Text(name)) = args.first() {
                    self.sounds.push(name.clone());
                }
                Ok(Value::Undefined)
            }
            ("Render", "SetEffect") => Ok(Value::Undefined),
            ("ScreenManager", "OnLoaded") => {
                if let Some(Value::Object(controller)) = args.get(1) {
                    self.controller = Some(*controller);
                }
                Ok(Value::Undefined)
            }
            ("ScreenManager", _) => Ok(Value::Undefined),
            _ => Err(format!("Unimplemented challenge HUD binding {native}.{method}")),
        }
    }
}

/// One component movie.
pub struct Runtime {
    pub vm: Vm,
    pub bindings: Bindings,
    pub component: Component,
    controller: usize,
    shown: bool,
    read: String,
    /// The timer showed a low time (under 10 s) at its last refresh.
    low: bool,
}

impl Runtime {
    /// `sounds`: the `fe` record names, registered as `FE_SOUND_<NAME>` like the front end's RegisterAudioEnum.
    pub fn load(component: Component, json: &serde_json::Value, sounds: &[String]) -> Result<Self, String> {
        let mut vm = Vm::new();
        for name in ["MovieClip", "FELanguage", "HUDComponents", "Math", "Audio", "Mission", "ScreenManager", "Render"] {
            let object = vm.object(ObjectKind::Native(name.into()));
            let prototype = vm.object(ObjectKind::Plain);
            vm.set(object, "prototype", Value::Object(prototype))?;
            vm.set(vm.global, name, Value::Object(object))?;
        }
        let Value::Object(manager) = vm.get(vm.global, "ScreenManager") else {
            return Err("ScreenManager".into());
        };
        for (name, colour) in crate::hud_central::SCREEN_MANAGER_COLOURS {
            vm.set(manager, name, Value::Number(colour))?;
        }
        for name in sounds {
            vm.set(vm.global, format!("FE_SOUND_{}", name.to_ascii_uppercase()), Value::Text(name.clone()))?;
        }
        vm.set(vm.global, "Screen_EdgeOffset", Value::Number(0.0))?;
        let mut bindings =
            Bindings { movie: Movie::load(json)?, state: ChallengeHudState::default(), sounds: Vec::new(), controller: None, shake: false, random: 0x9e3779b9 };
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
        let mut runtime = Self { vm, bindings, component, controller: usize::MAX, shown: false, read: String::new(), low: false };
        runtime.drain()?;
        runtime.controller = match (runtime.bindings.controller, runtime.vm.get(runtime.bindings.movie.root, "screen")) {
            (Some(c), _) | (None, Value::Object(c)) => c,
            _ => return Err(format!("{component:?} HUD controller was not constructed")),
        };
        Ok(runtime)
    }

    fn drain(&mut self) -> Result<(), String> {
        let mut calls = 0;
        while let Some(crate::apt_movie::Pending { object, offset, .. }) = self.bindings.movie.next_action() {
            calls += 1;
            if calls > 4096 {
                return Err(format!("{:?} HUD frame action limit", self.component));
            }
            let code = self.bindings.movie.actions.get(&offset.to_string()).ok_or("Missing challenge HUD action block")?.clone();
            self.vm.run_on(object, &code, &mut self.bindings)?;
        }
        Ok(())
    }

    fn set_state(&mut self, state: f64) -> Result<(), String> {
        self.vm.begin_update();
        self.vm.call_method(self.controller, "SetState", vec![Value::Number(state)], &mut self.bindings)?;
        self.drain()
    }

    /// Bring the movie to `state`: showing it plays its intro (SetState 1) then fills it, hiding plays its outro
    /// (SetState 2 for the components; the banners hide on any state but 1), and a change of what it reads while
    /// shown sends its refresh state. b844 section 4: the C++ caller is not decoded, this order is the movies' own.
    pub fn sync(&mut self, state: &ChallengeHudState) -> Result<(), String> {
        let shown = state.is_shown(self.component);
        let read = state.reads(self.component);
        let changed = read != self.read;
        self.bindings.state = state.clone();
        self.read = read;
        if shown && !self.shown {
            self.shown = true;
            if self.component == Component::Objective {
                self.set_state(100.0)?;
            }
            self.set_state(1.0)?;
            if let Some(refresh) = self.component.refresh_state() {
                self.set_state(refresh)?;
            }
        } else if !shown && self.shown {
            self.shown = false;
            self.set_state(2.0)?;
        } else if shown && changed {
            if let Some(refresh) = self.component.refresh_state() {
                self.set_state(refresh)?;
            }
            // The timer under 10 s flashes on every new second (SetState 4) with the `timer_end` front-end sound
            // (event 0xAABBE111A2855FF0 = hash64("timer_end"), b848) and goes back to normal (5) when the time rises
            // above it again (82663940, b845).
            if self.component == Component::Timer {
                let low = state.timer_flag == 0;
                if low {
                    self.bindings.sounds.push(LOW_TIME_SOUND.into());
                    self.set_state(4.0)?;
                } else if self.low {
                    self.set_state(5.0)?;
                }
                self.low = low;
            }
        }
        Ok(())
    }

    pub fn shown(&self) -> bool {
        self.shown
    }

    /// One frame of the movie's timelines.
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
    fn showing_and_hiding_tracks_each_component() {
        let mut s = ChallengeHudState::default();
        s.show(Component::Dpo, true);
        s.show(Component::Dpo, true);
        s.show(Component::Timer, true);
        assert_eq!(s.shown, [Component::Dpo, Component::Timer]);
        s.show(Component::Dpo, false);
        assert!(!s.is_shown(Component::Dpo) && s.is_shown(Component::Timer));
    }

    #[test]
    fn each_component_reads_only_its_own_element() {
        let mut a = ChallengeHudState::default();
        let mut b = a.clone();
        b.timer_text = "0:09".into();
        assert_ne!(a.reads(Component::Timer), b.reads(Component::Timer));
        assert_eq!(a.reads(Component::Dpo), b.reads(Component::Dpo));
        a.dpo[0].numerator = "120".into();
        assert_ne!(a.reads(Component::Dpo), b.reads(Component::Dpo));
    }

    /// The installed movies (SKATE3_ASSET_ROOT=<install>/assets, or the hud export folder in SKATE_SCORING_HUD_ROOT)
    /// load, show, refresh and hide without a binding they do not answer.
    #[test]
    #[ignore = "requires the converted challenge HUD movies (SKATE_SCORING_HUD_ROOT=<private/hud>)"]
    fn the_retail_movies_run_through_a_run() {
        let root = std::path::PathBuf::from(std::env::var_os("SKATE_SCORING_HUD_ROOT").unwrap());
        let mut state = ChallengeHudState::default();
        for component in Component::ALL {
            let json: serde_json::Value =
                serde_json::from_slice(&std::fs::read(root.join("runtime").join(component.file())).unwrap()).unwrap();
            let mut hud = Runtime::load(component, &json, &["challenge_ots".to_owned(), "killedit_intro".to_owned()]).unwrap();
            state.show(component, true);
            state.timer_text = "0:10".into();
            state.dpo[0] = DpoSlot { numerator: "0".into(), denominator: "1000".into(), title: "ID_CHALLENGE_OBJECTIVE_BEAT_OWNSPOT".into(), highlight: 1 };
            state.objectives = vec![ObjectiveRow { state: 3, kind: 0, title: "ID_CHALLENGE_OBJECTIVE_BEAT_OWNSPOT".into(), value: String::new() }];
            hud.sync(&state).unwrap();
            for _ in 0..30 {
                hud.update().unwrap();
            }
            state.timer_text = "0:09".into();
            state.dpo[0].numerator = "450".into();
            hud.sync(&state).unwrap();
            for _ in 0..30 {
                hud.update().unwrap();
            }
            let texts: Vec<String> = hud.vm.objects.iter().filter_map(|o| match o.fields.get("text") {
                Some(Value::Text(t)) if !t.is_empty() => Some(t.clone()),
                _ => None,
            }).collect();
            eprintln!("CHALLENGE_HUD {component:?} texts {texts:?} sounds {:?}", hud.bindings.sounds);
            match component {
                Component::Timer => assert!(texts.iter().any(|t| t.contains("0:09")), "{texts:?}"),
                Component::Dpo => assert!(texts.iter().any(|t| t.contains("450")), "{texts:?}"),
                _ => {}
            }
            state.show(component, false);
            hud.sync(&state).unwrap();
            for _ in 0..30 {
                hud.update().unwrap();
            }
            assert!(!hud.shown());
        }
    }
}
