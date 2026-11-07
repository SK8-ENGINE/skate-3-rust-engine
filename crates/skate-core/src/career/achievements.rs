//! Achievements (research b881; b895 §D; doc 43): granted once per profile, kept across a new career. Grants come
//! from the GrantAchievement op, the GrantAchievement presentation event, and the load replay (827CE7B0).
use super::graph::{Graph, Op};
use super::{Condition, Progression};
use std::collections::BTreeMap;

/// An achievement row (class 9CB9012080F727B9): the platform id and the enabled gate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AchievementRow {
    pub id: u32,
    pub enabled: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Achievements {
    /// name -> grant time (seconds since the Unix epoch).
    pub granted: BTreeMap<String, u64>,
}

impl Achievements {
    /// True when newly granted: the row must exist and be enabled, and it grants once.
    pub fn grant(&mut self, name: &str, row: Option<&AchievementRow>, now: u64) -> bool {
        if !row.is_some_and(|r| r.enabled) || self.granted.contains_key(name) {
            return false;
        }
        self.granted.insert(name.to_owned(), now);
        true
    }
}

fn granted_by(ops: &[Op], out: &mut Vec<String>) {
    for op in ops {
        match op {
            Op::GrantAchievement(name) => out.push(name.clone()),
            Op::If { then, otherwise, .. } => {
                granted_by(then, out);
                granted_by(otherwise, out);
            }
            _ => {}
        }
    }
}

/// The load replay pairs: actions with one board sales condition whose graph grants an achievement (by op or by
/// event), as (achievement, threshold). Retail checks the six at load with `> threshold - 1` (827CE7B0).
pub fn load_replay(data: &Progression, graphs: &BTreeMap<String, Graph>) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    for action in data.actions.values() {
        let ([Condition::BoardSales(threshold)], Some(graph)) = (action.conditions.as_slice(), action.graph.as_ref())
        else { continue };
        let Some(g) = graphs.get(graph) else { continue };
        let mut names = Vec::new();
        for state in g.states.values() {
            state.on.values().for_each(|ops| granted_by(ops, &mut names));
            names.extend(state.events.iter().filter(|e| e.kind == "GrantAchievement").filter_map(|e| e.args[0].clone()));
        }
        out.extend(names.into_iter().map(|n| (n, *threshold)));
    }
    out.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::career::graph::{Event, State};
    use crate::career::Action;

    #[test]
    fn grants_once_and_only_enabled_rows() {
        let mut a = Achievements::default();
        let on = AchievementRow { id: 1, enabled: true };
        assert!(!a.grant("x", None, 5), "missing row");
        assert!(!a.grant("x", Some(&AchievementRow { enabled: false, ..on }), 5), "disabled row");
        assert!(a.grant("x", Some(&on), 5) && !a.grant("x", Some(&on), 6));
        assert_eq!(a.granted["x"], 5);
    }

    #[test]
    fn load_replay_takes_single_sales_actions_whose_graph_grants() {
        let graph = Graph { states: BTreeMap::from([("initial".to_owned(), State {
            events: vec![Event { kind: "GrantAchievement".into(), args: [Some("employee_2".into()), None], mask: 0 }],
            on: BTreeMap::new() })]) };
        let action = |id: &str, conditions| (id.to_owned(), Action { id: id.into(), conditions, graph: Some("g".into()), ..Default::default() });
        let data = Progression { actions: BTreeMap::from([
            action("m1", vec![Condition::BoardSales(15_000)]),
            action("two", vec![Condition::BoardSales(1), Condition::Money(1)]),
        ]), ..Default::default() };
        let graphs = BTreeMap::from([("g".to_owned(), graph)]);
        assert_eq!(load_replay(&data, &graphs), [("employee_2".to_owned(), 15_000)]);
    }
}
