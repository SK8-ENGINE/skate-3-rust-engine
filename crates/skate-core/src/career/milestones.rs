//! Milestones, the crossbar's career facts and the milestone dots (research b873 §1.1, b895 §B / §C; doc 43).
use super::{Career, Condition, Progression};

/// An action whose parent chain reaches the configured milestone row (828DA2C8 / 828DA3E0).
pub fn is_milestone(data: &Progression, id: &str) -> bool {
    let root = data.config.milestone_parent.as_str();
    if root.is_empty() {
        return false;
    }
    let mut parent = data.actions.get(id).and_then(|a| a.parent.as_deref());
    for _ in 0..=data.actions.len() {
        match parent {
            Some(p) if p == root => return true,
            Some(p) => parent = data.actions.get(p).and_then(|a| a.parent.as_deref()),
            None => return false,
        }
    }
    false
}

/// The threshold of an action's first trigger when it is a board sales one (+0x10 of the first item).
pub fn threshold(data: &Progression, id: &str) -> u64 {
    match data.actions.get(id).and_then(|a| a.conditions.first()) {
        Some(Condition::BoardSales(t)) => *t,
        _ => 0,
    }
}

/// The milestones with their thresholds, sorted by threshold (827D2F58 [inferred key]).
pub fn milestones(data: &Progression) -> Vec<(String, u64)> {
    let mut out: Vec<(String, u64)> =
        data.actions.keys().filter(|id| is_milestone(data, id)).map(|id| (id.clone(), threshold(data, id))).collect();
    out.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    out
}

/// GetProgressionData [0]: -2 / -1 while a gate challenge has no status, else milestones presented =
/// count - pending - still armed (827CD728, b895 §B).
pub fn index(career: &Career, data: &Progression) -> i64 {
    for (i, gate) in data.config.crossbar_gates.iter().take(2).enumerate() {
        if !career.status.contains_key(gate) {
            return i as i64 - 2;
        }
    }
    let list = milestones(data);
    let armed = list.iter().filter(|(id, _)| career.tracker.armed.contains(id)).count() as i64;
    list.len() as i64 - career.pending.milestones as i64 - armed
}

/// GetProgressionData's six numbers (825B73D8): index, count, previous threshold, goal, board sales, money.
pub fn facts(career: &Career, data: &Progression) -> [f64; 6] {
    let list = milestones(data);
    let idx = index(career, data);
    let at = |i: i64| usize::try_from(i).ok().and_then(|i| list.get(i)).map_or(0, |m| m.1);
    [idx as f64, list.len() as f64, at(idx - 1) as f64, at(idx) as f64, career.counters.board_sales() as f64,
     career.counters.money as f64]
}

/// GetMilestoneDotData: [(i + 1) / n, threshold] per milestone (825AFC98).
pub fn dots(data: &Progression) -> Vec<(f64, f64)> {
    let list = milestones(data);
    let n = list.len() as f64;
    list.iter().enumerate().map(|(i, m)| ((i + 1) as f64 / n, m.1 as f64)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::career::{Action, CareerConfig};
    use crate::challenges::Completion;
    use std::collections::BTreeMap;

    fn data() -> Progression {
        let action = |id: &str, parent: &str, sales: u64| {
            (id.to_owned(), Action { id: id.into(), parent: Some(parent.into()),
                conditions: vec![Condition::BoardSales(sales)], ..Default::default() })
        };
        Progression {
            actions: BTreeMap::from([
                action("milestone_02", "milestone_actions", 50_000),
                action("milestone_01", "milestone_actions", 15_000),
                action("milestone_final", "milestone_02", 1_000_000),
                action("other", "rewards", 15_000),
            ]),
            config: CareerConfig { milestone_parent: "milestone_actions".into(),
                crossbar_gates: vec!["film".into(), "photo".into()] },
            ..Default::default()
        }
    }

    #[test]
    fn milestones_follow_the_parent_chain_and_sort_by_threshold() {
        let ids: Vec<String> = milestones(&data()).into_iter().map(|m| m.0).collect();
        assert_eq!(ids, ["milestone_01", "milestone_02", "milestone_final"]);
        assert_eq!(dots(&data()), [(1.0 / 3.0, 15_000.0), (2.0 / 3.0, 50_000.0), (1.0, 1_000_000.0)]);
    }

    #[test]
    fn gates_then_count_minus_pending_minus_armed() {
        let d = data();
        let mut c = Career::default();
        assert_eq!(index(&c, &d), -2);
        c.status.insert("film".into(), Completion::Owned);
        assert_eq!(index(&c, &d), -1);
        c.status.insert("photo".into(), Completion::Owned);
        c.tracker.armed = vec!["milestone_01".into(), "milestone_02".into(), "milestone_final".into()];
        assert_eq!(facts(&c, &d), [0.0, 3.0, 0.0, 15_000.0, 0.0, 0.0]);
        c.tracker.armed.retain(|a| a != "milestone_01");
        c.pending.milestones = 1;
        c.counters.sales_owned = 15_000;
        assert_eq!(index(&c, &d), 0, "earned but not presented yet");
        c.pending.milestones = 0;
        assert_eq!(facts(&c, &d), [1.0, 3.0, 15_000.0, 50_000.0, 15_000.0, 0.0]);
    }
}
