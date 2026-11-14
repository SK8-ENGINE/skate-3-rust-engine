//! The objective API the translated conditions call (research b840, b841, b864), for one objective's evaluation.
//! Functions are matched by their last name segment (the data mixes `ObjectivePoints.X` and bare `X`). Functions not
//! ported yet return an error, so the condition gives its default, as a failing call does in retail.
use super::objectives::{Activation, Context, Objective, State};
use super::predicate::{Api, Error, Value};

pub struct ObjectiveApi<'a> {
    /// The interned API names (index = the program's function index).
    pub names: &'a [String],
    pub objective: &'a Objective,
    pub ctx: Context,
    /// A volume's id from its `DISTRICT|name` string (the game's trigger volumes).
    pub volume: &'a dyn Fn(&str) -> Option<u64>,
    /// The volumes the player is in now (IsInVolume).
    pub inside: &'a [u64],
    /// Every objective's state when the message arrived (Flow.GetObjectiveStatus, GetTotalNumCompletedObjectives).
    pub statuses: &'a [State],
    /// Flow changes the condition asked for, applied to the group after the message's passes [inferred timing].
    pub flow: &'a mut Vec<FlowCommand>,
    pub skater: SkaterFacts,
    /// Tricks completed (landed, announced) this frame (the ThisFrame variants; retail's ended list, b865 §1), as
    /// trick keys.
    pub tricks_this_frame: &'a [u64],
    /// Tricks announced this frame (BeganTrick / BeganTrickGroup; retail's began list, b865 §2).
    pub tricks_began: &'a [u64],
    /// Trick group key -> member trick keys (descendants joined, b863).
    pub trick_groups: &'a std::collections::BTreeMap<u64, std::collections::BTreeSet<u64>>,
    /// A trick or group name to its key (the Attrib hash the data uses).
    pub key: &'a dyn Fn(&str) -> u64,
}

/// The skater flags ObjectiveLocomotion / Wipeout read this frame (b864 §2.2 / §2.3), from the engine's state, and
/// the player's raw pad values (Input.*: 82C637B8, 82C63310 / 82C63430).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkaterFacts {
    pub in_air: bool,
    pub right_stick_y: f32,
    /// Left, right trigger, 0..1.
    pub triggers: [f32; 2],
    /// The manual held now: (tail, seconds) (Manual.*: 82C646B0 reads the time while the manual flag is set).
    pub manual: Option<(bool, f32)>,
    /// The pump controller's acceleration and whether the last pump was the player's (GetPumpingAcceleration
    /// 82C69588 reads objective +0x4C4, IsLastPumpUserControlled 82C695F0 the scoring state's +0x20+0x144).
    pub pump_acceleration: f32,
    pub last_pump_user: bool,
    /// The rail being ground now: its [spline id, type signature] (Grind.IsGrindingGUID 82C65D88 compares the
    /// argument with grind +0x40 / +0x48).
    pub grind_rail: Option<[u64; 2]>,
    pub in_sequence: bool,
    /// A sequence ended this frame with a clean landing (a banked, not wiped out line).
    pub sequence_completed: bool,
    pub wiping_out: bool,
    /// Wipeout.IsWipeoutOver (82C64890, b967 §1.2): in the bail and the pose output's over flag (+599) set. NOT RETAIL
    /// YET: its other two bail-end flags (+0x44 / +0x45) are not mapped.
    pub wipeout_over: bool,
    /// The body tweak pose the last bail closed in (Hall of Meat P+0x3540; ObjectiveHOM.DidSpecificBodyTweakThisFrame
    /// 82C68880 compares its argument with it).
    pub bail_pose: Option<usize>,
    pub off_board: bool,
    pub went_off_board: bool,
    pub went_on_board: bool,
    /// In a grind or slide state (Grind.IsGrinding).
    pub grinding: bool,
    pub on_board: bool,
    /// Wheels on the ground (ground states, not air / grind / on foot).
    pub on_ground: bool,
    /// An air ended this frame without a wipeout (ObjectiveAir.AirCompleted; also the group's AirCompleted message).
    pub air_completed: bool,
    /// ObjectiveAir.GetTimeToGrind (82C65E70, b1013): while a grind is predicted on the center trajectory (air output
    /// +0x1BB), predicted contact time minus time in the air (+0xB4 - +0xB0), else [`NO_GRIND_TIME`].
    pub time_to_grind: f32,
    /// The board adjust now (82BE1AE8 copies the animation attribute packet's value: 1 left, 2 right, 3 up, 4 down,
    /// [`BOARD_ADJUST_UP`] ...), 0 for none (ObjectiveAir.IsDoingSpecificBoardAdjust 82C65FD0).
    pub board_adjust: u32,
    /// Follow the leader (b1006): the player's distance to each AI competitor (competitor 1 first), m; None = not there.
    pub competitor_distances: [Option<f32>; 4],
}

impl Default for SkaterFacts {
    fn default() -> Self {
        Self {
            in_air: false, right_stick_y: 0.0, triggers: [0.0; 2], manual: None, pump_acceleration: 0.0, last_pump_user: false,
            grind_rail: None, in_sequence: false, sequence_completed: false, wiping_out: false, wipeout_over: false, bail_pose: None, off_board: false,
            went_off_board: false, went_on_board: false, grinding: false, on_board: false, on_ground: false,
            air_completed: false, time_to_grind: NO_GRIND_TIME, board_adjust: 0, competitor_distances: [None; 4],
        }
    }
}

/// GetTimeToGrind without a predicted grind (DAT_8206D108 = FLT_MAX, b1013 / b277).
pub const NO_GRIND_TIME: f32 = f32::MAX;
pub const BOARD_ADJUST_LEFT: u32 = 1;
pub const BOARD_ADJUST_RIGHT: u32 = 2;
pub const BOARD_ADJUST_UP: u32 = 3;
pub const BOARD_ADJUST_DOWN: u32 = 4;

/// Flow.ActivateObjective / DeactivateObjective / ResetObjective by 0-based objective index (b864 §2.5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FlowCommand {
    Activate(usize),
    Deactivate(usize),
    Reset(usize),
    /// Reset.ResetAllObjectives (82C63FC0 -> 82C6F430): every objective fully reset and inactive.
    ResetAll,
    /// ObjectiveGameInfo.SetFractionValue: the "n of m" HUD counter of this objective (numerator, denominator).
    Fraction(f32, f32),
    /// Leader.AddFollowIndicator / RemoveFollowIndicator: the trail over the leader (b1006 / b566 style trail_follow).
    /// Retail adds {competitor, kind 0, active} to the indicator manager's actor list (82D4F470 via 82C6CC00), which the
    /// render side draws as a `trail_skatewith` ribbon (82D4FAD0, 827AEC80).
    FollowIndicator { competitor: u32, on: bool },
}

/// The functions this API answers (last name segment). A group whose programs call anything else does not run:
/// an unported call errors, and an erroring completion condition defaults to true (retail), so it would complete
/// at once.
pub const PORTED: [&str; 54] = ["IsWipeoutOver", "ResetAllObjectives", "DidSpecificBodyTweakThisFrame", "GetDistanceToNearestAISkater", "GetDistanceToSkater", "AddFollowIndicator", "RemoveFollowIndicator","BeganTrick", "BeganTrickGroup", "GetTimeToGrind", "IsDoingSpecificBoardAdjust", "EnteredVolume", "IsInVolume", "IsSequenceNewlyCompletePoints", "GetGroupAccumulatedPoints",
    "GetGroupPointsRequirement", "GetAccumulatedPoints", "ActivateObjective", "DeactivateObjective", "ResetObjective",
    "GetObjectiveStatus", "GetTotalNumCompletedObjectives", "IsInSequence", "SequenceCompleted", "IsWipingOut",
    "IsOffBoard", "WentOffBoard", "WentOnBoard", "JustUsedSessionMarker", "JustSetSessionMarker", "SetFractionValue",
    "DidSpecificTrick", "DidSpecificTrickGroup", "DidSpecificTrickThisFrame", "DidSpecificTrickGroupThisFrame",
    "GetNumTricksDoneInTrickGroup", "IsGrinding", "IsOnBoard", "IsOnGround", "AirCompleted", "IsInAir",
    "GetRawRightStickYValue", "IsLeftTriggerHeld", "IsRightTriggerHeld", "TailManualTime", "NoseManualTime",
    "ManualTime", "IsManualing", "IsNoseManualing", "IsTailManualing", "GetNumPushes", "GetPumpingAcceleration", "IsLastPumpUserControlled", "IsGrindingGUID"];

fn name_arg(args: &[Value]) -> Result<&str, Error> {
    match args.first() {
        Some(Value::Str(s)) => Ok(s.as_ref()),
        _ => Err(Error::Type("a trick or group name")),
    }
}

fn index(args: &[Value]) -> Result<usize, Error> {
    match args.first() {
        Some(Value::Num(n)) if *n >= 0.0 => Ok(*n as usize),
        _ => Err(Error::Type("an objective index")),
    }
}

pub fn ported(name: &str) -> bool {
    PORTED.contains(&name.rsplit('.').next().unwrap_or(name))
}

fn num(n: i32) -> Value {
    Value::Num(n as f32)
}

fn num_f(n: f32) -> Value {
    Value::Num(n)
}

impl Api for ObjectiveApi<'_> {
    fn call(&mut self, func: u16, args: &[Value]) -> Result<Value, Error> {
        let name = self.names.get(func as usize).map(String::as_str).unwrap_or("");
        let short = name.rsplit('.').next().unwrap_or(name);
        Ok(match short {
            // EnteredVolume(world id, "DISTRICT|name"): entered while active; a per-sequence objective (mode 5)
            // reads this sequence's list (b841 §2).
            "EnteredVolume" => {
                let Some(Value::Str(volume)) = args.get(1) else { return Err(Error::Type("EnteredVolume needs a volume name")) };
                let Some(id) = (self.volume)(volume) else { return Ok(Value::Bool(false)) };
                let list = if self.objective.definition.activation == Activation::Sequence {
                    &self.objective.records.entered_in_sequence
                } else {
                    &self.objective.records.entered
                };
                Value::Bool(list.contains(&id))
            }
            // IsInVolume(world id, "DISTRICT|name"): the player is in it now (b864 §4).
            "IsInVolume" => {
                let Some(Value::Str(volume)) = args.get(1) else { return Err(Error::Type("IsInVolume needs a volume name")) };
                Value::Bool((self.volume)(volume).is_some_and(|id| self.inside.contains(&id)))
            }
            // 82C652A0: context +8 and +9.
            "IsSequenceNewlyCompletePoints" => Value::Bool(self.ctx.sequence && self.ctx.newly_complete),
            "GetGroupAccumulatedPoints" => num(self.ctx.group_points),
            "GetGroupPointsRequirement" => num(self.ctx.requirement),
            "GetAccumulatedPoints" => num(self.objective.points),
            // 82C630A0: true for an inactive, armed or active objective (an inactive one activates).
            "ActivateObjective" => {
                let i = index(args)?;
                let ok = matches!(self.statuses.get(i), Some(State::Inactive | State::Armed | State::Active));
                if ok {
                    self.flow.push(FlowCommand::Activate(i));
                }
                Value::Bool(ok)
            }
            "DeactivateObjective" => {
                self.flow.push(FlowCommand::Deactivate(index(args)?));
                Value::Nil
            }
            "ResetObjective" => {
                self.flow.push(FlowCommand::Reset(index(args)?));
                Value::Nil
            }
            "ResetAllObjectives" => {
                self.flow.push(FlowCommand::ResetAll);
                Value::Nil
            }
            "IsWipeoutOver" => Value::Bool(self.skater.wipeout_over),
            "DidSpecificBodyTweakThisFrame" => Value::Bool(self.skater.bail_pose == Some(index(args)?)),
            "GetObjectiveStatus" => self.statuses.get(index(args)?).map_or(Value::Nil, |s| num(*s as i32)),
            "GetTotalNumCompletedObjectives" => num(self.statuses.iter().filter(|s| **s == State::Complete).count() as i32),
            "IsInSequence" => Value::Bool(self.skater.in_sequence),
            "SequenceCompleted" => Value::Bool(self.skater.sequence_completed),
            "IsWipingOut" => Value::Bool(self.skater.wiping_out),
            "IsOffBoard" => Value::Bool(self.skater.off_board),
            "WentOffBoard" => Value::Bool(self.skater.went_off_board),
            "WentOnBoard" => Value::Bool(self.skater.went_on_board),
            "IsGrinding" => Value::Bool(self.skater.grinding),
            "IsOnBoard" => Value::Bool(self.skater.on_board),
            "IsOnGround" => Value::Bool(self.skater.on_ground),
            "AirCompleted" => Value::Bool(self.skater.air_completed),
            "IsInAir" => Value::Bool(self.skater.in_air),
            // The raw value as our pad reports it (up positive); retail's sign is in 8255E010, not read [inferred].
            "GetRawRightStickYValue" => Value::Num(self.skater.right_stick_y),
            // Seconds the current manual of that kind has been held, 0 otherwise [inferred: retail's per-trick time].
            "TailManualTime" => num_f(self.skater.manual.filter(|m| m.0).map_or(0.0, |m| m.1)),
            "NoseManualTime" => num_f(self.skater.manual.filter(|m| !m.0).map_or(0.0, |m| m.1)),
            "ManualTime" => num_f(self.skater.manual.map_or(0.0, |m| m.1)),
            // 82C64230 / 82C643F0 / 82C645F0 (research b894, b953): the manual distance is above 0, the nose / tail bit
            // picks the side. Ours measures the manual's time, so the first held tick differs [inferred].
            "IsManualing" => Value::Bool(self.skater.manual.is_some_and(|m| m.1 > 0.0)),
            "IsNoseManualing" => Value::Bool(self.skater.manual.is_some_and(|m| !m.0 && m.1 > 0.0)),
            "IsTailManualing" => Value::Bool(self.skater.manual.is_some_and(|m| m.0 && m.1 > 0.0)),
            "GetNumPushes" => num(self.objective.records.pushes as i32),
            // The current pump controller values [inferred: retail keeps the last pump's per objective].
            "GetPumpingAcceleration" => num_f(self.skater.pump_acceleration),
            "IsLastPumpUserControlled" => Value::Bool(self.skater.last_pump_user),
            // The argument is a decimal retail id string (parsed by 82AE8F78, as the volume ids are).
            "IsGrindingGUID" => {
                let id = name_arg(args)?.trim().parse::<u64>().map_err(|_| Error::Type("a rail id"))?;
                Value::Bool(self.skater.grind_rail.is_some_and(|g| g.contains(&id)))
            }
            "IsLeftTriggerHeld" | "IsRightTriggerHeld" => {
                let threshold = match args.first() { Some(Value::Num(t)) => *t, _ => 0.0 };
                let value = self.skater.triggers[usize::from(short == "IsRightTriggerHeld")];
                Value::Bool(threshold <= value)
            }
            // obj +0x48C / +0x48D: the marker used / set since the last group tick (b894).
            "JustUsedSessionMarker" => Value::Bool(self.objective.records.used_marker),
            "JustSetSessionMarker" => Value::Bool(self.objective.records.set_marker),
            // b863 §2: the objective's tricks since it activated / was reset (82C66810 / 82C668F0), or this frame's
            // records (82C66DB0 / 82C66ED0); a group matches through its members (an unknown group matches nothing).
            "DidSpecificTrick" => Value::Bool(self.objective.records.tricks.contains(&(self.key)(name_arg(args)?))),
            "DidSpecificTrickThisFrame" => Value::Bool(self.tricks_this_frame.contains(&(self.key)(name_arg(args)?))),
            "GetTimeToGrind" => num_f(self.skater.time_to_grind),
            // 82C65FD0: a number argument equal to the board adjust now (a non-number compares a default and is false).
            "IsDoingSpecificBoardAdjust" => match args.first() {
                Some(Value::Num(n)) => Value::Bool(*n == self.skater.board_adjust as f32),
                _ => Value::Bool(false),
            },
            // 82C670D8 / 82C671D8: this frame's began records (the announce edge), exact key or group member.
            "BeganTrick" => Value::Bool(self.tricks_began.contains(&(self.key)(name_arg(args)?))),
            "BeganTrickGroup" => {
                let key = (self.key)(name_arg(args)?);
                Value::Bool(self.trick_groups.get(&key).is_some_and(|m| self.tricks_began.iter().any(|t| m.contains(t))))
            }
            "DidSpecificTrickGroup" | "DidSpecificTrickGroupThisFrame" | "GetNumTricksDoneInTrickGroup" => {
                let empty = std::collections::BTreeSet::new();
                let members = self.trick_groups.get(&(self.key)(name_arg(args)?)).unwrap_or(&empty);
                let count = if short == "DidSpecificTrickGroupThisFrame" {
                    self.tricks_this_frame.iter().filter(|t| members.contains(t)).count()
                } else {
                    self.objective.records.tricks.iter().filter(|t| members.contains(t)).count()
                };
                if short == "GetNumTricksDoneInTrickGroup" { num(count as i32) } else { Value::Bool(count > 0) }
            }
            // Follow the leader (b1006): the nearest AI competitor's distance (FTL rows fail at their max follow
            // distance), or competitor n's (ftl_indu_02 asks for 1). No competitor: 0 [ours: never a failure].
            "GetDistanceToNearestAISkater" => {
                num_f(self.skater.competitor_distances.iter().flatten().copied().reduce(f32::min).unwrap_or(0.0))
            }
            "GetDistanceToSkater" => {
                let n = match args.first() { Some(Value::Num(n)) if *n >= 1.0 => *n as usize, _ => return Err(Error::Type("a competitor number")) };
                num_f(self.skater.competitor_distances.get(n - 1).copied().flatten().unwrap_or(0.0))
            }
            "AddFollowIndicator" | "RemoveFollowIndicator" => {
                let competitor = match args.first() { Some(Value::Num(n)) if *n >= 0.0 => *n as u32, _ => 0 };
                self.flow.push(FlowCommand::FollowIndicator { competitor, on: short == "AddFollowIndicator" });
                Value::Nil
            }
            "SetFractionValue" => {
                let n = |i: usize| match args.get(i) { Some(Value::Num(v)) => *v, _ => 0.0 };
                self.flow.push(FlowCommand::Fraction(n(0), n(1)));
                Value::Nil
            }
            _ => return Err(Error::Api(format!("{name} is not ported"))),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::challenges::objectives::{Definition, Records, MASK_BYTES};
    use crate::challenges::predicate::{condition, CmpOp, Expr, Program, Stmt};

    fn spot(entered: &[u64]) -> Objective {
        let records = Records { entered_in_sequence: entered.iter().copied().collect(), ..Default::default() };
        Objective { definition: Definition { activation: Activation::Sequence, mask: [false; MASK_BYTES], failure_first: false },
            state: State::Active, records, points: 0 }
    }

    #[test]
    fn the_ots_conditions_read_volumes_and_points() {
        let names = ["ObjectiveTrigger.EnteredVolume".to_owned(), "ObjectivePoints.IsSequenceNewlyCompletePoints".to_owned(),
            "GetGroupAccumulatedPoints".to_owned(), "GetGroupPointsRequirement".to_owned()];
        let volume = |n: &str| (n == "DIST_DownTown|ots_a_spotvolume_01").then_some(5u64);
        let entered = Program { locals: 0, body: vec![Stmt::Return(Expr::Api { func: 0, args: vec![
            Expr::Lit(Value::Str("1".into())), Expr::Lit(Value::Str("DIST_DownTown|ots_a_spotvolume_01".into()))] })] };
        let points = Program { locals: 0, body: vec![Stmt::Return(Expr::Cmp(CmpOp::Ge,
            Box::new(Expr::Api { func: 2, args: vec![] }), Box::new(Expr::Api { func: 3, args: vec![] })))] };
        let newly = Program { locals: 0, body: vec![Stmt::Return(Expr::Api { func: 1, args: vec![] })] };
        let ctx = Context { sequence: true, newly_complete: true, group_points: 1200, requirement: 1000 };
        let run = |p: &Program, o: &Objective, ctx: Context| {
            condition(p, &mut ObjectiveApi { names: &names, objective: o, ctx, volume: &volume, inside: &[], statuses: &[],
                flow: &mut Vec::new(), skater: SkaterFacts::default(), tricks_this_frame: &[], tricks_began: &[], trick_groups: &Default::default(),
                key: &|_: &str| 0u64 }, &mut [], &mut [], false)
        };
        assert!(run(&entered, &spot(&[5]), ctx) && !run(&entered, &spot(&[4]), ctx));
        assert!(run(&points, &spot(&[]), ctx));
        assert!(!run(&points, &spot(&[]), Context { group_points: 900, ..ctx }));
        assert!(run(&newly, &spot(&[]), ctx) && !run(&newly, &spot(&[]), Context { newly_complete: false, ..ctx }));
        assert!(ported("ObjectiveTrigger.IsInVolume") && !ported("Tricks.DidSpecificTrickgroup"), "retail's typo stays unported");
    }

    #[test]
    fn trick_calls_match_keys_and_group_members() {
        let names = ["Tricks.DidSpecificTrick".to_owned(), "Tricks.DidSpecificTrickGroup".to_owned(),
            "Tricks.DidSpecificTrickGroupThisFrame".to_owned()];
        let key = |n: &str| -> u64 { match n { "kickflip" => 1, "fliptricks" => 10, "grabs" => 20, _ => 99 } };
        let groups = std::collections::BTreeMap::from([(10u64, [1u64, 2].into()), (20u64, [5u64].into())]);
        let mut o = spot(&[]);
        o.records.tricks.push_back(1);
        let call = |func: u16, arg: &str, o: &Objective, frame: &[u64]| {
            let p = Program { locals: 0, body: vec![Stmt::Return(Expr::Api { func, args: vec![Expr::Lit(Value::Str(arg.into()))] })] };
            condition(&p, &mut ObjectiveApi { names: &names, objective: o, ctx: Context::default(), volume: &|_: &str| None, inside: &[],
                statuses: &[], flow: &mut Vec::new(), skater: SkaterFacts::default(), tricks_this_frame: frame, tricks_began: &[],
                trick_groups: &groups, key: &key }, &mut [], &mut [], false)
        };
        assert!(call(0, "kickflip", &o, &[]) && !call(0, "heelflip", &o, &[]));
        assert!(call(1, "fliptricks", &o, &[]) && !call(1, "grabs", &o, &[]));
        assert!(call(2, "grabs", &o, &[5]) && !call(2, "fliptricks", &o, &[5]));
    }

    #[test]
    fn leader_calls_read_competitor_distances_and_push_the_indicator() {
        let names = ["Leader.GetDistanceToNearestAISkater".to_owned(), "Leader.GetDistanceToSkater".to_owned(),
            "Leader.AddFollowIndicator".to_owned(), "Leader.RemoveFollowIndicator".to_owned()];
        let o = spot(&[]);
        let far = |func: u16, args: Vec<Expr>, limit: f32, d: [Option<f32>; 4], flow: &mut Vec<FlowCommand>| {
            let p = Program { locals: 0, body: vec![Stmt::Return(Expr::Cmp(CmpOp::Ge,
                Box::new(Expr::Api { func, args }), Box::new(Expr::Lit(Value::Num(limit)))))] };
            condition(&p, &mut ObjectiveApi { names: &names, objective: &o, ctx: Context::default(), volume: &|_: &str| None,
                inside: &[], statuses: &[], flow, skater: SkaterFacts { competitor_distances: d, ..Default::default() },
                tricks_this_frame: &[], tricks_began: &[], trick_groups: &Default::default(), key: &|_: &str| 0u64 },
                &mut [], &mut [], false)
        };
        let mut flow = Vec::new();
        let d = [Some(60.0), Some(30.0), None, None];
        assert!(!far(0, vec![], 40.0, d, &mut flow), "nearest competitor is 30 m away");
        assert!(far(0, vec![], 40.0, [Some(60.0), None, None, None], &mut flow));
        assert!(far(1, vec![Expr::Lit(Value::Num(1.0))], 50.0, d, &mut flow) && !far(1, vec![Expr::Lit(Value::Num(2.0))], 50.0, d, &mut flow));
        assert!(!far(0, vec![], 40.0, [None; 4], &mut flow), "no competitor never fails");
        let call = |func: u16, flow: &mut Vec<FlowCommand>| {
            let p = Program { locals: 0, body: vec![Stmt::Call(Expr::Api { func, args: vec![Expr::Lit(Value::Num(1.0))] }), Stmt::Return(Expr::Lit(Value::Bool(true)))] };
            condition(&p, &mut ObjectiveApi { names: &names, objective: &o, ctx: Context::default(), volume: &|_: &str| None,
                inside: &[], statuses: &[], flow, skater: SkaterFacts::default(), tricks_this_frame: &[], tricks_began: &[],
                trick_groups: &Default::default(), key: &|_: &str| 0u64 }, &mut [], &mut [], false)
        };
        call(2, &mut flow);
        call(3, &mut flow);
        assert_eq!(flow, vec![FlowCommand::FollowIndicator { competitor: 1, on: true }, FlowCommand::FollowIndicator { competitor: 1, on: false }]);
    }
}
