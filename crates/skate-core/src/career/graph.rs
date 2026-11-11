//! Progression graphs as data and their runner (research b888 / b889 / b890 / b891; doc 43). Each graph is states by
//! name with handlers by message; a handler is the op list setup translated from retail's compiled Lua. One graph runs
//! at a time (FIFO queue); a graph ends when it enters `complete` (after that state's stateenter ops); the next queued
//! graph starts on the next update. Presentation ops come out as effects and the host answers with the reply message.
use crate::challenges::Completion;
use std::collections::{BTreeMap, VecDeque};

pub const INITIAL: &str = "initial";
pub const COMPLETE: &str = "complete";
pub const STATE_ENTER: &str = "stateenter";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventMode {
    /// ChangeState: no presentation events.
    None,
    /// ExecuteEventsAndChangeState (827D16F0): the state's events play as a group and the state changes in the group's
    /// finish callback (827D1420); a state without events changes at once.
    Execute,
    /// DeferEventsAndChangeState (827D1970): the events go to the deferred list (played later, FIFO), the state changes
    /// at once.
    Defer,
}

/// A challenge named by the op, or the one the school exit picked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Named(String),
    Picked,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cond {
    /// The flag is explicitly false (an unset flag is not false: retail tests `== false`).
    FlagFalse(String),
    /// The reply carries the request id this graph instance stored.
    ReplyMatches,
    CompletionIs { challenge: String, level: Completion },
    PickedNone,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    ChangeState { state: String, events: EventMode },
    ArmAction(String),
    UnlockReward { reward: String, big: bool },
    ActivateChallenge(String),
    DeactivateChallenge(String),
    ActivateGroup(String),
    BrandTier(String),
    GrantAchievement(String),
    SkateFeed(String),
    AutoSignUp(Target),
    UnlockTeammate,
    MilestoneEarned,
    AllowDiscovery,
    /// Discovery.SetDiscoveryStatus(challenge, eDiscoveryStatus.<status>).
    SetDiscovery { challenge: String, status: String },
    HideScreen,
    UnhideScreen,
    Show101 { title: String, description: String },
    ShowYesNo { title: String, description: String },
    LoadCareerPhoto { challenge: String },
    UnloadCareerPhoto(String),
    Teleport { locator: String, district: String },
    TeleportToChallenge(Target),
    SetFlag { name: String, value: bool },
    Pick(String),
    If { cond: Cond, then: Vec<Op>, otherwise: Vec<Op> },
}

/// A presentation event (research b892): its type name (b874 table), two string args and the mask.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Event {
    pub kind: String,
    pub args: [Option<String>; 2],
    pub mask: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// The presentation events, in play order (played on Execute / Defer).
    pub events: Vec<Event>,
    pub on: BTreeMap<String, Vec<Op>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Graph {
    pub states: BTreeMap<String, State>,
}

/// What a graph asks of the game: everything except the runner's own bookkeeping and the career flags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    ArmAction(String),
    UnlockReward { reward: String, big: bool },
    ActivateChallenge(String),
    DeactivateChallenge(String),
    ActivateGroup(String),
    BrandTier(String),
    GrantAchievement(String),
    SkateFeed(String),
    AutoSignUp(String),
    UnlockTeammate,
    MilestoneEarned,
    AllowDiscovery,
    SetDiscovery { challenge: String, status: String },
    HideScreen,
    UnhideScreen,
    /// Presentation the host answers with the reply message (M1 answers at once, logged NOT RETAIL YET).
    Show101 { title: String, description: String },
    ShowYesNo { title: String, description: String },
    LoadCareerPhoto { challenge: String, request: u32 },
    UnloadCareerPhoto(String),
    Teleport { locator: String, district: String },
    TeleportToChallenge(String),
    /// A state's presentation events (Execute plays them now, Defer queues them for later). For Execute the graph waits
    /// in `state` until the host calls [`Runner::events_finished`], then enters `target`.
    Events { graph: String, state: String, mode: EventMode, target: String },
    GraphStarted(String),
    GraphEnded(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Running {
    pub graph: String,
    pub state: String,
    pub picked: Option<String>,
    /// ExecuteEventsAndChangeState's target while the state's events play.
    pub waiting: Option<String>,
    pub request: Option<u32>,
}

/// One player's graph runner, the career flags and the request counter for career photo loads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Runner {
    pub running: Option<Running>,
    pub queue: VecDeque<String>,
    pub flags: BTreeMap<String, bool>,
    pub next_request: u32,
}

/// What the runner reads about the career while it runs ops.
pub trait CareerView {
    fn completion(&self, challenge: &str) -> Option<Completion>;
}

impl Runner {
    /// Queues the graph when one runs or others wait, else starts it at once.
    pub fn start(&mut self, graph: &str, graphs: &BTreeMap<String, Graph>, view: &dyn CareerView, out: &mut Vec<Request>) {
        if self.running.is_some() || !self.queue.is_empty() {
            self.queue.push_back(graph.to_owned());
        } else {
            self.begin(graph, graphs, view, out);
        }
    }

    /// The manager's update: when nothing runs, the next queued graph starts.
    pub fn update(&mut self, graphs: &BTreeMap<String, Graph>, view: &dyn CareerView, out: &mut Vec<Request>) {
        if self.running.is_none() {
            if let Some(next) = self.queue.pop_front() {
                self.begin(&next, graphs, view, out);
            }
        }
    }

    fn begin(&mut self, graph: &str, graphs: &BTreeMap<String, Graph>, view: &dyn CareerView, out: &mut Vec<Request>) {
        out.push(Request::GraphStarted(graph.to_owned()));
        // A graph without states never ends and blocks the queue, as in retail (b890; unreachable in retail data).
        self.running = Some(Running { graph: graph.to_owned(), state: INITIAL.into(), picked: None, waiting: None, request: None });
        if graphs.get(graph).is_some_and(|g| g.states.contains_key(INITIAL)) {
            self.deliver(STATE_ENTER, None, graphs, view, out);
        }
    }

    /// A message to the running graph's current state (stateenter, or a presentation reply such as hudcomplete /
    /// hudyes / hudno / challengeteambrandimagecomplete with the request id).
    pub fn deliver(&mut self, message: &str, request: Option<u32>, graphs: &BTreeMap<String, Graph>,
                   view: &dyn CareerView, out: &mut Vec<Request>) {
        let Some(run) = self.running.as_ref() else { return };
        let Some(ops) = graphs.get(&run.graph).and_then(|g| g.states.get(&run.state)).and_then(|s| s.on.get(message)).cloned()
        else { return };
        let mut change = None;
        self.run_ops(&ops, request, view, out, &mut change);
        if let Some((state, mode)) = change {
            self.change_state(&state, mode, graphs, view, out);
        }
    }

    fn change_state(&mut self, state: &str, mode: EventMode, graphs: &BTreeMap<String, Graph>, view: &dyn CareerView,
                    out: &mut Vec<Request>) {
        let Some(run) = self.running.as_mut() else { return };
        if mode != EventMode::None {
            out.push(Request::Events { graph: run.graph.clone(), state: run.state.clone(), mode, target: state.to_owned() });
        }
        let has_events = graphs.get(&run.graph).and_then(|g| g.states.get(&run.state)).is_some_and(|s| !s.events.is_empty());
        if mode == EventMode::Execute && has_events {
            run.waiting = Some(state.to_owned());
            return;
        }
        self.enter(state, graphs, view, out);
    }

    /// The Execute group finished (its callback 827D1420): the waiting state change happens now.
    pub fn events_finished(&mut self, graphs: &BTreeMap<String, Graph>, view: &dyn CareerView, out: &mut Vec<Request>) {
        let Some(target) = self.running.as_mut().and_then(|r| r.waiting.take()) else { return };
        self.enter(&target, graphs, view, out);
    }

    fn enter(&mut self, state: &str, graphs: &BTreeMap<String, Graph>, view: &dyn CareerView, out: &mut Vec<Request>) {
        let Some(run) = self.running.as_mut() else { return };
        run.state = state.to_owned();
        let graph = run.graph.clone();
        self.deliver(STATE_ENTER, None, graphs, view, out);
        if state == COMPLETE && self.running.as_ref().is_some_and(|r| r.state == COMPLETE) {
            self.running = None;
            out.push(Request::GraphEnded(graph));
        }
    }

    fn target(&self, t: &Target) -> String {
        match t {
            Target::Named(c) => c.clone(),
            Target::Picked => self.running.as_ref().and_then(|r| r.picked.clone()).unwrap_or_default(),
        }
    }

    fn holds(&self, cond: &Cond, request: Option<u32>, view: &dyn CareerView) -> bool {
        match cond {
            Cond::FlagFalse(name) => self.flags.get(name) == Some(&false),
            Cond::ReplyMatches => request.is_some() && self.running.as_ref().is_some_and(|r| r.request == request),
            Cond::CompletionIs { challenge, level } => view.completion(challenge) == Some(*level),
            Cond::PickedNone => self.running.as_ref().is_some_and(|r| r.picked.is_none()),
        }
    }

    fn run_ops(&mut self, ops: &[Op], request: Option<u32>, view: &dyn CareerView, out: &mut Vec<Request>,
               change: &mut Option<(String, EventMode)>) {
        for op in ops {
            match op {
                Op::ChangeState { state, events } => *change = Some((state.clone(), *events)),
                Op::SetFlag { name, value } => {
                    self.flags.insert(name.clone(), *value);
                }
                Op::Pick(c) => {
                    if let Some(r) = self.running.as_mut() {
                        r.picked = Some(c.clone());
                    }
                }
                Op::If { cond, then, otherwise } => {
                    let branch = if self.holds(cond, request, view) { then } else { otherwise };
                    self.run_ops(branch, request, view, out, change);
                }
                Op::LoadCareerPhoto { challenge } => {
                    self.next_request += 1;
                    let id = self.next_request;
                    if let Some(r) = self.running.as_mut() {
                        r.request = Some(id);
                    }
                    out.push(Request::LoadCareerPhoto { challenge: challenge.clone(), request: id });
                }
                Op::AutoSignUp(t) => out.push(Request::AutoSignUp(self.target(t))),
                Op::TeleportToChallenge(t) => out.push(Request::TeleportToChallenge(self.target(t))),
                Op::ArmAction(a) => out.push(Request::ArmAction(a.clone())),
                Op::UnlockReward { reward, big } => out.push(Request::UnlockReward { reward: reward.clone(), big: *big }),
                Op::ActivateChallenge(c) => out.push(Request::ActivateChallenge(c.clone())),
                Op::DeactivateChallenge(c) => out.push(Request::DeactivateChallenge(c.clone())),
                Op::ActivateGroup(g) => out.push(Request::ActivateGroup(g.clone())),
                Op::BrandTier(t) => out.push(Request::BrandTier(t.clone())),
                Op::GrantAchievement(n) => out.push(Request::GrantAchievement(n.clone())),
                Op::SkateFeed(t) => out.push(Request::SkateFeed(t.clone())),
                Op::UnlockTeammate => out.push(Request::UnlockTeammate),
                Op::MilestoneEarned => out.push(Request::MilestoneEarned),
                Op::AllowDiscovery => out.push(Request::AllowDiscovery),
                Op::SetDiscovery { challenge, status } => out.push(Request::SetDiscovery { challenge: challenge.clone(), status: status.clone() }),
                Op::HideScreen => out.push(Request::HideScreen),
                Op::UnhideScreen => out.push(Request::UnhideScreen),
                Op::Show101 { title, description } => out.push(Request::Show101 { title: title.clone(), description: description.clone() }),
                Op::ShowYesNo { title, description } => out.push(Request::ShowYesNo { title: title.clone(), description: description.clone() }),
                Op::UnloadCareerPhoto(c) => out.push(Request::UnloadCareerPhoto(c.clone())),
                Op::Teleport { locator, district } => out.push(Request::Teleport { locator: locator.clone(), district: district.clone() }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoCareer;
    impl CareerView for NoCareer {
        fn completion(&self, _: &str) -> Option<Completion> {
            None
        }
    }

    fn state(on: &[(&str, Vec<Op>)]) -> State {
        State { events: Vec::new(), on: on.iter().map(|(m, ops)| (m.to_string(), ops.clone())).collect() }
    }

    fn milestone() -> Graph {
        Graph { states: BTreeMap::from([
            (INITIAL.to_owned(), state(&[(STATE_ENTER, vec![
                Op::UnlockReward { reward: "milestone_01".into(), big: true },
                Op::MilestoneEarned,
                Op::UnlockTeammate,
                Op::ChangeState { state: COMPLETE.into(), events: EventMode::Defer }])])),
            (COMPLETE.to_owned(), state(&[(STATE_ENTER, vec![Op::SetFlag { name: "Done".into(), value: true }])])),
        ]) }
    }

    #[test]
    fn execute_waits_for_the_states_events_and_a_state_without_events_changes_at_once() {
        let mut initial = state(&[(STATE_ENTER, vec![Op::ChangeState { state: COMPLETE.into(), events: EventMode::Execute }])]);
        initial.events = vec![Event { kind: "Wait".into(), args: [Some("1".into()), None], mask: 0 }];
        let complete = state(&[(STATE_ENTER, vec![Op::SetFlag { name: "Done".into(), value: true }])]);
        let graphs = BTreeMap::from([("g".to_owned(), Graph { states: BTreeMap::from([
            (INITIAL.to_owned(), initial), (COMPLETE.to_owned(), complete.clone())]) })]);
        let mut r = Runner::default();
        let mut out = Vec::new();
        r.start("g", &graphs, &NoCareer, &mut out);
        assert!(out.contains(&Request::Events { graph: "g".into(), state: INITIAL.into(), mode: EventMode::Execute, target: COMPLETE.into() }));
        assert_eq!(r.running.as_ref().map(|x| (x.state.as_str(), x.waiting.as_deref())), Some((INITIAL, Some(COMPLETE))), "holds the state");
        assert_eq!(r.flags.get("Done"), None);
        out.clear();
        r.events_finished(&graphs, &NoCareer, &mut out);
        assert_eq!(r.flags.get("Done"), Some(&true));
        assert_eq!(out, [Request::GraphEnded("g".into())]);
        // The same op on a state without events: the change happens in the same call.
        let quiet = BTreeMap::from([("q".to_owned(), Graph { states: BTreeMap::from([
            (INITIAL.to_owned(), state(&[(STATE_ENTER, vec![Op::ChangeState { state: COMPLETE.into(), events: EventMode::Execute }])])),
            (COMPLETE.to_owned(), complete)]) })]);
        let mut r = Runner::default();
        r.start("q", &quiet, &NoCareer, &mut out);
        assert!(r.running.is_none() && r.flags.get("Done") == Some(&true));
    }

    #[test]
    fn a_milestone_graph_runs_to_complete_and_ends() {
        let graphs = BTreeMap::from([("m1".to_owned(), milestone())]);
        let mut r = Runner::default();
        let mut out = Vec::new();
        r.start("m1", &graphs, &NoCareer, &mut out);
        assert_eq!(out, [
            Request::GraphStarted("m1".into()),
            Request::UnlockReward { reward: "milestone_01".into(), big: true },
            Request::MilestoneEarned,
            Request::UnlockTeammate,
            Request::Events { graph: "m1".into(), state: INITIAL.into(), mode: EventMode::Defer, target: COMPLETE.into() },
            Request::GraphEnded("m1".into())]);
        assert_eq!(r.flags.get("Done"), Some(&true), "complete's stateenter ops run before the end");
        assert!(r.running.is_none());
    }

    #[test]
    fn graphs_queue_and_the_next_starts_on_the_update() {
        let card = Graph { states: BTreeMap::from([
            (INITIAL.to_owned(), state(&[
                (STATE_ENTER, vec![Op::Show101 { title: "T".into(), description: "D".into() }]),
                ("hudcomplete", vec![Op::ChangeState { state: COMPLETE.into(), events: EventMode::None }])])),
            (COMPLETE.to_owned(), State::default()),
        ]) };
        let graphs = BTreeMap::from([("card".to_owned(), card), ("m1".to_owned(), milestone())]);
        let mut r = Runner::default();
        let mut out = Vec::new();
        r.start("card", &graphs, &NoCareer, &mut out);
        r.start("m1", &graphs, &NoCareer, &mut out);
        assert_eq!(r.queue, ["m1"], "waits while the card is up");
        r.update(&graphs, &NoCareer, &mut out);
        assert_eq!(r.queue, ["m1"]);
        r.deliver("hudcomplete", None, &graphs, &NoCareer, &mut out);
        assert!(r.running.is_none());
        r.update(&graphs, &NoCareer, &mut out);
        assert!(r.queue.is_empty() && r.running.is_none(), "m1 ran to complete on this update");
    }

    #[test]
    fn the_school_exit_picks_the_first_false_flag() {
        let pick = |flag: &str, c: &str| Op::If { cond: Cond::FlagFalse(flag.into()), then: vec![Op::Pick(c.into())], otherwise: vec![] };
        let exit = Graph { states: BTreeMap::from([
            (INITIAL.to_owned(), state(&[("hudyes", vec![
                Op::If { cond: Cond::FlagFalse("A".into()), then: vec![Op::Pick("ftl_a".into())],
                    otherwise: vec![pick("B", "ftl_b")] },
                Op::If { cond: Cond::PickedNone, then: vec![Op::Teleport { locator: "gamestart".into(), district: "uni".into() }],
                    otherwise: vec![Op::TeleportToChallenge(Target::Picked), Op::AutoSignUp(Target::Picked)] },
                Op::ChangeState { state: COMPLETE.into(), events: EventMode::None }])])),
            (COMPLETE.to_owned(), State::default()),
        ]) };
        let graphs = BTreeMap::from([("exit".to_owned(), exit)]);
        let mut r = Runner::default();
        r.flags.insert("A".into(), true);
        r.flags.insert("B".into(), false);
        let mut out = Vec::new();
        r.start("exit", &graphs, &NoCareer, &mut out);
        r.deliver("hudyes", None, &graphs, &NoCareer, &mut out);
        assert!(out.contains(&Request::TeleportToChallenge("ftl_b".into())));
        assert!(out.contains(&Request::AutoSignUp("ftl_b".into())));
        let mut unset = Runner::default();
        let mut out = Vec::new();
        unset.start("exit", &graphs, &NoCareer, &mut out);
        unset.deliver("hudyes", None, &graphs, &NoCareer, &mut out);
        assert!(out.contains(&Request::Teleport { locator: "gamestart".into(), district: "uni".into() }),
            "unset flags are not false: nothing picked");
    }

    #[test]
    fn a_career_photo_reply_must_match_the_stored_request() {
        let g = Graph { states: BTreeMap::from([
            (INITIAL.to_owned(), state(&[
                (STATE_ENTER, vec![Op::LoadCareerPhoto { challenge: "photo_a".into() }]),
                ("challengeteambrandimagecomplete", vec![Op::If { cond: Cond::ReplyMatches,
                    then: vec![Op::ChangeState { state: COMPLETE.into(), events: EventMode::None }], otherwise: vec![] }])])),
            (COMPLETE.to_owned(), State::default()),
        ]) };
        let graphs = BTreeMap::from([("photo".to_owned(), g)]);
        let mut r = Runner::default();
        let mut out = Vec::new();
        r.start("photo", &graphs, &NoCareer, &mut out);
        let id = match out[1] { Request::LoadCareerPhoto { request, .. } => request, _ => panic!() };
        r.deliver("challengeteambrandimagecomplete", Some(id + 1), &graphs, &NoCareer, &mut out);
        assert!(r.running.is_some(), "another request's reply is ignored");
        r.deliver("challengeteambrandimagecomplete", Some(id), &graphs, &NoCareer, &mut out);
        assert!(r.running.is_none());
    }

    #[test]
    fn a_graph_without_states_blocks_the_queue_like_retail() {
        let graphs = BTreeMap::from([("empty".to_owned(), Graph::default()), ("m1".to_owned(), milestone())]);
        let mut r = Runner::default();
        let mut out = Vec::new();
        r.start("empty", &graphs, &NoCareer, &mut out);
        r.start("m1", &graphs, &NoCareer, &mut out);
        r.update(&graphs, &NoCareer, &mut out);
        assert_eq!(r.queue, ["m1"]);
    }
}
