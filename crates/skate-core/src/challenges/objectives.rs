//! A challenge's objective group, engine independent: objective states, activation modes, the per-objective
//! records the conditions read (entered volumes, tricks, session marker flags), which messages evaluate an
//! objective, and the Flow / Reset controls. The condition itself is a callback here; setup translates retail's
//! predicates into data for it (docs/hails-additions/42-challenges.md "Decision: objectives as data, not Lua").
//! Retail behaviour: research b841, b860, b863, b864, b865.
use std::collections::VecDeque;

/// Retail objective states (`obj+0x4F8`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    Inactive = 1,
    Armed = 2,
    Active = 4,
    Complete = 8,
    Failed = 0x10,
}

/// How an objective becomes active (row `ActivationMode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activation {
    /// 0: active at once.
    Immediate,
    /// 2 / 3: active inside a volume; leaving it fails the objective when `fail_on_exit`, else re-arms it.
    Volume { volume: u64, fail_on_exit: bool },
    /// 4: active from a wipeout, reset when the wipeout is over.
    Wipeout,
    /// 5: active from a sequence's begin, reset when it completes.
    Sequence,
}

/// The messages an objective group listens to. `mask_byte` is the `ObjectiveTriggers` byte that makes an active
/// objective evaluate on it (b864); messages without one only update records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Message {
    WipeoutOver,
    /// A finished sequence (cMsgSequenceComplete, b841): its score, and `clean` = not wiped out and no session
    /// marker used. Only a clean one is evaluated and scored; every one resets the per-sequence objectives.
    SequenceComplete { score: i32, clean: bool },
    TriggerEnter { volume: u64 },
    TriggerExit { volume: u64 },
    ChallengeComplete,
    MarkerUsed,
    MarkerSet,
    SkaterPushed,
    /// Off board, on board or grabbing an object.
    Board,
    ManualPumped,
    AirCompleted,
    GrindCompleted,
    /// The group's own tick (byte 12; byte 13 for the gated tick).
    Tick { gated: bool },
    Wipeout,
    SequenceBegin,
    /// A landed, announced trick (message 0x92CC9BBD; b865), keyed by hash64 of its scoring_trick name.
    TrickDone { key: u64 },
}

impl Message {
    pub fn mask_byte(self) -> Option<usize> {
        Some(match self {
            Message::WipeoutOver => 0,
            Message::SequenceComplete { clean: true, .. } => 1,
            Message::SequenceComplete { clean: false, .. } => return None,
            Message::TriggerEnter { .. } => 2,
            Message::ChallengeComplete => 3,
            Message::MarkerUsed | Message::MarkerSet => 5,
            Message::SkaterPushed => 6,
            Message::Board => 7,
            Message::ManualPumped => 8,
            Message::AirCompleted => 9,
            Message::GrindCompleted => 10,
            Message::Tick { gated: false } => 12,
            Message::Tick { gated: true } => 13,
            Message::TriggerExit { .. } | Message::Wipeout | Message::SequenceBegin | Message::TrickDone { .. } => return None,
        })
    }
}

/// Mask byte 4: a second pass, run only when the first pass completed an objective.
pub const SECOND_PASS: usize = 4;
pub const MASK_BYTES: usize = 14;
/// Entries kept in each per-objective list; the oldest drops first.
pub const LIST_MAX: usize = 32;

/// One objective's row data.
#[derive(Clone, Debug, PartialEq)]
pub struct Definition {
    pub activation: Activation,
    pub mask: [bool; MASK_BYTES],
    /// EvaluateFailureBeforeCompletionObjectives (82C752A8, research b940): failures are checked before completion.
    pub failure_first: bool,
}

/// What a condition may read about its objective.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Records {
    /// Volumes entered while active (refilled with the ones the player is in on activation).
    pub entered: VecDeque<u64>,
    /// Volumes entered in the current sequence.
    pub entered_in_sequence: VecDeque<u64>,
    /// Tricks done while active.
    pub tricks: VecDeque<u64>,
    /// Session marker used / set while active; cleared by the next group tick (b894).
    pub used_marker: bool,
    pub set_marker: bool,
    /// Pushes while active (objective +0x4C0, GetNumPushes 82C69508).
    pub pushes: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Objective {
    pub definition: Definition,
    pub state: State,
    pub records: Records,
    /// Points this objective accumulated (`+0x2BC`; GetAccumulatedPoints).
    pub points: i32,
}

/// What a condition sees besides its objective (the evaluation context block, b841 §1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Context {
    /// Context +8: evaluated for a clean sequence's points.
    pub sequence: bool,
    /// Context +9: the objective completed in this sequence's first pass.
    pub newly_complete: bool,
    /// GetGroupAccumulatedPoints: the sum of every objective's points.
    pub group_points: i32,
    /// GetGroupPointsRequirement (group `+0x4C`).
    pub requirement: i32,
}

/// The objective's two predicates: completion (tObjectiveDefinition) and PointsValid (default false).
pub trait Conditions {
    fn completion(&mut self, i: usize, objective: &Objective, ctx: Context) -> bool;
    fn points_valid(&mut self, _i: usize, _objective: &Objective, _ctx: Context) -> bool {
        false
    }
    /// The objective's failure rows (FailureObjectivesWithEvents, 82C75490): the first row whose predicate holds.
    fn failure(&mut self, _i: usize, _objective: &Objective, _ctx: Context) -> Option<usize> {
        None
    }
    /// The objective's hint rows (HintObjectives, 82C755B8): the first row whose predicate holds (research b942).
    fn hint(&mut self, _i: usize, _objective: &Objective, _ctx: Context) -> Option<usize> {
        None
    }
}

struct Completion<'a>(&'a mut dyn FnMut(usize, &Objective) -> bool);

impl Conditions for Completion<'_> {
    fn completion(&mut self, i: usize, objective: &Objective, _ctx: Context) -> bool {
        (self.0)(i, objective)
    }
}

/// What the group reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Activated(usize),
    Completed(usize),
    Failed(usize),
    /// A failure row held on an active objective (objectivefailed with its row index, 82C6E4B0); the objective stays
    /// active (research b940).
    FailureCondition { objective: usize, row: usize },
    /// A hint row held on an active objective that did not complete: its presentation plays, nothing else changes
    /// (82C755B8, research b942).
    Hint { objective: usize, row: usize },
    LeftScoringBoundary(usize),
    AllComplete,
}

fn push(list: &mut VecDeque<u64>, value: u64) {
    if list.len() == LIST_MAX {
        list.pop_front();
    }
    list.push_back(value);
}

/// One player's objective group. `inside` follows the trigger messages so an activation can refill `entered`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Group {
    pub objectives: Vec<Objective>,
    /// The points requirement (group `+0x4C`: the challenge's owned or killed points).
    pub requirement: i32,
    /// Hint rows run (group `+0x219`; MakeObjectives and CreateObjectiveGroup set it, research b942).
    pub hints: bool,
    inside: Vec<u64>,
    all_reported: bool,
}

impl Group {
    pub fn new(definitions: impl IntoIterator<Item = Definition>) -> Self {
        let objectives = definitions.into_iter()
            .map(|definition| Objective { definition, state: State::Inactive, records: Records::default(), points: 0 }).collect();
        Self { objectives, requirement: 0, hints: true, inside: Vec::new(), all_reported: false }
    }

    /// GetGroupAccumulatedPoints (82C65188).
    pub fn accumulated(&self) -> i32 {
        self.objectives.iter().map(|o| o.points).sum()
    }

    fn context(&self, sequence: bool, newly_complete: bool) -> Context {
        Context { sequence, newly_complete, group_points: self.accumulated(), requirement: self.requirement }
    }

    /// Flow.ActivateObjective: inactive -> active, or armed when the row waits for something to activate it.
    pub fn activate(&mut self, i: usize, out: &mut Vec<Event>) {
        let Some(o) = self.objectives.get_mut(i) else { return };
        if o.state != State::Inactive {
            return;
        }
        match o.definition.activation {
            Activation::Immediate => self.make_active(i, out),
            Activation::Volume { volume, .. } if self.inside.contains(&volume) => self.make_active(i, out),
            _ => o.state = State::Armed,
        }
    }

    /// Activates every objective (the start of a run).
    pub fn activate_all(&mut self, out: &mut Vec<Event>) {
        for i in 0..self.objectives.len() {
            self.activate(i, out);
        }
    }

    fn make_active(&mut self, i: usize, out: &mut Vec<Event>) {
        let inside = self.inside.clone();
        let o = &mut self.objectives[i];
        o.state = State::Active;
        o.records.entered.clear();
        for v in inside {
            push(&mut o.records.entered, v);
        }
        out.push(Event::Activated(i));
    }

    /// Flow.DeactivateObjective: armed or active -> inactive.
    pub fn deactivate(&mut self, i: usize) {
        if let Some(o) = self.objectives.get_mut(i).filter(|o| matches!(o.state, State::Armed | State::Active)) {
            o.state = State::Inactive;
        }
    }

    /// ResetObjective (82C6F4D0 -> 82C73EB8; research b936): every record cleared. With `reactivate` (the binding's
    /// 4th argument; Flow.ResetObjective 82C63200 passes it) an active or finished objective goes back to waiting for
    /// its activation (82C75740); without it (the tutorial graph's objectivefailed) the state is kept.
    pub fn reset(&mut self, i: usize, reactivate: bool) {
        let Some(o) = self.objectives.get_mut(i) else { return };
        o.records = Records::default();
        o.points = 0;
        if reactivate && o.state != State::Inactive {
            o.state = if o.definition.activation == Activation::Immediate { State::Active } else { State::Armed };
        }
        self.all_reported = false;
    }

    /// Reset.ResetAllObjectives: full reset of every objective, then all inactive.
    pub fn reset_all(&mut self) {
        for o in &mut self.objectives {
            o.records = Records::default();
            o.state = State::Inactive;
            o.points = 0;
        }
        self.all_reported = false;
    }

    /// Reset.FailAllObjectives.
    pub fn fail_all(&mut self, out: &mut Vec<Event>) {
        for (i, o) in self.objectives.iter_mut().enumerate() {
            if o.state != State::Inactive && o.state != State::Failed {
                o.state = State::Failed;
                out.push(Event::Failed(i));
            }
        }
    }

    pub fn status(&self, i: usize) -> Option<State> {
        self.objectives.get(i).map(|o| o.state)
    }

    /// One message: records first, then the evaluation pass for the message's mask byte (and the second pass when
    /// it completed something), then the activation-mode resets. `condition` is the objective's predicate.
    pub fn handle(&mut self, message: Message, condition: &mut dyn FnMut(usize, &Objective) -> bool) -> Vec<Event> {
        self.handle_with(message, &mut Completion(condition))
    }

    /// `handle` with both predicates. A clean SequenceComplete runs (82C6DB50, b841 §1): the first pass; PointsValid
    /// for every active objective and every one that just completed, adding the sequence score on true; the second
    /// pass when something completed; then the per-sequence reset.
    pub fn handle_with(&mut self, message: Message, conditions: &mut dyn Conditions) -> Vec<Event> {
        let mut out = Vec::new();
        match message {
            Message::TriggerEnter { volume } => {
                if !self.inside.contains(&volume) {
                    self.inside.push(volume);
                }
                for o in self.objectives.iter_mut().filter(|o| o.state == State::Active) {
                    push(&mut o.records.entered, volume);
                    push(&mut o.records.entered_in_sequence, volume);
                }
                let armed: Vec<usize> = (0..self.objectives.len())
                    .filter(|&i| self.objectives[i].state == State::Armed
                        && matches!(self.objectives[i].definition.activation, Activation::Volume { volume: v, .. } if v == volume))
                    .collect();
                for i in armed {
                    self.make_active(i, &mut out);
                }
            }
            Message::TriggerExit { volume } => {
                self.inside.retain(|&v| v != volume);
                for (i, o) in self.objectives.iter_mut().enumerate() {
                    if let (State::Active, Activation::Volume { volume: v, fail_on_exit }) = (o.state, o.definition.activation) {
                        if v == volume {
                            if fail_on_exit {
                                o.state = State::Failed;
                                out.push(Event::LeftScoringBoundary(i));
                                out.push(Event::Failed(i));
                            } else {
                                o.records = Records::default();
                                o.state = State::Armed;
                            }
                        }
                    }
                }
            }
            Message::MarkerUsed | Message::MarkerSet => {
                for o in self.objectives.iter_mut().filter(|o| o.state == State::Active) {
                    if message == Message::MarkerUsed { o.records.used_marker = true } else { o.records.set_marker = true }
                }
            }
            Message::SkaterPushed => {
                for o in self.objectives.iter_mut().filter(|o| o.state == State::Active) {
                    o.records.pushes = o.records.pushes.saturating_add(1);
                }
            }
            Message::TrickDone { key } => {
                for o in self.objectives.iter_mut().filter(|o| o.state == State::Active) {
                    push(&mut o.records.tricks, key);
                }
            }
            Message::SequenceBegin => {
                for o in &mut self.objectives {
                    o.records.entered_in_sequence.clear();
                }
                self.activate_mode(Activation::Sequence, &mut out);
            }
            Message::Wipeout => self.activate_mode(Activation::Wipeout, &mut out),
            _ => {}
        }
        if let Some(byte) = message.mask_byte() {
            let completed = self.evaluate(byte, conditions, &mut out);
            if let Message::SequenceComplete { score, clean: true } = message {
                self.award(score, &completed, conditions);
            }
            if !completed.is_empty() {
                self.evaluate(SECOND_PASS, conditions, &mut out);
            }
        }
        match message {
            Message::SequenceComplete { .. } => self.mode_reset(Activation::Sequence),
            Message::WipeoutOver => self.mode_reset(Activation::Wipeout),
            // The group tick clears the session marker flags after its evaluation pass (82C73FB8, research b894):
            // "just used / set the marker" holds until the end of the next tick.
            Message::Tick { .. } => {
                for o in &mut self.objectives {
                    o.records.used_marker = false;
                    o.records.set_marker = false;
                }
            }
            _ => {}
        }
        if !self.all_reported && !self.objectives.is_empty() && self.objectives.iter().all(|o| o.state == State::Complete) {
            self.all_reported = true;
            out.push(Event::AllComplete);
        }
        out
    }

    fn activate_mode(&mut self, mode: Activation, out: &mut Vec<Event>) {
        let armed: Vec<usize> = (0..self.objectives.len())
            .filter(|&i| self.objectives[i].state == State::Armed && self.objectives[i].definition.activation == mode)
            .collect();
        for i in armed {
            self.make_active(i, out);
        }
    }

    /// The per-sequence / per-wipeout reset (82C6DB50): an unfinished objective of that mode clears its tricks and
    /// waits for the next begin; its marker flags stay (only the full reset clears them).
    fn mode_reset(&mut self, mode: Activation) {
        for o in self.objectives.iter_mut().filter(|o| o.definition.activation == mode && o.state == State::Active) {
            o.records.tricks.clear();
            o.state = State::Armed;
        }
    }

    /// One evaluation pass; returns the objectives it completed.
    fn evaluate(&mut self, byte: usize, conditions: &mut dyn Conditions, out: &mut Vec<Event>) -> Vec<usize> {
        let mut completed = Vec::new();
        for i in 0..self.objectives.len() {
            let ctx = self.context(false, false);
            let o = &self.objectives[i];
            if o.state != State::Active || !o.definition.mask[byte] {
                continue;
            }
            // 82C752A8: completion first unless the row asks for failures first; hints, then failures, run only when
            // the objective did not complete; a fired failure ends the pass for this objective (research b940, b942).
            if o.definition.failure_first {
                self.hint(i, ctx, conditions, out);
                if let Some(row) = conditions.failure(i, &self.objectives[i], ctx) {
                    out.push(Event::FailureCondition { objective: i, row });
                    continue;
                }
            }
            let o = &self.objectives[i];
            if conditions.completion(i, o, ctx) {
                self.objectives[i].state = State::Complete;
                out.push(Event::Completed(i));
                completed.push(i);
            } else if !o.definition.failure_first {
                self.hint(i, ctx, conditions, out);
                if let Some(row) = conditions.failure(i, &self.objectives[i], ctx) {
                    out.push(Event::FailureCondition { objective: i, row });
                }
            }
        }
        completed
    }

    fn hint(&self, i: usize, ctx: Context, conditions: &mut dyn Conditions, out: &mut Vec<Event>) {
        if self.hints {
            if let Some(row) = conditions.hint(i, &self.objectives[i], ctx) {
                out.push(Event::Hint { objective: i, row });
            }
        }
    }

    /// The points step of a clean sequence (82C6F2B8 / 82C75690): objective points += the sequence score when its
    /// PointsValid holds.
    fn award(&mut self, score: i32, completed: &[usize], conditions: &mut dyn Conditions) {
        for i in 0..self.objectives.len() {
            let newly = completed.contains(&i);
            if self.objectives[i].state != State::Active && !newly {
                continue;
            }
            let ctx = self.context(true, newly);
            if conditions.points_valid(i, &self.objectives[i], ctx) {
                self.objectives[i].points += score;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(activation: Activation, bytes: &[usize]) -> Definition {
        let mut mask = [false; MASK_BYTES];
        for &b in bytes {
            mask[b] = true;
        }
        Definition { activation, mask, failure_first: false }
    }

    /// Own the spot as retail's rows build it (b840 / b841): a spot objective (mode 5, byte 1, completion = spot
    /// entered this sequence, PointsValid = IsSequenceNewlyCompletePoints) and the points objective (bytes 1 and 4,
    /// completion = group points >= requirement, PointsValid false).
    struct Ots;
    impl Conditions for Ots {
        fn completion(&mut self, i: usize, o: &Objective, ctx: Context) -> bool {
            if i == 0 { o.records.entered_in_sequence.contains(&5) } else { ctx.group_points >= ctx.requirement }
        }
        fn points_valid(&mut self, i: usize, _o: &Objective, ctx: Context) -> bool {
            i == 0 && ctx.sequence && ctx.newly_complete
        }
    }

    #[test]
    fn own_the_spot_scores_the_line_that_lands_in_the_spot() {
        let mut g = Group::new([def(Activation::Sequence, &[1]), def(Activation::Immediate, &[1, SECOND_PASS])]);
        g.requirement = 1000;
        g.activate_all(&mut Vec::new());
        g.handle_with(Message::SequenceBegin, &mut Ots);
        assert!(g.handle_with(Message::SequenceComplete { score: 2000, clean: true }, &mut Ots).is_empty(), "spot not entered");
        assert_eq!(g.accumulated(), 0);
        g.handle_with(Message::SequenceBegin, &mut Ots);
        g.handle_with(Message::TriggerEnter { volume: 5 }, &mut Ots);
        assert!(g.handle_with(Message::SequenceComplete { score: 800, clean: false }, &mut Ots).is_empty(), "a bail scores nothing");
        g.handle_with(Message::SequenceBegin, &mut Ots);
        g.handle_with(Message::TriggerEnter { volume: 5 }, &mut Ots);
        let out = g.handle_with(Message::SequenceComplete { score: 1200, clean: true }, &mut Ots);
        assert_eq!(out, [Event::Completed(0), Event::Completed(1), Event::AllComplete]);
        assert_eq!((g.objectives[0].points, g.accumulated()), (1200, 1200));
    }

    /// Completion, failure and hint answers per call, for the order tests.
    #[derive(Default)]
    struct Fails {
        complete: bool,
        fail: Option<usize>,
        failure_checks: usize,
        hint: Option<usize>,
    }

    impl Conditions for Fails {
        fn completion(&mut self, _i: usize, _o: &Objective, _ctx: Context) -> bool {
            self.complete
        }
        fn failure(&mut self, _i: usize, _o: &Objective, _ctx: Context) -> Option<usize> {
            self.failure_checks += 1;
            self.fail
        }
        fn hint(&mut self, _i: usize, _o: &Objective, _ctx: Context) -> Option<usize> {
            self.hint
        }
    }

    #[test]
    fn hint_rows_play_before_failures_and_only_while_not_complete() {
        let mut g = Group::new([def(Activation::Immediate, &[2])]);
        g.activate_all(&mut Vec::new());
        let enter = Message::TriggerEnter { volume: 1 };
        let mut c = Fails { hint: Some(2), fail: Some(0), ..Fails::default() };
        assert_eq!(g.handle_with(enter.clone(), &mut c), [Event::Hint { objective: 0, row: 2 }, Event::FailureCondition { objective: 0, row: 0 }]);
        assert_eq!(g.status(0), Some(State::Active), "a hint changes no state");
        g.hints = false;
        let mut c = Fails { hint: Some(2), ..Fails::default() };
        assert!(g.handle_with(enter.clone(), &mut c).is_empty(), "group +0x219 off: no hints");
        g.hints = true;
        let mut c = Fails { complete: true, hint: Some(1), ..Fails::default() };
        assert_eq!(g.handle_with(enter, &mut c), [Event::Completed(0), Event::AllComplete], "no hint on the completing pass");
    }

    #[test]
    fn failure_rows_fire_on_active_objectives_in_retail_order() {
        let mut g = Group::new([def(Activation::Immediate, &[2])]);
        g.activate_all(&mut Vec::new());
        let enter = Message::TriggerEnter { volume: 1 };
        let mut c = Fails { complete: false, fail: Some(1), ..Fails::default() };
        assert_eq!(g.handle_with(enter.clone(), &mut c), [Event::FailureCondition { objective: 0, row: 1 }]);
        assert_eq!(g.status(0), Some(State::Active), "a failure row keeps the objective active");
        let mut c = Fails { complete: true, fail: Some(0), ..Fails::default() };
        assert_eq!(g.handle_with(Message::TriggerEnter { volume: 2 }, &mut c), [Event::Completed(0), Event::AllComplete]);
        assert_eq!(c.failure_checks, 0, "completion first: a completed objective is not checked for failures");

        let mut first = def(Activation::Immediate, &[2]);
        first.failure_first = true;
        let mut g = Group::new([first]);
        g.activate_all(&mut Vec::new());
        let mut c = Fails { complete: true, fail: Some(0), ..Fails::default() };
        assert_eq!(g.handle_with(enter, &mut c), [Event::FailureCondition { objective: 0, row: 0 }]);
        assert_eq!(g.status(0), Some(State::Active), "failure first: a fired failure skips completion");
    }

    #[test]
    fn a_trigger_objective_completes_on_entering_its_volume() {
        let mut g = Group::new([def(Activation::Immediate, &[2])]);
        let mut out = Vec::new();
        g.activate_all(&mut out);
        assert_eq!(out, [Event::Activated(0)]);
        let mut cond = |_: usize, o: &Objective| o.records.entered.contains(&7);
        assert!(g.handle(Message::TriggerEnter { volume: 3 }, &mut cond).is_empty());
        assert_eq!(g.handle(Message::TriggerEnter { volume: 7 }, &mut cond), [Event::Completed(0), Event::AllComplete]);
        assert!(g.handle(Message::TriggerEnter { volume: 7 }, &mut cond).is_empty(), "reported once");
    }

    #[test]
    fn a_per_sequence_objective_sees_only_this_lines_tricks() {
        let mut g = Group::new([def(Activation::Sequence, &[1])]);
        let mut out = Vec::new();
        g.activate_all(&mut out);
        assert_eq!(g.status(0), Some(State::Armed));
        let mut two = |_: usize, o: &Objective| o.records.tricks.len() >= 2;
        g.handle(Message::TrickDone { key: 1 }, &mut two);
        assert!(g.objectives[0].records.tricks.is_empty(), "not active outside a line");
        g.handle(Message::SequenceBegin, &mut two);
        g.handle(Message::TrickDone { key: 1 }, &mut two);
        assert!(g.handle(Message::SequenceComplete { score: 0, clean: true },&mut two).is_empty());
        assert_eq!(g.status(0), Some(State::Armed));
        assert!(g.objectives[0].records.tricks.is_empty(), "reset at the line's end");
        g.handle(Message::SequenceBegin, &mut two);
        g.handle(Message::TrickDone { key: 1 }, &mut two);
        g.handle(Message::TrickDone { key: 2 }, &mut two);
        assert_eq!(g.handle(Message::SequenceComplete { score: 0, clean: true },&mut two), [Event::Completed(0), Event::AllComplete]);
    }

    #[test]
    fn records_keep_the_newest_32_and_marker_flags_last_until_the_next_tick() {
        let mut g = Group::new([def(Activation::Sequence, &[])]);
        let mut out = Vec::new();
        g.activate_all(&mut out);
        let mut never = |_: usize, _: &Objective| false;
        g.handle(Message::SequenceBegin, &mut never);
        for k in 0..40 {
            g.handle(Message::TrickDone { key: k }, &mut never);
        }
        assert_eq!((g.objectives[0].records.tricks.len(), g.objectives[0].records.tricks[0]), (32, 8));
        g.handle(Message::MarkerUsed, &mut never);
        g.handle(Message::SequenceComplete { score: 0, clean: true },&mut never);
        assert!(g.objectives[0].records.used_marker, "a sequence end keeps it");
        g.handle(Message::Tick { gated: false }, &mut never);
        assert!(!g.objectives[0].records.used_marker, "the tick clears it after its evaluation pass");
    }

    #[test]
    fn a_volume_activation_refills_entered_and_leaving_fails_or_rearms() {
        let mut g = Group::new([
            def(Activation::Volume { volume: 5, fail_on_exit: true }, &[]),
            def(Activation::Volume { volume: 5, fail_on_exit: false }, &[]),
        ]);
        let mut out = Vec::new();
        let mut never = |_: usize, _: &Objective| false;
        g.handle(Message::TriggerEnter { volume: 9 }, &mut never);
        g.activate_all(&mut out);
        assert_eq!(g.status(0), Some(State::Armed));
        assert_eq!(g.handle(Message::TriggerEnter { volume: 5 }, &mut never), [Event::Activated(0), Event::Activated(1)]);
        assert_eq!(g.objectives[0].records.entered, [9, 5], "already inside 9, then 5");
        assert_eq!(g.handle(Message::TriggerExit { volume: 5 }, &mut never), [Event::LeftScoringBoundary(0), Event::Failed(0)]);
        assert_eq!((g.status(0), g.status(1)), (Some(State::Failed), Some(State::Armed)));
    }

    #[test]
    fn the_second_pass_runs_only_after_a_completion() {
        let mut g = Group::new([def(Activation::Immediate, &[9]), def(Activation::Immediate, &[SECOND_PASS])]);
        let mut out = Vec::new();
        g.activate_all(&mut out);
        let mut cond = |_: usize, _: &Objective| true;
        assert!(g.handle(Message::GrindCompleted, &mut cond).is_empty(), "byte 10 is not in either mask");
        assert_eq!(g.handle(Message::AirCompleted, &mut cond), [Event::Completed(0), Event::Completed(1), Event::AllComplete]);
    }
}
