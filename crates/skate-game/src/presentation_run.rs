//! The presentation event handlers by type name (research b964 §3 / §5 step 3) for skate-core's
//! `presentation::Manager`. Every retail type has a handler, so no group stalls on an unported one: the ported hosts
//! replace the logged stubs one by one; a mod registers or overrides a handler by name.
//! Details: docs/hails-additions/42-challenges.md#presentation-handler-registry
#![allow(dead_code)] // set_engine / set_mod: the ported hosts and the mod API are the next callers
use bevy::log::info;
use skate_core::presentation::{Event, Handlers, Manager, Out, EVENT_TYPES};
use skate_core::presentation_fade::{Fade, FadeMode, FadeMsg};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// The game's one presentation manager (retail's PresentationManager singleton) with its handlers and the fade
/// object the FadeIn / FadeOut handlers share.
pub struct Presenter {
    pub manager: Manager,
    pub handlers: Registry,
    pub fade: Arc<Mutex<FadeHost>>,
}

impl Default for Presenter {
    fn default() -> Self {
        let fade = Arc::new(Mutex::new(FadeHost::default()));
        let mut handlers = Registry::default();
        handlers.set_engine("FadeIn", Box::new(FadeHandler { host: fade.clone(), mode: FadeMode::In }));
        handlers.set_engine("FadeOut", Box::new(FadeHandler { host: fade.clone(), mode: FadeMode::Out }));
        Self { manager: Manager::default(), handlers, fade }
    }
}

impl Presenter {
    /// One update: the fade object steps its 60 Hz ticks first (the owner's update 825523E0), then the manager.
    pub fn update(&mut self, dt: f32) -> Vec<Out> {
        if let Ok(mut f) = self.fade.lock() {
            f.advance(dt);
        }
        self.manager.update(dt, &mut self.handlers)
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

/// 82AE9028: skips leading characters that cannot start a number, then reads the longest float prefix.
fn lenient_float(s: &str) -> f32 {
    let start = s.find(|c: char| c.is_ascii_digit() || c == '-' || c == '.').unwrap_or(s.len());
    let rest = &s[start..];
    (1..=rest.len()).rev().filter(|&i| rest.is_char_boundary(i)).find_map(|i| rest[..i].parse::<f32>().ok()).unwrap_or(0.0)
}

/// One event type's behaviour (retail start / update / done / undo / stop slots; b900).
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
