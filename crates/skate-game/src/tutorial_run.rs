//! Tutorials in play (research b858; doc 42 "Tutorials"): the `tutorials` state machine
//! (`skate_core::challenges::tutorial`) driven by the frame's inputs, its objectives from the exported rows
//! (`challenge_objectives.rs`). Presentation, the tutorial HUD and the end-of-run dialog are answered at once and
//! logged NOT RETAIL YET until they are ported; a tutorial whose objectives call unported API functions does not run.
use crate::challenge_objectives::{DataObjectives, ObjectiveData};
use crate::challenges::Start;
use bevy::prelude::*;
use serde::Deserialize;
use skate_core::challenges::objectives::{Event as GroupEvent, Message};
use skate_core::challenges::tutorial::{Command, Event, Params, Tutorial};
use skate_core::challenges::Completion;
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
    /// The end dialog's pick (end_dialog_host.rs), taken while the run waits in its end options.
    pub end_choice: Option<skate_core::challenges::end_dialog::Action>,
}

/// Off board = on foot (500 biped ground / air, 502 pushing off board); wipeout = 300 (b864 §2.2 / §2.3 in our
/// state ids).
fn off_board(state: u32) -> bool {
    matches!(state, 500 | 501 | 502)
}

/// What the game must do for the run.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Action {
    Teleport { matrix: [[f32; 4]; 4], offboard: bool },
    Finished(Completion),
    /// Show the end dialog (retail FrontEnd.OpenContinueRetryDialog); tutorials open it as passed (b920).
    OpenEndDialog,
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
    was_off_board: bool,
    last_state: u32,
    trick_groups: crate::challenge_objectives::TrickGroups,
}

impl TutorialRun {
    pub(crate) fn new(data: TutorialData, trick_groups: crate::challenge_objectives::TrickGroups) -> Self {
        let params = Params { intro: !data.presentation.intro.is_empty(), outro: !data.presentation.outro.is_empty(),
            restart: !data.presentation.restart.is_empty(), ignore_initial_reposition: data.ignore_initial_reposition,
            spawn_offboard: data.spawn_offboard };
        Self { data, machine: Tutorial::new(params), objectives: None, evaluating: false, awaiting_teleport: false, end_open: false,
            force_pass: std::env::var("SKATE_VERIFY_TUTORIAL_PASS").is_ok_and(|v| v == "1"), started: false,
            was_off_board: false, last_state: 0, trick_groups }
    }

    fn feed(&mut self, message: Message, events: &mut Vec<Event>) {
        if !self.evaluating {
            return;
        }
        let Some(objectives) = self.objectives.as_mut() else { return };
        for e in objectives.handle(message) {
            match e {
                GroupEvent::AllComplete => events.push(Event::AllObjectivesComplete),
                // objectivefailed comes only from a failure row on an active objective (82C6E4B0, research b940);
                // fail-all and fail-on-exit post none. NOT RETAIL YET: the row's presentation (the tip) is not shown.
                GroupEvent::FailureCondition { objective, row } => {
                    info!("SKATE_TUTORIAL {} objective {objective} failure row {row} (tip presentation NOT RETAIL YET)", self.data.id);
                    events.push(Event::ObjectiveFailed(objective));
                }
                // A hint row plays its presentation only; no message to the tutorial graph (82C755B8, research b942).
                GroupEvent::Hint { objective, row } => {
                    info!("SKATE_TUTORIAL {} objective {objective} hint row {row} (tip presentation NOT RETAIL YET)", self.data.id);
                }
                _ => {}
            }
        }
    }

    /// One frame: inputs to the objectives, timers, then every command and its immediate reply.
    pub(crate) fn step(&mut self, input: &Inputs) -> Vec<Action> {
        let mut actions = Vec::new();
        let mut commands = Vec::new();
        self.machine.set_distance(input.distance);
        if !self.started {
            self.started = true;
            commands.extend(self.machine.start());
        }
        let off = off_board(input.state_id);
        let in_air = |s: u32| (200..=202).contains(&s);
        let air_completed = in_air(self.last_state) && !in_air(input.state_id) && input.state_id != 300;
        self.last_state = input.state_id;
        if let Some(o) = self.objectives.as_mut() {
            o.skater = skate_core::challenges::api::SkaterFacts {
                in_sequence: input.in_sequence,
                sequence_completed: input.banked.iter().any(|(_, dirty)| !dirty),
                wiping_out: input.state_id == 300,
                off_board: off,
                went_off_board: off && !self.was_off_board,
                went_on_board: !off && self.was_off_board,
                grinding: (400..=405).contains(&input.state_id),
                on_board: !off && input.state_id != 300 && input.state_id < 700,
                on_ground: (100..=105).contains(&input.state_id),
                air_completed,
                in_air: in_air(input.state_id),
                right_stick_y: input.right_stick_y,
                triggers: input.triggers,
                manual: input.manual,
                pump_acceleration: input.pump_acceleration,
                last_pump_user: input.pump_user,
                grind_rail: input.grind_rail,
            };
            o.tricks_this_frame = input.tricks.to_vec();
        }
        self.was_off_board = off;
        let mut events = Vec::new();
        for &v in input.exited {
            self.feed(Message::TriggerExit { volume: v }, &mut events);
        }
        for &v in input.entered {
            self.feed(Message::TriggerEnter { volume: v }, &mut events);
        }
        if air_completed {
            self.feed(Message::AirCompleted, &mut events);
        }
        for _ in 0..input.pushes {
            self.feed(Message::SkaterPushed, &mut events);
        }
        for &key in input.tricks {
            self.feed(Message::TrickDone { key }, &mut events);
        }
        for &used in input.markers {
            self.feed(if used { Message::MarkerUsed } else { Message::MarkerSet }, &mut events);
        }
        for &(score, dirty) in input.banked {
            self.feed(Message::SequenceComplete { score, clean: !dirty }, &mut events);
            self.feed(Message::SequenceBegin, &mut events);
        }
        self.feed(Message::Tick { gated: false }, &mut events);
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
                Pick::ChangeDifficulty | Pick::Nothing => {}
            }
        }
        for e in events {
            commands.extend(self.machine.handle(e));
        }
        for _ in 0..input.ticks {
            commands.extend(self.machine.tick());
        }
        let id = self.data.id.clone();
        let mut guard = 0;
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
                        info!("SKATE_TUTORIAL {id} presentation {p:?} NOT RETAIL YET: finished at once");
                        replies.push(Event::PresentationFinished(p));
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
                    Command::Complete(c) => {
                        info!("SKATE_TUTORIAL {id} complete {c:?}");
                        actions.push(Action::Finished(c));
                    }
                    other => info!("SKATE_TUTORIAL {id} {other:?}"),
                }
            }
            for e in replies {
                commands.extend(self.machine.handle(e));
            }
        }
        actions
    }

    pub(crate) fn abandon(&mut self) -> Vec<Action> {
        let mut out = Vec::new();
        for c in self.machine.handle(Event::Abandon) {
            if let Command::Complete(r) = c {
                out.push(Action::Finished(r));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::challenge_objectives::EventList;

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
                failures: Vec::new(), failure_first: false, hints: Vec::new() }],
        }
    }

    fn input<'a>(volumes: &'a [(String, u64)], entered: &'a [u64], pending: bool, ticks: u32, distance: f32) -> Inputs<'a> {
        Inputs { volumes, entered, exited: &[], banked: &[], ticks, teleport_pending: pending, distance, state_id: 100, in_sequence: false, markers: &[], tricks: &[], right_stick_y: 0.0, triggers: [0.0; 2], manual: None, pushes: 0, pump_acceleration: 0.0, pump_user: false, grind_rail: None, end_choice: None }
    }

    #[test]
    fn a_tutorial_teleports_runs_its_objective_and_finishes_owned() {
        let volumes = vec![("tut_test_gate_01".to_owned(), 9)];
        let mut run = TutorialRun::new(data(), Default::default());
        let actions = run.step(&input(&volumes, &[], false, 0, 30.0));
        assert!(matches!(actions.as_slice(), [Action::Teleport { offboard: true, .. }]), "{actions:?}");
        assert!(run.step(&input(&volumes, &[], true, 0, 0.0)).is_empty(), "waits for the arrival");
        assert!(run.step(&input(&volumes, &[], false, 1, 0.0)).is_empty(), "arrived, objective running");
        let mut done = run.step(&input(&volumes, &[9], false, 1, 0.0));
        for _ in 0..240 {
            done.extend(run.step(&input(&volumes, &[], false, 1, 0.0)));
        }
        assert_eq!(done, [Action::OpenEndDialog], "waits in the end dialog");
        let mut pick = input(&volumes, &[], false, 1, 0.0);
        pick.end_choice = Some(skate_core::challenges::end_dialog::Action::Continue);
        let mut done = run.step(&pick);
        for _ in 0..240 {
            done.extend(run.step(&input(&volumes, &[], false, 1, 0.0)));
        }
        assert_eq!(done, [Action::Finished(Completion::Owned)]);
    }
}
