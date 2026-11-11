//! A contest run (street / vert contests, demos), re-implemented from the retail graph `contest_shared`, single player
//! (research b1009, b877). Rounds rank the competitors by their round score and give place points (N - i, a tie keeps
//! the higher points); the player wins with the top total (ties win). Details:
//! docs/hails-additions/42-challenges.md#contests
use super::timers::{TimerId, Timers};
use super::Completion;

/// How a round is run (eRunType; b877 / b1009).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RunType {
    /// Everyone skates the round at once; the round score comes from the score events.
    Normal,
    /// The player has `runs` runs, the best counts; each AI rolls one score in `ai_min..=ai_max` at the round's setup
    /// (no adjustment).
    SoloRun { runs: u32, ai_min: f32, ai_max: f32 },
    /// One timed run to the path end: the round timer running out scores the player 0.
    TimedRunSingle,
}

/// One round's values (ContestRounds rows; setup export or a mod).
#[derive(Clone, Debug, PartialEq)]
pub struct Round {
    pub round_time: f32,
    pub run_type: RunType,
    /// CumulativeScoring: scores add up (each at least `cumulative_min`); else the best single score counts.
    pub cumulative: bool,
    pub cumulative_min: f32,
    /// AIScoreAdjustment: every AI score in a normal round is multiplied by it (single player).
    pub ai_adjustment: f32,
    /// A demo round's target (DemoTargetScore): below it the demo fails.
    pub demo_target: Option<f32>,
}

/// One contest's values; graph constants as defaults (a mod can replace any of them).
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    pub rounds: Vec<Round>,
    /// AI competitors (NumCompetitors = ai + 1).
    pub ai: usize,
    pub demo: bool,
    pub countdown_time: f32,
    /// Seconds before the round's end when the music's AlmostOver fades it (AudioRoundEndTimerID = RoundTime - 3).
    pub audio_end_time: f32,
    pub solo_pause: f32,
    pub demo_pause: f32,
    pub cleanup_fade: f32,
    /// The RNG seed for the SoloRun rolls (the authority's).
    pub seed: u32,
}

impl Params {
    pub fn new(rounds: Vec<Round>, ai: usize) -> Self {
        Self { rounds, ai, demo: false, countdown_time: 3.0, audio_end_time: 3.0, solo_pause: 2.0, demo_pause: 2.0, cleanup_fade: 0.3, seed: 1 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    SetupRound,
    Countdown,
    RunRound,
    EndRound,
    Results,
    CleanupRound,
    EndChallenge,
    EndOptions,
    Complete,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A competitor's score (0 = the player, 1.. = AI) after the trick-type gates (run_round.sequencecomplete).
    Score { competitor: usize, points: f32 },
    /// The player reached the path end (TimedRunSingle).
    PathEnd,
    /// Continue on the round results / the end options.
    Continue,
    Retry,
    Abandon,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// The audio's mission music trigger (eSk8MissionMusicTrigger: 0 Countdown, 1 Start, 6 AlmostOver, 7 Over, 8 Win,
    /// 9 Lose, 5 Abort).
    MusicTrigger(u8),
    StartCountdownHud { ticks: i32 },
    /// The round runs: scoring on, AI unpinned, the round timer (ticks) on the HUD; `run` counts SoloRun runs from 1.
    RoundStart { round: usize, run: u32, ticks: i32 },
    /// Scoring off, AI pinned.
    RoundStop,
    /// Each competitor's round and total score after a change.
    Scores { round: Vec<f32>, total: Vec<u32> },
    /// The round's results (place points given): show them, then Continue.
    RoundResults { round: usize },
    Banner(bool),
    /// The end options (Continue / Retry / Quit).
    EndOptions { won: bool },
    Complete(Completion),
}

#[derive(Clone, Debug)]
pub struct ContestRun {
    params: Params,
    state: State,
    timers: Timers,
    round: usize,
    run: u32,
    round_scores: Vec<f32>,
    best_run: f32,
    totals: Vec<u32>,
    won: bool,
    demo_failed: bool,
    rng: u32,
    round_timer: Option<TimerId>,
    audio_end: Option<TimerId>,
    countdown: Option<TimerId>,
    pause: Option<TimerId>,
    fade: Option<TimerId>,
}

impl ContestRun {
    pub fn new(params: Params) -> Self {
        let n = params.ai + 1;
        Self {
            rng: params.seed.max(1),
            params,
            state: State::SetupRound,
            timers: Timers::default(),
            round: 0,
            run: 0,
            round_scores: vec![0.0; n],
            best_run: 0.0,
            totals: vec![0; n],
            won: false,
            demo_failed: false,
            round_timer: None,
            audio_end: None,
            countdown: None,
            pause: None,
            fade: None,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }
    pub fn round(&self) -> usize {
        self.round
    }
    pub fn round_scores(&self) -> &[f32] {
        &self.round_scores
    }
    pub fn totals(&self) -> &[u32] {
        &self.totals
    }
    pub fn round_ticks(&self) -> Option<i32> {
        self.round_timer.and_then(|id| self.timers.remaining(id))
    }

    pub fn start(&mut self) -> Vec<Command> {
        let mut out = Vec::new();
        self.round = 0;
        self.totals = vec![0; self.params.ai + 1];
        self.enter(State::SetupRound, &mut out);
        out
    }

    fn current(&self) -> &Round {
        &self.params.rounds[self.round.saturating_sub(1).min(self.params.rounds.len().saturating_sub(1))]
    }

    fn random(&mut self) -> u32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng
    }

    pub fn handle(&mut self, event: Event) -> Vec<Command> {
        let mut out = Vec::new();
        match event {
            Event::Score { competitor, points } if self.state == State::RunRound && competitor < self.round_scores.len() => {
                self.update_score(competitor, points);
                out.push(self.scores());
            }
            Event::PathEnd if self.state == State::RunRound && self.current().run_type == RunType::TimedRunSingle => {
                self.enter(State::EndRound, &mut out);
            }
            Event::Continue if self.state == State::Results => self.enter(State::CleanupRound, &mut out),
            Event::Continue if self.state == State::EndOptions => {
                self.state = State::Complete;
                out.push(Command::Complete(if self.won { Completion::Owned } else { Completion::Abandoned }));
            }
            Event::Retry if self.state == State::EndOptions => out.extend(self.start()),
            Event::Abandon if self.state != State::Complete => {
                out.push(Command::MusicTrigger(5));
                out.push(Command::RoundStop);
                self.state = State::Complete;
                out.push(Command::Complete(Completion::Abandoned));
            }
            _ => {}
        }
        out
    }

    /// One 60 Hz update: the countdown timers.
    pub fn tick(&mut self) -> Vec<Command> {
        let mut out = Vec::new();
        for id in self.timers.tick() {
            if self.state == State::Complete {
                break;
            }
            self.expired(id, &mut out);
        }
        out
    }

    /// UpdateScore (setup_code closure 1): AI scores take the round's adjustment; cumulative rounds add scores of at
    /// least the minimum, others keep the best.
    fn update_score(&mut self, competitor: usize, points: f32) {
        let round = self.current().clone();
        let points = if competitor == 0 { points } else { points * round.ai_adjustment };
        let score = &mut self.round_scores[competitor];
        if round.cumulative {
            if round.cumulative_min <= points {
                *score += points;
            }
        } else if *score < points {
            *score = points;
        }
    }

    fn scores(&self) -> Command {
        Command::Scores { round: self.round_scores.clone(), total: self.totals.clone() }
    }

    fn enter(&mut self, state: State, out: &mut Vec<Command>) {
        self.state = state;
        match state {
            State::SetupRound => {
                self.round += 1;
                self.run = 1;
                self.best_run = 0.0;
                self.round_scores = vec![0.0; self.params.ai + 1];
                if let RunType::SoloRun { ai_min, ai_max, .. } = self.current().run_type {
                    if !self.params.demo {
                        for c in 1..self.round_scores.len() {
                            let r = self.random() as f32 / u32::MAX as f32;
                            self.round_scores[c] = ai_min + (ai_max - ai_min) * r;
                        }
                    }
                }
                out.push(self.scores());
                self.enter(State::Countdown, out);
            }
            State::Countdown => {
                out.push(Command::MusicTrigger(0));
                out.push(Command::StartCountdownHud { ticks: Timers::ticks(self.params.countdown_time) });
                self.countdown = Some(self.timers.add_countdown(self.params.countdown_time));
            }
            State::RunRound => {
                out.push(Command::MusicTrigger(1));
                let time = self.current().round_time;
                self.round_timer = Some(self.timers.add_countdown(time));
                if time > self.params.audio_end_time {
                    self.audio_end = Some(self.timers.add_countdown(time - self.params.audio_end_time));
                }
                out.push(Command::RoundStart { round: self.round, run: self.run, ticks: Timers::ticks(time) });
            }
            State::EndRound => {
                self.clear_round_timers();
                out.push(Command::RoundStop);
                out.push(Command::MusicTrigger(7));
                self.give_place_points();
                out.push(self.scores());
                self.state = State::Results;
                out.push(Command::RoundResults { round: self.round });
            }
            State::Results => {}
            State::CleanupRound => {
                let round = self.current().clone();
                self.demo_failed = self.params.demo && round.demo_target.is_some_and(|t| self.round_scores[0] < t);
                if self.demo_failed || self.round >= self.params.rounds.len() {
                    self.enter(State::EndChallenge, out);
                } else {
                    self.fade = Some(self.timers.add_countdown(self.params.cleanup_fade));
                }
            }
            State::EndChallenge => {
                // end_challenge.stateenter: a demo wins unless it failed; otherwise the top total wins (ties win).
                let top = self.totals.iter().copied().max().unwrap_or(0);
                self.won = if self.params.demo { !self.demo_failed } else { self.totals[0] == top };
                out.push(Command::MusicTrigger(if self.won { 8 } else { 9 }));
                out.push(Command::Banner(self.won));
                self.state = State::EndOptions;
                out.push(Command::EndOptions { won: self.won });
            }
            State::EndOptions | State::Complete => {}
        }
    }

    /// end_round: rank by round score, place points N - i into the totals; a tie with the place above keeps its points.
    fn give_place_points(&mut self) {
        let n = self.round_scores.len();
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|&a, &b| self.round_scores[b].total_cmp(&self.round_scores[a]));
        let mut points = 0u32;
        for (place, &c) in order.iter().enumerate() {
            let tied = place > 0 && self.round_scores[c] == self.round_scores[order[place - 1]];
            if !tied {
                points = (n - 1 - place) as u32;
            }
            self.totals[c] += points;
        }
    }

    fn clear_round_timers(&mut self) {
        for id in [self.round_timer.take(), self.audio_end.take(), self.pause.take()].into_iter().flatten() {
            self.timers.remove(id);
        }
    }

    fn expired(&mut self, id: TimerId, out: &mut Vec<Command>) {
        if Some(id) == self.countdown {
            self.countdown = None;
            self.enter(State::RunRound, out);
        } else if Some(id) == self.audio_end {
            self.audio_end = None;
            out.push(Command::MusicTrigger(6));
        } else if Some(id) == self.round_timer {
            self.round_timer = None;
            let round = self.current().clone();
            match round.run_type {
                RunType::SoloRun { runs, .. } => {
                    self.best_run = self.best_run.max(self.round_scores[0]);
                    out.push(Command::RoundStop);
                    if self.run >= runs {
                        self.round_scores[0] = self.best_run;
                        self.enter(State::EndRound, out);
                    } else {
                        self.pause = Some(self.timers.add_countdown(self.params.solo_pause));
                    }
                }
                RunType::TimedRunSingle => {
                    self.round_scores[0] = 0.0;
                    self.enter(State::EndRound, out);
                }
                RunType::Normal if self.params.demo => {
                    out.push(Command::RoundStop);
                    self.pause = Some(self.timers.add_countdown(self.params.demo_pause));
                }
                RunType::Normal => self.enter(State::EndRound, out),
            }
        } else if Some(id) == self.pause {
            self.pause = None;
            if matches!(self.current().run_type, RunType::SoloRun { .. }) {
                // The next run starts at 0 (the best so far is kept).
                self.run += 1;
                self.round_scores[0] = 0.0;
                out.push(self.scores());
                self.enter(State::RunRound, out);
            } else {
                self.enter(State::EndRound, out);
            }
        } else if Some(id) == self.fade {
            self.fade = None;
            self.enter(State::SetupRound, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round(run_type: RunType) -> Round {
        Round { round_time: 2.0, run_type, cumulative: false, cumulative_min: 0.0, ai_adjustment: 1.0, demo_target: None }
    }

    fn run_until(run: &mut ContestRun, state: State) -> Vec<Command> {
        let mut out = Vec::new();
        for _ in 0..10_000 {
            if run.state() == state {
                return out;
            }
            out.extend(run.tick());
        }
        panic!("never reached {state:?}");
    }

    #[test]
    fn place_points_give_n_minus_i_and_ties_keep_the_higher_points() {
        let mut run = ContestRun::new(Params::new(vec![round(RunType::Normal)], 3));
        run.start();
        run_until(&mut run, State::RunRound);
        for (c, p) in [(0, 500.0), (1, 900.0), (2, 500.0), (3, 100.0)] {
            run.handle(Event::Score { competitor: c, points: p });
        }
        run_until(&mut run, State::Results);
        assert_eq!(run.totals(), [2, 3, 2, 0]);
    }

    #[test]
    fn the_best_score_counts_unless_cumulative_and_ai_scores_take_the_adjustment() {
        let mut r = round(RunType::Normal);
        r.ai_adjustment = 0.5;
        let mut run = ContestRun::new(Params::new(vec![r], 1));
        run.start();
        run_until(&mut run, State::RunRound);
        run.handle(Event::Score { competitor: 0, points: 300.0 });
        run.handle(Event::Score { competitor: 0, points: 200.0 });
        run.handle(Event::Score { competitor: 1, points: 800.0 });
        assert_eq!(run.round_scores(), [300.0, 400.0]);
        let mut r = round(RunType::Normal);
        (r.cumulative, r.cumulative_min) = (true, 100.0);
        let mut run = ContestRun::new(Params::new(vec![r], 1));
        run.start();
        run_until(&mut run, State::RunRound);
        for p in [300.0, 50.0, 200.0] {
            run.handle(Event::Score { competitor: 0, points: p });
        }
        assert_eq!(run.round_scores()[0], 500.0);
    }

    #[test]
    fn a_tie_for_the_top_total_wins_and_the_music_follows_the_rounds() {
        let mut run = ContestRun::new(Params::new(vec![round(RunType::Normal), round(RunType::Normal)], 1));
        let mut music = Vec::new();
        let take = |cmds: Vec<Command>, music: &mut Vec<u8>| {
            music.extend(cmds.into_iter().filter_map(|c| if let Command::MusicTrigger(m) = c { Some(m) } else { None }));
        };
        take(run.start(), &mut music);
        for (player, ai) in [(900.0, 100.0), (100.0, 900.0)] {
            take(run_until(&mut run, State::RunRound), &mut music);
            run.handle(Event::Score { competitor: 0, points: player });
            run.handle(Event::Score { competitor: 1, points: ai });
            take(run_until(&mut run, State::Results), &mut music);
            take(run.handle(Event::Continue), &mut music);
        }
        assert_eq!(run.totals(), [1, 1]);
        assert_eq!(run.state(), State::EndOptions);
        assert_eq!(music, [0, 1, 7, 0, 1, 7, 8], "the round timer (2 s) is shorter than the audio end lead (3 s)");
        assert_eq!(run.handle(Event::Continue), [Command::Complete(Completion::Owned)]);
    }

    #[test]
    fn solo_runs_keep_the_best_run_and_ai_roll_once() {
        let mut p = Params::new(vec![round(RunType::SoloRun { runs: 2, ai_min: 100.0, ai_max: 100.0 })], 1);
        p.seed = 7;
        let mut run = ContestRun::new(p);
        run.start();
        assert_eq!(run.round_scores(), [0.0, 100.0]);
        run_until(&mut run, State::RunRound);
        run.handle(Event::Score { competitor: 0, points: 300.0 });
        run.tick();
        let mut runs = 1;
        for _ in 0..10_000 {
            if run.state() == State::Results {
                break;
            }
            for c in run.tick() {
                if let Command::RoundStart { run: r, .. } = c {
                    runs = r;
                    assert_eq!(run.round_scores()[0], 0.0, "each run starts at 0");
                    run.handle(Event::Score { competitor: 0, points: 50.0 });
                }
            }
        }
        assert_eq!((runs, run.round_scores()[0], run.totals()), (2, 300.0, &[1, 0][..]));
    }

    #[test]
    fn a_timed_run_scores_zero_when_time_runs_out_and_a_demo_below_target_fails() {
        let mut run = ContestRun::new(Params::new(vec![round(RunType::TimedRunSingle)], 0));
        run.start();
        run_until(&mut run, State::RunRound);
        run.handle(Event::Score { competitor: 0, points: 500.0 });
        run_until(&mut run, State::Results);
        assert_eq!(run.round_scores(), [0.0]);
        let mut r = round(RunType::Normal);
        r.demo_target = Some(1000.0);
        let mut p = Params::new(vec![r, round(RunType::Normal)], 0);
        p.demo = true;
        let mut run = ContestRun::new(p);
        run.start();
        run_until(&mut run, State::RunRound);
        run.handle(Event::Score { competitor: 0, points: 400.0 });
        run_until(&mut run, State::Results);
        let out = run.handle(Event::Continue);
        assert!(out.contains(&Command::MusicTrigger(9)) && out.contains(&Command::EndOptions { won: false }), "{out:?}");
    }
}
