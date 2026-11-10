//! A running challenge's objectives from setup's data (doc 42 "Decision: objectives as data, not Lua"): the exported
//! rows (activation mode, trigger bytes, completion and PointsValid programs) become an objective group
//! (`skate_core::challenges::objectives`) whose conditions run through the objective API. Own the spot's native check
//! (`challenges.rs` `Objectives`) is the fallback for an export without objectives.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use skate_core::challenges::api::{FlowCommand, ObjectiveApi, SkaterFacts};
use skate_core::challenges::objectives::{Activation, Conditions, Context, Definition, Event, Group, Message, Objective, State, MASK_BYTES};
use skate_core::challenges::predicate::{condition, Program, Value};
use skate_data::objective_program::{program, Names};

/// One objective row as `challenges.py` exports it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ObjectiveData {
    pub key: String,
    pub condition: serde_json::Value,
    #[serde(default)]
    pub points_valid: Option<serde_json::Value>,
    #[serde(default)]
    pub triggers: Vec<bool>,
    #[serde(default)]
    pub activation: u32,
    /// Failure rows in order (FailureObjectivesWithEvents, research b940); an untranslatable row has no condition.
    #[serde(default)]
    pub failures: Vec<FailureData>,
    #[serde(default)]
    pub failure_first: bool,
    /// Hint rows in order (HintObjectives, research b942), the same row shape as the failures.
    #[serde(default)]
    pub hints: Vec<FailureData>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct FailureData {
    pub key: String,
    #[serde(default)]
    pub condition: Option<serde_json::Value>,
    /// The presentation events the row plays when it fires.
    #[serde(default)]
    pub presentation: EventList,
}

/// One exported presentation event (`progression.presentation_events`: 16-byte records, research b892 / b943).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct EventRow {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub args: [Option<String>; 2],
    #[serde(default)]
    pub mask: u32,
}

/// A row's presentation events; an export from before the event lists has only their count.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum EventList {
    Count(usize),
    Events(Vec<EventRow>),
}

impl Default for EventList {
    fn default() -> Self {
        Self::Count(0)
    }
}

impl EventList {
    pub fn len(&self) -> usize {
        match self {
            Self::Count(n) => *n,
            Self::Events(e) => e.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The events for the presentation manager (none from a count-only export).
    pub fn events(&self) -> Vec<skate_core::presentation::Event> {
        match self {
            Self::Count(_) => Vec::new(),
            Self::Events(e) => e.iter().map(|r| skate_core::presentation::Event { kind: r.kind.clone(), args: r.args.clone(), mask: r.mask }).collect(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct ObjectiveSets {
    #[serde(default)]
    pub owned: Vec<ObjectiveData>,
    #[serde(default)]
    pub killed: Vec<ObjectiveData>,
}

/// eTriggerActivationMode to the group's modes (b864). Modes 1 and 2 / 3 without their activation volume read as
/// immediate [inferred]; own the spot uses 0 and 5.
fn activation(mode: u32) -> Activation {
    match mode {
        4 => Activation::Wipeout,
        5 => Activation::Sequence,
        _ => Activation::Immediate,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DataObjectives {
    group: Group,
    programs: Vec<(Program, Option<Program>)>,
    /// Per objective: its failure rows' programs by row index (None: the row never fires).
    failures: Vec<Vec<Option<Program>>>,
    /// Per objective: its hint rows' programs by row index (None: the row never fires).
    hints: Vec<Vec<Option<Program>>>,
    names: Names,
    vars: Vec<Vec<Value>>,
    globals: Vec<Value>,
    /// The challenge's volumes as (`name`, id) for EnteredVolume's `DISTRICT|name` argument.
    volumes: Vec<(String, u64)>,
    /// The volumes the player is in (IsInVolume), kept ahead of the group's own message handling.
    inside: Vec<u64>,
    /// This frame's skater flags (ObjectiveLocomotion / Wipeout).
    pub skater: SkaterFacts,
    /// The last "n of m" counter a condition set (SetFractionValue), for the objective HUD.
    pub fraction: Option<(f32, f32)>,
    /// Trick group key -> member trick keys (b863), and the tricks announced this frame.
    trick_groups: TrickGroups,
    pub tricks_this_frame: Vec<u64>,
}

pub(crate) type TrickGroups = std::sync::Arc<std::collections::BTreeMap<u64, std::collections::BTreeSet<u64>>>;

fn trick_key(name: &str) -> u64 {
    skate_data::attrib_hash::hash(name)
}

struct Eval<'a> {
    programs: &'a [(Program, Option<Program>)],
    failures: &'a [Vec<Option<Program>>],
    hints: &'a [Vec<Option<Program>>],
    names: &'a Names,
    vars: &'a mut [Vec<Value>],
    globals: &'a mut [Value],
    volumes: &'a [(String, u64)],
    inside: &'a [u64],
    statuses: Vec<State>,
    flow: Vec<FlowCommand>,
    skater: SkaterFacts,
    tricks_this_frame: &'a [u64],
    trick_groups: &'a std::collections::BTreeMap<u64, std::collections::BTreeSet<u64>>,
}

impl Eval<'_> {
    fn run(&mut self, p: &Program, i: usize, o: &Objective, ctx: Context, default: bool) -> bool {
        let volumes = self.volumes;
        let lookup = move |arg: &str| {
            let name = arg.rsplit('|').next().unwrap_or(arg);
            volumes.iter().find(|(n, _)| n == name).map(|(_, id)| *id)
        };
        let mut api = ObjectiveApi { names: &self.names.api, objective: o, ctx, volume: &lookup, inside: self.inside,
            statuses: &self.statuses, flow: &mut self.flow, skater: self.skater, tricks_this_frame: self.tricks_this_frame,
            trick_groups: self.trick_groups, key: &trick_key };
        condition(p, &mut api, &mut self.vars[i], self.globals, default)
    }
}

impl Conditions for Eval<'_> {
    fn completion(&mut self, i: usize, o: &Objective, ctx: Context) -> bool {
        let programs = self.programs;
        self.run(&programs[i].0, i, o, ctx, true)
    }
    fn points_valid(&mut self, i: usize, o: &Objective, ctx: Context) -> bool {
        let programs = self.programs;
        programs[i].1.as_ref().is_some_and(|p| self.run(p, i, o, ctx, false))
    }
    /// 82C75490: the rows in order, the first that holds wins (later rows do not run; they share the objective's vars).
    fn failure(&mut self, i: usize, o: &Objective, ctx: Context) -> Option<usize> {
        let failures = self.failures;
        self.first_row(failures.get(i)?, i, o, ctx)
    }
    /// 82C755B8: the same walk over the hint rows (research b942).
    fn hint(&mut self, i: usize, o: &Objective, ctx: Context) -> Option<usize> {
        let hints = self.hints;
        self.first_row(hints.get(i)?, i, o, ctx)
    }
}

impl Eval<'_> {
    fn first_row(&mut self, rows: &[Option<Program>], i: usize, o: &Objective, ctx: Context) -> Option<usize> {
        rows.iter().enumerate().find_map(|(row, p)| p.as_ref().filter(|p| self.run(p, i, o, ctx, false)).map(|_| row))
    }
}

/// Compiles one kind of side rows (failures, hints) after the group's own API check: a row that does not load, or that
/// needs an unported API, drops that kind for this group only (the group still runs from data).
fn side_rows<'a>(id: &str, kind: &str, lists: impl Iterator<Item = &'a [FailureData]>, names: &mut Names) -> Vec<Vec<Option<Program>>> {
    let before = names.api.len();
    let mut error = None;
    let mut out: Vec<Vec<Option<Program>>> = lists
        .map(|rows| {
            rows.iter()
                .map(|f| match f.condition.as_ref().map(|c| program(c, names)).transpose() {
                    Ok(p) => p,
                    Err(e) => {
                        error.get_or_insert(format!("{} ({e})", f.key));
                        None
                    }
                })
                .collect()
        })
        .collect();
    let unported: Vec<String> = names.api[before..].iter().filter(|n| !skate_core::challenges::api::ported(n)).cloned().collect();
    if error.is_some() || !unported.is_empty() {
        info!("SKATE_CHALLENGE {id} {kind} rows NOT RETAIL YET: {error:?} unported API {unported:?}");
        out.iter_mut().for_each(Vec::clear);
    }
    out
}

/// What a banked sequence did to the group.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Banked {
    /// An objective completed (the spot reached in a landed line).
    pub completed: bool,
    pub all_complete: bool,
    pub points: i32,
}

impl DataObjectives {
    /// The group for one objective set (own the spot: the owned set); None when the export has none or a program
    /// does not load (logged; the native check runs instead).
    pub(crate) fn new(id: &str, rows: &[ObjectiveData], volumes: Vec<(String, u64)>, trick_groups: TrickGroups) -> Option<Self> {
        if rows.is_empty() {
            return None;
        }
        let mut names = Names::default();
        let mut programs = Vec::new();
        for r in rows {
            let loaded = program(&r.condition, &mut names)
                .and_then(|c| Ok((c, r.points_valid.as_ref().map(|p| program(p, &mut names)).transpose()?)));
            match loaded {
                Ok(p) => programs.push(p),
                Err(e) => {
                    warn!("SKATE_CHALLENGE {id} objective {} does not load ({e}): native check", r.key);
                    return None;
                }
            }
        }
        let group = Group::new(rows.iter().map(|r| {
            let mut mask = [false; MASK_BYTES];
            for (i, on) in r.triggers.iter().take(MASK_BYTES).enumerate() {
                mask[i] = *on;
            }
            Definition { activation: activation(r.activation), mask, failure_first: r.failure_first }
        }));
        let unported: Vec<&String> = names.api.iter().filter(|n| !skate_core::challenges::api::ported(n)).collect();
        if !unported.is_empty() {
            info!("SKATE_CHALLENGE {id} objectives NOT RETAIL YET: unported API {unported:?}");
            return None;
        }
        let failures = side_rows(id, "failure", rows.iter().map(|r| r.failures.as_slice()), &mut names);
        let hints = side_rows(id, "hint", rows.iter().map(|r| r.hints.as_slice()), &mut names);
        let count = |l: &[Vec<Option<Program>>]| l.iter().map(Vec::len).sum::<usize>();
        let vars = vec![vec![Value::Nil; names.vars.len()]; rows.len()];
        let globals = vec![Value::Nil; names.globals.len()];
        info!("SKATE_CHALLENGE {id} objectives from data: {} rows, {} failure rows, {} hint rows, api {:?}", rows.len(),
            count(&failures), count(&hints), names.api);
        Some(Self { group, programs, failures, hints, names, vars, globals, volumes, inside: Vec::new(), skater: SkaterFacts::default(), fraction: None,
            trick_groups, tricks_this_frame: Vec::new() })
    }

    pub(crate) fn handle(&mut self, message: Message) -> Vec<Event> {
        match message {
            Message::TriggerEnter { volume } if !self.inside.contains(&volume) => self.inside.push(volume),
            Message::TriggerExit { volume } => self.inside.retain(|&v| v != volume),
            _ => {}
        }
        let statuses = self.group.objectives.iter().map(|o| o.state).collect();
        let mut eval = Eval { programs: &self.programs, failures: &self.failures, hints: &self.hints, names: &self.names, vars: &mut self.vars, globals: &mut self.globals,
            volumes: &self.volumes, inside: &self.inside, statuses, flow: Vec::new(), skater: self.skater,
            tricks_this_frame: &self.tricks_this_frame, trick_groups: &self.trick_groups };
        let mut events = self.group.handle_with(message, &mut eval);
        for command in std::mem::take(&mut eval.flow) {
            match command {
                FlowCommand::Activate(i) => self.group.activate(i, &mut events),
                FlowCommand::Deactivate(i) => self.group.deactivate(i),
                FlowCommand::Reset(i) => self.group.reset(i, true),
                FlowCommand::Fraction(n, d) => self.fraction = Some((n, d)),
            }
        }
        events
    }

    /// ResetObjectives: every objective back to the start, activated, and a sequence begun.
    pub(crate) fn reset(&mut self, requirement: i32) {
        self.group.reset_all();
        self.group.requirement = requirement;
        self.vars.iter_mut().for_each(|v| v.fill(Value::Nil));
        self.group.activate_all(&mut Vec::new());
        self.handle(Message::SequenceBegin);
    }

    /// Reset.ResetAllObjectives without re-activation (a tutorial's restart; ActivateStartObjectives follows).
    pub(crate) fn reset_only(&mut self) {
        self.group.reset_all();
        self.vars.iter_mut().for_each(|v| v.fill(Value::Nil));
    }

    /// ActivateChallengeStartObjectives: the first objective; the rows chain the rest with Flow.ActivateObjective
    /// [inferred: which rows retail activates at the start is not read].
    pub(crate) fn activate_first(&mut self) {
        self.group.activate(0, &mut Vec::new());
        self.handle(Message::SequenceBegin);
    }

    /// The graph's ResetObjective(group, i, true, false) on objectivefailed: progress cleared, state kept (b936).
    pub(crate) fn reset_one(&mut self, i: usize) {
        self.group.reset(i, false);
    }

    pub(crate) fn enter(&mut self, volume: u64) {
        self.handle(Message::TriggerEnter { volume });
    }

    pub(crate) fn exit(&mut self, volume: u64) {
        self.handle(Message::TriggerExit { volume });
    }

    /// A banked sequence (cMsgSequenceComplete), then the next sequence's begin.
    pub(crate) fn bank(&mut self, score: i32, wiped_out: bool, requirement: i32) -> Banked {
        self.group.requirement = requirement;
        let events = self.handle(Message::SequenceComplete { score, clean: !wiped_out });
        self.handle(Message::SequenceBegin);
        Banked {
            completed: events.iter().any(|e| matches!(e, Event::Completed(_))),
            all_complete: events.contains(&Event::AllComplete),
            points: self.group.accumulated(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of retail's own the spot rows (b840): the spot (mode 5, byte 1, EnteredVolume, PointsValid
    /// IsSequenceNewlyCompletePoints) and the points objective (bytes 1 and 4, group points >= requirement).
    fn rows() -> Vec<ObjectiveData> {
        let mut spot_triggers = vec![false; 14];
        spot_triggers[1] = true;
        let mut points_triggers = spot_triggers.clone();
        points_triggers[4] = true;
        vec![
            ObjectiveData { key: "ots_a_0".into(), activation: 5, triggers: spot_triggers,
                condition: serde_json::json!({"locals": 0, "body": [{"return": {"api": "ObjectiveTrigger.EnteredVolume",
                    "args": [{"lit": "1"}, {"lit": "DIST_DownTown|ots_a_spotvolume_01"}]}}]}),
                points_valid: Some(serde_json::json!({"locals": 0, "body": [{"return": {"api": "ObjectivePoints.IsSequenceNewlyCompletePoints", "args": []}}]})),
                failures: Vec::new(), failure_first: false, hints: Vec::new() },
            ObjectiveData { key: "global_points_objective".into(), activation: 0, triggers: points_triggers,
                condition: serde_json::json!({"locals": 0, "body": [{"return": {"cmp": "ge", "args": [
                    {"api": "GetGroupAccumulatedPoints", "args": []}, {"api": "GetGroupPointsRequirement", "args": []}]}}]}),
                points_valid: None, failures: Vec::new(), failure_first: false, hints: Vec::new() },
        ]
    }

    #[test]
    fn own_the_spot_runs_from_the_exported_rows() {
        let mut d = DataObjectives::new("ots_a", &rows(), vec![("ots_a_spotvolume_01".into(), 0x55)], Default::default()).unwrap();
        d.reset(1000);
        assert_eq!(d.bank(5000, false, 1000), Banked::default(), "a line outside the spot");
        d.enter(0x55);
        assert_eq!(d.bank(600, false, 1000), Banked { completed: true, all_complete: false, points: 600 }, "not enough");
        d.reset(1000);
        d.enter(0x55);
        assert_eq!(d.bank(1500, true, 1000).completed, false, "a bail");
        d.enter(0x55);
        assert_eq!(d.bank(1500, false, 1000), Banked { completed: true, all_complete: true, points: 1500 });
    }
}
