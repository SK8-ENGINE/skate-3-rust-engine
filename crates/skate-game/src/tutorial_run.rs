//! Tutorials in play (research b858; doc 42 "Tutorials"): the `tutorials` state machine
//! (`skate_core::challenges::tutorial`) driven by the frame's inputs, its objectives from the exported rows
//! (`challenge_objectives.rs`). The intro / outro / restart groups and the failure / hint rows' tips play on the
//! presentation manager (`presentation_run.rs`; doc 42 "Tutorial presentation"); the tutorial HUD is answered at once
//! and logged NOT RETAIL YET; a tutorial whose objectives call unported API functions does not run.
use crate::challenge_objectives::{DataObjectives, ObjectiveData};
use crate::challenges::Start;
use crate::presentation_run::Presenter;
use bevy::prelude::*;
use serde::Deserialize;
use skate_core::challenges::objectives::Event as GroupEvent;
use skate_core::challenges::tutorial::{Command, Event, Params, Presentation, Tutorial};
use skate_core::challenges::Completion;
use skate_core::presentation::{Group, Out, Started};
use std::path::Path;

/// The tutorial's intro / outro / restart presentation events (lists, or counts from an older export).
#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct PresentationCounts {
    #[serde(default)]
    pub intro: crate::challenge_objectives::EventList,
    #[serde(default)]
    pub outro: crate::challenge_objectives::EventList,
    #[serde(default)]
    pub restart: crate::challenge_objectives::EventList,
}

/// One tutorial row as `challenges.py` exports it (`tutorials` in challenges.json).
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct TutorialData {
    pub id: String,
    pub map: String,
    #[serde(default)]
    pub title_id: Option<String>,
    pub start: Start,
    pub location: Start,
    pub sign_up_radius: f32,
    #[serde(default)]
    pub spawn_offboard: bool,
    /// Stays signable after a result (CompleteChallenge re-activate flag, research b918 / b919).
    #[serde(default)]
    pub persistent: bool,
    #[serde(default)]
    pub ignore_initial_reposition: Option<bool>,
    #[serde(default)]
    pub presentation: PresentationCounts,
    #[serde(default)]
    pub objectives: Vec<ObjectiveData>,
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    tutorials: Vec<TutorialData>,
}

/// The tutorials of the challenges export (none in an export from before them).
pub(crate) fn load(assets: &Path) -> Vec<TutorialData> {
    std::fs::read(assets.join("private/challenges.json")).ok()
        .and_then(|b| serde_json::from_slice::<File>(&b).ok()).map(|f| f.tutorials).unwrap_or_default()
}

/// What the frame brings the run.
pub(crate) struct Inputs<'a> {
    /// Every trigger volume of the map as (name, id), for the objectives' `DISTRICT|name` arguments.
    pub volumes: &'a [(String, u64)],
    pub entered: &'a [u64],
    pub exited: &'a [u64],
    /// Banked sequences as (score, wiped out or session marker).
    pub banked: &'a [(i32, bool)],
    pub ticks: u32,
    pub teleport_pending: bool,
    /// The player's distance from the row's start locator.
    pub distance: f32,
    /// The player's physical state id and whether a sequence is running (scoring).
    pub state_id: u32,
    pub in_sequence: bool,
    /// Session marker this frame: true = used (Go To Marker, cMsgUseMarker), false = set (Place Marker, cMsgSetMarker).
    pub markers: &'a [bool],
    /// Trick keys announced this frame (retail's trick-done message, b865).
    pub tricks: &'a [u64],
    /// Trick keys announced this frame (retail's began list, b865 §2: BeganTrick).
    pub began: &'a [u64],
    /// The player's raw right stick Y and triggers (Input.*).
    pub right_stick_y: f32,
    pub triggers: [f32; 2],
    /// The manual held now: (tail, seconds).
    pub manual: Option<(bool, f32)>,
    /// New push contacts this frame (the skater-pushed message).
    pub pushes: u32,
    /// The ground pump controller: acceleration and whether the player drove it.
    pub pump_acceleration: f32,
    pub pump_user: bool,
    /// The rail being ground: [spline id, type signature].
    pub grind_rail: Option<[u64; 2]>,
    /// ObjectiveAir.GetTimeToGrind and the board adjust now (b1013).
    pub time_to_grind: f32,
    pub board_adjust: u32,
    /// The pose output's over flag (Wipeout.IsWipeoutOver while bailing).
    pub wipeout_over: bool,
    pub bail_pose: Option<usize>,
    /// The end dialog's pick (end_dialog_host.rs), taken while the run waits in its end options.
    pub end_choice: Option<skate_core::challenges::end_dialog::Action>,
}

/// Off board = on foot (500 biped ground / air, 502 pushing off board); wipeout = 300 (b864 §2.2 / §2.3 in our
/// state ids).
pub(crate) fn off_board(state: u32) -> bool {
    matches!(state, 500 | 501 | 502)
}

/// What the game must do for the run.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Action {
    Teleport { matrix: [[f32; 4]; 4], offboard: bool },
    Finished(Completion),
    /// Show the end dialog (retail FrontEnd.OpenContinueRetryDialog); tutorials open it as passed (b920).
    OpenEndDialog,
    /// HUD2 show / hide of one of the tutorial's HUD components (ObjectiveHUD, OwnedItHUD; b858 make_hud).
    ShowHud(skate_core::challenges::tutorial::Hud, bool),
}

pub(crate) struct TutorialRun {
    pub data: TutorialData,
    machine: Tutorial,
    objectives: Option<DataObjectives>,
    evaluating: bool,
    awaiting_teleport: bool,
    /// The end dialog shows; its pick feeds HudContinue / HudRetry.
    end_open: bool,
    /// Test runs (`SKATE_VERIFY_TUTORIAL_PASS=1`): the objectives count as done once they evaluate, so the end
    /// dialog and the career's reward chain can be checked without playing.
    force_pass: bool,
    started: bool,
    trick_groups: crate::challenge_objectives::TrickGroups,
    /// The tutorial's groups on the manager whose finish the state machine waits for.
    waiting: Vec<(String, Presentation)>,
    /// Failure / hint row tips fired this frame, started before the frame's events reach the state machine.
    tips: Vec<(String, Vec<skate_core::presentation::Event>)>,
    /// The skater facts, the frame's messages and the 60 Hz evaluations for the objective group (shared with the
    /// objectives_shared host).
    feed: crate::challenge_objectives::ObjectiveFeed,
}

impl TutorialRun {
    pub(crate) fn new(data: TutorialData, trick_groups: crate::challenge_objectives::TrickGroups) -> Self {
        let params = Params { intro: !data.presentation.intro.is_empty(), outro: !data.presentation.outro.is_empty(),
            restart: !data.presentation.restart.is_empty(), ignore_initial_reposition: data.ignore_initial_reposition,
            spawn_offboard: data.spawn_offboard };
        Self { data, machine: Tutorial::new(params), objectives: None, evaluating: false, awaiting_teleport: false, end_open: false,
            force_pass: std::env::var("SKATE_VERIFY_TUTORIAL_PASS").is_ok_and(|v| v == "1"), started: false,
            trick_groups, waiting: Vec::new(), tips: Vec::new(), feed: Default::default() }
    }

    /// Adds one of this tutorial's groups to the manager (owner = the tutorial id). `None` = nothing to wait for: no
    /// events (an export with counts only) or the queue was full.
    fn start_group(&self, slot: &str, events: Vec<skate_core::presentation::Event>, presenter: &mut Presenter) -> Option<String> {
        if events.is_empty() {
            return None;
        }
        let id = format!("{}/{slot}", self.data.id);
        let started = presenter.manager.start(Group { id: id.clone(), owner: self.data.id.clone(), events });
        info!("SKATE_TUTORIAL {} presentation {id} {started:?}", self.data.id);
        // Skipped = the same group already plays or is last in the queue: its finish is the one to wait for.
        (started != Started::Dropped).then_some(id)
    }

    fn row_tip(&mut self, kind: &str, objective: usize, row: usize) {
        let rows = self.data.objectives.get(objective).map(|o| if kind == "failure" { &o.failures } else { &o.hints });
        let events = rows.and_then(|r| r.get(row)).map(|r| r.presentation.events()).unwrap_or_default();
        self.tips.push((format!("obj{objective}/{kind}{row}"), events));
    }

    /// The objective group's events, as the tutorial graph's events.
    fn take_events(&mut self, raw: Vec<GroupEvent>, events: &mut Vec<Event>) {
        for e in raw {
            match e {
                GroupEvent::AllComplete => events.push(Event::AllObjectivesComplete),
                // objectivefailed comes only from a failure row on an active objective (82C6E4B0, research b940);
                // fail-all and fail-on-exit post none. The row's tip group is queued first.
                GroupEvent::FailureCondition { objective, row } => {
                    info!("SKATE_TUTORIAL {} objective {objective} failure row {row}", self.data.id);
                    self.row_tip("failure", objective, row);
                    events.push(Event::ObjectiveFailed(objective));
                }
                // A hint row plays its presentation only; no message to the tutorial graph (82C755B8, research b942).
                GroupEvent::Hint { objective, row } => {
                    info!("SKATE_TUTORIAL {} objective {objective} hint row {row}", self.data.id);
                    self.row_tip("hint", objective, row);
                }
                _ => {}
            }
        }
    }

    /// One frame: inputs to the objectives, timers, then every command and its immediate reply.
    pub(crate) fn step(&mut self, input: &Inputs, presenter: &mut Presenter) -> Vec<Action> {
        let mut actions = Vec::new();
        let mut commands = Vec::new();
        self.machine.set_distance(input.distance);
        if !self.started {
            self.started = true;
            commands.extend(self.machine.start());
        }
        let feed_input = crate::challenge_objectives::FeedInput {
            entered: input.entered, exited: input.exited, banked: input.banked, ticks: input.ticks,
            state_id: input.state_id, in_sequence: input.in_sequence, markers: input.markers, tricks: input.tricks,
            began: input.began, right_stick_y: input.right_stick_y, triggers: input.triggers, manual: input.manual,
            pushes: input.pushes, pump_acceleration: input.pump_acceleration, pump_user: input.pump_user,
            grind_rail: input.grind_rail, time_to_grind: input.time_to_grind, board_adjust: input.board_adjust,
            wipeout_over: input.wipeout_over, bail_pose: input.bail_pose,
            competitor_distances: [None; 4],
        };
        let mut groups: Vec<&mut DataObjectives> = self.objectives.as_mut().into_iter().collect();
        let raw: Vec<GroupEvent> = self.feed.step(&mut groups, &feed_input, self.evaluating).into_iter().map(|(_, e)| e).collect();
        let mut events = Vec::new();
        self.take_events(raw, &mut events);
        if self.awaiting_teleport && !input.teleport_pending {
            self.awaiting_teleport = false;
            events.push(Event::TeleportFinished);
        }
        if self.force_pass && self.evaluating {
            self.force_pass = false;
            info!("SKATE_TUTORIAL {} objectives passed by SKATE_VERIFY_TUTORIAL_PASS", self.data.id);
            events.push(Event::AllObjectivesComplete);
        }
        if let Some(choice) = input.end_choice.filter(|_| self.end_open) {
            use skate_core::challenges::end_dialog::Action as Pick;
            match choice {
                Pick::Continue => { self.end_open = false; events.push(Event::HudContinue) }
                Pick::Retry { .. } => { self.end_open = false; events.push(Event::HudRetry) }
                Pick::ChangeDifficulty | Pick::Page | Pick::Nothing => {}
            }
        }
        for (slot, list) in std::mem::take(&mut self.tips) {
            self.start_group(&slot, list, presenter);
        }
        for e in events {
            commands.extend(self.machine.handle(e));
        }
        for _ in 0..input.ticks {
            commands.extend(self.machine.tick());
        }
        let id = self.data.id.clone();
        let mut guard = 0;
        // The manager runs once per frame on the frame's game time; a group a reply starts gets zero-time passes so
        // instant groups finish in the frame they start, as retail's events with no wait do.
        let mut dt = input.ticks as f32 / skate_core::challenges::timers::TICKS_PER_SECOND;
        for _pass in 0..8 {
            while !commands.is_empty() && guard < 64 {
                guard += 1;
                let mut replies = Vec::new();
                for c in std::mem::take(&mut commands) {
                    match c {
                        Command::CreateHud => {
                            info!("SKATE_TUTORIAL {id} HUD NOT RETAIL YET: loaded at once");
                            replies.push(Event::HudLoaded);
                        }
                        Command::MakeObjectives => {
                            self.objectives = DataObjectives::new(&id, &self.data.objectives, input.volumes.to_vec(), self.trick_groups.clone());
                            if self.objectives.is_none() {
                                info!("SKATE_TUTORIAL {id} cannot run its objectives: abandoned");
                                replies.push(Event::Abandon);
                            }
                        }
                        Command::Evaluation(on) => self.evaluating = on,
                        Command::StartPresentation(p) => {
                            let (slot, list) = match p {
                                Presentation::Intro => ("intro", &self.data.presentation.intro),
                                Presentation::Outro => ("outro", &self.data.presentation.outro),
                                Presentation::Restart => ("restart", &self.data.presentation.restart),
                            };
                            match self.start_group(slot, list.events(), presenter) {
                                Some(group) => self.waiting.push((group, p)),
                                None => replies.push(Event::PresentationFinished(p)),
                            }
                        }
                        Command::LoadAssets => replies.push(Event::AssetLoadComplete),
                        Command::Reposition { offboard } => {
                            actions.push(Action::Teleport { matrix: self.data.start.matrix, offboard });
                            self.awaiting_teleport = true;
                        }
                        Command::ResetObjectives => {
                            if let Some(o) = self.objectives.as_mut() {
                                o.reset_only();
                            }
                        }
                        Command::ActivateStartObjectives => {
                            if let Some(o) = self.objectives.as_mut() {
                                o.activate_first();
                            }
                        }
                        Command::ResetObjective(i) => {
                            if let Some(o) = self.objectives.as_mut() {
                                o.reset_one(i);
                            }
                        }
                        Command::OpenEndOptions => {
                            info!("SKATE_TUTORIAL {id} end options");
                            self.end_open = true;
                            actions.push(Action::OpenEndDialog);
                        }
                        Command::ShowHud(hud, shown) => {
                            info!("SKATE_TUTORIAL {id} ShowHud({hud:?}, {shown})");
                            actions.push(Action::ShowHud(hud, shown));
                        }
                        Command::Complete(c) => {
                            info!("SKATE_TUTORIAL {id} complete {c:?}");
                            self.cleanup(presenter);
                            actions.push(Action::Finished(c));
                        }
                        other => info!("SKATE_TUTORIAL {id} {other:?}"),
                    }
                }
                for e in replies {
                    commands.extend(self.machine.handle(e));
                }
            }
            for out in presenter.update(std::mem::take(&mut dt)) {
                if let Out::GroupFinished(group) = out {
                    if let Some(i) = self.waiting.iter().position(|(w, _)| *w == group) {
                        let (_, p) = self.waiting.remove(i);
                        commands.extend(self.machine.handle(Event::PresentationFinished(p)));
                    }
                }
            }
            if commands.is_empty() {
                break;
            }
        }
        actions
    }

    /// The tutorial's groups leave the manager (retail's challenge cleanup, 82DBA8F8): the running event stops.
    fn cleanup(&mut self, presenter: &mut Presenter) {
        presenter.manager.remove_owner(&self.data.id, &mut presenter.handlers);
        presenter.reset_callout();
        presenter.reset_sim();
        self.waiting.clear();
        self.tips.clear();
    }

    pub(crate) fn abandon(&mut self, presenter: &mut Presenter) -> Vec<Action> {
        let mut out = Vec::new();
        for c in self.machine.handle(Event::Abandon) {
            if let Command::Complete(r) = c {
                out.push(Action::Finished(r));
            }
        }
        self.cleanup(presenter);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::challenge_objectives::{EventList, EventRow};

    fn data() -> TutorialData {
        let mut gate = vec![false; 14];
        gate[12] = true;
        let m = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [50., 0., 0., 1.]];
        TutorialData {
            id: "tut_test".into(), map: "SkateSchool".into(), title_id: None,
            start: Start { locator: "s".into(), matrix: m }, location: Start { locator: "l".into(), matrix: m },
            sign_up_radius: 4.0, spawn_offboard: true, persistent: false, ignore_initial_reposition: None,
            presentation: PresentationCounts { intro: EventList::Count(2), outro: EventList::Count(0), restart: EventList::Count(3) },
            objectives: vec![ObjectiveData { key: "tut_test_0".into(), activation: 0, triggers: gate, points_valid: None,
                condition: serde_json::json!({"locals": 0, "body": [{"return": {"api": "ObjectiveTrigger.IsInVolume",
                    "args": [{"lit": "1"}, {"lit": "DIST_SkateSchool|tut_test_gate_01"}]}}]}),
                failures: Vec::new(), failures_inline: Vec::new(), failure_first: false, hints: Vec::new(), title_id: None, title_arg: None, display_in_hud: true }],
        }
    }

    fn input<'a>(volumes: &'a [(String, u64)], entered: &'a [u64], pending: bool, ticks: u32, distance: f32) -> Inputs<'a> {
        Inputs { volumes, entered, exited: &[], banked: &[], ticks, teleport_pending: pending, distance, state_id: 100, in_sequence: false, markers: &[], tricks: &[], began: &[], right_stick_y: 0.0, triggers: [0.0; 2], manual: None, pushes: 0, pump_acceleration: 0.0, pump_user: false, grind_rail: None, time_to_grind: skate_core::challenges::api::NO_GRIND_TIME, board_adjust: 0, wipeout_over: false, bail_pose: None, end_choice: None }
    }

    /// One step without the HUD actions (checked on their own below).
    fn step(run: &mut TutorialRun, input: &Inputs, p: &mut Presenter) -> Vec<Action> {
        run.step(input, p).into_iter().filter(|a| !matches!(a, Action::ShowHud(..))).collect()
    }

    #[test]
    fn the_objective_hud_shows_on_arrival_and_the_owned_it_banner_at_the_end() {
        use skate_core::challenges::tutorial::Hud;
        let volumes = vec![("tut_test_gate_01".to_owned(), 9)];
        let mut run = TutorialRun::new(data(), Default::default());
        let mut p = Presenter::default();
        let mut all = run.step(&input(&volumes, &[], false, 0, 30.0), &mut p);
        all.extend(run.step(&input(&volumes, &[], true, 0, 0.0), &mut p));
        all.extend(run.step(&input(&volumes, &[], false, 1, 0.0), &mut p));
        assert!(all.contains(&Action::ShowHud(Hud::Objective, true)), "{all:?}");
        let mut done = run.step(&input(&volumes, &[9], false, 1, 0.0), &mut p);
        for _ in 0..240 {
            done.extend(run.step(&input(&volumes, &[], false, 1, 0.0), &mut p));
        }
        assert!(done.contains(&Action::ShowHud(Hud::OwnedIt, true)), "{done:?}");
    }

    #[test]
    fn a_tutorial_teleports_runs_its_objective_and_finishes_owned() {
        let volumes = vec![("tut_test_gate_01".to_owned(), 9)];
        let mut run = TutorialRun::new(data(), Default::default());
        let mut p = Presenter::default();
        let actions = step(&mut run, &input(&volumes, &[], false, 0, 30.0), &mut p);
        assert!(matches!(actions.as_slice(), [Action::Teleport { offboard: true, .. }]), "{actions:?}");
        assert!(step(&mut run, &input(&volumes, &[], true, 0, 0.0), &mut p).is_empty(), "waits for the arrival");
        assert!(step(&mut run, &input(&volumes, &[], false, 1, 0.0), &mut p).is_empty(), "arrived, objective running");
        let mut done = step(&mut run, &input(&volumes, &[9], false, 1, 0.0), &mut p);
        for _ in 0..240 {
            done.extend(step(&mut run, &input(&volumes, &[], false, 1, 0.0), &mut p));
        }
        assert_eq!(done, [Action::OpenEndDialog], "waits in the end dialog");
        let mut pick = input(&volumes, &[], false, 1, 0.0);
        pick.end_choice = Some(skate_core::challenges::end_dialog::Action::Continue);
        let mut done = step(&mut run, &pick, &mut p);
        for _ in 0..240 {
            done.extend(step(&mut run, &input(&volumes, &[], false, 1, 0.0), &mut p));
        }
        assert_eq!(done, [Action::Finished(Completion::Owned)]);
    }

    #[test]
    fn the_intro_plays_on_the_manager_before_the_teleport_and_cleanup_removes_it() {
        let volumes = vec![("tut_test_gate_01".to_owned(), 9)];
        let mut d = data();
        d.presentation.intro = EventList::Events(vec![EventRow { kind: "Wait".into(), args: [Some("1".into()), None], mask: 0 }]);
        let mut run = TutorialRun::new(d, Default::default());
        let mut p = Presenter::default();
        assert!(step(&mut run, &input(&volumes, &[], false, 0, 30.0), &mut p).is_empty());
        assert_eq!(p.manager.playing(), Some("tut_test/intro"));
        for _ in 0..50 {
            assert!(step(&mut run, &input(&volumes, &[], false, 1, 30.0), &mut p).is_empty(), "the intro holds the start");
        }
        let mut actions = Vec::new();
        for _ in 0..20 {
            actions.extend(step(&mut run, &input(&volumes, &[], false, 1, 30.0), &mut p));
        }
        assert!(matches!(actions.as_slice(), [Action::Teleport { .. }]), "{actions:?}");
        assert_eq!(p.manager.playing(), None);
        p.manager.start(Group { id: "tut_test/obj0/hint0".into(), owner: "tut_test".into(),
            events: vec![skate_core::presentation::Event { kind: "Wait".into(), args: [Some("9".into()), None], mask: 0 }] });
        run.abandon(&mut p);
        assert_eq!(p.manager.playing(), None, "the tutorial's groups leave with it");
    }
}
