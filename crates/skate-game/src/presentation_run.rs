//! The presentation event handlers by type name (research b964 §3 / §5 step 3) for skate-core's
//! `presentation::Manager`. Every retail type has a handler, so no group stalls on an unported one: the ported hosts
//! replace the logged stubs one by one; a mod registers or overrides a handler by name.
//! Details: docs/hails-additions/42-challenges.md#presentation-handler-registry
#![allow(dead_code)] // set_engine / set_mod: the ported hosts and the mod API are the next callers
use bevy::log::info;
use skate_core::presentation::{Event, Handlers, Manager, Out, EVENT_TYPES};
use skate_core::challenges::callout::{Instruction, Panel};
use skate_core::presentation_fade::{Fade, FadeMode, FadeMsg};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// The game's one presentation manager (retail's PresentationManager singleton) with its handlers and the hosts
/// they share. One resource for every owner (tutorials, the career, mods); updated once a frame.
#[derive(bevy::prelude::Resource)]
pub struct Presenter {
    pub manager: Manager,
    pub handlers: Registry,
    pub fade: Arc<Mutex<FadeHost>>,
    pub callout: Arc<Mutex<CalloutHost>>,
    pub sim: Arc<Mutex<SimControl>>,
    /// Achievement names the GrantAchievement events asked for, for the career to grant.
    pub grants: Arc<Mutex<Vec<String>>>,
    /// The presentation speech lines (nis.big export) and the speaker names, loaded at startup.
    pub speech: Arc<Mutex<SpeechLines>>,
    /// Lines the Speech events started, for the audio side to play.
    pub speech_out: Arc<Mutex<Vec<SpeechPlay>>>,
    /// Cast lines the Speech events asked for (ids under 5000), for the world speech's main cast to play.
    pub cast_out: Arc<Mutex<Vec<CastSpeech>>>,
    /// Groups that finished or were removed (`presentationeventgroupfinished`), for owners that wait on a group
    /// (the career's Execute); the newest 64 kept.
    finished: Vec<String>,
    /// A tutorial already ran the update this frame (with its zero-time passes).
    driven: bool,
}

impl Default for Presenter {
    fn default() -> Self {
        let fade = Arc::new(Mutex::new(FadeHost::default()));
        let mut handlers = Registry::default();
        handlers.set_engine("FadeIn", Box::new(FadeHandler { host: fade.clone(), mode: FadeMode::In }));
        handlers.set_engine("FadeOut", Box::new(FadeHandler { host: fade.clone(), mode: FadeMode::Out }));
        let callout = Arc::new(Mutex::new(CalloutHost::default()));
        handlers.set_engine("ShowTutorialCalloutHUD", Box::new(CalloutHandler { host: callout.clone(), show: true }));
        handlers.set_engine("HideTutorialCalloutHUD", Box::new(CalloutHandler { host: callout.clone(), show: false }));
        let sim = Arc::new(Mutex::new(SimControl::default()));
        handlers.set_engine("SetSimRate", Box::new(SimRateHandler { host: sim.clone(), set: true }));
        handlers.set_engine("RestoreSimRate", Box::new(SimRateHandler { host: sim.clone(), set: false }));
        let grants = Arc::new(Mutex::new(Vec::new()));
        handlers.set_engine("GrantAchievement", Box::new(GrantHandler { grants: grants.clone() }));
        let speech = Arc::new(Mutex::new(SpeechLines::default()));
        let speech_out = Arc::new(Mutex::new(Vec::new()));
        let cast_out = Arc::new(Mutex::new(Vec::new()));
        for (kind, wait) in [("Speech", true), ("SpeechNoWait", false)] {
            handlers.set_engine(kind, Box::new(SpeechHandler { lines: speech.clone(), out: speech_out.clone(), cast: cast_out.clone(), wait, left: 0.0 }));
        }
        Self { manager: Manager::default(), handlers, fade, callout, sim, grants, speech, speech_out, cast_out, finished: Vec::new(), driven: false }
    }
}

impl Presenter {
    /// One update: the fade object steps its 60 Hz ticks first (the owner's update 825523E0), then the manager.
    pub fn update(&mut self, dt: f32) -> Vec<Out> {
        if let Ok(mut f) = self.fade.lock() {
            f.advance(dt);
        }
        if let Ok(mut s) = self.speech.lock() {
            s.playing = (s.playing - dt).max(0.0);
        }
        let out = self.manager.update(dt, &mut self.handlers);
        self.driven = true;
        for o in &out {
            let (Out::GroupFinished(id) | Out::GroupRemoved(id)) = o;
            self.finished.push(id.clone());
        }
        let over = self.finished.len().saturating_sub(64);
        self.finished.drain(..over);
        if let Ok(mut c) = self.callout.lock() {
            c.update(dt);
        }
        out
    }

    /// Whether `group` finished or was removed since the last ask (each id is reported once).
    pub fn take_finished(&mut self, group: &str) -> bool {
        let found = self.finished.iter().position(|g| g == group);
        found.map(|i| self.finished.remove(i)).is_some()
    }

    pub fn take_grants(&self) -> Vec<String> {
        self.grants.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default()
    }

    /// The sim controller's timestep while a SetSimRate holds (posted every frame), else None.
    pub fn sim_timestep(&self) -> Option<f32> {
        self.sim.lock().ok().and_then(|s| (s.timestep > 0.0).then_some(s.timestep))
    }

    /// Controller reset (8256E460, the challenge's cleanup): the rate goes back to normal.
    pub fn reset_sim(&mut self) {
        if let Ok(mut s) = self.sim.lock() {
            s.restore();
        }
    }

    /// The tutorial ended: the callout hides (its instruction goes back to the reset state, 82D29B18).
    pub fn reset_callout(&mut self) {
        if let Ok(mut c) = self.callout.lock() {
            c.instruction.hide();
            c.update(0.0);
            let visible = c.instruction.visible;
            c.instruction = Instruction { revision: c.instruction.revision.wrapping_add(1), visible, ..Instruction::default() };
        }
    }

    /// The fade overlay alpha (0 clear, 1 covered). Not drawn yet: the look is open (research b904).
    pub fn fade_alpha(&self) -> f32 {
        self.fade.lock().map(|f| f.alpha).unwrap_or(0.0)
    }
}

/// The fade object on the game clock: whole 60 Hz ticks from the frame time, the last alpha, the finishes seen.
#[derive(Default)]
pub struct FadeHost {
    pub fade: Fade,
    pub alpha: f32,
    finished: [bool; 4],
    carry: f32,
}

impl FadeHost {
    fn take(&mut self, msgs: Vec<FadeMsg>) {
        for m in msgs {
            match m {
                FadeMsg::Alpha(a) => self.alpha = a,
                FadeMsg::Other(_) => {}
                FadeMsg::Finished(mode) => self.finished[mode as usize] = true,
            }
        }
    }

    pub fn advance(&mut self, dt: f32) {
        self.carry += dt.max(0.0);
        let step = self.fade.params.step.max(1e-4);
        let mut n = 0;
        while self.carry >= step && n < 60 {
            self.carry -= step;
            let msgs = self.fade.tick();
            self.take(msgs);
            n += 1;
        }
        if n == 60 {
            self.carry = 0.0;
        }
    }
}

/// FadeIn (0) / FadeOut (1), 8257B040: arg1 seconds (lenient float); done on the fade's finish message for its own
/// mode; undo cancels every slot (82D5C010, overlay off at once); stop does nothing.
pub struct FadeHandler {
    host: Arc<Mutex<FadeHost>>,
    mode: FadeMode,
}

impl Handler for FadeHandler {
    fn start(&mut self, event: &Event) {
        let seconds = event.args[0].as_deref().map(lenient_float).unwrap_or(0.0);
        if let Ok(mut h) = self.host.lock() {
            h.finished[self.mode as usize] = false;
            let msgs = h.fade.start(self.mode, seconds);
            h.take(msgs);
        }
        info!("PRESENTATION {} {seconds} s (fade alpha only, not drawn: NOT RETAIL YET)", event.kind);
    }
    fn is_done(&mut self, _event: &Event) -> bool {
        self.host.lock().map(|h| h.finished[self.mode as usize]).unwrap_or(true)
    }
    fn undo(&mut self, _event: &Event) {
        if let Ok(mut h) = self.host.lock() {
            h.fade.cancel_all();
        }
    }
}

/// The once-a-frame update for every group, unless a tutorial already ran it this frame.
pub fn tick(mut presenter: bevy::prelude::ResMut<Presenter>, time: bevy::prelude::Res<bevy::prelude::Time>) {
    if !presenter.driven {
        presenter.update(time.delta_secs());
    }
    presenter.driven = false;
}

/// Startup: the presentation speech lines from the install (none without the export: Speech events then only log).
pub fn load_speech(presenter: bevy::prelude::Res<Presenter>, config: bevy::prelude::Res<crate::config::Config>) {
    let lines = SpeechLines::load(&config.asset_root);
    info!("PRESENTATION speech: {} speakers, {} lines", lines.speakers.len(), lines.lines.len());
    if let Ok(mut s) = presenter.speech.lock() {
        *s = lines;
    }
}

/// The nis.big export (`private/audio/speech/nis.json`, decoded lines in `speech/nis/<file>.wav`) by (character,
/// speech id), and the eSk8Characters speaker names (`menu-tables.json` speakers). Data a mod can replace.
#[derive(Default)]
pub struct SpeechLines {
    pub speakers: Vec<String>,
    /// A line's takes in nisresident.big's file-table order (824A5A08).
    pub lines: BTreeMap<(u32, u32), Vec<String>>,
    pub audio: std::path::PathBuf,
    /// Per line: the take order this pass and the cursor (82482130).
    picks: BTreeMap<(u32, u32), (Vec<usize>, usize)>,
    /// The shuffle's random source (retail: the global PRNG 82A8AF10; ours seeded, so runs repeat).
    rng: u32,
    /// Seconds left of the line playing now (one stream: a request meanwhile does not play, b1014).
    pub playing: f32,
}

impl SpeechLines {
    pub fn load(asset_root: &std::path::Path) -> Self {
        let speakers = std::fs::read_to_string(asset_root.join(crate::retail_menus::TABLES)).ok()
            .and_then(|t| skate_data::menu_tables::MenuTables::from_json(&t).ok())
            .map(|t| t.speakers).unwrap_or_default();
        let audio = asset_root.join("private/audio/speech/nis");
        let mut lines: BTreeMap<(u32, u32), Vec<String>> = BTreeMap::new();
        let index = std::fs::read_to_string(asset_root.join("private/audio/speech/nis.json")).ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
        for l in index.as_ref().and_then(|i| i["lines"].as_array()).into_iter().flatten() {
            let (Some(c), Some(id), Some(file)) = (l["character"].as_u64(), l["id"].as_u64(), l["file"].as_str()) else { continue };
            lines.entry((c as u32, id as u32)).or_default().push(file.to_owned());
        }
        Self { speakers, lines, audio, picks: BTreeMap::new(), rng: 0x9E37_79B9, playing: 0.0 }
    }

    fn random(&mut self) -> u32 {
        if self.rng == 0 {
            self.rng = 0x9E37_79B9;
        }
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng
    }

    /// 82482130: takes play round-robin; at the start of each pass a line with more than 2 takes is shuffled with
    /// count / 2 swaps of random pairs (824821C0), so 2 takes alternate in file order.
    pub fn pick(&mut self, line: (u32, u32)) -> Option<String> {
        let count = self.lines.get(&line)?.len();
        if count == 0 {
            return None;
        }
        let (mut order, cursor) = self.picks.remove(&line).unwrap_or_else(|| ((0..count).collect(), 0));
        if cursor == 0 && count > 2 {
            for _ in 0..count / 2 {
                let (a, b) = (self.random() as usize % count, self.random() as usize % count);
                order.swap(a, b);
            }
        }
        let take = order[cursor];
        self.picks.insert(line, (order, (cursor + 1) % count));
        self.lines.get(&line).map(|t| t[take].clone())
    }

    /// 8257C308: the speaker name's eSk8Characters id (exact name).
    pub fn character(&self, speaker: &str) -> Option<u32> {
        self.speakers.iter().position(|s| s == speaker).map(|i| i as u32)
    }
}

/// A line a Speech event started.
#[derive(Clone, Debug, PartialEq)]
pub struct SpeechPlay {
    pub file: std::path::PathBuf,
    pub character: u32,
    pub id: u32,
    pub seconds: f32,
}

/// Seconds of a 16-bit PCM wav from its header (byte rate) and size; None if unreadable.
fn wav_seconds(path: &std::path::Path) -> Option<f32> {
    use std::io::Read;
    let mut head = [0u8; 44];
    let mut f = std::fs::File::open(path).ok()?;
    f.read_exact(&mut head).ok()?;
    let rate = u32::from_le_bytes(head[28..32].try_into().ok()?);
    let len = f.metadata().ok()?.len();
    (rate > 0 && &head[0..4] == b"RIFF").then(|| (len.saturating_sub(44)) as f32 / rate as f32)
}

/// Speech (2) / SpeechNoWait (3), 8257C308 (b874): arg1 the speaker (eSk8Characters name), arg2 the speech id (atoi).
/// Speech holds its group until the line ends or 10.0 s (821963E4), SpeechNoWait finishes at once.
/// The take: `SpeechLines::pick` (b1014). NOT RETAIL YET: the Unknown speaker's AudioCharacterOverride attribute;
/// Coach Frank's quit line (31 / 5706) queued after the current line.
pub struct SpeechHandler {
    lines: Arc<Mutex<SpeechLines>>,
    out: Arc<Mutex<Vec<SpeechPlay>>>,
    cast: Arc<Mutex<Vec<CastSpeech>>>,
    wait: bool,
    left: f32,
}

/// A cast line a Speech event asked for: the speaker's character (the eSk8Characters index, also its main-cast
/// voice) and the main-cast event of the speech id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CastSpeech {
    pub character: u32,
    pub event: u16,
    pub id: u32,
}

/// The first NIS speech id: lower ids are cast / announcer / camera-man lines (824A4EE8: `4999 < id`).
pub const NIS_FIRST_ID: u32 = 5000;

/// Retail's speech id -> main-cast event table (TU3 `8224F1B8`, 73 rows of [event, id], read by 824AC050). Retail
/// values as the default; data a mod or a later setup export can replace.
pub const CAST_SPEECH_EVENTS: [(u16, u32); 73] = [(0, 101), (1, 104), (3, 130), (6, 202), (7, 308), (8, 309), (9, 310), (11, 601), (14, 901), (15, 902), (16, 903), (17, 904), (18, 908), (19, 909), (20, 910), (77, 501), (115, 201), (125, 906), (129, 1706), (131, 1705), (133, 1704), (135, 1703), (136, 1002), (137, 1003), (138, 1006), (141, 1014), (193, 1011), (195, 1010), (213, 1005), (241, 400), (247, 338), (248, 312), (250, 912), (251, 307), (253, 344), (254, 311), (255, 1307), (257, 1232), (258, 1233), (259, 1234), (260, 1235), (261, 1236), (262, 1237), (263, 1238), (264, 1239), (265, 1201), (266, 1203), (267, 1204), (268, 1205), (269, 1206), (270, 1208), (271, 1207), (272, 1209), (273, 1210), (274, 1215), (275, 1216), (276, 1217), (277, 1218), (278, 1221), (279, 1222), (280, 1223), (281, 1225), (282, 1226), (283, 1227), (284, 1230), (285, 1231), (286, 1224), (287, 150), (288, 151), (289, 1707), (290, 911), (292, 1015), (293, 1016)];

/// The main-cast event of a cast speech id (the first row with that id, as 824AC050 scans).
pub fn cast_event(id: u32) -> Option<u16> {
    CAST_SPEECH_EVENTS.iter().find(|r| r.1 == id).map(|r| r.0)
}

/// Speech's own limit (821963E4).
pub const SPEECH_TIMEOUT: f32 = 10.0;

impl Handler for SpeechHandler {
    fn start(&mut self, event: &Event) {
        self.left = 0.0;
        let speaker = event.args[0].as_deref().unwrap_or("");
        let id = event.args[1].as_deref().map(lenient_float).unwrap_or(0.0) as u32;
        let Ok(mut lines) = self.lines.lock() else { return };
        let Some(character) = lines.character(speaker) else {
            info!("PRESENTATION {} {speaker} {id}: unknown speaker (no speaker table?)", event.kind);
            return;
        };
        // Ids under 5000 are cast lines, not NIS (Speech_Play 824A4EE8): the id's main-cast event from retail's table
        // (824AC050), asked of the speaker on the world speech's main cast. The event does not wait on them.
        if id < NIS_FIRST_ID {
            match cast_event(id) {
                Some(cast_event) => {
                    info!("PRESENTATION {} {speaker} ({character}) {id}: main-cast event {cast_event}", event.kind);
                    if let Ok(mut out) = self.cast.lock() {
                        out.push(CastSpeech { character, event: cast_event, id });
                    }
                }
                None => info!("PRESENTATION {} {speaker} ({character}) {id}: no main-cast event for this id", event.kind),
            }
            return;
        }
        // A line already playing: the request ends at once (Started + Ended on the next update, b1014).
        if lines.playing > 0.0 {
            info!("PRESENTATION {} {speaker} ({character}) {id}: a line is playing, not played", event.kind);
            return;
        }
        let Some(file) = lines.pick((character, id)) else {
            info!("PRESENTATION {} {speaker} ({character}) {id}: no line in the export", event.kind);
            return;
        };
        let path = lines.audio.join(format!("{file}.wav"));
        let seconds = wav_seconds(&path).unwrap_or(0.0);
        info!("PRESENTATION {} {speaker} ({character}) {id}: {file} {seconds:.2} s", event.kind);
        lines.playing = seconds;
        if self.wait {
            self.left = seconds.min(SPEECH_TIMEOUT);
        }
        if let Ok(mut out) = self.out.lock() {
            out.push(SpeechPlay { file: path, character, id, seconds });
        }
    }
    fn update(&mut self, _event: &Event, dt: f32) {
        self.left -= dt;
    }
    fn is_done(&mut self, _event: &Event) -> bool {
        self.left <= 0.0
    }
    fn stop(&mut self, _event: &Event) {
        self.left = 0.0;
    }
}

/// GrantAchievement: the career grants the named achievement (done at once).
pub struct GrantHandler {
    grants: Arc<Mutex<Vec<String>>>,
}

impl Handler for GrantHandler {
    fn start(&mut self, event: &Event) {
        if let (Some(name), Ok(mut g)) = (event.args[0].clone(), self.grants.lock()) {
            g.push(name);
        }
    }
}

/// The presentation sim controller (owner +0x1C8, research b900 §5, b902 §2): its rate as a timestep (0 = not set).
/// FixSkaterInAir, PauseSim, input restrictions and objective evaluation while paused are not ported (logged stubs).
#[derive(Default)]
pub struct SimControl {
    pub timestep: f32,
}

impl SimControl {
    /// vt+0x80: +0x14 = 0, so the controller stops posting and the sim returns to 1/60 about 2 ticks later.
    pub fn restore(&mut self) {
        if self.timestep > 0.0 {
            info!("PRESENTATION RestoreSimRate (timestep {:.4} -> normal)", self.timestep);
        }
        self.timestep = 0.0;
    }
}

/// SetSimRate (47, 8282A2F8 via the handler): timestep 1 / atof(arg1), arg1 a tick rate ("30" = 30 ticks a second,
/// half speed against 60); undo restores. RestoreSimRate (48). Both done at once; the handlers' stop (82579B90)
/// restores the rate.
pub struct SimRateHandler {
    host: Arc<Mutex<SimControl>>,
    set: bool,
}

impl Handler for SimRateHandler {
    fn start(&mut self, event: &Event) {
        let Ok(mut s) = self.host.lock() else { return };
        if !self.set {
            s.restore();
            return;
        }
        let rate = event.args[0].as_deref().map(lenient_float).unwrap_or(0.0);
        if rate > 0.0 {
            s.timestep = 1.0 / rate;
            info!("PRESENTATION SetSimRate {rate} (timestep {:.4})", s.timestep);
        } else {
            info!("PRESENTATION SetSimRate {:?}: not a rate, ignored", event.args[0]);
        }
    }
    fn undo(&mut self, _event: &Event) {
        if self.set {
            if let Ok(mut s) = self.host.lock() {
                s.restore();
            }
        }
    }
    fn stop(&mut self, _event: &Event) {
        if let Ok(mut s) = self.host.lock() {
            s.restore();
        }
    }
}

/// The tutorial callout's instruction and HUD component (skate-core `challenges::callout`); the states it produced
/// wait in `states` until the challenge HUD takes them.
#[derive(Default)]
pub struct CalloutHost {
    pub instruction: Instruction,
    pub panel: Panel,
    /// PlayerStance_IsRegular is false (the gestures re-place on a change, state 77).
    pub goofy: bool,
    pub states: Vec<i32>,
}

impl CalloutHost {
    pub fn update(&mut self, dt: f32) {
        let states = self.panel.update(&self.instruction, dt, !self.goofy);
        for s in &states {
            info!("PRESENTATION callout state {s} key {} text {}", self.instruction.key, self.instruction.text);
        }
        self.states.extend(states);
    }
}

/// ShowTutorialCalloutHUD (20): arg1 the gesture record key (empty = text only), arg2 the text; HideTutorialCalloutHUD
/// (21). Both done at once.
pub struct CalloutHandler {
    host: Arc<Mutex<CalloutHost>>,
    show: bool,
}

impl Handler for CalloutHandler {
    fn start(&mut self, event: &Event) {
        if let Ok(mut h) = self.host.lock() {
            if self.show {
                h.instruction.show(event.args[0].as_deref(), event.args[1].as_deref().unwrap_or(""));
            } else {
                h.instruction.hide();
            }
        }
    }
}

/// 82AE9028: skips leading characters that cannot start a number, then reads the longest float prefix.
fn lenient_float(s: &str) -> f32 {
    let start = s.find(|c: char| c.is_ascii_digit() || c == '-' || c == '.').unwrap_or(s.len());
    let rest = &s[start..];
    (1..=rest.len()).rev().filter(|&i| rest.is_char_boundary(i)).find_map(|i| rest[..i].parse::<f32>().ok()).unwrap_or(0.0)
}

/// One event type's behavior (retail start / update / done / undo / stop slots; b900).
pub trait Handler: Send + Sync {
    fn start(&mut self, event: &Event);
    fn update(&mut self, _event: &Event, _dt: f32) {}
    /// Instant types are done at once.
    fn is_done(&mut self, _event: &Event) -> bool {
        true
    }
    fn undo(&mut self, _event: &Event) {}
    fn stop(&mut self, _event: &Event) {}
}

/// A type whose host is not ported yet: logged, done at once (NOT RETAIL YET for the waiting types such as NIS, FMV,
/// Speech, which retail holds until their end message).
pub struct Stub;

impl Handler for Stub {
    fn start(&mut self, event: &Event) {
        info!("PRESENTATION stub {} {:?} (not ported: done at once)", event.kind, event.args);
    }
}

/// Wait (type 4, 8257C820): holds the group for arg1 seconds of game time (atof; an unreadable number waits 0).
#[derive(Default)]
pub struct Wait {
    left: f32,
}

impl Handler for Wait {
    fn start(&mut self, event: &Event) {
        self.left = event.args[0].as_deref().and_then(|a| a.trim().parse::<f32>().ok()).unwrap_or(0.0);
    }
    fn update(&mut self, _event: &Event, dt: f32) {
        self.left -= dt;
    }
    fn is_done(&mut self, _event: &Event) -> bool {
        self.left <= 0.0
    }
    fn stop(&mut self, _event: &Event) {
        self.left = 0.0;
    }
}

/// Who registered a handler: the engine's own, or a mod (dropped with `remove_mod`, the engine's comes back).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Engine,
    Mod(String),
}

struct Entry {
    handler: Box<dyn Handler>,
    source: Source,
}

/// The handler map (retail's map by type; R11). Unknown names log once per event and finish.
pub struct Registry {
    engine: BTreeMap<String, Box<dyn Handler>>,
    overrides: BTreeMap<String, Entry>,
}

impl Default for Registry {
    /// Every retail type: Wait ported, the rest logged stubs.
    fn default() -> Self {
        let mut engine: BTreeMap<String, Box<dyn Handler>> = EVENT_TYPES.iter().map(|n| (n.to_string(), Box::new(Stub) as Box<dyn Handler>)).collect();
        engine.insert("Wait".into(), Box::new(Wait::default()));
        Self { engine, overrides: BTreeMap::new() }
    }
}

impl Registry {
    /// The engine's handler for a type (a ported host replacing its stub).
    pub fn set_engine(&mut self, kind: &str, handler: Box<dyn Handler>) {
        self.engine.insert(kind.into(), handler);
    }

    /// A mod's handler: a new type, or an override of a retail one while the mod is on.
    pub fn set_mod(&mut self, mod_id: &str, kind: &str, handler: Box<dyn Handler>) {
        self.overrides.insert(kind.into(), Entry { handler, source: Source::Mod(mod_id.into()) });
    }

    /// Mod disable: its handlers go (the manager's `remove_owner` stops its running events first).
    pub fn remove_mod(&mut self, mod_id: &str) {
        self.overrides.retain(|_, e| e.source != Source::Mod(mod_id.into()));
    }

    pub fn source(&self, kind: &str) -> Option<Source> {
        match self.overrides.get(kind) {
            Some(e) => Some(e.source.clone()),
            None => self.engine.contains_key(kind).then_some(Source::Engine),
        }
    }

    fn get(&mut self, kind: &str) -> Option<&mut Box<dyn Handler>> {
        match self.overrides.get_mut(kind) {
            Some(e) => Some(&mut e.handler),
            None => self.engine.get_mut(kind),
        }
    }
}

impl Handlers for Registry {
    fn start(&mut self, event: &Event) {
        match self.get(&event.kind) {
            Some(h) => h.start(event),
            None => info!("PRESENTATION unknown type {} {:?}: done at once", event.kind, event.args),
        }
    }
    fn update(&mut self, event: &Event, dt: f32) {
        if let Some(h) = self.get(&event.kind) {
            h.update(event, dt);
        }
    }
    fn is_done(&mut self, event: &Event) -> bool {
        self.get(&event.kind).is_none_or(|h| h.is_done(event))
    }
    fn undo(&mut self, event: &Event) {
        if let Some(h) = self.get(&event.kind) {
            h.undo(event);
        }
    }
    fn stop(&mut self, event: &Event) {
        if let Some(h) = self.get(&event.kind) {
            h.stop(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skate_core::presentation::{Group, Manager, Out};

    fn ev(kind: &str, arg: Option<&str>) -> Event {
        Event { kind: kind.into(), args: [arg.map(Into::into), None], mask: 0 }
    }

    fn group(id: &str, events: Vec<Event>) -> Group {
        Group { id: id.into(), owner: "t".into(), events }
    }

    #[test]
    fn every_retail_type_has_a_handler_and_unknown_ones_finish() {
        let mut r = Registry::default();
        assert!(EVENT_TYPES.iter().all(|k| r.source(k) == Some(Source::Engine)));
        let mut m = Manager::default();
        m.start(group("a", vec![ev("NIS", Some("intro")), ev("NoSuchType", None), ev("CentralMessage", Some("ID_X"))]));
        assert_eq!(m.update(0.016, &mut r), [Out::GroupFinished("a".into())]);
    }

    /// Retail's id table (8224F1B8): the follow the leader lines 308 / 309 / 310 are main-cast events 7 / 8 / 9.
    #[test]
    fn cast_speech_ids_map_to_main_cast_events() {
        assert_eq!((cast_event(308), cast_event(309), cast_event(310)), (Some(7), Some(8), Some(9)));
        assert_eq!(cast_event(101), Some(0));
        assert_eq!(cast_event(5044), None);
    }

    /// A SpeechNoWait with a cast id asks the main cast for the event in the speaker's voice and does not wait.
    #[test]
    fn a_cast_speech_event_goes_to_the_main_cast() {
        let mut p = Presenter::default();
        if let Ok(mut s) = p.speech.lock() {
            s.speakers = (0..31).map(|i| if i == 30 { "Reda".to_string() } else { format!("s{i}") }).collect();
        }
        p.manager.start(group("ftl", vec![Event { kind: "SpeechNoWait".into(), args: [Some("Reda".into()), Some("309".into())], mask: 0 }]));
        assert_eq!(p.update(1.0 / 60.0), [Out::GroupFinished("ftl".into())]);
        let out = p.cast_out.lock().map(|o| o.clone()).unwrap_or_default();
        assert_eq!(out, [CastSpeech { character: 30, event: 8, id: 309 }]);
    }

    #[test]
    fn wait_holds_the_group_for_its_seconds() {
        let mut r = Registry::default();
        let mut m = Manager::default();
        m.start(group("a", vec![ev("Wait", Some("0.5")), ev("SFX", Some("x"))]));
        assert!(m.update(0.25, &mut r).is_empty());
        assert_eq!(m.update(0.25, &mut r), [Out::GroupFinished("a".into())]);
    }

    #[test]
    fn fade_out_holds_its_group_until_black_and_a_fade_in_clears_it() {
        let mut p = Presenter::default();
        p.manager.start(group("a", vec![ev("FadeOut", Some("0.5"))]));
        let mut finished = 0;
        for n in 1..=40 {
            if !p.update(1.0 / 60.0).is_empty() {
                finished = n;
                break;
            }
        }
        // the start update posts nothing; the fade reaches black after 30 to 31 ticks
        assert!((30..=32).contains(&finished), "{finished}");
        assert_eq!(p.fade_alpha(), 1.0);
        p.manager.start(group("b", vec![ev("FadeIn", Some("x0.25"))]));
        let mut done = false;
        for _ in 0..40 {
            done |= !p.update(1.0 / 60.0).is_empty();
        }
        assert!(done && p.fade_alpha() < 1e-3 && !p.fade.lock().unwrap().fade.overlay);
        assert_eq!(lenient_float("abc-1.5e1z"), -15.0);
    }

    #[test]
    fn callout_events_reach_the_challenge_hud_in_order_and_cleanup_hides_it() {
        let mut p = Presenter::default();
        let mut hint = ev("ShowTutorialCalloutHUD", Some("kickflip"));
        hint.args[1] = Some("ID_CHALLENGE_TUT_SKSC_11_HINT_N_01".into());
        p.manager.start(group("a", vec![hint, ev("Wait", Some("0.5"))]));
        p.update(0.0);
        p.update(1.0 / 60.0);
        let mut hud = crate::hud_challenge::ChallengeHudState::default();
        crate::hud_challenge::take_callout(&mut hud, &p, None);
        assert_eq!((hud.callout.states.as_slice(), hud.callout.sent, hud.callout.text.as_str()), (&[-10, 10][..], 2, "ID_CHALLENGE_TUT_SKSC_11_HINT_N_01"));
        p.reset_callout();
        crate::hud_challenge::take_callout(&mut hud, &p, None);
        assert_eq!((hud.callout.states.last(), hud.callout.sent), (Some(&-20), 3));
    }

    #[test]
    fn set_sim_rate_holds_a_tick_rate_until_restored_or_reset() {
        let mut p = Presenter::default();
        p.manager.start(group("a", vec![ev("SetSimRate", Some("30"))]));
        p.update(0.0);
        assert_eq!(p.sim_timestep(), Some(1.0 / 30.0));
        p.manager.start(group("b", vec![ev("RestoreSimRate", Some("60"))]));
        p.update(0.0);
        assert_eq!(p.sim_timestep(), None);
        p.manager.start(group("c", vec![ev("SetSimRate", Some("45"))]));
        p.update(0.0);
        assert_eq!(p.sim_timestep(), Some(1.0 / 45.0));
        p.reset_sim();
        assert_eq!(p.sim_timestep(), None);
        p.manager.start(group("d", vec![ev("SetSimRate", Some("junk"))]));
        p.update(0.0);
        assert_eq!(p.sim_timestep(), None, "an unreadable rate is ignored");
    }

    /// A mono 16-bit wav of `seconds` at 48 kHz.
    fn wav(seconds: f32) -> Vec<u8> {
        let data = (seconds * 48_000.0) as u32 * 2;
        let mut w = Vec::new();
        w.extend(b"RIFF");
        w.extend((36 + data).to_le_bytes());
        w.extend(b"WAVEfmt ");
        w.extend(16u32.to_le_bytes());
        w.extend(1u16.to_le_bytes());
        w.extend(1u16.to_le_bytes());
        w.extend(48_000u32.to_le_bytes());
        w.extend(96_000u32.to_le_bytes());
        w.extend(2u16.to_le_bytes());
        w.extend(16u16.to_le_bytes());
        w.extend(b"data");
        w.extend(data.to_le_bytes());
        w.resize(w.len() + data as usize, 0);
        w
    }

    #[test]
    fn speech_plays_the_speakers_line_and_waits_for_its_end_while_no_wait_finishes_at_once() {
        let dir = std::env::temp_dir().join(format!("skate-speech-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("31_5012_TUT-X_3.wav"), wav(0.5)).unwrap();
        let mut p = Presenter::default();
        {
            let mut lines = p.speech.lock().unwrap();
            lines.speakers = (0..104).map(|i| if i == 31 { "SkateCoach".to_owned() } else { format!("S{i}") }).collect();
            lines.lines.insert((31, 5012), vec!["31_5012_TUT-X_3".into()]);
            lines.audio = dir.clone();
        }
        let mut speech = ev("Speech", Some("SkateCoach"));
        speech.args[1] = Some("5012".into());
        p.manager.start(group("a", vec![speech.clone()]));
        assert!(p.update(0.0).is_empty(), "holds for the line");
        assert!(p.update(0.25).is_empty());
        assert_eq!(p.update(0.3), [Out::GroupFinished("a".into())]);
        let played = std::mem::take(&mut *p.speech_out.lock().unwrap());
        assert_eq!((played.len(), played[0].character, played[0].id), (1, 31, 5012));
        assert!((played[0].seconds - 0.5).abs() < 1e-3);
        speech.kind = "SpeechNoWait".into();
        p.manager.start(group("b", vec![speech, ev("Speech", Some("Nobody"))]));
        assert_eq!(p.update(0.0), [Out::GroupFinished("b".into())], "no wait; an unknown speaker finishes at once");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn takes_play_round_robin_two_alternate_and_more_shuffle_each_pass() {
        let mut l = SpeechLines::default();
        l.lines.insert((31, 1), vec!["a".into(), "b".into()]);
        l.lines.insert((31, 2), (0..5).map(|i| i.to_string()).collect());
        assert_eq!([l.pick((31, 1)), l.pick((31, 1)), l.pick((31, 1))], [Some("a".into()), Some("b".into()), Some("a".into())]);
        let mut pass: Vec<String> = (0..5).filter_map(|_| l.pick((31, 2))).collect();
        pass.sort();
        assert_eq!(pass, ["0", "1", "2", "3", "4"], "every take once per pass");
        assert_eq!(l.pick((9, 9)), None);
    }

    struct Hold;
    impl Handler for Hold {
        fn start(&mut self, _event: &Event) {}
        fn is_done(&mut self, _event: &Event) -> bool {
            false
        }
    }

    #[test]
    fn a_mod_overrides_a_type_until_it_is_removed() {
        let mut r = Registry::default();
        r.set_mod("hold", "FadeIn", Box::new(Hold));
        assert_eq!(r.source("FadeIn"), Some(Source::Mod("hold".into())));
        let mut m = Manager::default();
        m.start(group("a", vec![ev("FadeIn", None)]));
        assert!(m.update(0.1, &mut r).is_empty(), "the mod's handler holds the group");
        r.remove_mod("hold");
        assert_eq!(r.source("FadeIn"), Some(Source::Engine));
        assert_eq!(m.update(0.1, &mut r), [Out::GroupFinished("a".into())]);
    }
}
