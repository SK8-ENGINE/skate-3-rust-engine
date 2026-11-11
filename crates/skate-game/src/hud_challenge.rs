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
use std::collections::BTreeMap;

/// The components a challenge HUD shows, in draw order (retail eHUDComponent names).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum Component {
    Timer,
    Objective,
    Dpo,
    OwnedIt,
    KilledIt,
    /// The 3-2-1-GO before a race (`hud2/hudcountdown`, b995 item 4).
    Countdown,
    /// The race standings (`hud2/leaderboardhud`, b995 item 1).
    Leaderboard,
    /// The tutorial callout, "Coach Frank Says" (`hud2/hudtutorial`, b866 Â§1, b945 Â§2).
    Tutorial,
}

impl Component {
    pub const ALL: [Component; 8] = [Component::Timer, Component::Objective, Component::Dpo, Component::OwnedIt, Component::KilledIt,
        Component::Countdown, Component::Leaderboard, Component::Tutorial];

    /// The converted movie (`private/hud/runtime/<file>`, tools/prepare_hud.py).
    pub fn file(self) -> &'static str {
        match self {
            Component::Timer => "hudtimer.json",
            Component::Objective => "hudobjective2.json",
            Component::Dpo => "dpo.json",
            Component::OwnedIt => "ownthespot.json",
            Component::KilledIt => "killedit.json",
            Component::Countdown => "hudcountdown.json",
            Component::Leaderboard => "leaderboardhud.json",
            Component::Tutorial => "hudtutorial.json",
        }
    }

    /// The movie's refresh state while shown (b844: timer 3 `UpdateTimerHUD`, dpo 3 `Update`, objective 3
    /// `SetupObjectives`); the banners have none.
    fn refresh_state(self) -> Option<f64> {
        match self {
            Component::Timer | Component::Dpo | Component::Objective => Some(3.0),
            Component::OwnedIt | Component::KilledIt | Component::Countdown | Component::Leaderboard | Component::Tutorial => None,
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
    /// `DPOSetIcon` (item 3, eDPOIcon; the movie does not draw it, b997 section 1).
    #[serde(default)]
    pub icon: u8,
    /// `DPOSetFormatType` + `DPOSetParams`: when set, the texts come from these values (formatter 8266AF68) instead of
    /// `numerator` / `denominator`.
    #[serde(default)]
    pub value: Option<DpoValue>,
}

/// A DPO's format (eDPOFormat value) and its two numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct DpoValue {
    pub format: u8,
    pub numerator: f32,
    pub denominator: f32,
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
    /// Whole seconds left on the race countdown (`Countdown_GetCountdownTime` "#%d"; 0 draws GO).
    #[serde(default)]
    pub countdown: i32,
    #[serde(default)]
    pub leaderboard: LeaderboardHud,
    #[serde(default)]
    pub callout: CalloutHud,
}

/// The tutorial callout as the movie reads it, and the SetState values the callout component sent (doc 42,
/// "Tutorial callout").
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct CalloutHud {
    /// GetTutorialHalText: a text id or literal.
    pub text: String,
    /// GetTutorialGesturesButtons: gesture, button, connector per slot.
    pub gestures: Vec<[String; 3]>,
    /// PlayerStance_IsRegular.
    pub regular: bool,
    /// The latest states, oldest first (at most 16), and the running count of every state sent.
    pub states: Vec<i32>,
    pub sent: u32,
}

/// Move the callout states the presenter produced into the HUD state, with the text and gestures they show.
pub(crate) fn take_callout(hud: &mut ChallengeHudState, presenter: &crate::presentation_run::Presenter, guide: Option<&crate::trick_guide_host::TrickGuideData>) {
    let Ok(mut c) = presenter.callout.lock() else { return };
    if c.states.is_empty() {
        return;
    }
    let states = std::mem::take(&mut c.states);
    let t = &c.instruction;
    let regular = !c.goofy;
    let stance = skate_core::trick_guide::Stance { regular, ..Default::default() };
    let callout = &mut hud.callout;
    callout.text = t.text.clone();
    callout.regular = regular;
    callout.gestures = if t.text_only {
        Vec::new()
    } else {
        guide.zip(skate_data::trick_guide::record_key(&t.key)).map(|(g, k)| g.0.callout_gestures(k, stance)).unwrap_or_default()
    };
    callout.sent = callout.sent.wrapping_add(states.len() as u32);
    callout.states.extend(states);
    let over = callout.states.len().saturating_sub(16);
    callout.states.drain(..over);
}

/// The variant text id the leaderboard movie treats as "no variant".
pub(crate) const LEADERBOARD_NO_VARIANT: &str = "ID_COMMON_UNKNOWN";

/// The leaderboard movie's data (`Leaderboard_Get*Block`, b995 item 1; layouts read from the disc movie 2026-11-08).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct LeaderboardHud {
    /// eLeaderBoardHUD layout (the movie's own values: RankNameTime 4 for races).
    pub layout: i32,
    /// `Mission.GetTitle` (a text id or literal).
    pub title: String,
    /// The variant line under the title (setup block item 3, HUD2 SetVariantString); the movie hides it for
    /// [`LEADERBOARD_NO_VARIANT`] ("#Unknown" too).
    pub variant: String,
    /// One row per racer, in data order (stable racer ids).
    pub rows: Vec<LeaderboardRow>,
}

/// One racer row: the 8 data-block items (rank, score, score text, highlight, turn, team, row slot, sort id) and its name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct LeaderboardRow {
    pub id: u32,
    pub rank: i32,
    /// Below 0 the movie shows "--" instead of `score_text`.
    pub score: f32,
    pub score_text: String,
    /// A text id (localized by the movie's language) or a `#literal`.
    pub name: String,
    /// The local player's row.
    pub highlight: bool,
}

/// The race layout (`eLeaderBoardHUD_RankNameTime`, set to 4 in leaderboardhud's init).
pub(crate) const LEADERBOARD_RANK_NAME_TIME: i32 = 4;

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
            Component::OwnedIt | Component::KilledIt | Component::Tutorial => String::new(),
            Component::Countdown => self.countdown.to_string(),
            Component::Leaderboard => format!("{:?}", self.leaderboard),
        }
    }
}

/// A DPO's numerator and denominator text: its values through the retail formatter (skate-core `dpo`, templates
/// localized and filled like FELanguage), else the texts as written.
fn dpo_texts(slot: &DpoSlot, localize: impl Fn(&str) -> String) -> (String, String) {
    use skate_core::challenges::dpo::{self, DpoFormat, DpoText};
    let Some(v) = slot.value else { return (slot.numerator.clone(), slot.denominator.clone()) };
    let format = DpoFormat::from_value(v.format).unwrap_or_default();
    let fill = |t: DpoText| match t {
        DpoText::Plain(s) => s,
        DpoText::Template { id, value } => crate::fe_language::format_string(
            &crate::fe_language::NumberFormat::default(), &localize(id), &[crate::fe_language::Arg::Number(value)]),
    };
    (fill(dpo::numerator(format, v.numerator)), fill(dpo::denominator(format, v.denominator)))
}

pub(crate) fn array(vm: &mut Vm, values: impl IntoIterator<Item = Value>) -> Result<Value, String> {
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
    /// Object.registerClass bindings: linkage name -> class function.
    classes: BTreeMap<String, usize>,
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
        // MovieClip.attachMovie(linkage, name, depth[, init]) on one of this movie's clips (the gesture item attaches
        // its "g_" + gesture symbol); the new subtree's class constructors run before it returns, as on the menu
        // stage (apt_stage).
        // MovieClip.createEmptyMovieClip(name, depth): an empty sprite in this movie's display tree (gesture_item's
        // holder for the attached gesture).
        if method == "createEmptyMovieClip" && self.movie.instances.contains_key(&object) {
            let name = args.first().map(Value::text).unwrap_or_default();
            let depth = args.get(1).map_or(0, crate::apt_vm::to_integer);
            let id = crate::apt_stage::new_clip(vm, object, &name)?;
            self.movie.adopt_empty(vm, id, object, depth)?;
            return Ok(Value::Object(id));
        }
        if method == "attachMovie" && self.movie.instances.contains_key(&object) {
            let linkage = args.first().map(Value::text).unwrap_or_default();
            let name = args.get(1).map(Value::text).unwrap_or_default();
            let depth = args.get(2).map_or(0, crate::apt_vm::to_integer);
            let init = match args.get(3) {
                Some(Value::Object(o)) => Some(*o),
                _ => None,
            };
            let before = self.movie.immediate.len();
            let Some(child) = self.movie.attach(vm, object, &linkage, &name, depth, init)? else {
                return Ok(Value::Undefined);
            };
            let queued: Vec<crate::apt_movie::Pending> = self.movie.immediate.drain(before.min(self.movie.immediate.len())..).collect();
            let (mut ctors, rest): (Vec<_>, Vec<_>) = queued.into_iter().partition(|p| p.event == crate::apt_movie::CLASS_CONSTRUCTOR);
            self.movie.immediate.extend(rest);
            ctors.sort_by_key(|p| p.object == child);
            let ctors: Vec<(usize, usize)> = ctors
                .iter()
                .filter_map(|p| Some((p.object, *self.movie.classes.get(&self.movie.instances.get(&p.object)?.character)?)))
                .collect();
            for (clip, class) in ctors {
                vm.call_function(class, clip, vec![], self)?;
            }
            return Ok(Value::Object(child));
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
            ("Object", "registerClass") => {
                if let (Some(name), Some(Value::Object(class))) = (args.first(), args.get(1)) {
                    self.classes.insert(name.text(), *class);
                    return Ok(Value::Bool(true));
                }
                Ok(Value::Bool(false))
            }
            ("HUDComponents", "Timer_GetData") => array(vm, [Value::Text(s.timer_text.clone()), Value::Number(s.timer_flag as f64)]),
            ("HUDComponents", "DPO_IsUsingTwoDPOs") => Ok(Value::Bool(s.two_dpos)),
            // hudcountdown's ShowCountdown: "#3" / "#2" / "#1" pick the number frame, anything else GO (8265D910 "#%d").
            ("HUDComponents", "Countdown_GetCountdownTime") => Ok(Value::Text(format!("#{}", s.countdown))),
            // The stadium voice set (FE_SOUND_CHALLENGE_STAD_*) is for contests in a stadium; races use COUNT_*.
            ("HUDComponents", "Countdown_IsInStadium") => Ok(Value::Bool(false)),
            // leaderboardhud SetupScreen: [players, top team players, layout, -, is teams, online mission, show players].
            ("HUDComponents", "Leaderboard_GetSetupDataBlock") => {
                let l = &s.leaderboard;
                array(vm, [Value::Number(l.rows.len() as f64), Value::Number(0.0), Value::Number(l.layout as f64), Value::Text(l.variant.clone()),
                    Value::Bool(false), Value::Bool(false), Value::Bool(true)])
            }
            ("HUDComponents", "Leaderboard_GetLeaderboardNameBlock") => {
                let names: Vec<Value> = s.leaderboard.rows.iter().map(|r| Value::Text(self.movie.text_assets.localize(&r.name))).collect();
                array(vm, names)
            }
            // RefreshAll: per row (playerStructItems 8) rank, score, score text, highlight, turn, team, row slot (the
            // `player_<n>` clip, by rank), sort id.
            ("HUDComponents", "Leaderboard_GetLeaderboardDataBlock") => {
                let items: Vec<Value> = s.leaderboard.rows.iter().flat_map(|r| {
                    let slot = (r.rank - 1).clamp(0, 7);
                    [Value::Number(r.rank as f64), Value::Number(r.score as f64), Value::Text(r.score_text.clone()),
                        Value::Bool(r.highlight), Value::Number(0.0), Value::Number(0.0), Value::Number(slot as f64), Value::Number(r.id as f64)]
                }).collect();
                array(vm, items)
            }
            ("HUDComponents", "GetTutorialHalText") => Ok(Value::Text(self.movie.text_assets.localize(&s.callout.text))),
            // [count, gesture, button, connector, ...] (82661238).
            ("HUDComponents", "GetTutorialGesturesButtons") => {
                let g = &s.callout.gestures;
                let items: Vec<Value> = std::iter::once(Value::Number(g.len() as f64)).chain(g.iter().flatten().map(|t| Value::Text(t.clone()))).collect();
                array(vm, items)
            }
            ("HUDComponents", "PlayerStance_IsRegular") => Ok(Value::Bool(s.callout.regular)),
            ("Mission", "GetTitle") => Ok(Value::Text(self.movie.text_assets.localize(&s.leaderboard.title))),
            // Voice chat status per row: only read in online missions.
            ("FELeaderboard", "GetVoipStatus") => {
                let n = s.leaderboard.rows.len();
                array(vm, (0..n).map(|_| Value::Number(0.0)))
            }
            ("HUDComponents", "DPO_GetParams") => {
                let slots = if s.two_dpos { &s.dpo[..] } else { &s.dpo[..1] };
                let text = &self.movie.text_assets;
                let items: Vec<Value> = slots
                    .iter()
                    .flat_map(|d| {
                        let (numerator, denominator) = dpo_texts(d, |id| text.localize(id));
                        [Value::Text(numerator), Value::Text(denominator), Value::Text(d.title.clone()),
                            Value::Number(d.icon as f64), Value::Number(d.highlight as f64)]
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
    /// The callout state count already sent to hudtutorial.
    callout_sent: u32,
}

/// `_global` values one HUD movie writes and another reads. Retail runs every HUD movie on one `_global`; ours each run
/// on their own VM, so these are copied from the writer to the others after each update. The DPO moves up by the
/// leaderboard's height (doc 42, "The leaderboard").
pub(crate) const SHARED_GLOBALS: &[(Component, &str)] = &[(Component::Leaderboard, "LeaderboardHUD_Height")];

impl Runtime {
    /// A plain `_global` value (objects belong to this VM and are never shared).
    pub fn shared_global(&self, key: &str) -> Option<Value> {
        self.vm.objects.get(self.vm.global)?.fields.get(key).filter(|v| !matches!(v, Value::Object(_))).cloned()
    }

    pub fn set_shared_global(&mut self, key: &str, value: Value) {
        let global = self.vm.global;
        if let Some(o) = self.vm.objects.get_mut(global) {
            if o.fields.get(key) != Some(&value) {
                o.fields.insert(key.to_owned(), value);
            }
        }
    }

    /// `sounds`: the `fe` record names, registered as `FE_SOUND_<NAME>` like the front end's RegisterAudioEnum.
    #[cfg(test)]
    pub fn load(component: Component, json: &serde_json::Value, sounds: &[String]) -> Result<Self, String> {
        Self::load_linked(component, json, sounds, &crate::hud_imports::HudLibraries::default())
    }

    /// As `load`, with the movie's imports linked from `libraries` (hudtutorial's gesture / button items).
    pub fn load_linked(component: Component, json: &serde_json::Value, sounds: &[String], libraries: &crate::hud_imports::HudLibraries) -> Result<Self, String> {
        let mut vm = Vm::new();
        for name in ["MovieClip", "FELanguage", "HUDComponents", "Math", "Audio", "Mission", "ScreenManager", "Render", "FELeaderboard"] {
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
        // Linked library classes register through Object.registerClass (gesture_item: "Gesture Item").
        if !libraries.is_empty() {
            let object = vm.object(ObjectKind::Native("Object".into()));
            let prototype = vm.object(ObjectKind::Plain);
            vm.set(object, "prototype", Value::Object(prototype))?;
            vm.set(vm.global, "Object", Value::Object(object))?;
        }
        let mut bindings = Bindings {
            movie: libraries.link(&format!("hud/{}", component.file()), json)?,
            state: ChallengeHudState::default(),
            sounds: Vec::new(),
            controller: None,
            shake: false,
            random: 0x9e3779b9,
            classes: BTreeMap::new(),
        };
        for code in &libraries.init {
            vm.run(code, &mut bindings)?;
        }
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
        // registerClass binds by linkage name: every clip of that character gets the class (as the menu stage does).
        let movie = &bindings.movie;
        let classes: BTreeMap<i32, usize> = movie.linkage.iter().map(|(name, character)| (*character, name))
            .chain(movie.class_links.iter().map(|(character, name)| (*character, name)))
            .filter_map(|(character, name)| Some((character, *bindings.classes.get(name)?)))
            .collect();
        bindings.movie.classes.extend(classes);
        vm.begin_update();
        bindings.movie.initialize(&mut vm)?;
        let mut runtime = Self { vm, bindings, component, controller: usize::MAX, shown: false, read: String::new(), low: false, callout_sent: 0 };
        runtime.drain()?;
        runtime.controller = match (runtime.bindings.controller, runtime.vm.get(runtime.bindings.movie.root, "screen")) {
            (Some(c), _) | (None, Value::Object(c)) => c,
            _ => return Err(format!("{component:?} HUD controller was not constructed")),
        };
        Ok(runtime)
    }

    fn drain(&mut self) -> Result<(), String> {
        let mut calls = 0;
        while let Some(crate::apt_movie::Pending { object, offset, event }) = self.bindings.movie.next_action() {
            calls += 1;
            if calls > 4096 {
                return Err(format!("{:?} HUD frame action limit", self.component));
            }
            if event == crate::apt_movie::CLASS_CONSTRUCTOR {
                let movie = &self.bindings.movie;
                if let Some(class) = movie.instances.get(&object).and_then(|i| movie.classes.get(&i.character)).copied() {
                    self.vm.call_function(class, object, vec![], &mut self.bindings)?;
                }
                continue;
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
        // hudcountdown has its own states: 2 = ShowCountdown (one per new second, the number and its sound), 1 =
        // HideCountdown (the movie's SetState, read from the disc movie 2026-11-08).
        // leaderboardhud: 3 = SetupScreen then 1 = Show(true) to show, 4 = RefreshAll on a change, 2 = Show(false).
        if self.component == Component::Leaderboard {
            if shown && !self.shown {
                self.shown = true;
                self.set_state(3.0)?;
                self.set_state(1.0)?;
                self.set_state(4.0)?;
            } else if shown && changed {
                self.set_state(4.0)?;
            } else if !shown && self.shown {
                self.shown = false;
                self.set_state(2.0)?;
            }
            return Ok(());
        }
        // hudtutorial: the states the callout component sent since the last sync, in order.
        if self.component == Component::Tutorial {
            let new = state.callout.sent.wrapping_sub(self.callout_sent) as usize;
            self.callout_sent = state.callout.sent;
            let states = &state.callout.states;
            for s in states[states.len().saturating_sub(new)..].to_vec() {
                use skate_core::challenges::callout as c;
                match s {
                    c::STATE_HIDE | c::STATE_HIDE_TEXT => self.shown = false,
                    c::STATE_SHOW | c::STATE_GESTURES | c::STATE_TEXT | c::STATE_TEXT_FIRST => self.shown = true,
                    _ => {}
                }
                self.set_state(s as f64)?;
            }
            return Ok(());
        }
        if self.component == Component::Countdown {
            if shown && (!self.shown || changed) {
                self.shown = true;
                self.set_state(2.0)?;
            } else if !shown && self.shown {
                self.shown = false;
                self.set_state(1.0)?;
            }
            return Ok(());
        }
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
    fn dpo_values_become_the_movie_texts_through_the_language() {
        let language = |id: &str| match id {
            "ID_COMMON_NUMBER" => "{0:d}".to_string(),
            other => other.to_string(),
        };
        let position = DpoSlot { value: Some(DpoValue { format: 0, numerator: 3.0, denominator: 5.0 }), ..Default::default() };
        assert_eq!(dpo_texts(&position, language), ("3".to_string(), "5".to_string()));
        let killed = DpoSlot { value: Some(DpoValue { format: 2, numerator: 0.0, denominator: 65.0 }), ..Default::default() };
        assert_eq!(dpo_texts(&killed, language), ("0:00.00".to_string(), "1:05.00".to_string()));
        let written = DpoSlot { numerator: "120".into(), denominator: "1000".into(), ..Default::default() };
        assert_eq!(dpo_texts(&written, language), ("120".to_string(), "1000".to_string()), "own the spot writes its texts");
    }

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
            state.dpo[0] = DpoSlot { numerator: "0".into(), denominator: "1000".into(), title: "ID_CHALLENGE_OBJECTIVE_BEAT_OWNSPOT".into(), highlight: 1, ..Default::default() };
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

    /// leaderboardhud with three race rows: shown, set up and refreshed without an unanswered binding, the ranks, names
    /// and times in its text fields.
    #[test]
    #[ignore = "requires the converted challenge HUD movies (SKATE_SCORING_HUD_ROOT=<private/hud>)"]
    fn the_leaderboard_shows_ranks_names_and_times() {
        let root = std::path::PathBuf::from(std::env::var_os("SKATE_SCORING_HUD_ROOT").unwrap());
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("runtime").join(Component::Leaderboard.file())).unwrap()).unwrap();
        let mut hud = Runtime::load(Component::Leaderboard, &json, &[]).unwrap();
        let mut state = ChallengeHudState::default();
        let row = |id: u32, rank: i32, score: f32, name: &str, highlight: bool| LeaderboardRow {
            id, rank, score, score_text: skate_core::challenges::dpo::time_text(score), name: name.into(), highlight };
        state.leaderboard = LeaderboardHud { layout: LEADERBOARD_RANK_NAME_TIME, title: String::new(), variant: LEADERBOARD_NO_VARIANT.into(), rows: vec![
            row(0, 2, 1.5, "#Player", true), row(1, 1, 0.0, "ID_COMMON_DANNY_WAY", false), row(2, 3, 4.25, "ID_COMMON_MIKE_CARROLL", false)] };
        state.show(Component::Leaderboard, true);
        hud.sync(&state).unwrap();
        for _ in 0..30 {
            hud.update().unwrap();
        }
        state.leaderboard.rows[0].score = 2.0;
        state.leaderboard.rows[0].score_text = "0:02.00".into();
        hud.sync(&state).unwrap();
        for _ in 0..10 {
            hud.update().unwrap();
        }
        let texts: Vec<String> = hud.vm.objects.iter().filter_map(|o| match o.fields.get("text") {
            Some(Value::Text(t)) if !t.is_empty() => Some(t.clone()),
            _ => None,
        }).collect();
        eprintln!("LEADERBOARD texts {texts:?}");
        let height = hud.shared_global("LeaderboardHUD_Height");
        eprintln!("LEADERBOARD height {height:?}");
        assert!(height.as_ref().is_some_and(|h| h.number() > 0.0), "the DPO moves up by this: {height:?}");
        assert!(!texts.iter().any(|t| t == "0" || t == LEADERBOARD_NO_VARIANT), "no variant line: {texts:?}");
        for want in ["Player", "0:02.00", "0:04.25"] {
            assert!(texts.iter().any(|t| t.contains(want)), "{want} in {texts:?}");
        }
        assert!(texts.iter().any(|t| t.contains('2')), "a rank in {texts:?}");
        state.show(Component::Leaderboard, false);
        hud.sync(&state).unwrap();
        assert!(!hud.shown());
    }

    /// hudtutorial: a gesture tip (-10 then 10) fills the text and the gesture row, a text tip (11) clears the row,
    /// and the hides (13, -20) leave it invisible.
    #[test]
    #[ignore = "requires the converted challenge HUD movies (SKATE_SCORING_HUD_ROOT=<private/hud>)"]
    fn the_tutorial_callout_shows_its_text_and_gestures() {
        let root = std::path::PathBuf::from(std::env::var_os("SKATE_SCORING_HUD_ROOT").unwrap());
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("runtime").join(Component::Tutorial.file())).unwrap()).unwrap();
        let libraries = crate::hud_imports::HudLibraries::load(&root.join("../menu-movies"), &[&json]);
        assert!(!libraries.is_empty(), "gesture_item / button_item2 from the menu movie set: {:?}", libraries.failures);
        let mut hud = Runtime::load_linked(Component::Tutorial, &json, &[], &libraries).unwrap();
        assert!(!libraries.imported_shapes(&hud.bindings.movie).is_empty(), "imported shapes for the gesture row");
        let mut all_shapes: crate::apt_scene::Shapes = serde_json::from_value(json["shapes"].clone()).unwrap();
        all_shapes.extend(libraries.imported_shapes(&hud.bindings.movie));
        let mut state = ChallengeHudState::default();
        let get = |hud: &Runtime, path: &[&str]| {
            let mut at = Value::Object(hud.vm.global);
            for key in path {
                at = match at {
                    Value::Object(o) => hud.vm.get(o, key),
                    _ => Value::Undefined,
                };
            }
            at
        };
        let screen = ["hudtutorial", "mScreen"];
        let label = ["hudtutorial", "mScreen", "mContainer", "mTextAnim", "mText", "label", "text"];
        let gesture = ["hudtutorial", "mScreen", "mContainer", "mGesturesAnim", "mGestures", "mcGestureItem0", "_visible"];
        let send = |hud: &mut Runtime, state: &mut ChallengeHudState, text: &str, gestures: Vec<[String; 3]>, states: &[i32]| {
            state.callout.text = text.into();
            state.callout.gestures = gestures;
            state.callout.regular = true;
            // The object SetState writes: the registered controller's mScreen (not a path through _global).
            // hudtutorial's code reads the class static `hudtutorial.mScreen` (constant 1 = hudtutorial).
            let vis = |hud: &Runtime| format!("{:?} units {}", get(hud, &["hudtutorial", "mScreen", "_visible"]),
                crate::apt_scene::draw(&hud.bindings.movie, &hud.vm, &all_shapes).map(|d| d.len()).unwrap_or(0));
            eprintln!("CALLOUT trace before {states:?}: mScreen visible {:?}", vis(hud));
            // One state per sync, to see which one changes the panel's visibility.
            for &one in states {
                state.callout.states = vec![one];
                state.callout.sent += 1;
                hud.sync(state).unwrap();
                eprintln!("CALLOUT trace after state {one}: mScreen visible {:?}", vis(hud));
            }
            let mut last = vis(hud);
            for n in 0..30 {
                hud.update().unwrap();
                let now = vis(hud);
                if now != last {
                    eprintln!("CALLOUT trace update {n}: mScreen visible {last:?} -> {now:?}");
                    last = now;
                }
            }
        };
        let kickflip = vec![["ollie_kickflip".to_string(), String::new(), String::new()]];
        // A text tip first on its own movie, for comparison (it draws in game).
        let mut text_hud = Runtime::load_linked(Component::Tutorial, &json, &[], &libraries).unwrap();
        send(&mut text_hud, &mut ChallengeHudState::default(), "#3 pushes", Vec::new(), &[-10, 12]);
        send(&mut hud, &mut state, "#Do a kickflip", kickflip, &[-10, 10]);
        eprintln!("CALLOUT gestures: label {:?} gesture0 visible {:?} shown {}", get(&hud, &label), get(&hud, &gesture), hud.shown());
        assert!(hud.shown());
        assert_eq!(get(&hud, &label).text(), "Do a kickflip");
        assert_eq!(get(&hud, &gesture), Value::Bool(true), "the kickflip gesture item shows");
        // It draws from the menu set's gesture_item textures (paths re-rooted at the HUD folder, files present).
        let mut shapes: crate::apt_scene::Shapes = serde_json::from_value(json["shapes"].clone()).unwrap();
        shapes.extend(libraries.imported_shapes(&hud.bindings.movie));
        let drawn: Vec<String> = crate::apt_scene::draw(&hud.bindings.movie, &hud.vm, &shapes).unwrap().into_iter().map(|d| d.texture).collect();
        let gestures: Vec<&String> = drawn.iter().filter(|t| t.contains("gesture_item")).collect();
        eprintln!("CALLOUT drawn {} units, gesture textures {gestures:?}", drawn.len());
        // Layout after the gesture state (the in-game gesture tip drew nothing: doc 42 "Tutorial callout").
        let base = ["hudtutorial", "mScreen"];
        for clip in [&[][..], &["mContainer"][..], &["mContainer", "mPanelAnim"][..], &["mContainer", "mPanelAnim", "mPanel"][..],
            &["mContainer", "mTextAnim"][..], &["mContainer", "mTextAnim", "mText"][..], &["mContainer", "mGesturesAnim"][..],
            &["mContainer", "mGesturesAnim", "mGestures"][..], &["mContainer", "mGesturesAnim", "mGestures", "mcGestureItem0"][..]] {
            let path: Vec<&str> = base.iter().chain(clip.iter()).copied().collect();
            let props: Vec<String> = ["_x", "_y", "_width", "_height", "_xscale", "_yscale", "_alpha", "_visible", "_currentframe"].iter()
                .map(|k| { let mut p = path.clone(); p.push(k); format!("{k}={:?}", get(&hud, &p)) }).collect();
            eprintln!("CALLOUT layout {}: {}", clip.join("."), props.join(" "));
        }
        assert!(!gestures.is_empty() && gestures.iter().all(|t| root.join(t.as_str()).is_file()), "gesture textures draw and exist");
        send(&mut hud, &mut state, "#Turn", Vec::new(), &[11]);
        eprintln!("CALLOUT text: label {:?} gesture0 visible {:?}", get(&hud, &label), get(&hud, &gesture));
        assert_eq!(get(&hud, &label).text(), "Turn");
        send(&mut hud, &mut state, "#", Vec::new(), &[13, -20]);
        eprintln!("CALLOUT hidden: screen visible {:?}", get(&hud, &[screen[0], screen[1], "_visible"]));
        assert!(!hud.shown());
    }

    /// hudcountdown: each new value shows its number frame ("#3" frame 0, "#2" 10, "#1" 20; GO on the language label)
    /// with its count sound.
    #[test]
    #[ignore = "requires the converted challenge HUD movies (SKATE_SCORING_HUD_ROOT=<private/hud>)"]
    fn the_countdown_shows_each_number_frame() {
        let root = std::path::PathBuf::from(std::env::var_os("SKATE_SCORING_HUD_ROOT").unwrap());
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("runtime").join(Component::Countdown.file())).unwrap()).unwrap();
        let sounds: Vec<String> = ["3", "2", "1", "go"].iter().map(|n| format!("challenge_count_{n}")).collect();
        let mut hud = Runtime::load(Component::Countdown, &json, &sounds).unwrap();
        let mut state = ChallengeHudState::default();
        state.show(Component::Countdown, true);
        let (mut frames, mut textures) = (Vec::new(), Vec::new());
        for n in [3, 2, 1, 0] {
            state.countdown = n;
            hud.sync(&state).unwrap();
            for _ in 0..6 {
                hud.update().unwrap();
            }
            let path = ["hudcountdown", "mScreen", "mcCountdownText", "mcNumber"];
            let mut at = Value::Object(hud.vm.global);
            for key in path {
                at = match at {
                    Value::Object(o) => hud.vm.get(o, key),
                    _ => Value::Undefined,
                };
            }
            let frame = match at {
                Value::Object(o) => hud.bindings.movie.instances.get(&o).map(|i| i.frame),
                _ => None,
            };
            eprintln!("COUNTDOWN {n}: mcNumber frame {frame:?} sounds {:?}", hud.bindings.sounds);
            // The drawn picture: one texture per number (the install's json and texture files must agree).
            let shapes: crate::apt_scene::Shapes = serde_json::from_value(json["shapes"].clone()).unwrap();
            let drawn: Vec<String> = crate::apt_scene::draw(&hud.bindings.movie, &hud.vm, &shapes).unwrap()
                .into_iter().filter(|d| !d.texture.is_empty()).map(|d| d.texture).collect();
            eprintln!("  DRAW {drawn:?}");
            textures.push(drawn);
            frames.push(frame);
        }
        assert_eq!(&frames[..3], &[Some(0), Some(10), Some(20)], "#3 / #2 / #1 frames");
        assert!(frames[3].is_some_and(|f| f >= 30), "GO on a language frame: {frames:?}");
        for (i, a) in textures.iter().enumerate() {
            assert!(!a.is_empty(), "value {i} draws nothing");
            assert!(textures[i + 1..].iter().all(|b| b != a), "3, 2, 1 and GO draw different pictures: {textures:?}");
        }
    }
}
