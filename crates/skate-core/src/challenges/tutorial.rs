//! Tutorials, re-implemented from the retail state graph `tutorials` (research b858; doc 42). Same runner rules as
//! own the spot (`ots.rs`, research b832): deferred state changes, enter on flush, state handlers before global ones.
//! The teaching itself (which trick, hints, prompts) lives in the objective rows, run by the objective group.
use super::timers::{TimerId, Timers};
use super::Completion;

/// Retail values of one tutorial row.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Params {
    /// Whether the row lists intro / outro / restart presentation events.
    pub intro: bool,
    pub outro: bool,
    pub restart: bool,
    /// `IgnoreInitialTutorialReposition`: Some(false) always repositions; unset (no retail row sets it) or true
    /// repositions only beyond `REPOSITION_DISTANCE` (the graph's `== false or (~= false and distance > 10)`).
    pub ignore_initial_reposition: Option<bool>,
    /// `SpawnOffboard` for the reposition.
    pub spawn_offboard: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    Initial,
    MakeHud,
    MakeObjectives,
    IntroPres,
    Setup,
    AssetLoad,
    ChallengeRunning,
    ShowResults,
    PlayPresEvent,
    EndOptions,
    Restarted,
    ChallengeCleanup,
    Complete,
}

/// The three presentation groups the row fills.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Presentation {
    Intro,
    Outro,
    Restart,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hud {
    Objective,
    OwnedIt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    HudLoaded,
    PresentationFinished(Presentation),
    AssetLoadComplete,
    TeleportFinished,
    AllObjectivesComplete,
    ObjectiveFailed(usize),
    HudContinue,
    HudRetry,
    Abandon,
    RestartChallenge,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    PauseMenu(bool),
    /// HUD2.CreateGroup: objective, tutorial instruction and owned it components.
    CreateHud,
    MakeObjectives,
    Evaluation(bool),
    StartPresentation(Presentation),
    HomVisuals(bool),
    /// The asset load screen: answered by AssetLoadComplete (at once when nothing is left to load).
    LoadAssets,
    /// Remove then add the row's visual indicators (so a restart does not double them).
    AddIndicators,
    Reposition { offboard: bool },
    CloseLoadingScreen,
    OpenLoadingScreen,
    ShowHud(Hud, bool),
    ResetObjectives,
    ResetObjective(usize),
    ActivateStartObjectives,
    FadeIn { seconds: f32 },
    NotifyRunning,
    LockProgressionPresentation(bool),
    /// The continue / retry dialog after a win.
    OpenEndOptions,
    /// Delete the HUD group, trick analyzer and manual meter off, indicators and presentation groups removed.
    Cleanup,
    Complete(Completion),
}

/// Retail's distance beyond which a tutorial with `IgnoreInitialTutorialReposition` still repositions (b858).
pub const REPOSITION_DISTANCE: f32 = 10.0;
/// show_results waits this long (210 ticks at 60 Hz).
pub const RESULTS_SECONDS: f32 = 3.5;

#[derive(Clone, Debug, PartialEq)]
pub struct Tutorial {
    params: Params,
    state: State,
    pending: Vec<State>,
    timers: Timers,
    not_yet: bool,
    teleported: bool,
    restarted: bool,
    result: Completion,
    wait: Option<TimerId>,
    distance: f32,
}

impl Tutorial {
    pub fn new(params: Params) -> Self {
        Self { params, state: State::Initial, pending: Vec::new(), timers: Timers::default(), not_yet: true,
            teleported: false, restarted: false, result: Completion::Abandoned, wait: None, distance: 0.0 }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn finished(&self) -> bool {
        self.state == State::Complete
    }

    /// The player's distance from the row's StartLocation (AI.SkaterDistanceFromLocation), read on entering
    /// challenge_running.
    pub fn set_distance(&mut self, metres: f32) {
        self.distance = metres;
    }

    pub fn start(&mut self) -> Vec<Command> {
        let mut out = Vec::new();
        self.state = State::Initial;
        self.enter(&mut out);
        self.flush(&mut out);
        out
    }

    pub fn handle(&mut self, event: Event) -> Vec<Command> {
        let mut out = Vec::new();
        if !self.finished() {
            self.on(event, &mut out);
            self.flush(&mut out);
        }
        out
    }

    /// One 60 Hz tick of the challenge timers.
    pub fn tick(&mut self) -> Vec<Command> {
        let mut out = Vec::new();
        for id in self.timers.tick() {
            if self.state == State::ShowResults && Some(id) == self.wait {
                out.push(Command::ShowHud(Hud::OwnedIt, false));
                out.push(Command::PauseMenu(true));
                self.change(State::PlayPresEvent);
            }
            self.flush(&mut out);
        }
        out
    }

    fn change(&mut self, state: State) {
        self.pending.push(state);
    }

    fn flush(&mut self, out: &mut Vec<Command>) {
        while !self.pending.is_empty() {
            let next = self.pending.remove(0);
            if next == self.state {
                continue;
            }
            self.state = next;
            self.enter(out);
        }
    }

    /// The start block of challenge_running (both the no-teleport path and teleporttransitionfinished).
    fn begin_running(&mut self, out: &mut Vec<Command>) {
        out.push(Command::ShowHud(Hud::Objective, true));
        if self.restarted {
            out.push(Command::ResetObjectives);
        }
        out.push(Command::ActivateStartObjectives);
        out.push(Command::Evaluation(true));
        self.not_yet = false;
    }

    fn back_to_setup(&mut self, out: &mut Vec<Command>) {
        self.not_yet = true;
        self.restarted = true;
        out.push(Command::Evaluation(false));
        self.change(State::Setup);
    }

    fn enter(&mut self, out: &mut Vec<Command>) {
        match self.state {
            State::Initial => self.change(State::MakeHud),
            State::MakeHud => {
                out.push(Command::PauseMenu(false));
                out.push(Command::CreateHud);
            }
            State::MakeObjectives => {
                out.push(Command::MakeObjectives);
                out.push(Command::Evaluation(false));
                self.restarted = false;
                self.change(State::IntroPres);
            }
            State::IntroPres => {
                if self.params.intro {
                    out.push(Command::StartPresentation(Presentation::Intro));
                } else {
                    self.change(State::Setup);
                }
            }
            State::Setup => {
                out.push(Command::HomVisuals(false));
                self.not_yet = true;
                self.teleported = false;
                self.result = Completion::Abandoned;
                self.change(State::AssetLoad);
            }
            State::AssetLoad => out.push(Command::LoadAssets),
            State::ChallengeRunning => {
                out.push(Command::PauseMenu(true));
                out.push(Command::AddIndicators);
                if self.params.ignore_initial_reposition == Some(false) || self.distance > REPOSITION_DISTANCE {
                    out.push(Command::Reposition { offboard: self.params.spawn_offboard });
                    self.teleported = true;
                } else {
                    self.begin_running(out);
                    out.push(Command::FadeIn { seconds: 0.25 });
                }
                out.push(Command::NotifyRunning);
            }
            State::ShowResults => {
                out.push(Command::PauseMenu(false));
                out.push(Command::ShowHud(Hud::OwnedIt, true));
                self.wait = Some(self.timers.add_countdown(RESULTS_SECONDS));
            }
            State::PlayPresEvent => {
                if self.params.outro {
                    out.push(Command::StartPresentation(Presentation::Outro));
                } else {
                    self.change(State::EndOptions);
                }
            }
            State::EndOptions => {
                out.push(Command::LockProgressionPresentation(true));
                out.push(Command::OpenEndOptions);
            }
            State::Restarted => {
                out.push(Command::OpenLoadingScreen);
                if self.params.restart {
                    out.push(Command::StartPresentation(Presentation::Restart));
                } else {
                    self.back_to_setup(out);
                }
            }
            State::ChallengeCleanup => self.change(State::Complete),
            State::Complete => {
                out.push(Command::PauseMenu(true));
                out.push(Command::Cleanup);
                out.push(Command::HomVisuals(true));
                out.push(Command::Complete(self.result));
            }
        }
    }

    fn on(&mut self, event: Event, out: &mut Vec<Command>) {
        match (self.state, event) {
            (State::MakeHud, Event::HudLoaded) => self.change(State::MakeObjectives),
            (State::IntroPres, Event::PresentationFinished(Presentation::Intro)) => self.change(State::Setup),
            (State::AssetLoad, Event::AssetLoadComplete) => self.change(State::ChallengeRunning),
            (State::ChallengeRunning, Event::TeleportFinished) if self.teleported => {
                out.push(Command::CloseLoadingScreen);
                self.teleported = false;
                self.begin_running(out);
            }
            // No group or wipeout check, unlike own the spot.
            (State::ChallengeRunning, Event::AllObjectivesComplete) => {
                self.result = Completion::Owned;
                self.change(State::ShowResults);
            }
            // A failed objective re-arms; the tutorial never fails as a whole.
            (State::ChallengeRunning, Event::ObjectiveFailed(i)) if !self.not_yet => out.push(Command::ResetObjective(i)),
            (State::PlayPresEvent, Event::PresentationFinished(Presentation::Outro)) => self.change(State::EndOptions),
            (State::EndOptions, Event::HudRetry) => {
                out.push(Command::LockProgressionPresentation(false));
                self.change(State::Restarted);
            }
            // Retail does not unlock the presentation here (b858 open).
            (State::EndOptions, Event::HudContinue) => self.change(State::ChallengeCleanup),
            (State::Restarted, Event::PresentationFinished(Presentation::Restart)) => self.back_to_setup(out),
            // Global handlers.
            (_, Event::Abandon) => self.change(State::ChallengeCleanup),
            (_, Event::RestartChallenge) => self.change(State::Restarted),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(t: &mut Tutorial, events: &[Event]) -> Vec<Command> {
        events.iter().flat_map(|e| t.handle(*e)).collect()
    }

    #[test]
    fn a_tutorial_runs_to_owned_and_completes_on_continue() {
        let mut t = Tutorial::new(Params::default());
        t.set_distance(25.0);
        let start = t.start();
        assert!(start.contains(&Command::CreateHud));
        let out = run(&mut t, &[Event::HudLoaded, Event::AssetLoadComplete]);
        assert!(out.contains(&Command::Reposition { offboard: false }), "{out:?}");
        assert_eq!(t.state(), State::ChallengeRunning);
        let out = t.handle(Event::TeleportFinished);
        assert!(out.contains(&Command::ActivateStartObjectives) && out.contains(&Command::Evaluation(true)));
        assert!(t.handle(Event::ObjectiveFailed(2)).contains(&Command::ResetObjective(2)));
        t.handle(Event::AllObjectivesComplete);
        assert_eq!(t.state(), State::ShowResults);
        let mut out = Vec::new();
        for _ in 0..Timers::ticks(RESULTS_SECONDS) + 2 {
            out.extend(t.tick());
        }
        assert!(out.contains(&Command::OpenEndOptions), "no outro: straight to the end options");
        assert_eq!(t.handle(Event::HudContinue).last(), Some(&Command::Complete(Completion::Owned)));
        assert!(t.finished());
    }

    #[test]
    fn near_the_start_it_starts_in_place_and_retry_resets() {
        let mut t = Tutorial::new(Params::default());
        t.set_distance(4.0);
        t.start();
        let out = run(&mut t, &[Event::HudLoaded, Event::AssetLoadComplete]);
        assert!(out.contains(&Command::FadeIn { seconds: 0.25 }) && !out.iter().any(|c| matches!(c, Command::Reposition { .. })));
        assert!(!out.contains(&Command::ResetObjectives), "first run: no reset");
        t.handle(Event::AllObjectivesComplete);
        for _ in 0..Timers::ticks(RESULTS_SECONDS) + 2 {
            t.tick();
        }
        let out = run(&mut t, &[Event::HudRetry, Event::AssetLoadComplete]);
        assert!(out.contains(&Command::ResetObjectives), "a retry resets the objectives: {out:?}");
    }

    #[test]
    fn an_explicit_false_always_repositions() {
        let mut t = Tutorial::new(Params { ignore_initial_reposition: Some(false), ..Default::default() });
        t.start();
        let out = run(&mut t, &[Event::HudLoaded, Event::AssetLoadComplete]);
        assert!(out.iter().any(|c| matches!(c, Command::Reposition { .. })));
    }

    #[test]
    fn abandoning_completes_as_abandoned() {
        let mut t = Tutorial::new(Params::default());
        t.start();
        assert_eq!(t.handle(Event::Abandon).last(), Some(&Command::Complete(Completion::Abandoned)));
    }
}
