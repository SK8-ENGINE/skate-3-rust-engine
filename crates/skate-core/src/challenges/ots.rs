//! Own the spot, re-implemented from the retail state graph `ownthespot_shared_mlui` (research b838; doc 42).
//!
//! The machine takes engine events and returns commands. State changes are deferred like retail `ChangeState`
//! (research b832): a handler queues the change, the queue is flushed after the message, each new state's enter
//! runs then, and the current state's handlers run before the global ones. Not ported yet (single player, offline):
//! SimulPlay, online scores, the intro presentation (no events: `view_nis` passes straight on).

use super::timers::{TimerId, Timers};
use super::{Completion, Flow, Tier};

/// Retail values of one challenge, from the setup export (challenges.json) or a mod.
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    pub boundary: u64,
    pub owned_points: i32,
    pub killed_points: i32,
    pub out_of_bounds_time: f32,
    pub owned_display_time: f32,
    pub killed_display_time: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    Initial,
    Setup,
    Functions,
    MakeObjective,
    ViewNis,
    WaitForTrigger,
    SetupHud,
    DisplayHud,
    ChallengeSetup,
    Teleport,
    ChallengeRunning,
    ChallengeCleanup,
    HighlightReel,
    EndOptions,
    Restart,
    Complete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HudComponent {
    Countdown,
    Timer,
    Objective,
    Dpo,
    OwnedIt,
    KilledIt,
}

/// The objective line the HUD shows (retail text ids).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TargetText {
    BeatOwnSpot,
    BeatKillSpot,
    BeatPersonalScore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    HudLoaded,
    TriggerEnter { volume: u64 },
    TriggerExit { volume: u64 },
    AllObjectivesComplete { points: i32 },
    NotEnoughPoints { points: i32 },
    Wipeout,
    WipeoutOver,
    ExitDiscovery,
    HighlightReelComplete,
    Abandon,
    Terminate,
    RestartChallenge,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    CreateHud,
    ShowHud(HudComponent, bool),
    StartTimerHud { ticks: i32 },
    UpdateHud { score: i32, target: i32, personal_best: i32, text: TargetText },
    ReturnToAreaMessage,
    NewPersonalBestMessage,
    ResetTriggerVolumes,
    PauseMenu(bool),
    Teleport,
    ResetObjectives,
    Evaluation(bool),
    SetPointRequirement(i32),
    SetHighScore(i32),
    MarkIntroSeen,
    PlayHighlightReel,
    Complete(Completion),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ots {
    params: Params,
    state: State,
    pending: Vec<State>,
    timers: Timers,
    personal_best: i32,
    tier: Tier,
    target: i32,
    text: TargetText,
    result: Flow,
    complete_result: Completion,
    score: i32,
    changed_stage: bool,
    locked_out: bool,
    wipe_out: bool,
    out_of_boundary: bool,
    hud_created: bool,
    completed: bool,
    intro_seen: bool,
    out_of_bounds: Option<TimerId>,
    point_freeze: Option<TimerId>,
    treatment: Option<TimerId>,
    replay_wait: Option<TimerId>,
}

impl Ots {
    /// A new run; `personal_best` and `intro_seen` come from the profile.
    pub fn new(params: Params, personal_best: i32, intro_seen: bool) -> Self {
        Self {
            params,
            state: State::Initial,
            pending: Vec::new(),
            timers: Timers::default(),
            personal_best,
            tier: Tier::Owned,
            target: 0,
            text: TargetText::BeatOwnSpot,
            result: Flow::Lose,
            complete_result: Completion::Abandoned,
            score: 0,
            changed_stage: false,
            locked_out: false,
            wipe_out: false,
            out_of_boundary: false,
            hud_created: false,
            completed: false,
            intro_seen,
            out_of_bounds: None,
            point_freeze: None,
            treatment: None,
            replay_wait: None,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn personal_best(&self) -> i32 {
        self.personal_best
    }

    pub fn finished(&self) -> bool {
        self.state == State::Complete
    }

    /// Starts the graph: the default state's enter runs (retail Start queues it, research b832).
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
            self.dispatch(Message::Event(event), &mut out);
            self.flush(&mut out);
        }
        out
    }

    /// One 60 Hz tick of the challenge timers; each expiry is a `timerexpired` message.
    pub fn tick(&mut self) -> Vec<Command> {
        let mut out = Vec::new();
        for id in self.timers.tick() {
            if self.finished() {
                break;
            }
            self.dispatch(Message::Timer(id), &mut out);
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

    fn setup_target(&mut self) {
        let (owned, killed, pb) = (self.params.owned_points, self.params.killed_points, self.personal_best);
        (self.tier, self.target, self.text) = if owned != 0 && pb < owned {
            (Tier::Owned, owned, TargetText::BeatOwnSpot)
        } else if killed != 0 && pb < killed {
            (Tier::Killed, killed, TargetText::BeatKillSpot)
        } else {
            (Tier::PersonalBest, pb, TargetText::BeatPersonalScore)
        };
    }

    fn update_hud(&self, score: i32, out: &mut Vec<Command>) {
        out.push(Command::UpdateHud { score, target: self.target, personal_best: self.personal_best, text: self.text });
    }

    fn hide_play_hud(out: &mut Vec<Command>, timer_too: bool) {
        out.push(Command::ShowHud(HudComponent::Objective, false));
        out.push(Command::ShowHud(HudComponent::Dpo, false));
        if timer_too {
            out.push(Command::ShowHud(HudComponent::Timer, false));
        }
    }

    fn out_of_bound(&mut self, out: &mut Vec<Command>) {
        self.result = Flow::Restart;
        out.push(Command::ReturnToAreaMessage);
        let id = self.timers.add_countdown(self.params.out_of_bounds_time);
        self.out_of_bounds = Some(id);
        out.push(Command::StartTimerHud { ticks: Timers::ticks(self.params.out_of_bounds_time) });
        self.update_hud(0, out);
        Self::hide_play_hud(out, false);
        out.push(Command::Evaluation(false));
    }

    fn cleanup(&mut self, out: &mut Vec<Command>) {
        self.completed = true;
        for id in [self.out_of_bounds.take(), self.point_freeze.take(), self.treatment.take(), self.replay_wait.take()]
            .into_iter()
            .flatten()
        {
            self.timers.remove(id);
        }
        self.hud_created = false;
        out.push(Command::PauseMenu(true));
    }

    fn enter(&mut self, out: &mut Vec<Command>) {
        match self.state {
            State::Initial => self.change(State::Setup),
            State::Setup => {
                self.tier = Tier::Owned;
                self.result = Flow::Lose;
                self.complete_result = Completion::Abandoned;
                (self.changed_stage, self.locked_out, self.completed, self.out_of_boundary) = (false, false, false, false);
                out.push(Command::PauseMenu(false));
                self.change(State::Functions);
            }
            State::Functions => self.change(State::MakeObjective),
            State::MakeObjective => {
                self.setup_target();
                out.push(Command::Evaluation(false));
                out.push(Command::SetPointRequirement(self.target));
                self.change(State::ViewNis);
            }
            State::ViewNis => self.change(State::WaitForTrigger),
            State::WaitForTrigger => {
                out.push(Command::ResetTriggerVolumes);
                out.push(Command::PauseMenu(false));
                self.setup_target();
                out.push(Command::SetPointRequirement(self.target));
                self.result = Flow::Lose;
                self.wipe_out = false;
                if !self.intro_seen {
                    self.intro_seen = true;
                    out.push(Command::MarkIntroSeen);
                }
                self.change(State::SetupHud);
            }
            State::SetupHud => {
                self.changed_stage = false;
                if self.hud_created {
                    out.push(Command::ShowHud(HudComponent::Timer, false));
                    self.change(State::DisplayHud);
                } else {
                    out.push(Command::CreateHud);
                }
            }
            State::DisplayHud => {
                self.update_hud(0, out);
                if !self.out_of_boundary {
                    out.push(Command::ShowHud(HudComponent::Objective, true));
                    out.push(Command::ShowHud(HudComponent::Dpo, true));
                }
                self.change(State::ChallengeSetup);
            }
            State::ChallengeSetup => {
                self.result = Flow::Lose;
                self.change(State::Teleport);
            }
            State::Teleport => {
                out.push(Command::Teleport);
                self.change(State::ChallengeRunning);
            }
            State::ChallengeRunning => {
                out.push(Command::PauseMenu(true));
                out.push(Command::ResetObjectives);
                out.push(Command::Evaluation(true));
            }
            State::ChallengeCleanup => {
                out.push(Command::Evaluation(false));
                out.push(Command::PauseMenu(false));
                match self.result {
                    Flow::Restart => {
                        self.locked_out = false;
                        self.change(State::ChallengeRunning);
                    }
                    Flow::Win | Flow::Kill => self.change(State::HighlightReel),
                    Flow::Exit => self.change(State::EndOptions),
                    Flow::Lose => {}
                }
            }
            State::HighlightReel => {
                out.push(Command::PauseMenu(false));
                let wait = if self.result == Flow::Kill {
                    self.params.killed_display_time
                } else {
                    self.params.owned_display_time
                };
                self.replay_wait = Some(self.timers.add_countdown(wait));
            }
            State::EndOptions => {
                out.push(Command::PauseMenu(true));
                if matches!(self.result, Flow::Win | Flow::Kill) {
                    let old = self.tier;
                    if self.score > self.personal_best {
                        self.personal_best = self.score;
                        out.push(Command::SetHighScore(self.score));
                    }
                    self.setup_target();
                    self.complete_result =
                        if self.result == Flow::Win { Completion::Owned } else { Completion::Killed };
                    self.changed_stage = matches!(
                        (old, self.tier),
                        (Tier::Owned, Tier::Killed) | (Tier::Killed, Tier::PersonalBest) | (Tier::Owned, Tier::PersonalBest)
                    );
                } else {
                    self.complete_result = Completion::Abandoned;
                }
                self.change(if self.changed_stage { State::Complete } else { State::Restart });
            }
            State::Restart => {
                out.push(Command::ResetObjectives);
                self.update_hud(0, out);
                self.locked_out = false;
                out.push(Command::PauseMenu(true));
                self.change(State::WaitForTrigger);
            }
            State::Complete => {
                self.cleanup(out);
                let completion = if self.changed_stage { self.complete_result } else { Completion::Abandoned };
                out.push(Command::Complete(completion));
            }
        }
    }

    fn dispatch(&mut self, message: Message, out: &mut Vec<Command>) {
        self.state_handler(message, out);
        self.global_handler(message, out);
    }

    fn state_handler(&mut self, message: Message, out: &mut Vec<Command>) {
        use Event::*;
        let boundary = self.params.boundary;
        match (self.state, message) {
            (State::WaitForTrigger, Message::Event(ExitDiscovery)) => {
                Self::hide_play_hud(out, true);
                self.change(State::Complete);
            }
            (State::SetupHud, Message::Event(HudLoaded)) => {
                self.hud_created = true;
                if self.out_of_boundary {
                    self.out_of_bound(out);
                }
                self.change(State::DisplayHud);
            }
            (State::SetupHud, Message::Event(TriggerExit { volume })) if volume == boundary => {
                self.out_of_boundary = true;
            }
            (State::SetupHud, Message::Event(TriggerEnter { volume })) if volume == boundary && self.out_of_boundary => {
                self.out_of_boundary = false;
            }
            (State::ChallengeRunning, Message::Event(AllObjectivesComplete { points }))
                if !self.wipe_out && !self.locked_out =>
            {
                self.locked_out = true;
                // Retail reads the display times crossed here (research b836 / b838); kept as is.
                let (component, time, flow) = if points >= self.params.killed_points {
                    (HudComponent::KilledIt, self.params.owned_display_time, Flow::Kill)
                } else {
                    (HudComponent::OwnedIt, self.params.killed_display_time, Flow::Win)
                };
                self.treatment = Some(self.timers.add_countdown(time));
                out.push(Command::ShowHud(component, true));
                self.result = flow;
                self.update_hud(points, out);
                self.score = points;
                self.change(State::ChallengeCleanup);
            }
            (State::ChallengeRunning, Message::Event(NotEnoughPoints { points })) if !self.locked_out => {
                self.update_hud(points, out);
                if self.personal_best < points {
                    self.personal_best = points;
                    out.push(Command::SetHighScore(points));
                    out.push(Command::NewPersonalBestMessage);
                }
                out.push(Command::ResetObjectives);
                out.push(Command::Evaluation(false));
                self.point_freeze = Some(self.timers.add_countdown(0.5));
            }
            (State::ChallengeRunning, Message::Timer(id)) if !self.completed => {
                if Some(id) == self.out_of_bounds {
                    self.out_of_bounds = None;
                    Self::hide_play_hud(out, true);
                    self.change(State::Complete);
                } else if Some(id) == self.point_freeze {
                    self.point_freeze = None;
                    self.update_hud(0, out);
                    out.push(Command::Evaluation(true));
                }
            }
            (State::ChallengeRunning, Message::Event(TriggerExit { volume })) if volume == boundary => {
                self.out_of_boundary = true;
                self.out_of_bound(out);
            }
            (State::ChallengeRunning, Message::Event(TriggerEnter { volume })) if volume == boundary => {
                self.result = Flow::Lose;
                self.out_of_boundary = false;
                if let Some(id) = self.out_of_bounds.take() {
                    self.timers.remove(id);
                }
                out.push(Command::ShowHud(HudComponent::Objective, true));
                out.push(Command::ShowHud(HudComponent::Dpo, true));
                out.push(Command::Evaluation(true));
            }
            (State::ChallengeRunning, Message::Event(Wipeout)) => {
                self.wipe_out = true;
                out.push(Command::Evaluation(false));
            }
            (State::ChallengeRunning, Message::Event(WipeoutOver)) => {
                self.wipe_out = false;
                out.push(Command::ResetObjectives);
                out.push(Command::Evaluation(true));
            }
            (State::ChallengeRunning, Message::Event(ExitDiscovery)) => {
                Self::hide_play_hud(out, true);
                if let Some(id) = self.out_of_bounds.take() {
                    self.timers.remove(id);
                }
                self.change(State::Complete);
            }
            (State::HighlightReel, Message::Timer(id)) if Some(id) == self.replay_wait => {
                self.replay_wait = None;
                out.push(Command::PlayHighlightReel);
            }
            (State::HighlightReel, Message::Event(HighlightReelComplete)) => self.change(State::EndOptions),
            _ => {}
        }
    }

    fn global_handler(&mut self, message: Message, out: &mut Vec<Command>) {
        match message {
            Message::Event(Event::Abandon | Event::Terminate) => self.change(State::Complete),
            Message::Event(Event::RestartChallenge) => self.change(State::Restart),
            Message::Timer(id) if Some(id) == self.treatment => {
                self.treatment = None;
                out.push(Command::ShowHud(HudComponent::OwnedIt, false));
                out.push(Command::ShowHud(HudComponent::KilledIt, false));
            }
            _ => {}
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Message {
    Event(Event),
    Timer(TimerId),
}

#[cfg(test)]
mod tests;
