//! A race run, re-implemented from the retail race graph `race_shared` (research b876, b982; doc 42). Player first:
//! opponents only report their finishes. First place in time owns it, first place under KilledItTime kills it, behind
//! an AI skater loses (end_heat's place rule, research b984).
//! Details: docs/hails-additions/42-challenges.md#change-the-race-run-cratesskate-coresrcchallengesrace_runrs

use super::timers::{TimerId, Timers, TICKS_PER_SECOND};
use super::{Completion, Flow};

/// One race's values from the setup export (challenges.json `races`) or a mod; graph constants as defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    /// Split trigger volume ids per leg; the last of the last leg is the finish.
    pub splits: Vec<Vec<u64>>,
    /// Seconds added to the heat timer at the end of each leg but the last (EndTimeBonus; 0 on every disc row).
    pub end_time_bonus: Vec<f32>,
    pub time_limit: f32,
    pub time_limit_warning: f32,
    pub killed_it_time: f32,
    pub place_to_win: u32,
    pub include_321: bool,
    pub countdown_delay: f32,
    pub countdown_time: f32,
    pub end_round_time: f32,
    pub owned_display_time: f32,
    pub killed_display_time: f32,
    pub lose_display_time: f32,
    /// A stored best under this does not count as a record.
    pub no_record_below: f32,
}

impl Params {
    pub fn new(splits: Vec<Vec<u64>>, time_limit: f32, time_limit_warning: f32, killed_it_time: f32) -> Self {
        Self {
            splits,
            end_time_bonus: Vec::new(),
            time_limit,
            time_limit_warning,
            killed_it_time,
            place_to_win: 1,
            include_321: true,
            countdown_delay: 1.5,
            countdown_time: 3.0,
            end_round_time: 3.0,
            owned_display_time: 3.1,
            killed_display_time: 3.3,
            lose_display_time: 2.0,
            no_record_below: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    SetupHeat,
    Countdown,
    RunHeat,
    KilledIt,
    CleanupHeat,
    Results,
    Complete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    TriggerEnter { volume: u64 },
    /// An opponent crossed the finish (the finish order moves on).
    OpponentFinished,
    Continue,
    Retry,
    RestartChallenge,
    Abandon,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Banner {
    OwnedIt,
    KilledIt,
    Failed,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    TeleportToStart,
    RaceReset,
    RaceStart,
    RaceSetProgress { leg: usize, split: usize },
    RaceFinishPlayer { time: f32 },
    RaceStop,
    ShowGates { leg: usize },
    RemoveGates,
    StartCountdownHud { ticks: i32 },
    StartTimerHud { ticks: i32 },
    Leaderboard(bool),
    RaceFinishedMessage,
    TimeUpMessage,
    Banner(Banner, bool),
    KilledItEffect(bool),
    SetBestTime(f32),
    Results { time: Option<f32>, place: Option<u32> },
    PauseMenu(bool),
    Complete(Completion),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RaceRun {
    params: Params,
    state: State,
    timers: Timers,
    best_time: f32,
    leg: usize,
    split: usize,
    racing: bool,
    countdown_hud: bool,
    elapsed_ticks: i64,
    time: Option<f32>,
    place: Option<u32>,
    next_place: u32,
    result: Flow,
    warning: Option<TimerId>,
    limit: Option<TimerId>,
    countdown: Option<TimerId>,
    end_round: Option<TimerId>,
    banner: Option<TimerId>,
}

impl RaceRun {
    /// A new run; `best_time` is the stored record (0 = none).
    pub fn new(params: Params, best_time: f32) -> Self {
        Self {
            params,
            state: State::SetupHeat,
            timers: Timers::default(),
            best_time,
            leg: 0,
            split: 0,
            racing: false,
            countdown_hud: false,
            elapsed_ticks: 0,
            time: None,
            place: None,
            next_place: 1,
            result: Flow::Lose,
            warning: None,
            limit: None,
            countdown: None,
            end_round: None,
            banner: None,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn best_time(&self) -> f32 {
        self.best_time
    }

    /// The count-up race time in seconds.
    pub fn elapsed(&self) -> f32 {
        self.elapsed_ticks as f32 / TICKS_PER_SECOND
    }

    pub fn limit_ticks(&self) -> Option<i32> {
        self.limit.and_then(|id| self.timers.remaining(id))
    }

    pub fn finished(&self) -> bool {
        self.state == State::Complete
    }

    pub fn start(&mut self) -> Vec<Command> {
        let mut out = Vec::new();
        self.enter(State::SetupHeat, &mut out);
        out
    }

    pub fn handle(&mut self, event: Event) -> Vec<Command> {
        let mut out = Vec::new();
        if self.finished() {
            return out;
        }
        match event {
            Event::TriggerEnter { volume } if self.state == State::RunHeat && self.racing => self.split_entered(volume, &mut out),
            Event::OpponentFinished if self.state == State::RunHeat => self.next_place += 1,
            Event::Continue if self.state == State::Results => self.complete(&mut out),
            Event::Retry if self.state == State::Results => self.enter(State::SetupHeat, &mut out),
            Event::RestartChallenge => {
                out.push(Command::RaceStop);
                self.enter(State::SetupHeat, &mut out);
            }
            Event::Abandon => {
                out.push(Command::RaceStop);
                self.result = Flow::Exit;
                self.complete(&mut out);
            }
            _ => {}
        }
        out
    }

    /// One 60 Hz update: the count-up race time, then the countdown timers.
    pub fn tick(&mut self) -> Vec<Command> {
        let mut out = Vec::new();
        if self.state == State::RunHeat && self.racing {
            self.elapsed_ticks += 1;
        }
        for id in self.timers.tick() {
            if self.finished() {
                break;
            }
            self.expired(id, &mut out);
        }
        out
    }

    fn enter(&mut self, state: State, out: &mut Vec<Command>) {
        self.state = state;
        match state {
            State::SetupHeat => {
                self.timers = Timers::default();
                (self.leg, self.split, self.elapsed_ticks, self.next_place) = (0, 0, 0, 1);
                (self.racing, self.countdown_hud, self.time, self.place, self.result) = (false, false, None, None, Flow::Lose);
                (self.warning, self.limit, self.countdown, self.end_round, self.banner) = (None, None, None, None, None);
                out.extend([Command::TeleportToStart, Command::RaceReset, Command::Leaderboard(true)]);
                self.enter(State::Countdown, out);
            }
            State::Countdown => {
                out.push(Command::ShowGates { leg: 0 });
                if self.params.include_321 {
                    self.countdown = Some(self.timers.add_countdown(self.params.countdown_delay));
                } else {
                    self.enter(State::RunHeat, out);
                }
            }
            State::RunHeat => {
                self.racing = true;
                out.push(Command::RaceStart);
                let p = &self.params;
                if p.time_limit > 0.0 {
                    if p.time_limit_warning < p.time_limit {
                        self.warning = Some(self.timers.add_countdown(p.time_limit - p.time_limit_warning));
                    } else {
                        let id = self.timers.add_countdown(p.time_limit);
                        self.limit = Some(id);
                        out.push(Command::StartTimerHud { ticks: Timers::ticks(p.time_limit) });
                    }
                }
            }
            State::KilledIt => {
                out.push(Command::RaceStop);
                let won = self.place.is_some_and(|p| p <= self.params.place_to_win);
                let killed = won && self.time.is_some_and(|t| t > 0.0 && t <= self.params.killed_it_time);
                self.result = if killed { Flow::Kill } else if won { Flow::Win } else { Flow::Lose };
                match self.result {
                    Flow::Kill => out.extend([Command::KilledItEffect(true), Command::Banner(Banner::KilledIt, true)]),
                    Flow::Win => out.push(Command::Banner(Banner::OwnedIt, true)),
                    _ => {}
                }
                self.end_round = Some(self.timers.add_countdown(self.params.end_round_time));
            }
            State::CleanupHeat => {
                out.extend([Command::RemoveGates, Command::Leaderboard(false)]);
                let display = match self.result {
                    Flow::Kill => self.params.killed_display_time,
                    Flow::Win => self.params.owned_display_time,
                    _ => {
                        out.push(Command::Banner(Banner::Failed, true));
                        self.params.lose_display_time
                    }
                };
                if let Some(t) = self.time.filter(|&t| t > 0.0) {
                    if self.best_time < self.params.no_record_below || t < self.best_time {
                        self.best_time = t;
                        out.push(Command::SetBestTime(t));
                    }
                }
                self.banner = Some(self.timers.add_countdown(display));
            }
            State::Results => {
                out.push(Command::Results { time: self.time, place: self.place });
                out.push(Command::PauseMenu(true));
            }
            State::Complete => {}
        }
    }

    fn split_entered(&mut self, volume: u64, out: &mut Vec<Command>) {
        let splits = &self.params.splits[self.leg];
        let Some(i) = splits.iter().enumerate().skip(self.split).filter(|&(_, &v)| v == volume).map(|(i, _)| i + 1).last() else { return };
        self.split = i;
        out.push(Command::RaceSetProgress { leg: self.leg, split: i });
        if i < splits.len() {
            return;
        }
        if self.leg + 1 < self.params.splits.len() {
            let bonus = self.params.end_time_bonus.get(self.leg).copied().unwrap_or(0.0);
            if let Some(left) = self.limit_ticks().filter(|_| self.params.time_limit > 0.0 && bonus > 0.0) {
                let seconds = left as f32 / TICKS_PER_SECOND + bonus;
                self.timers.remove(self.limit.take().unwrap_or(TimerId(0)));
                self.limit = Some(self.timers.add_countdown(seconds));
                out.push(Command::StartTimerHud { ticks: Timers::ticks(seconds) });
            }
            self.leg += 1;
            self.split = 0;
            out.push(Command::ShowGates { leg: self.leg });
            return;
        }
        self.racing = false;
        let time = self.elapsed();
        self.time = Some(time);
        self.place = Some(self.next_place);
        self.next_place += 1;
        out.extend([Command::RaceFinishPlayer { time }, Command::RaceFinishedMessage]);
        self.drop_heat_timers();
        self.enter(State::KilledIt, out);
    }

    fn expired(&mut self, id: TimerId, out: &mut Vec<Command>) {
        if Some(id) == self.countdown {
            self.countdown = None;
            if !self.countdown_hud {
                self.countdown_hud = true;
                out.push(Command::StartCountdownHud { ticks: Timers::ticks(self.params.countdown_time) });
                self.countdown = Some(self.timers.add_countdown(self.params.countdown_time));
            } else {
                self.enter(State::RunHeat, out);
            }
        } else if Some(id) == self.warning {
            self.warning = None;
            let p = &self.params;
            let id = self.timers.add_countdown(p.time_limit_warning);
            self.limit = Some(id);
            out.push(Command::StartTimerHud { ticks: Timers::ticks(p.time_limit_warning) });
        } else if Some(id) == self.limit {
            self.limit = None;
            self.racing = false;
            out.push(Command::TimeUpMessage);
            self.drop_heat_timers();
            self.enter(State::KilledIt, out);
        } else if Some(id) == self.end_round {
            self.end_round = None;
            if self.result == Flow::Kill {
                out.push(Command::KilledItEffect(false));
            }
            self.enter(State::CleanupHeat, out);
        } else if Some(id) == self.banner {
            self.banner = None;
            self.enter(State::Results, out);
        }
    }

    fn drop_heat_timers(&mut self) {
        for id in [self.warning.take(), self.limit.take()].into_iter().flatten() {
            self.timers.remove(id);
        }
    }

    fn complete(&mut self, out: &mut Vec<Command>) {
        let completion = match self.result {
            Flow::Kill => Completion::Killed,
            Flow::Win => Completion::Owned,
            _ => Completion::Abandoned,
        };
        self.drop_heat_timers();
        out.extend([Command::PauseMenu(false), Command::Complete(completion)]);
        self.state = State::Complete;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> Params {
        Params::new(vec![vec![1, 2]], 60.0, 30.0, 10.0)
    }

    fn expire(run: &mut RaceRun, seconds: f32) -> Vec<Command> {
        (0..=Timers::ticks(seconds)).flat_map(|_| run.tick()).collect()
    }

    fn racing(best: f32) -> RaceRun {
        let mut run = RaceRun::new(params(), best);
        run.start();
        assert_eq!(run.state(), State::Countdown);
        assert!(expire(&mut run, 1.5).contains(&Command::StartCountdownHud { ticks: 180 }));
        expire(&mut run, 3.0);
        assert_eq!(run.state(), State::RunHeat);
        run
    }

    fn finish_after(run: &mut RaceRun, seconds: f32) -> Vec<Command> {
        for _ in 0..Timers::ticks(seconds) {
            run.tick();
        }
        run.handle(Event::TriggerEnter { volume: 1 });
        run.handle(Event::TriggerEnter { volume: 2 })
    }

    fn to_complete(run: &mut RaceRun, display: f32) -> Vec<Command> {
        let mut out = expire(run, 3.0);
        out.extend(expire(run, display));
        assert_eq!(run.state(), State::Results);
        out.extend(run.handle(Event::Continue));
        out
    }

    #[test]
    fn first_place_under_the_killed_it_time_kills_it() {
        let mut run = racing(0.0);
        let out = finish_after(&mut run, 8.0);
        assert!(out.contains(&Command::RaceFinishPlayer { time: 8.0 }));
        assert!(out.contains(&Command::Banner(Banner::KilledIt, true)));
        let out = to_complete(&mut run, 3.3);
        assert!(out.contains(&Command::SetBestTime(8.0)));
        assert_eq!(out.last(), Some(&Command::Complete(Completion::Killed)));
    }

    #[test]
    fn first_place_over_the_killed_it_time_owns_it() {
        let mut run = racing(0.0);
        assert!(finish_after(&mut run, 20.0).contains(&Command::Banner(Banner::OwnedIt, true)));
        assert_eq!(to_complete(&mut run, 3.1).last(), Some(&Command::Complete(Completion::Owned)));
    }

    #[test]
    fn finishing_behind_an_ai_skater_loses_but_keeps_the_best_time() {
        let mut run = racing(0.0);
        run.handle(Event::OpponentFinished);
        let out = finish_after(&mut run, 5.0);
        assert!(!out.iter().any(|c| matches!(c, Command::Banner(..))));
        let out = to_complete(&mut run, 2.0);
        assert!(out.contains(&Command::Banner(Banner::Failed, true)));
        assert!(out.contains(&Command::SetBestTime(5.0)), "retail records the time on any finish");
        assert_eq!(out.last(), Some(&Command::Complete(Completion::Abandoned)));
    }

    #[test]
    fn the_time_limit_shows_its_last_thirty_seconds_then_fails_the_race() {
        let mut run = racing(0.0);
        assert!(expire(&mut run, 30.0).contains(&Command::StartTimerHud { ticks: 1800 }));
        let out = expire(&mut run, 30.0);
        assert!(out.contains(&Command::TimeUpMessage));
        assert_eq!(run.state(), State::KilledIt);
        let out = to_complete(&mut run, 2.0);
        assert!(!out.iter().any(|c| matches!(c, Command::SetBestTime(_))));
        assert_eq!(out.last(), Some(&Command::Complete(Completion::Abandoned)));
    }

    #[test]
    fn splits_may_be_skipped_but_never_counted_backwards() {
        let mut run = RaceRun::new(Params::new(vec![vec![1, 2, 3]], 60.0, 30.0, 10.0), 0.0);
        run.start();
        expire(&mut run, 1.5);
        expire(&mut run, 3.0);
        assert_eq!(run.handle(Event::TriggerEnter { volume: 2 }), vec![Command::RaceSetProgress { leg: 0, split: 2 }]);
        assert!(run.handle(Event::TriggerEnter { volume: 1 }).is_empty());
        assert!(run.handle(Event::TriggerEnter { volume: 3 }).contains(&Command::RaceFinishedMessage));
    }

    #[test]
    fn a_leg_end_moves_to_the_next_leg() {
        let mut run = RaceRun::new(Params::new(vec![vec![1], vec![2]], 0.0, 30.0, 10.0), 0.0);
        run.start();
        expire(&mut run, 1.5);
        expire(&mut run, 3.0);
        assert!(run.handle(Event::TriggerEnter { volume: 1 }).contains(&Command::ShowGates { leg: 1 }));
        assert!(run.handle(Event::TriggerEnter { volume: 2 }).contains(&Command::RaceSetProgress { leg: 1, split: 1 }));
        assert_eq!(run.state(), State::KilledIt);
    }

    #[test]
    fn the_best_time_only_improves_and_retry_runs_the_heat_again() {
        let mut run = racing(7.0);
        finish_after(&mut run, 8.0);
        let out = expire(&mut run, 3.0);
        assert!(!out.iter().any(|c| matches!(c, Command::SetBestTime(_))));
        expire(&mut run, 3.3);
        run.handle(Event::Retry);
        assert_eq!(run.state(), State::Countdown);
        assert_eq!(run.best_time(), 7.0);
        let mut fresh = racing(0.5);
        finish_after(&mut fresh, 9.0);
        assert!(expire(&mut fresh, 3.0).contains(&Command::SetBestTime(9.0)), "a stored best under 1 s is no record");
    }

    #[test]
    fn a_volume_listed_twice_counts_as_its_later_split() {
        let mut run = RaceRun::new(Params::new(vec![vec![1, 1, 2]], 60.0, 30.0, 10.0), 0.0);
        run.start();
        expire(&mut run, 1.5);
        expire(&mut run, 3.0);
        assert_eq!(run.handle(Event::TriggerEnter { volume: 1 }), vec![Command::RaceSetProgress { leg: 0, split: 2 }]);
    }

    #[test]
    fn a_leg_end_time_bonus_extends_the_heat_timer() {
        let mut params = Params::new(vec![vec![1], vec![2]], 10.0, 30.0, 10.0);
        params.end_time_bonus = vec![5.0];
        let mut run = RaceRun::new(params, 0.0);
        run.start();
        expire(&mut run, 1.5);
        assert!(expire(&mut run, 3.0).contains(&Command::StartTimerHud { ticks: 600 }));
        assert!(run.handle(Event::TriggerEnter { volume: 1 }).contains(&Command::StartTimerHud { ticks: 900 }));
        assert_eq!(run.limit_ticks(), Some(900));
    }

    #[test]
    fn abandon_completes_as_abandoned() {
        let mut run = racing(0.0);
        let out = run.handle(Event::Abandon);
        assert_eq!(out.last(), Some(&Command::Complete(Completion::Abandoned)));
        assert!(run.finished());
    }
}
