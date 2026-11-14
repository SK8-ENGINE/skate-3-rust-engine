//! Hall of Meat challenges, re-implemented from the retail state graph `hom_challenge` (research b1005, b876, b967;
//! doc 42 "Hall of Meat"). Single player, one competitor; the per-competitor values sit in [`Competitor`] so an
//! online authority can hold one per skater. Same runner rules as the other machines: deferred state changes, enter
//! on flush. The bail score, the objective API (ObjectiveHOM) and the homscoring HUD are the host's; this machine
//! reads the bail's total at WipeoutOver and the groups' completions.

use super::timers::{TimerId, Timers};
use super::Completion;

/// One row's values (setup export or a mod) and the graph's constants as overridable defaults (b1005 §3, §6).
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    /// TimeLimit in seconds; None = the row sets none.
    pub time_limit: Option<f32>,
    /// DisableTimeLimitForSinglePlayer: offline runs count up instead (setup_code_specific@67-75).
    pub disable_time_limit_for_single_player: bool,
    pub single_player: bool,
    pub boundary: Option<u64>,
    pub has_killed_group: bool,
    /// The row's stored result: a replay of an owned / killed row with a killed group shows only the killed group.
    pub stored: Completion,
    pub disable_hom: bool,
    pub intro_nis: bool,
    pub intro_presentation: bool,
    pub outro_presentation: bool,
    /// Retail single player: a bail of an unfinished run ends with a teleport to the start (wipeout@46-77,
    /// wipeoutover@131-180). A mod can switch it off.
    pub restart_on_wipeout: bool,
    pub countdown_delay: f32,
    pub out_of_bounds_time: f32,
    pub owned_display_time: f32,
    pub killed_display_time: f32,
    pub hud_countdown: f32,
    pub owned_pending: f32,
    pub time_up_delay: f32,
    pub asset_wait: f32,
    pub load_delay: f32,
}

impl Params {
    pub fn retail() -> Self {
        Self {
            time_limit: None,
            disable_time_limit_for_single_player: false,
            single_player: true,
            boundary: None,
            has_killed_group: false,
            stored: Completion::Abandoned,
            disable_hom: false,
            intro_nis: false,
            intro_presentation: false,
            outro_presentation: false,
            restart_on_wipeout: true,
            countdown_delay: 1.5,
            out_of_bounds_time: 5.0,
            owned_display_time: 3.1,
            killed_display_time: 3.3,
            hud_countdown: 3.0,
            owned_pending: 0.5,
            time_up_delay: 1.0,
            asset_wait: 0.5,
            load_delay: 1.0,
        }
    }

    /// `UsingTimeLimit = (SinglePlayer == false) or (DisableTimeLimitForSinglePlayer == false)`, and a row without a
    /// TimeLimit has none.
    pub fn time_limited(&self) -> bool {
        self.time_limit.is_some() && (!self.single_player || !self.disable_time_limit_for_single_player)
    }

    /// `ShowKilledItObj`: a replay of an owned or killed row that has a killed group.
    pub fn show_killed_it(&self) -> bool {
        self.has_killed_group && matches!(self.stored, Completion::Owned | Completion::Killed)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    Initial,
    SetupCodeCommon,
    SetupCodeSpecific,
    WaitForAssetLoad,
    DynamicCharacterNis,
    IntroPresentation,
    SetupChallengeCommon,
    SetupChallengeSpecific,
    Countdown,
    RunChallenge,
    EndChallenge,
    CalculateScores,
    Highlight,
    OutroPresentation,
    Results,
    Cleanup,
    Complete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HudComponent {
    MiniMap,
    TrickDisplay,
    Objective,
    Dpo,
    Timer,
    Countdown,
    OwnedIt,
    KilledIt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Presentation {
    Intro,
    Outro,
}

/// Why a competitor finished (FinishReason).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Finish {
    Normal,
    TimeUp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    HudLoaded,
    NisComplete,
    PresentationFinished,
    TeleportFinished,
    /// The bail began (the host keeps its first replay frame for the highlight).
    Wipeout,
    /// The bail closed with its Hall of Meat total (Trick.GetHOMScore).
    WipeoutOver { score: i32 },
    UseMarker,
    /// The shown group's accumulated points.
    ObjectivePointsAdded { points: i32 },
    ObjectiveComplete,
    AllObjectivesComplete { killed: bool },
    DidNotGetEnoughPoints { killed: bool },
    TriggerExit { volume: u64 },
    TriggerEnter { volume: u64 },
    HighlightReelComplete,
    Continue,
    Retry,
    Restart,
    Abandon,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    PinPlayer(bool),
    /// EnableHOMVisuals(false) for a row with DisableHOM.
    DisableHomVisuals,
    CreateHud,
    ShowHud(HudComponent, bool),
    /// HUD2.SetHOMHudGroup: true = the challenge's group, false = back to "main".
    HomHudInChallenge(bool),
    EnterNis,
    StartPresentation(Presentation),
    RemoveAllHighlights,
    /// RepositionAI(player, StartLocation, StartLocator).
    RepositionToStart,
    ResetObjectives,
    Evaluation(bool),
    CloseLoadingScreen,
    /// The start is the reset marker.
    SetSessionMarker,
    StartCountdownHud { ticks: i32 },
    StartTimerHud { ticks: i32 },
    PauseMenu(bool),
    NotifyRunning,
    ReturnToChallengeArea,
    /// The red C.ModifiedCentralMessageHALIDScoredLow.
    ScoredLow,
    /// The highlight becomes the bail that just closed (single player: from its first frame to the last written).
    HighlightFromBail,
    /// No highlight yet: one from C.HighlightDefinition (the last 180 frames).
    HighlightFromDefinition,
    PlayHighlightReel,
    Dpo { score: i32 },
    ClearTeleports,
    Scoring(bool),
    OpenContinueRetry,
    /// C.EndTeleport -> RepositionAI(C.Location).
    EndTeleport,
    Complete(Completion),
}

/// The graph's per-competitor values (Competitor[i]).
#[derive(Clone, Debug, PartialEq)]
pub struct Competitor {
    pub complete: bool,
    pub result: Completion,
    /// Seconds from the run's start to the finish; -1 until finished.
    pub time: f32,
    pub highest_score: i32,
    pub score: i32,
    pub should_restart_on_wipeout: bool,
    pub finish: Option<Finish>,
    wiping_out: bool,
    has_highlight: bool,
    handled: bool,
}

impl Competitor {
    fn fresh() -> Self {
        Self { complete: false, result: Completion::Abandoned, time: -1.0, highest_score: 0, score: 0,
            should_restart_on_wipeout: false, finish: None, wiping_out: false, has_highlight: false, handled: false }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Ids {
    asset_wait: Option<TimerId>,
    load_delay: Option<TimerId>,
    countdown_delay: Option<TimerId>,
    countdown: Option<TimerId>,
    challenge: Option<TimerId>,
    owned_pending: Option<TimerId>,
    time_up: Option<TimerId>,
    close_overlay: Option<TimerId>,
    end_challenge: Option<TimerId>,
    out_of_bounds: Option<TimerId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HomRun {
    params: Params,
    state: State,
    pending: Vec<State>,
    timers: Timers,
    ids: Ids,
    competitor: Competitor,
    elapsed_ticks: i32,
    result: Completion,
}

impl HomRun {
    pub fn new(params: Params) -> Self {
        Self { params, state: State::Initial, pending: Vec::new(), timers: Timers::default(), ids: Ids::default(),
            competitor: Competitor::fresh(), elapsed_ticks: 0, result: Completion::Abandoned }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn competitor(&self) -> &Competitor {
        &self.competitor
    }

    /// The run's result for the career (ChallengeCompleteResult).
    pub fn result(&self) -> Completion {
        self.result
    }

    pub fn finished(&self) -> bool {
        self.state == State::Complete
    }

    /// The challenge clock's ticks left on a timed row.
    pub fn time_left_ticks(&self) -> Option<i32> {
        self.ids.challenge.and_then(|id| self.timers.remaining(id))
    }

    /// Ticks since the run started (the count-up, read for the result time).
    pub fn elapsed_ticks(&self) -> i32 {
        self.elapsed_ticks
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
            self.on_event(event, &mut out);
            self.flush(&mut out);
        }
        out
    }

    /// One 60 Hz tick of the challenge timers.
    pub fn tick(&mut self) -> Vec<Command> {
        let mut out = Vec::new();
        if self.state == State::RunChallenge && !self.competitor.complete {
            self.elapsed_ticks += 1;
        }
        for id in self.timers.tick() {
            if self.finished() {
                break;
            }
            self.on_timer(id, &mut out);
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

    fn add(&mut self, seconds: f32) -> Option<TimerId> {
        Some(self.timers.add_countdown(seconds))
    }

    fn remove(&mut self, id: Option<TimerId>) {
        if let Some(id) = id {
            self.timers.remove(id);
        }
    }

    fn enter(&mut self, out: &mut Vec<Command>) {
        match self.state {
            State::Initial => self.change(State::SetupCodeCommon),
            State::SetupCodeCommon => {
                out.push(Command::PinPlayer(true));
                if self.params.disable_hom {
                    out.push(Command::DisableHomVisuals);
                }
                self.change(State::SetupCodeSpecific);
            }
            State::SetupCodeSpecific => out.push(Command::CreateHud),
            State::WaitForAssetLoad => self.ids.asset_wait = self.add(self.params.asset_wait),
            State::DynamicCharacterNis => {
                if self.params.intro_nis {
                    out.push(Command::EnterNis);
                } else {
                    self.change(State::IntroPresentation);
                }
            }
            State::IntroPresentation => {
                if self.params.intro_presentation {
                    out.push(Command::StartPresentation(Presentation::Intro));
                } else {
                    self.change(State::SetupChallengeCommon);
                }
            }
            State::SetupChallengeCommon => {
                self.competitor.score = 0;
                out.extend([Command::RemoveAllHighlights, Command::RepositionToStart, Command::PinPlayer(true),
                    Command::ResetObjectives, Command::Evaluation(false)]);
            }
            State::SetupChallengeSpecific => {
                self.competitor = Competitor::fresh();
                self.result = Completion::Abandoned;
                self.elapsed_ticks = 0;
                out.extend([Command::Dpo { score: 0 }, Command::ShowHud(HudComponent::OwnedIt, false),
                    Command::ShowHud(HudComponent::KilledIt, false), Command::CloseLoadingScreen]);
                self.change(State::Countdown);
            }
            State::Countdown => {
                out.push(Command::SetSessionMarker);
                self.ids.countdown_delay = self.add(self.params.countdown_delay);
            }
            State::RunChallenge => {
                out.push(Command::PinPlayer(false));
                if self.params.time_limited() {
                    let limit = self.params.time_limit.unwrap_or_default();
                    self.ids.challenge = self.add(limit);
                    out.push(Command::StartTimerHud { ticks: Timers::ticks(limit) });
                }
                out.extend([Command::Evaluation(true), Command::PauseMenu(true), Command::NotifyRunning]);
            }
            State::EndChallenge => {
                out.extend([Command::PauseMenu(false), Command::ClearTeleports, Command::Scoring(false),
                    Command::PinPlayer(true), Command::Evaluation(false), Command::ShowHud(HudComponent::Timer, false)]);
                self.change(State::CalculateScores);
            }
            State::CalculateScores => self.change(State::Highlight),
            State::Highlight => {
                if !self.competitor.has_highlight {
                    self.competitor.has_highlight = true;
                    out.push(Command::HighlightFromDefinition);
                }
                out.push(Command::PlayHighlightReel);
            }
            State::OutroPresentation => {
                if self.params.outro_presentation {
                    out.push(Command::StartPresentation(Presentation::Outro));
                } else {
                    self.change(State::Results);
                }
            }
            State::Results => {
                self.result = self.competitor.result;
                out.push(Command::OpenContinueRetry);
            }
            State::Cleanup => {
                out.extend([Command::PinPlayer(false), Command::HomHudInChallenge(false), Command::EndTeleport]);
                self.change(State::Complete);
            }
            State::Complete => out.push(Command::Complete(self.result)),
        }
    }

    fn on_event(&mut self, event: Event, out: &mut Vec<Command>) {
        match event {
            Event::Abandon => {
                self.result = Completion::Abandoned;
                self.competitor.result = Completion::Abandoned;
                self.change(State::Cleanup);
                return;
            }
            Event::Restart => {
                self.clear_run_timers();
                self.change(State::SetupChallengeCommon);
                return;
            }
            _ => {}
        }
        match (self.state, event) {
            (State::SetupCodeSpecific, Event::HudLoaded) => {
                out.extend([Command::ShowHud(HudComponent::MiniMap, true), Command::ShowHud(HudComponent::TrickDisplay, false),
                    Command::HomHudInChallenge(true), Command::ShowHud(HudComponent::Objective, true),
                    Command::ShowHud(HudComponent::Dpo, true)]);
                self.change(State::WaitForAssetLoad);
            }
            (State::DynamicCharacterNis, Event::NisComplete) => self.change(State::IntroPresentation),
            (State::IntroPresentation, Event::PresentationFinished) => self.change(State::SetupChallengeCommon),
            (State::SetupChallengeCommon, Event::TeleportFinished) => self.ids.load_delay = self.add(self.params.load_delay),
            (State::RunChallenge, e) => self.running(e, out),
            (State::Highlight, Event::HighlightReelComplete) => self.change(State::OutroPresentation),
            (State::OutroPresentation, Event::PresentationFinished) => self.change(State::Results),
            (State::Results, Event::Continue) => self.change(State::Cleanup),
            (State::Results, Event::Retry) => self.change(State::SetupChallengeCommon),
            _ => {}
        }
    }

    fn running(&mut self, event: Event, out: &mut Vec<Command>) {
        let c = &mut self.competitor;
        match event {
            Event::Wipeout => {
                c.wiping_out = true;
                if !c.complete && self.params.single_player && self.params.restart_on_wipeout {
                    c.should_restart_on_wipeout = true;
                }
            }
            Event::WipeoutOver { score } => {
                c.wiping_out = false;
                if c.highest_score < score {
                    c.highest_score = score;
                    if !c.complete {
                        c.has_highlight = true;
                        out.push(Command::HighlightFromBail);
                    }
                }
                if c.should_restart_on_wipeout && !c.complete {
                    out.push(Command::RepositionToStart);
                }
                c.should_restart_on_wipeout = false;
            }
            Event::UseMarker => c.should_restart_on_wipeout = false,
            Event::ObjectivePointsAdded { points } => {
                if points > c.score {
                    c.score = points;
                    out.push(Command::Dpo { score: points });
                }
            }
            Event::AllObjectivesComplete { killed: false } => {
                if !self.params.show_killed_it() && !c.complete {
                    c.complete = true;
                    c.time = self.elapsed_ticks as f32 / 60.0;
                    if c.result != Completion::Killed {
                        c.result = Completion::Owned;
                    }
                    c.has_highlight = true;
                    out.push(Command::HighlightFromBail);
                    self.ids.owned_pending = self.add(self.params.owned_pending);
                }
            }
            Event::AllObjectivesComplete { killed: true } => {
                if !c.handled {
                    let pending = self.ids.owned_pending.take();
                    self.remove(pending);
                    let c = &mut self.competitor;
                    c.complete = true;
                    c.result = Completion::Killed;
                    if c.time < 0.0 {
                        c.time = self.elapsed_ticks as f32 / 60.0;
                    }
                    c.has_highlight = true;
                    out.push(Command::HighlightFromBail);
                    self.challenge_complete(Some(Completion::Killed), out);
                }
            }
            Event::DidNotGetEnoughPoints { killed: false } => {
                if !c.complete {
                    c.complete = true;
                    out.push(Command::ScoredLow);
                    self.challenge_complete(None, out);
                }
            }
            Event::TriggerExit { volume } if Some(volume) == self.params.boundary => {
                if self.ids.out_of_bounds.is_none() {
                    self.ids.out_of_bounds = self.add(self.params.out_of_bounds_time);
                    out.push(Command::ReturnToChallengeArea);
                }
            }
            Event::TriggerEnter { volume } if Some(volume) == self.params.boundary => {
                let id = self.ids.out_of_bounds.take();
                self.remove(id);
            }
            _ => {}
        }
    }

    /// HandleChallengeComplete (setup_code_specific closure 3): the banner for an owned or killed finish, the overlay
    /// closes after its display time, evaluation stops, and with every competitor done the challenge ends after it.
    fn challenge_complete(&mut self, banner: Option<Completion>, out: &mut Vec<Command>) {
        let c = &mut self.competitor;
        if c.handled {
            return;
        }
        c.handled = true;
        c.finish.get_or_insert(Finish::Normal);
        let longest = self.params.owned_display_time.max(self.params.killed_display_time);
        let shown = match banner {
            Some(Completion::Killed) => {
                out.push(Command::ShowHud(HudComponent::KilledIt, true));
                if self.params.single_player { self.params.killed_display_time } else { longest }
            }
            Some(Completion::Owned) => {
                out.push(Command::ShowHud(HudComponent::OwnedIt, true));
                self.params.owned_display_time
            }
            _ => longest,
        };
        out.extend([Command::PauseMenu(false), Command::Evaluation(false)]);
        self.ids.close_overlay = self.add(shown);
        self.ids.end_challenge = self.add(shown);
        let id = self.ids.challenge.take();
        self.remove(id);
    }

    fn clear_run_timers(&mut self) {
        let ids = std::mem::take(&mut self.ids);
        for id in [ids.asset_wait, ids.load_delay, ids.countdown_delay, ids.countdown, ids.challenge, ids.owned_pending,
            ids.time_up, ids.close_overlay, ids.end_challenge, ids.out_of_bounds] {
            self.remove(id);
        }
    }

    fn on_timer(&mut self, id: TimerId, out: &mut Vec<Command>) {
        let is = |slot: Option<TimerId>| slot == Some(id);
        if is(self.ids.asset_wait) {
            self.ids.asset_wait = None;
            self.change(State::DynamicCharacterNis);
        } else if is(self.ids.load_delay) {
            self.ids.load_delay = None;
            self.change(State::SetupChallengeSpecific);
        } else if is(self.ids.countdown_delay) {
            self.ids.countdown_delay = None;
            out.push(Command::StartCountdownHud { ticks: Timers::ticks(self.params.hud_countdown) });
            self.ids.countdown = self.add(self.params.hud_countdown);
        } else if is(self.ids.countdown) {
            self.ids.countdown = None;
            self.change(State::RunChallenge);
        } else if is(self.ids.challenge) {
            self.ids.challenge = None;
            let c = &mut self.competitor;
            if !c.complete {
                c.finish = Some(Finish::TimeUp);
                out.push(Command::PinPlayer(true));
                if c.wiping_out {
                    out.push(Command::RepositionToStart);
                }
                self.ids.time_up = self.add(self.params.time_up_delay);
            }
        } else if is(self.ids.time_up) {
            self.ids.time_up = None;
            self.change(State::EndChallenge);
        } else if is(self.ids.owned_pending) {
            self.ids.owned_pending = None;
            self.challenge_complete(Some(Completion::Owned), out);
        } else if is(self.ids.close_overlay) {
            self.ids.close_overlay = None;
            out.extend([Command::PauseMenu(true), Command::ShowHud(HudComponent::KilledIt, false),
                Command::ShowHud(HudComponent::OwnedIt, false)]);
        } else if is(self.ids.end_challenge) {
            self.ids.end_challenge = None;
            if !self.competitor.has_highlight {
                self.competitor.has_highlight = true;
                out.push(Command::HighlightFromDefinition);
            }
            self.change(State::EndChallenge);
        } else if is(self.ids.out_of_bounds) {
            self.ids.out_of_bounds = None;
            out.push(Command::RepositionToStart);
        }
    }
}

#[cfg(test)]
#[path = "hom_run_tests.rs"]
mod tests;
