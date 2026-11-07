//! The Skate Profile's progression page from the career (Progression natives 825AFBA8; research b873 §2.1,
//! b895 §A / §C; doc 43).
use super::{milestones, Career, Progression};
use crate::challenges::Completion;
use crate::offline_profile::{ChallengeRow, MilestoneRow, ProfileData, ProfileOverall};

/// A progression_summary_group row with its references resolved to progression ids.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SummaryGroup {
    pub key: String,
    /// List 0 (milestone entry) or list 1 (challenge entry).
    pub milestone: bool,
    pub title: String,
    pub icon: String,
    pub action: Option<String>,
    pub groups: Vec<String>,
    /// The row has an unlock and it is not unlocked: left out (DLC groups show only with the DLC).
    pub locked_out: bool,
    pub flags: [bool; 3],
}

/// The milestone entry (825B0380): sales, the action's first threshold, the flags, and locked while the threshold
/// is above the current goal (goal 0 = finished or before the gates: not locked [inferred for the gates]).
pub fn milestone_row(g: &SummaryGroup, career: &Career, data: &Progression) -> MilestoneRow {
    let goal = milestones::facts(career, data)[3];
    let threshold = g.action.as_deref().map_or(0, |a| milestones::threshold(data, a)) as f64;
    MilestoneRow {
        title: g.title.clone(),
        icon: g.icon.clone(),
        sales: career.counters.board_sales() as f64,
        threshold,
        flags: g.flags,
        locked: goal != 0.0 && threshold > goal,
    }
}

/// The challenge entry (825B06C8): counted over each listed group's members in turn (a challenge in two groups
/// counts twice).
pub fn challenge_row(g: &SummaryGroup, career: &Career, data: &Progression) -> ChallengeRow {
    let mut row = ChallengeRow { title: g.title.clone(), icon: g.icon.clone(), ..Default::default() };
    for member in g.groups.iter().filter_map(|id| data.groups.get(id)).flatten() {
        row.total += 1.0;
        let status = career.status.get(member);
        if matches!(status, Some(Completion::Owned | Completion::Killed)) {
            row.done += 1.0;
        }
        if data.killable.contains(member) {
            row.killable += 1.0;
            if status == Some(&Completion::Killed) {
                row.killed += 1.0;
            }
        }
    }
    row
}

/// Fills the progression page: the overall bars from the two named rows, the two lists (rows in the given order,
/// locked-out rows left out; the overall rows are bars only, not list rows: retail capture 2026-11-06) and the dots.
pub fn fill(page: &mut ProfileData, groups: &[SummaryGroup], overall: [&str; 2], career: &Career, data: &Progression) {
    page.milestone_rows.clear();
    page.challenge_rows.clear();
    for g in groups.iter().filter(|g| !g.locked_out) {
        if g.milestone {
            let r = milestone_row(g, career, data);
            if g.key == overall[0] {
                page.career = ProfileOverall { title: r.title, icon: r.icon, current: r.sales, total: r.threshold };
            } else {
                page.milestone_rows.push(r);
            }
        } else {
            let r = challenge_row(g, career, data);
            if g.key == overall[1] {
                page.challenges = ProfileOverall { title: r.title, icon: r.icon, current: r.done, total: r.total };
            } else {
                page.challenge_rows.push(r);
            }
        }
    }
    page.dots = milestones::dots(data);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::career::{Action, CareerConfig, Condition};
    use std::collections::BTreeMap;

    fn data() -> Progression {
        let m = |id: &str, sales| (id.to_owned(), Action { id: id.into(), parent: Some("milestone_actions".into()),
            conditions: vec![Condition::BoardSales(sales)], ..Default::default() });
        Progression {
            actions: BTreeMap::from([m("m1", 15_000), m("m2", 50_000)]),
            groups: BTreeMap::from([("ots".into(), vec!["a".into(), "b".into()]), ("race".into(), vec!["b".into(), "r".into()])]),
            killable: ["b".to_owned(), "r".to_owned()].into(),
            config: CareerConfig { milestone_parent: "milestone_actions".into(), crossbar_gates: vec![] },
            ..Default::default()
        }
    }

    #[test]
    fn rows_count_members_per_group_and_lock_future_milestones() {
        let d = data();
        let mut c = Career::default();
        c.tracker.armed = vec!["m2".into()];
        c.counters.sales_owned = 20_000;
        c.status = BTreeMap::from([("a".into(), Completion::Owned), ("b".into(), Completion::Killed)]);
        let row = |key: &str, milestone, action: Option<&str>, groups: &[&str]| SummaryGroup {
            key: key.into(), milestone, title: key.into(), action: action.map(Into::into),
            groups: groups.iter().map(|g| g.to_string()).collect(), ..Default::default() };
        let groups = [row("all_m", true, Some("m2"), &[]), row("m1", true, Some("m1"), &[]), row("m2", true, Some("m2"), &[]),
            row("all_c", false, None, &["ots", "race"]), row("mixed", false, None, &["ots", "race"]),
            SummaryGroup { locked_out: true, ..row("dlc", false, None, &["ots"]) }];
        let mut page = ProfileData::default();
        fill(&mut page, &groups, ["all_m", "all_c"], &c, &d);
        assert_eq!((page.career.current, page.career.total), (20_000.0, 50_000.0));
        assert_eq!(page.milestone_rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(), ["m1", "m2"],
            "the overall rows are bars only (retail capture)");
        assert_eq!(page.milestone_rows.iter().map(|r| r.locked).collect::<Vec<_>>(), [false, false],
            "goal is m2 (50k): nothing above it");
        let mixed = &page.challenge_rows[0];
        assert_eq!((mixed.total, mixed.done, mixed.killable, mixed.killed), (4.0, 3.0, 3.0, 2.0), "b counts in both groups");
        assert_eq!(page.challenge_rows.len(), 1, "the overall row and locked-out rows are not list rows");
        assert_eq!((page.challenges.current, page.challenges.total), (3.0, 4.0));
        assert_eq!(page.dots.len(), 2);
    }
}
