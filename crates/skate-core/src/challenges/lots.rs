//! Own the lot (research b905, b916, b922): a lot is owned when every listed activity has a status; the lot list
//! (screen 0x1B) groups the activities by type, a header before each type.
//! Details: docs/hails-additions/42-challenges.md#change-own-the-lot-export-toolsasset_pipelinechallengespy-own_the_lots-research-b905-b916-b922

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LotChild {
    pub id: String,
    /// eFreeskateType: 0 accumulation, 1 gap, 2 tag, 3 simultaneous trick, 4 survival, 5 group trick list.
    pub activity_type: Option<u32>,
    pub title_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row<'a> {
    /// A type header: its text id.
    Header(&'a str),
    Activity(&'a LotChild),
}

/// The header text ids per type (table 8302CAB8, offline column), a mod can replace or extend them.
pub fn default_headers() -> Vec<String> {
    ["ID_CHALLENGE_TYPE_ACCUMULATION", "ID_CHALLENGE_TYPE_GAP", "ID_CHALLENGE_TYPE_TAG", "ID_OTL_OFFLINE_SIMULPLAY",
     "ID_CHALLENGE_TYPE_SURVIVAL", "ID_CHALLENGE_TYPE_GROUPTRICKLIST"].map(String::from).to_vec()
}

/// Shown for a type without a header.
pub const INVALID_HEADER: &str = "#Invalid OTL Header";

/// Owned when every listed activity has a status (b905).
pub fn owned(children: &[LotChild], has_status: impl Fn(&str) -> bool) -> bool {
    !children.is_empty() && children.iter().all(|c| has_status(&c.id))
}

/// The lot list: activities sorted by type (stable, disc order within a type), a header before each new type.
pub fn list<'a>(children: &'a [LotChild], headers: &'a [String]) -> Vec<Row<'a>> {
    let mut sorted: Vec<&LotChild> = children.iter().collect();
    sorted.sort_by_key(|c| c.activity_type.unwrap_or(u32::MAX));
    let mut rows = Vec::with_capacity(sorted.len() + 6);
    let mut last = None;
    for child in sorted {
        if last != Some(child.activity_type) {
            last = Some(child.activity_type);
            let header = child.activity_type.and_then(|t| headers.get(t as usize)).map_or(INVALID_HEADER, String::as_str);
            rows.push(Row::Header(header));
        }
        rows.push(Row::Activity(child));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn child(id: &str, t: u32) -> LotChild {
        LotChild { id: id.into(), activity_type: Some(t), title_id: None }
    }

    #[test]
    fn list_groups_by_type_keeping_disc_order() {
        let kids = [child("gap_1", 1), child("tag_1", 2), child("gap_2", 1), child("accu", 0), child("odd", 9)];
        let headers = default_headers();
        let ids: Vec<&str> = list(&kids, &headers).iter().map(|r| match r {
            Row::Header(h) => *h,
            Row::Activity(c) => c.id.as_str(),
        }).collect();
        assert_eq!(ids, ["ID_CHALLENGE_TYPE_ACCUMULATION", "accu", "ID_CHALLENGE_TYPE_GAP", "gap_1", "gap_2",
            "ID_CHALLENGE_TYPE_TAG", "tag_1", INVALID_HEADER, "odd"]);
    }

    #[test]
    fn owned_needs_every_listed_activity() {
        let kids = [child("a", 0), child("b", 1)];
        assert!(!owned(&kids, |id| id == "a"));
        assert!(owned(&kids, |_| true));
        assert!(!owned(&[], |_| true), "an empty lot is never owned");
    }
}
