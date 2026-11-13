//! Films, anywhere films and follow the leader, re-implemented from the retail state graph `objectives_shared`
//! (research b875, follow the leader b1006; doc 42 "Films and follow the leader: the shared machine"). Same runner rules
//! as own the spot (`ots.rs`, b832): deferred state changes, enter on flush, state handlers before global ones.
//! Not ported here: SimulPlay (online), the photo graph's display_photo / shot picker (photo_skate2), presentation
//! content (the host plays it and answers with events).

use super::timers::{TimerId, Timers};
use super::{Completion, Flow};

/// `Challenge.GetChallengeType` for the families this graph runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// 7 (films) and spot films.
    Film,
    SpotFilm,
    /// 0x27: no boundary, starts where the player is; the start becomes the session marker.
    AnywhereFilm,
    /// 0x17: follow the leader.
    Follow,
}

impl Kind {
    fn film(self) -> bool {
        matches!(self, Kind::Film | Kind::SpotFilm | Kind::AnywhereFilm)
    }
    fn anywhere(self) -> bool {
        self == Kind::AnywhereFilm
    }
}

/// The per-type rule flags `setup` sets (b875 section 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rules {
    pub fail_on_wipeout: bool,
    pub fail_on_objective_fail: bool,
    pub fail_outside_boundary: bool,
    pub no_boundary: bool,
}

impl Rules {
    pub fn retail(kind: Kind) -> Self {
        match kind {
            Kind::AnywhereFilm => Self { fail_on_wipeout: true, fail_on_objective_fail: true, fail_outside_boundary: false, no_boundary: true },
            _ => Self { fail_on_wipeout: true, fail_on_objective_fail: true, fail_outside_boundary: true, no_boundary: false },
        }
    }
}

/// One challenge's values: row params (setup export or a mod) and the graph's constants as overridable defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    pub kind: Kind,
    pub rules: Rules,
    pub competitors: u32,
    pub intro_events: bool,
    pub boundary: Option<u64>,
    pub ftl_end_volume: Option<u64>,
    pub countdown_delay: f32,
    pub prefilm_time: f32,
    pub total_film_time: f32,
    /// FilmTimerLowLimit: the low-time speech this many seconds before TotalFilmTime; 0 = the row does not set it (no
    /// warning: anywhere films, b1003 §4).
    pub film_low_limit: f32,
    /// SimulPlay: the film timer (TotalFilmTime, its HUD clock and its time-up loss) runs only in simul play; offline
    /// films have no time limit (countdown.timerexpired@19-27, b999 §0.1, b1003 §4). A mod can switch the timed
    /// variant on for single player.
    pub simul_play: bool,
    pub out_of_bounds_time: f32,
    pub owned_display_time: f32,
    pub killed_display_time: f32,
    pub allow_player_bail: bool,
    pub allow_objective_failure: bool,
    pub allow_session_markers: bool,
    pub reset_objectives_on_marker: bool,
    pub reset_ai_on_bail: bool,
    pub reset_ai_on_marker: bool,
    pub use_line_timer: bool,
    /// Graph constants (b875 / b1006): the FTL window after the leader finishes, the owned banner's wait for a
    /// killed completion, the wipeout and objective-failed re-arm delays, the AI restart delays, the asset poll.
    pub ftl_window: f32,
    pub owned_pending: f32,
    pub wipeout_delay: f32,
    pub objective_failed_delay: f32,
    pub ai_restart_delay: f32,
    pub marker_reset_delay: f32,
    pub marker_ai_restart_delay: f32,
    pub ai_poll: f32,
    pub central_message_delay: f32,
}

impl Params {
    /// A row's family defaults (b875 section 2); the row then overrides what it sets.
    pub fn retail(kind: Kind) -> Self {
        let (film, prefilm) = match kind {
            Kind::Film | Kind::SpotFilm => (180.0, 0.0),
            Kind::AnywhereFilm => (30.0, 1.0),
            Kind::Follow => (0.0, 0.0),
        };
        Self {
            kind,
            rules: Rules::retail(kind),
            competitors: u32::from(kind == Kind::Follow),
            intro_events: false,
            boundary: None,
            ftl_end_volume: None,
            countdown_delay: 1.5,
            prefilm_time: prefilm,
            total_film_time: film,
            film_low_limit: if matches!(kind, Kind::Film | Kind::SpotFilm) { 5.0 } else { 0.0 },
            simul_play: false,
            out_of_bounds_time: 5.0,
            owned_display_time: 3.1,
            killed_display_time: 3.3,
            allow_player_bail: kind == Kind::Follow,
            allow_objective_failure: kind == Kind::AnywhereFilm,
            allow_session_markers: kind.film(),
            reset_objectives_on_marker: kind.film(),
            reset_ai_on_bail: true,
            reset_ai_on_marker: true,
            use_line_timer: false,
            ftl_window: 10.0,
            owned_pending: 0.5,
            wipeout_delay: 1.5,
            objective_failed_delay: 1.5,
            ai_restart_delay: 1.0,
            marker_reset_delay: 1.0,
            marker_ai_restart_delay: 0.2,
            ai_poll: 0.25,
            central_message_delay: 1.5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    Initial,
    PinPlayer,
    MakeObjectives,
    PrefilmDisplay,
    NisOverview,
    Setup,
    WaitForAssetLoad,
    SpawnPlayer,
    CheckAi,
    Wait,
    Countdown,
    ChallengeRunning,
    ChallengeCleanup,
    KilledIt,
    HighlightReel,
    EndOptions,
    Restarted,
    Complete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HudComponent {
    Countdown,
    Timer,
    Objective,
    OwnedIt,
    KilledIt,
    DistanceMeter,
}

/// The central messages (`ID_CHALLENGE_CENTRAL_MESSAGE_OBJECTIVE_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Message {
    TimeUp,
    OutOfBounds,
    ReturnToArea,
    ScoredLow,
    LineTimerExpired,
    ChallengeFailed,
    /// The objective's own failure text (GetObjectiveFailureHALText), when it has one.
    ObjectiveFailure,
}

/// The speech hooks the graph plays (row speech ids; the host maps them).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Speech {
    Setup,
    Restart,
    LoseTimerExpired,
    TimeLimitLow,
    NotEnoughPoints,
    Wipeout,
    LineTimerExpired,
    LeftChallenge,
    CompleteAnObjective,
    Succeed,
    ReinforceOnFail,
    BeginReplay,
    Won,
    FailedAttemptOneObjective,
    Failed,
    Bailed,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    HudLoaded,
    PresentationFinished,
    AssetsLoaded,
    /// check_ai: whether every competitor skater exists.
    AiExist(bool),
    TeleportFinished,
    /// A skater entered a volume: `competitor` 0 = the local player, 1.. = AI competitors.
    TriggerEnter { volume: u64, competitor: u32 },
    TriggerExit { volume: u64 },
    ObjectiveComplete { killed: bool, last: bool },
    AllObjectivesComplete { killed: bool },
    ObjectiveFailed,
    NotEnoughPoints,
    Wipeout,
    LineTimerStarted,
    LineTimerExpired,
    UseMarker,
    NisComplete,
    HighlightReelComplete,
    Retry,
    Continue,
    RestartChallenge,
    Abandon,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    PinPlayer(bool),
    PauseMenu(bool),
    SessionMarkerUse(bool),
    SessionMarkerSet(bool),
    /// At the start, the anywhere types set the session marker where the player stands; a restart goes back to it.
    SetSessionMarkerHere,
    TeleportToSessionMarker,
    CreateHud { distance_meter: bool },
    ShowHud(HudComponent, bool),
    StartPresentation,
    LoadAiPaths,
    SpawnAi,
    PinAi(bool),
    StartAiPaths,
    RespawnAiAtStart,
    RepositionPlayer,
    /// The leader reached the end: it stops (AI.PinAI(leader)).
    PinLeader,
    DistanceUpdates(bool),
    CloseLoadingScreen,
    StartCountdownHud { ticks: i32 },
    StartTimerHud { ticks: i32 },
    Evaluation(bool),
    ActivateStartObjectives,
    ResetObjectives,
    FailAllObjectives,
    RemoveAllHighlights,
    CentralMessage(Message),
    Speech(Speech),
    KilledItEffect(bool),
    PauseSimulation(bool),
    PlayHighlightReel,
    OpenContinueRetry,
    NotifyRunning,
    Complete(Completion),
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Ids {
    countdown: Option<TimerId>,
    countdown_delay: Option<TimerId>,
    film: Option<TimerId>,
    warning: Option<TimerId>,
    out_of_bounds: Option<TimerId>,
    not_enough_points: Option<TimerId>,
    owned_pending: Option<TimerId>,
    wipeout: Option<TimerId>,
    objective_failed: Option<TimerId>,
    restart_ai_path: Option<TimerId>,
    session_reset: Option<TimerId>,
    ftl_remaining: Option<TimerId>,
    central_message_delay: Option<TimerId>,
    replay_wait: Option<TimerId>,
    wait: Option<TimerId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FilmRun {
    params: Params,
    state: State,
    pending: Vec<State>,
    timers: Timers,
    ids: Ids,
    first_run: bool,
    restarted: bool,
    locked_out: bool,
    result: Flow,
    complete_result: Completion,
    wipe_out: bool,
    film_timer_expired: bool,
    all_complete: bool,
    completed_one: bool,
    objective_fail: bool,
    out_of_bound: bool,
    line_timer_started: bool,
}

impl FilmRun {
    pub fn new(params: Params) -> Self {
        Self {
            params,
            state: State::Initial,
            pending: Vec::new(),
            timers: Timers::default(),
            ids: Ids::default(),
            first_run: true,
            restarted: false,
            locked_out: false,
            result: Flow::Lose,
            complete_result: Completion::Abandoned,
            wipe_out: false,
            film_timer_expired: false,
            all_complete: false,
            completed_one: false,
            objective_fail: false,
            out_of_bound: false,
            line_timer_started: false,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn result(&self) -> Flow {
        self.result
    }

    pub fn finished(&self) -> bool {
        self.state == State::Complete
    }

    /// The film timer's ticks left while it runs (the HUD timer reads it).
    pub fn film_ticks(&self) -> Option<i32> {
        self.ids.film.and_then(|id| self.timers.remaining(id))
    }

    /// The follow the leader window's ticks left after the leader finished.
    pub fn ftl_ticks(&self) -> Option<i32> {
        self.ids.ftl_remaining.and_then(|id| self.timers.remaining(id))
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

    /// `ChallengeResultSelector` (b875): the film types end only with every objective complete or the film timer out,
    /// and otherwise repair and go on; follow the leader ends unless it has a boundary the player left [inferred].
    fn selector(&mut self, out: &mut Vec<Command>) -> bool {
        if self.params.kind.film() {
            if self.all_complete || self.film_timer_expired {
                return true;
            }
            if self.out_of_bound {
                out.push(Command::RepositionPlayer);
                self.out_of_bound = false;
            }
            self.wipe_out = false;
            return false;
        }
        !(self.params.boundary.is_some() && self.out_of_bound)
    }

    fn enter(&mut self, out: &mut Vec<Command>) {
        let p = self.params.clone();
        match self.state {
            State::Initial => self.change(State::PinPlayer),
            State::PinPlayer => {
                out.push(Command::PinPlayer(true));
                out.push(Command::PauseMenu(false));
                out.push(Command::SessionMarkerUse(false));
                self.change(State::MakeObjectives);
            }
            State::MakeObjectives => {
                out.push(Command::Evaluation(false));
                self.change(State::PrefilmDisplay);
            }
            State::PrefilmDisplay => {
                out.push(Command::CreateHud { distance_meter: p.kind == Kind::Follow });
                self.out_of_bound = false;
                self.film_timer_expired = false;
            }
            State::NisOverview => {
                if p.intro_events {
                    out.push(Command::StartPresentation);
                } else {
                    self.change(State::Setup);
                }
            }
            State::Setup => {
                let timers = std::mem::take(&mut self.ids);
                for id in [timers.countdown, timers.countdown_delay, timers.out_of_bounds, timers.warning, timers.central_message_delay, timers.replay_wait, timers.not_enough_points, timers.owned_pending, timers.restart_ai_path, timers.ftl_remaining] {
                    self.remove(id);
                }
                self.ids.film = timers.film;
                if p.competitors > 0 {
                    out.push(Command::LoadAiPaths);
                }
                self.line_timer_started = false;
                self.result = Flow::Lose;
                self.wipe_out = false;
                self.film_timer_expired = false;
                self.all_complete = false;
                self.completed_one = false;
                self.objective_fail = false;
                self.complete_result = Completion::Abandoned;
                if p.allow_session_markers {
                    out.push(Command::SessionMarkerSet(true));
                }
                self.change(State::WaitForAssetLoad);
            }
            State::WaitForAssetLoad => {}
            State::SpawnPlayer => {
                if self.first_run && p.competitors > 0 {
                    out.push(Command::SpawnAi);
                    self.change(State::CheckAi);
                } else {
                    self.change(State::Countdown);
                }
            }
            State::CheckAi => {}
            State::Wait => self.ids.wait = self.add(p.ai_poll),
            State::Countdown => {
                out.push(Command::PauseMenu(true));
                self.locked_out = false;
                out.push(Command::ShowHud(HudComponent::Objective, true));
                if p.kind.anywhere() {
                    out.push(if self.restarted { Command::TeleportToSessionMarker } else { Command::SetSessionMarkerHere });
                    out.push(Command::CloseLoadingScreen);
                    self.ids.countdown = self.add(0.0);
                } else {
                    out.push(Command::RepositionPlayer);
                }
                if self.restarted && p.competitors > 0 {
                    out.push(Command::RespawnAiAtStart);
                }
                if p.kind == Kind::Follow {
                    out.push(Command::ShowHud(HudComponent::DistanceMeter, true));
                    out.push(Command::DistanceUpdates(true));
                }
            }
            State::ChallengeRunning => {
                out.push(Command::SessionMarkerUse(true));
                out.push(Command::NotifyRunning);
            }
            State::ChallengeCleanup => {
                let film = self.ids.film.take();
                self.remove(film);
                out.push(Command::Evaluation(false));
                if p.kind == Kind::Follow {
                    out.push(Command::DistanceUpdates(false));
                    out.push(Command::ShowHud(HudComponent::DistanceMeter, false));
                    out.push(Command::ShowHud(HudComponent::Timer, false));
                }
                if self.result == Flow::Lose {
                    out.push(Command::CentralMessage(Message::ChallengeFailed));
                }
                out.push(Command::SessionMarkerSet(false));
                self.change(State::KilledIt);
            }
            State::KilledIt => {
                out.push(Command::PauseMenu(false));
                out.push(Command::PinPlayer(true));
                match self.result {
                    Flow::Lose => self.ids.central_message_delay = self.add(p.central_message_delay),
                    Flow::Kill => self.ids.replay_wait = self.add(p.killed_display_time),
                    _ => self.ids.replay_wait = self.add(p.owned_display_time),
                }
            }
            State::HighlightReel => {
                out.push(Command::Speech(Speech::BeginReplay));
                out.push(Command::PlayHighlightReel);
            }
            State::EndOptions => {
                out.push(Command::PauseMenu(true));
                out.push(Command::Speech(match self.result {
                    Flow::Win | Flow::Kill => Speech::Won,
                    _ if self.wipe_out => Speech::Bailed,
                    _ if self.film_timer_expired => Speech::LoseTimerExpired,
                    _ if self.completed_one || self.objective_fail => Speech::FailedAttemptOneObjective,
                    _ => Speech::Failed,
                }));
                out.push(Command::OpenContinueRetry);
            }
            State::Restarted => {
                for id in [self.ids.film.take(), self.ids.warning.take()] {
                    self.remove(id);
                }
                out.push(Command::ResetObjectives);
                out.push(Command::Evaluation(false));
                out.push(Command::RemoveAllHighlights);
                self.restarted = true;
                self.first_run = false;
                out.push(Command::PinPlayer(true));
                out.push(Command::PinAi(true));
                out.push(Command::ShowHud(HudComponent::Objective, false));
                out.push(Command::PauseMenu(false));
                out.push(Command::SessionMarkerUse(false));
                self.change(State::Setup);
            }
            State::Complete => {
                let film = self.ids.film.take();
                self.remove(film);
                out.push(Command::PinPlayer(false));
                out.push(Command::SessionMarkerSet(true));
                out.push(Command::PauseMenu(true));
                out.push(Command::Complete(self.complete_result));
            }
        }
    }

    /// `MapChallengeCompleteResult`: Win -> Owned, Kill -> Killed, Lose -> Abandoned.
    fn map_result(&mut self) {
        self.complete_result = match self.result {
            Flow::Win => Completion::Owned,
            Flow::Kill => Completion::Killed,
            _ => Completion::Abandoned,
        };
    }

    fn on_event(&mut self, event: Event, out: &mut Vec<Command>) {
        let p = self.params.clone();
        match (self.state, event) {
            (State::PrefilmDisplay, Event::HudLoaded) => self.change(State::NisOverview),
            (State::NisOverview, Event::PresentationFinished) => self.change(State::Setup),
            (State::WaitForAssetLoad, Event::AssetsLoaded) => self.change(State::SpawnPlayer),
            (State::CheckAi, Event::AiExist(all)) => {
                if all {
                    out.push(Command::PinAi(true));
                }
                self.change(if all { State::Countdown } else { State::Wait });
            }
            (State::Countdown, Event::TeleportFinished) if !p.kind.anywhere() => {
                out.push(Command::ShowHud(HudComponent::Objective, true));
                out.push(Command::CloseLoadingScreen);
                self.ids.countdown_delay = self.add(p.countdown_delay);
                out.push(Command::Speech(if self.first_run { Speech::Setup } else { Speech::Restart }));
            }
            (State::ChallengeRunning, Event::Wipeout) if !self.locked_out && !self.film_timer_expired => {
                if !p.allow_player_bail {
                    out.push(Command::FailAllObjectives);
                    self.wipe_out = true;
                    self.result = Flow::Lose;
                    if p.kind.film() {
                        out.push(Command::PauseMenu(false));
                        out.push(Command::RemoveAllHighlights);
                        self.ids.wipeout = self.add(p.wipeout_delay);
                        out.push(Command::Speech(Speech::Wipeout));
                    } else {
                        self.change(State::ChallengeCleanup);
                    }
                }
            }
            (State::ChallengeRunning, Event::LineTimerStarted) => self.line_timer_started = true,
            (State::ChallengeRunning, Event::LineTimerExpired) => {
                if p.use_line_timer && self.completed_one && !self.wipe_out && !self.locked_out && self.line_timer_started {
                    out.push(Command::CentralMessage(Message::LineTimerExpired));
                    if p.kind != Kind::SpotFilm {
                        out.push(Command::Speech(Speech::LineTimerExpired));
                    }
                    self.result = Flow::Lose;
                    if self.selector(out) {
                        self.change(State::ChallengeCleanup);
                    }
                }
            }
            (State::ChallengeRunning, Event::TriggerEnter { volume, competitor }) => {
                if competitor == 0 && Some(volume) == p.boundary {
                    let id = self.ids.out_of_bounds.take();
                    self.remove(id);
                }
                // b1006 §0.6: the leader reached the end volume (no LockedOut gate): it stops, a 10 s window opens.
                if p.kind == Kind::Follow && competitor == 1 && Some(volume) == p.ftl_end_volume && self.ids.ftl_remaining.is_none() {
                    out.push(Command::PinLeader);
                    out.push(Command::ShowHud(HudComponent::DistanceMeter, false));
                    self.ids.ftl_remaining = self.add(p.ftl_window);
                    out.push(Command::ShowHud(HudComponent::Timer, true));
                    out.push(Command::StartTimerHud { ticks: Timers::ticks(p.ftl_window) });
                }
            }
            (State::ChallengeRunning, Event::TriggerExit { volume }) => {
                if Some(volume) == p.boundary && !self.locked_out && !p.rules.no_boundary {
                    out.push(Command::Speech(Speech::LeftChallenge));
                    self.ids.out_of_bounds = self.add(p.out_of_bounds_time);
                    out.push(Command::CentralMessage(Message::ReturnToArea));
                }
            }
            (State::ChallengeRunning, Event::ObjectiveComplete { last, .. }) if !self.locked_out && self.ids.owned_pending.is_none() => {
                if !last {
                    out.push(Command::Speech(Speech::CompleteAnObjective));
                }
                self.completed_one = true;
            }
            (State::ChallengeRunning, Event::AllObjectivesComplete { killed }) if !self.locked_out || killed => {
                self.locked_out = true;
                out.push(Command::PauseMenu(false));
                out.push(Command::SessionMarkerUse(false));
                if killed {
                    let owned = self.ids.owned_pending.take();
                    self.remove(owned);
                    self.result = Flow::Kill;
                    self.all_complete = true;
                    if self.selector(out) {
                        out.push(Command::Speech(Speech::Succeed));
                        out.push(Command::KilledItEffect(true));
                        out.push(Command::ShowHud(HudComponent::KilledIt, true));
                        self.change(State::ChallengeCleanup);
                    }
                } else {
                    self.result = Flow::Win;
                    self.all_complete = true;
                    self.ids.owned_pending = self.add(p.owned_pending);
                }
            }
            (State::ChallengeRunning, Event::NotEnoughPoints) if !self.locked_out => {
                self.locked_out = true;
                self.ids.not_enough_points = self.add(0.0);
                out.push(Command::FailAllObjectives);
                out.push(Command::Evaluation(false));
            }
            (State::ChallengeRunning, Event::ObjectiveFailed) if !self.locked_out && !p.allow_objective_failure => {
                out.push(Command::FailAllObjectives);
                out.push(Command::CentralMessage(Message::ObjectiveFailure));
                self.result = Flow::Lose;
                out.push(Command::Speech(Speech::ReinforceOnFail));
                self.objective_fail = true;
                // b1006 §0.3: with AllowObjectiveFailure false the selector decides; FTL's ends the run.
                if self.selector(out) {
                    self.change(State::ChallengeCleanup);
                }
            }
            (State::ChallengeRunning, Event::UseMarker) if !self.locked_out => {
                if p.reset_ai_on_marker && p.competitors > 0 {
                    out.push(Command::RespawnAiAtStart);
                    if p.kind.film() {
                        self.ids.restart_ai_path = self.add(p.ai_restart_delay);
                    }
                }
                if p.reset_objectives_on_marker {
                    out.push(Command::FailAllObjectives);
                    out.push(Command::RemoveAllHighlights);
                    self.ids.session_reset = self.add(p.marker_reset_delay);
                }
            }
            (State::KilledIt, Event::NisComplete) => {
                self.hide_banners(out);
                self.change(State::HighlightReel);
            }
            (State::HighlightReel, Event::HighlightReelComplete) => self.change(State::EndOptions),
            (State::EndOptions, Event::Retry) => {
                out.push(Command::PauseSimulation(false));
                self.film_timer_expired = false;
                out.push(Command::PinPlayer(true));
                self.change(State::Restarted);
            }
            (State::EndOptions, Event::Continue) => {
                out.push(Command::PauseSimulation(false));
                self.map_result();
                if p.kind == Kind::Follow && self.result == Flow::Lose {
                    out.push(Command::Speech(Speech::Quit));
                }
                self.change(State::Complete);
            }
            // Global handlers.
            (_, Event::RestartChallenge) => {
                self.locked_out = true;
                if p.kind == Kind::Follow {
                    out.push(Command::DistanceUpdates(false));
                    out.push(Command::ShowHud(HudComponent::DistanceMeter, false));
                }
                self.change(State::Restarted);
            }
            (_, Event::Abandon) => {
                self.complete_result = Completion::Abandoned;
                out.push(Command::Speech(Speech::Quit));
                self.change(State::Complete);
            }
            _ => {}
        }
    }

    fn hide_banners(&mut self, out: &mut Vec<Command>) {
        out.push(Command::ShowHud(HudComponent::OwnedIt, false));
        out.push(Command::ShowHud(HudComponent::KilledIt, false));
        out.push(Command::KilledItEffect(false));
    }

    fn on_timer(&mut self, id: TimerId, out: &mut Vec<Command>) {
        let p = self.params.clone();
        let is = |t: Option<TimerId>| t == Some(id);
        match self.state {
            State::Wait if is(self.ids.wait) => self.change(State::CheckAi),
            State::Countdown if is(self.ids.countdown_delay) => {
                self.ids.countdown = self.add(p.prefilm_time);
                out.push(Command::StartCountdownHud { ticks: Timers::ticks(p.prefilm_time) });
            }
            State::Countdown if is(self.ids.countdown) => {
                // the film timer only in simul play; the low-time speech whenever the row sets a limit below it
                if p.simul_play && p.total_film_time > 0.0 {
                    self.ids.film = self.add(p.total_film_time);
                    out.push(Command::StartTimerHud { ticks: Timers::ticks(p.total_film_time) });
                }
                if p.film_low_limit > 0.0 && p.film_low_limit < p.total_film_time {
                    self.ids.warning = self.add(p.total_film_time - p.film_low_limit);
                }
                out.push(Command::PinPlayer(false));
                if p.competitors > 0 {
                    out.push(Command::PinAi(false));
                    out.push(Command::StartAiPaths);
                }
                out.push(Command::ActivateStartObjectives);
                out.push(Command::Evaluation(true));
                self.change(State::ChallengeRunning);
            }
            State::ChallengeRunning => self.running_timer(id, &p, out),
            State::KilledIt if is(self.ids.central_message_delay) => {
                self.hide_banners(out);
                self.change(State::EndOptions);
            }
            State::KilledIt if is(self.ids.replay_wait) => {
                self.hide_banners(out);
                out.push(Command::PauseSimulation(true));
                self.change(State::HighlightReel);
            }
            _ => {}
        }
    }

    fn running_timer(&mut self, id: TimerId, p: &Params, out: &mut Vec<Command>) {
        let is = |t: Option<TimerId>| t == Some(id);
        if is(self.ids.film) && !self.locked_out {
            out.push(Command::CentralMessage(Message::TimeUp));
            out.push(Command::Speech(Speech::LoseTimerExpired));
            self.result = Flow::Lose;
            self.film_timer_expired = true;
            self.ids.film = None;
            self.change(State::ChallengeCleanup);
        } else if is(self.ids.out_of_bounds) {
            self.ids.out_of_bounds = None;
            out.push(Command::CentralMessage(Message::OutOfBounds));
            self.result = Flow::Lose;
            // the selector runs at the handler's entry, before the flag is set (timerexpired@0-3, b999 §2): a film
            // repositions once here; the stale flag repositions again at the selector's next call, as in retail
            let ends = self.selector(out);
            self.out_of_bound = true;
            if ends {
                self.change(State::ChallengeCleanup);
            } else {
                out.push(Command::RepositionPlayer);
            }
        } else if is(self.ids.warning) {
            self.ids.warning = None;
            out.push(Command::Speech(Speech::TimeLimitLow));
        } else if is(self.ids.not_enough_points) {
            self.ids.not_enough_points = None;
            out.push(Command::FailAllObjectives);
            out.push(Command::CentralMessage(Message::ScoredLow));
            out.push(Command::Speech(Speech::NotEnoughPoints));
            self.result = Flow::Lose;
            out.push(Command::ResetObjectives);
            out.push(Command::Evaluation(true));
            self.locked_out = false;
            if self.selector(out) {
                self.change(State::ChallengeCleanup);
            }
        } else if is(self.ids.wipeout) {
            self.ids.wipeout = None;
            out.push(Command::PauseMenu(true));
            out.push(Command::ResetObjectives);
            if !p.allow_player_bail && self.selector(out) {
                self.change(State::ChallengeCleanup);
            } else {
                if matches!(p.kind, Kind::Film | Kind::SpotFilm) {
                    out.push(Command::RepositionPlayer);
                }
                if p.reset_ai_on_bail && p.competitors > 0 {
                    out.push(Command::RespawnAiAtStart);
                    self.ids.restart_ai_path = self.add(p.ai_restart_delay);
                }
            }
        } else if is(self.ids.objective_failed) {
            self.ids.objective_failed = None;
            out.push(Command::ResetObjectives);
            if p.allow_objective_failure && self.selector(out) {
                self.change(State::ChallengeCleanup);
            }
        } else if is(self.ids.owned_pending) {
            self.ids.owned_pending = None;
            out.push(Command::Speech(Speech::Succeed));
            out.push(Command::ShowHud(HudComponent::OwnedIt, true));
            self.change(State::ChallengeCleanup);
        } else if is(self.ids.ftl_remaining) {
            // b1006 §0.7: the window ran out: Abandoned (with Lose), cleanup.
            self.ids.ftl_remaining = None;
            self.complete_result = Completion::Abandoned;
            self.change(State::ChallengeCleanup);
        } else if is(self.ids.restart_ai_path) {
            self.ids.restart_ai_path = None;
            out.push(Command::StartAiPaths);
        } else if is(self.ids.session_reset) {
            self.ids.session_reset = None;
            out.push(Command::ResetObjectives);
            if p.reset_ai_on_marker && p.competitors > 0 {
                self.ids.restart_ai_path = self.add(p.marker_ai_restart_delay);
            }
        }
    }
}

#[cfg(test)]
#[path = "film_run_tests.rs"]
mod tests;
