//! Career progression, engine independent: the board sales and money counters, progression actions with their AND
//! conditions, the armed / fired tracker and the event-driven checks (research b855, design b887; doc 43). The game
//! loads `progression.json` (setup `progression.py`) into these types.
use crate::challenges::Completion;
use std::collections::BTreeMap;

pub mod achievements;
pub mod boot;
pub mod graph;
pub mod milestones;
pub mod presentation;
pub mod profile;
pub mod team;
pub mod unlocks;

/// The six counters of retail's progression manager (+0x48, b855): owned it, killed it, bonus and set board sales,
/// money now and money earned. Offline play keeps `sales_bonus` and `sales_set` at 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    pub sales_owned: u64,
    pub sales_killed: u64,
    pub sales_bonus: u64,
    pub sales_set: u64,
    pub money: u64,
    pub money_earned: u64,
}

impl Counters {
    /// Board sales: the sum the milestones, branding levels and the crossbar read (condition type 0).
    pub fn board_sales(&self) -> u64 {
        self.sales_owned + self.sales_killed + self.sales_bonus + self.sales_set
    }
    pub fn add_money(&mut self, amount: u64) {
        self.money += amount;
        self.money_earned += amount;
    }
    /// Spending clamps at 0 (827CEE00).
    pub fn spend(&mut self, amount: u64) {
        self.money = self.money.saturating_sub(amount);
    }
}

/// One condition of an action (82C1CA48; types 0..5, anything above leaves the result true).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Condition {
    BoardSales(u64),
    GroupDone { group: String, at_least: u64 },
    ChallengeDone(String),
    Never,
    MoneyEarned(u64),
    Money(u64),
    Ignored(u32),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Action {
    pub id: String,
    /// The bank row it inherits from (milestones: `milestone_actions`).
    pub parent: Option<String>,
    pub conditions: Vec<Condition>,
    /// The progression graph the action starts.
    pub graph: Option<String>,
    /// Kept on the fired list (only `starting_action` in retail data).
    pub record_fired: bool,
}

/// What one action check reads: the counters, challenge statuses and group members.
pub struct World<'a> {
    pub counters: &'a Counters,
    pub status: &'a BTreeMap<String, Completion>,
    pub groups: &'a BTreeMap<String, Vec<String>>,
}

impl World<'_> {
    fn done(&self, challenge: &str) -> bool {
        matches!(self.status.get(challenge), Some(Completion::Owned | Completion::Killed))
    }
}

impl Condition {
    pub fn holds(&self, w: &World) -> bool {
        match self {
            Condition::BoardSales(n) => w.counters.board_sales() >= *n,
            Condition::GroupDone { group, at_least } => {
                w.groups.get(group).map_or(0, |m| m.iter().filter(|c| w.done(c)).count() as u64) >= *at_least
            }
            Condition::ChallengeDone(id) => w.done(id),
            Condition::Never => false,
            Condition::MoneyEarned(n) => w.counters.money_earned >= *n,
            Condition::Money(n) => w.counters.money >= *n,
            Condition::Ignored(_) => true,
        }
    }
}

impl Action {
    /// All conditions AND-ed; an action without conditions holds at once.
    pub fn holds(&self, w: &World) -> bool {
        self.conditions.iter().all(|c| c.holds(w))
    }
    fn watches_sales(&self) -> bool {
        self.conditions.iter().any(|c| matches!(c, Condition::BoardSales(_)))
    }
    fn watches_money(&self) -> bool {
        self.conditions.iter().any(|c| matches!(c, Condition::Money(_) | Condition::MoneyEarned(_)))
    }
    fn watches_challenge(&self, challenge: &str, groups: &BTreeMap<String, Vec<String>>) -> bool {
        self.conditions.iter().any(|c| match c {
            Condition::ChallengeDone(id) => id == challenge,
            Condition::GroupDone { group, .. } => groups.get(group).is_some_and(|m| m.iter().any(|x| x == challenge)),
            _ => false,
        })
    }
}

/// What made the tracker look at its armed actions (retail checks on events, not every frame: b855).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// A challenge's first owned or killed result (82C1C698).
    ChallengeDone(String),
    /// Board sales changed (82C1C2A0).
    Sales,
    /// Money changed (82C1C498).
    Money,
}

/// The armed and fired action lists (tracker +0x04 / +0x14). Order is the arm order, so checks are deterministic.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tracker {
    pub armed: Vec<String>,
    pub fired: Vec<String>,
}

impl Tracker {
    /// ArmAction (82C1BF50, research b888, checked): an unknown or already armed action is left alone; otherwise its
    /// conditions are checked at once, and an action that holds fires without ever being armed (returns true); one
    /// that does not goes to the FRONT of the armed list.
    pub fn arm(&mut self, id: &str, actions: &BTreeMap<String, Action>, w: &World) -> bool {
        let Some(a) = actions.get(id) else { return false };
        if self.armed.iter().any(|x| x == id) {
            return false;
        }
        if a.holds(w) {
            if a.record_fired {
                self.fired.push(id.to_owned());
            }
            return true;
        }
        self.armed.insert(0, id.to_owned());
        false
    }

    /// The armed actions the trigger concerns whose conditions now hold: they leave the armed list (82C1DCE0 after the
    /// walk, research b888) and their ids come back in list order.
    pub fn check(&mut self, trigger: &Trigger, actions: &BTreeMap<String, Action>, w: &World) -> Vec<String> {
        let mut fired = Vec::new();
        self.armed.retain(|id| {
            let Some(a) = actions.get(id) else { return true };
            let concerned = match trigger {
                Trigger::ChallengeDone(c) => a.watches_challenge(c, w.groups),
                Trigger::Sales => a.watches_sales(),
                Trigger::Money => a.watches_money(),
            };
            if concerned && a.holds(w) {
                fired.push(id.clone());
                false
            } else {
                true
            }
        });
        for id in &fired {
            if actions.get(id).is_some_and(|a| a.record_fired) {
                self.fired.push(id.clone());
            }
        }
        fired
    }
}

/// One owned it or killed it reward of a challenge (tObservatoryProgressionReward: money, board sales, reward ids).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reward {
    pub money: u64,
    pub sales: u64,
    pub rewards: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChallengeRewards {
    pub owned: Reward,
    pub killed: Reward,
}

/// The exported progression data (`progression.json`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progression {
    pub actions: BTreeMap<String, Action>,
    pub groups: BTreeMap<String, Vec<String>>,
    pub challenge_rewards: BTreeMap<String, ChallengeRewards>,
    /// Challenges with a killed it goal (b895 §C).
    pub killable: std::collections::BTreeSet<String>,
    pub config: CareerConfig,
}

/// Career rows named by the data (setup writes the retail ones; a mod overlay can change them).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CareerConfig {
    /// Actions whose parent chain reaches this row are the milestones (b873).
    pub milestone_parent: String,
    /// The crossbar index is -2 while the first has no status, -1 while the second has none (b895 §B).
    pub crossbar_gates: Vec<String>,
}

/// What a career change did; serialisable by the game for logs, saves and multiplayer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// A challenge's reward paid out: money, board sales and the reward ids to grant.
    Paid { challenge: String, tier: Completion, money: u64, sales: u64, rewards: Vec<String> },
    StatusChanged { challenge: String, completion: Completion },
    ActionFired { action: String, graph: Option<String> },
}

/// One player's career state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Career {
    pub counters: Counters,
    pub status: BTreeMap<String, Completion>,
    pub tracker: Tracker,
    pub runner: graph::Runner,
    pub pending: Pending,
    pub team: team::Team,
    pub unlocks: unlocks::Unlocks,
    /// Challenges the career activated (ActivateChallenge / ActivateGroup, minus DeactivateChallenge): with gating
    /// on, only these can be discovered and signed up for.
    pub active: std::collections::BTreeSet<String>,
    /// Deferred presentation events (DeferEventsAndChangeState), played FIFO when the presentation releases.
    pub deferred: std::collections::VecDeque<graph::Event>,
    /// This result's cash and board sales for the reward summary, cleared when it closes.
    pub summary: presentation::Totals,
}

/// Earned but not presented yet: the reward summary's milestone (+0x04) and teammate (+0x18) counts, cleared when
/// the reward presentation ends (b893, b895 §B).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pending {
    pub milestones: u32,
    pub teammates: u32,
}

struct StatusView<'a>(&'a BTreeMap<String, Completion>);

impl graph::CareerView for StatusView<'_> {
    fn completion(&self, challenge: &str) -> Option<Completion> {
        self.0.get(challenge).copied()
    }
}

fn rank(c: Option<Completion>) -> u8 {
    match c {
        Some(Completion::Killed) => 2,
        Some(Completion::Owned) => 1,
        _ => 0,
    }
}

impl Career {
    /// A new or loaded career: the actions with the auto-start flag that have not fired yet are armed (827CEA60,
    /// research b888: only `starting_action` in retail data, which fires at once and starts its graph).
    pub fn start(&mut self, data: &Progression, graphs: &BTreeMap<String, graph::Graph>) -> Vec<graph::Request> {
        let mut effects = Vec::new();
        for (id, action) in &data.actions {
            if action.record_fired && !self.tracker.fired.contains(id) {
                effects.extend(self.arm(id, data));
            }
        }
        self.drive(effects, data, graphs)
    }

    /// Fired actions start their graphs; a graph's ArmAction arms through the career (which can fire more actions,
    /// whose graphs queue); every other request goes to the game.
    pub fn drive(&mut self, effects: Vec<Effect>, data: &Progression, graphs: &BTreeMap<String, graph::Graph>)
                 -> Vec<graph::Request> {
        let mut pending = std::collections::VecDeque::new();
        self.start_graphs(effects, graphs, &mut pending);
        self.settle(pending, data, graphs)
    }

    /// The manager's update: the next queued graph starts when none runs.
    pub fn update(&mut self, data: &Progression, graphs: &BTreeMap<String, graph::Graph>) -> Vec<graph::Request> {
        let mut out = Vec::new();
        self.runner.update(graphs, &StatusView(&self.status), &mut out);
        self.settle(out.into(), data, graphs)
    }

    /// A presentation reply (or any message) to the running graph.
    pub fn deliver(&mut self, message: &str, request: Option<u32>, data: &Progression,
                   graphs: &BTreeMap<String, graph::Graph>) -> Vec<graph::Request> {
        let mut out = Vec::new();
        self.runner.deliver(message, request, graphs, &StatusView(&self.status), &mut out);
        self.settle(out.into(), data, graphs)
    }

    fn start_graphs(&mut self, effects: Vec<Effect>, graphs: &BTreeMap<String, graph::Graph>,
                    pending: &mut std::collections::VecDeque<graph::Request>) {
        for e in effects {
            if let Effect::ActionFired { graph: Some(g), .. } = e {
                let mut out = Vec::new();
                self.runner.start(&g, graphs, &StatusView(&self.status), &mut out);
                pending.extend(out);
            }
        }
    }

    fn settle(&mut self, mut pending: std::collections::VecDeque<graph::Request>, data: &Progression,
              graphs: &BTreeMap<String, graph::Graph>) -> Vec<graph::Request> {
        let mut out = Vec::new();
        while let Some(request) = pending.pop_front() {
            match request {
                graph::Request::ArmAction(action) => {
                    let effects = self.arm(&action, data);
                    self.start_graphs(effects, graphs, &mut pending);
                }
                graph::Request::MilestoneEarned => {
                    self.pending.milestones += 1;
                    out.push(graph::Request::MilestoneEarned);
                }
                graph::Request::UnlockTeammate => {
                    if self.team.unlock_teammate().is_some() {
                        self.pending.teammates += 1;
                    }
                    out.push(graph::Request::UnlockTeammate);
                }
                graph::Request::BrandTier(tier) => {
                    self.team.brand_tier(&tier);
                    out.push(graph::Request::BrandTier(tier));
                }
                graph::Request::ActivateChallenge(id) => {
                    self.activate(&id);
                    out.push(graph::Request::ActivateChallenge(id));
                }
                graph::Request::DeactivateChallenge(id) => {
                    self.active.remove(&id);
                    out.push(graph::Request::DeactivateChallenge(id));
                }
                graph::Request::ActivateGroup(group) => {
                    for id in data.groups.get(&group).into_iter().flatten() {
                        self.activate(id);
                    }
                    out.push(graph::Request::ActivateGroup(group));
                }
                other => out.push(other),
            }
        }
        out
    }

    fn fire(&mut self, trigger: Trigger, data: &Progression, out: &mut Vec<Effect>) {
        let w = World { counters: &self.counters, status: &self.status, groups: &data.groups };
        let mut tracker = std::mem::take(&mut self.tracker);
        for action in tracker.check(&trigger, &data.actions, &w) {
            let graph = data.actions.get(&action).and_then(|a| a.graph.clone());
            out.push(Effect::ActionFired { action, graph });
        }
        self.tracker = tracker;
    }

    /// Board sales from a mod (sdk.career): into the bonus counter (online only in retail, so 0 offline), then the
    /// sales checks, as a challenge payout runs them.
    pub fn add_sales(&mut self, amount: u64, data: &Progression) -> Vec<Effect> {
        let mut out = Vec::new();
        if amount > 0 {
            self.counters.sales_bonus += amount;
            self.fire(Trigger::Sales, data, &mut out);
        }
        out
    }

    /// Money from a mod (sdk.career), then the money checks.
    pub fn add_money(&mut self, amount: u64, data: &Progression) -> Vec<Effect> {
        let mut out = Vec::new();
        if amount > 0 {
            self.counters.add_money(amount);
            self.fire(Trigger::Money, data, &mut out);
        }
        out
    }

    /// Arms an action (ArmAction); an action that already holds fires at once.
    pub fn arm(&mut self, action: &str, data: &Progression) -> Vec<Effect> {
        let w = World { counters: &self.counters, status: &self.status, groups: &data.groups };
        let mut tracker = std::mem::take(&mut self.tracker);
        let fired = tracker.arm(action, &data.actions, &w);
        self.tracker = tracker;
        if fired {
            vec![Effect::ActionFired { action: action.to_owned(), graph: data.actions.get(action).and_then(|a| a.graph.clone()) }]
        } else {
            Vec::new()
        }
    }

    /// Retail Activate(key, force 0) (82C1D7D0 via career vt+0x14, research b846 / b908): a key that is already
    /// active or completed is skipped. Graph activations and discovery (b910) both come through here.
    pub fn activate(&mut self, id: &str) -> bool {
        if self.active.contains(id) || self.status.get(id).is_some_and(|c| *c != Completion::Abandoned) {
            return false;
        }
        self.active.insert(id.to_owned())
    }

    /// The active list after a result (CompleteChallenge 8283FD00, research b918, checked): an owned / killed result
    /// always takes the key off the list (82C1D878); with `reactivate` (own the spot passes it on every result,
    /// tutorials pass their Persistent field, every other family nothing) it goes straight back in, forced, so an
    /// owned spot stays signable for killed it. Abandoned changes nothing.
    pub fn settle_active(&mut self, challenge: &str, result: Completion, reactivate: bool) {
        if result == Completion::Abandoned {
            return;
        }
        self.active.remove(challenge);
        if reactivate {
            self.active.insert(challenge.to_owned());
        }
    }

    /// A challenge result, in retail's order (827CDDD8, research b855, checked): with no earlier owned / killed result
    /// the owned it reward pays; a kill not killed before pays the killed it reward too (a first kill pays both,
    /// nothing pays twice, abandoned pays nothing). Money and its checks, then board sales and theirs, then the status
    /// (upgrade only) and, on a first owned / killed, the challenge checks.
    pub fn challenge_result(&mut self, challenge: &str, result: Completion, data: &Progression) -> Vec<Effect> {
        let mut out = Vec::new();
        if result == Completion::Abandoned {
            return out;
        }
        let prior = self.status.get(challenge).copied().filter(|c| *c != Completion::Abandoned);
        let empty = ChallengeRewards::default();
        let rewards = data.challenge_rewards.get(challenge).unwrap_or(&empty);
        let mut paid = Vec::new();
        if prior.is_none() {
            paid.push((Completion::Owned, &rewards.owned));
        }
        if result == Completion::Killed && prior != Some(Completion::Killed) {
            paid.push((Completion::Killed, &rewards.killed));
        }
        let (mut money, mut owned_sales, mut killed_sales) = (0, 0, 0);
        for (tier, r) in &paid {
            money += r.money;
            if *tier == Completion::Owned { owned_sales += r.sales } else { killed_sales += r.sales }
            self.summary.add(*tier == Completion::Killed, r.money, r.sales);
            out.push(Effect::Paid { challenge: challenge.to_owned(), tier: *tier, money: r.money, sales: r.sales,
                rewards: r.rewards.clone() });
        }
        if money > 0 {
            self.counters.add_money(money);
            self.fire(Trigger::Money, data, &mut out);
        }
        if owned_sales + killed_sales > 0 {
            self.counters.sales_owned += owned_sales;
            self.counters.sales_killed += killed_sales;
            self.fire(Trigger::Sales, data, &mut out);
        }
        if rank(Some(result)) > rank(prior) {
            self.status.insert(challenge.to_owned(), result);
            out.push(Effect::StatusChanged { challenge: challenge.to_owned(), completion: result });
            if prior.is_none() {
                self.fire(Trigger::ChallengeDone(challenge.to_owned()), data, &mut out);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(id: &str, conditions: Vec<Condition>) -> (String, Action) {
        (id.to_owned(), Action { id: id.to_owned(), parent: None, conditions, graph: None, record_fired: false })
    }

    #[test]
    fn board_sales_are_the_four_sales_counters() {
        let mut c = Counters { sales_owned: 10_000, sales_killed: 2_500, ..Default::default() };
        assert_eq!(c.board_sales(), 12_500);
        c.add_money(300);
        c.spend(500);
        assert_eq!((c.money, c.money_earned), (0, 300), "spending clamps at 0, earned keeps counting");
    }

    #[test]
    fn conditions_are_and_ed_and_group_counts_owned_or_killed() {
        let counters = Counters { sales_owned: 15_000, ..Default::default() };
        let status = BTreeMap::from([("a".to_owned(), Completion::Owned), ("b".to_owned(), Completion::Killed),
            ("c".to_owned(), Completion::Abandoned)]);
        let groups = BTreeMap::from([("g".to_owned(), vec!["a".to_owned(), "b".to_owned(), "c".to_owned()])]);
        let w = World { counters: &counters, status: &status, groups: &groups };
        let (_, a) = action("x", vec![Condition::BoardSales(15_000), Condition::GroupDone { group: "g".into(), at_least: 2 }]);
        assert!(a.holds(&w));
        let (_, b) = action("y", vec![Condition::BoardSales(15_000), Condition::GroupDone { group: "g".into(), at_least: 3 }]);
        assert!(!b.holds(&w), "abandoned does not count");
        assert!(action("z", vec![]).1.holds(&w), "no conditions: holds at once");
        assert!(!action("n", vec![Condition::Never]).1.holds(&w));
        assert!(action("i", vec![Condition::Ignored(7)]).1.holds(&w));
    }

    #[test]
    fn a_trigger_checks_only_the_actions_it_concerns_and_they_fire_once() {
        let actions = BTreeMap::from([
            action("milestone_01", vec![Condition::BoardSales(15_000)]),
            action("rich", vec![Condition::Money(1_000_000)]),
            action("spot", vec![Condition::ChallengeDone("ots_a".into())]),
        ]);
        let mut t = Tracker::default();
        let (zero, none, groups) = (Counters::default(), BTreeMap::new(), BTreeMap::new());
        let empty = World { counters: &zero, status: &none, groups: &groups };
        for id in ["milestone_01", "rich", "spot"] {
            assert!(!t.arm(id, &actions, &empty));
        }
        assert_eq!(t.armed, ["spot", "rich", "milestone_01"], "each new one goes to the front");
        let mut counters = Counters { sales_owned: 20_000, money: 2_000_000, ..Default::default() };
        let status = BTreeMap::from([("ots_a".to_owned(), Completion::Owned)]);
        let w = World { counters: &counters, status: &status, groups: &groups };
        assert_eq!(t.check(&Trigger::Sales, &actions, &w), ["milestone_01"]);
        assert_eq!(t.armed, ["spot", "rich"], "money and challenge actions wait for their own triggers");
        assert_eq!(t.check(&Trigger::ChallengeDone("ots_a".into()), &actions, &w), ["spot"]);
        assert!(t.check(&Trigger::Sales, &actions, &w).is_empty(), "fired actions do not fire again");
        counters.money = 0;
        let w = World { counters: &counters, status: &status, groups: &groups };
        assert!(t.check(&Trigger::Money, &actions, &w).is_empty());
    }

    fn data() -> Progression {
        let reward = |sales, ids: &[&str]| Reward { money: 0, sales, rewards: ids.iter().map(|s| s.to_string()).collect() };
        Progression {
            actions: BTreeMap::from([
                action("milestone_01", vec![Condition::BoardSales(15_000)]),
                action("spot_done", vec![Condition::ChallengeDone("ots_a".into())]),
            ]),
            groups: BTreeMap::new(),
            challenge_rewards: BTreeMap::from([("ots_a".to_owned(), ChallengeRewards {
                owned: reward(10_000, &["park_piece"]), killed: reward(2_500, &[]) })]),
            ..Default::default()
        }
    }

    #[test]
    fn a_result_drops_the_key_unless_the_family_reactivates_it() {
        let mut c = Career::default();
        for id in ["ots", "race", "quit"] {
            c.active.insert(id.into());
        }
        c.settle_active("ots", Completion::Owned, true);
        c.settle_active("race", Completion::Owned, false);
        c.settle_active("quit", Completion::Abandoned, false);
        assert_eq!(c.active.iter().map(String::as_str).collect::<Vec<_>>(), ["ots", "quit"]);
    }

    #[test]
    fn activation_skips_completed_challenges() {
        let mut c = Career::default();
        c.status.insert("done".into(), Completion::Owned);
        c.status.insert("quit".into(), Completion::Abandoned);
        assert!(!c.activate("done"));
        assert!(c.activate("quit"));
        assert!(c.activate("new"));
        assert!(!c.activate("new"));
        assert_eq!(c.active.iter().map(String::as_str).collect::<Vec<_>>(), ["new", "quit"]);
    }

    #[test]
    fn graphs_activate_groups_and_deactivate_challenges() {
        use graph::{Graph, Op, State};
        let mut d = data();
        d.groups.insert("g".into(), vec!["a".into(), "b".into()]);
        let ops = vec![Op::ActivateGroup("g".into()), Op::DeactivateChallenge("b".into()), Op::ActivateChallenge("c".into())];
        let graphs = BTreeMap::from([("act".to_owned(), Graph { states: BTreeMap::from([("initial".to_owned(),
            State { on: BTreeMap::from([("stateenter".to_owned(), ops)]), ..Default::default() })]) })]);
        let mut c = Career::default();
        c.drive(vec![Effect::ActionFired { action: "x".into(), graph: Some("act".into()) }], &d, &graphs);
        assert_eq!(c.active.iter().map(String::as_str).collect::<Vec<_>>(), ["a", "c"]);
    }

    #[test]
    fn mod_sales_count_in_board_sales_and_fire_the_milestone() {
        let mut c = Career::default();
        c.arm("milestone_01", &data());
        let fired = c.add_sales(15_000, &data());
        assert_eq!(c.counters.board_sales(), 15_000);
        assert!(matches!(&fired[..], [Effect::ActionFired { action, .. }] if action == "milestone_01"));
        assert!(c.add_sales(0, &data()).is_empty());
    }

    fn paid(effects: &[Effect]) -> Vec<(Completion, u64)> {
        effects.iter().filter_map(|e| match e { Effect::Paid { tier, sales, .. } => Some((*tier, *sales)), _ => None }).collect()
    }

    #[test]
    fn the_pay_table_follows_retail() {
        let d = data();
        let mut c = Career::default();
        assert!(c.challenge_result("ots_a", Completion::Abandoned, &d).is_empty(), "abandoned pays nothing");
        assert_eq!(paid(&c.challenge_result("ots_a", Completion::Owned, &d)), [(Completion::Owned, 10_000)]);
        assert!(paid(&c.challenge_result("ots_a", Completion::Owned, &d)).is_empty(), "owned again: nothing");
        assert_eq!(paid(&c.challenge_result("ots_a", Completion::Killed, &d)), [(Completion::Killed, 2_500)]);
        assert!(paid(&c.challenge_result("ots_a", Completion::Killed, &d)).is_empty(), "killed again: nothing");
        assert_eq!(c.counters.board_sales(), 12_500);
        let mut first_kill = Career::default();
        assert_eq!(paid(&first_kill.challenge_result("ots_a", Completion::Killed, &d)),
            [(Completion::Owned, 10_000), (Completion::Killed, 2_500)], "a first kill pays both");
    }

    #[test]
    fn sales_checks_run_before_the_status_and_the_challenge_checks_after() {
        let d = data();
        let mut c = Career::default();
        c.arm("milestone_01", &d);
        c.arm("spot_done", &d);
        c.counters.sales_owned = 6_000;
        let effects = c.challenge_result("ots_a", Completion::Owned, &d);
        let order: Vec<String> = effects.iter().map(|e| match e {
            Effect::Paid { .. } => "paid".to_owned(),
            Effect::StatusChanged { .. } => "status".to_owned(),
            Effect::ActionFired { action, .. } => action.clone(),
        }).collect();
        assert_eq!(order, ["paid", "milestone_01", "status", "spot_done"]);
    }

    #[test]
    fn arming_an_action_whose_conditions_hold_fires_it_and_records_it_when_asked() {
        let mut actions = BTreeMap::from([action("starting_action", vec![])]);
        actions.get_mut("starting_action").unwrap().record_fired = true;
        let mut t = Tracker::default();
        let (counters, status, groups) = (Counters::default(), BTreeMap::new(), BTreeMap::new());
        let w = World { counters: &counters, status: &status, groups: &groups };
        assert!(t.arm("starting_action", &actions, &w), "no conditions: fires inside the arm call");
        assert!(t.armed.is_empty(), "never armed");
        assert_eq!(t.fired, ["starting_action"]);
        assert!(!t.arm("missing", &actions, &w));
    }
}
