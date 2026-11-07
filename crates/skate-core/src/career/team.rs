//! The player's team: five members (0 = the player) and the brand tier set (research b870 §2.3; b895 §D; doc 43).
use std::collections::{BTreeMap, BTreeSet};

/// A brand tier row (skatercollections class 939D7B12889CCF21): its bit in retail's 512-bit set, kind and whether a
/// new career starts with it. Rows without a bit inherit `default`'s 0, so tiers can share a bit (retail too).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TierRow {
    pub bit: u32,
    pub kind: u32,
    pub default_unlocked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Team {
    pub members: [bool; 5],
    /// Tier ids set (saved as ids; a query compares bits, as retail's set does).
    pub tiers: BTreeSet<String>,
}

impl Default for Team {
    fn default() -> Self {
        Self { members: [true, false, false, false, false], tiers: BTreeSet::new() }
    }
}

impl Team {
    /// Tiers a new or loaded career starts with [inferred b870].
    pub fn apply_defaults(&mut self, rows: &BTreeMap<String, TierRow>) {
        self.tiers.extend(rows.iter().filter(|(_, r)| r.default_unlocked).map(|(id, _)| id.clone()));
    }

    /// TeamBrand.UnlockTier (827D0140): idempotent; true when newly set.
    pub fn brand_tier(&mut self, id: &str) -> bool {
        self.tiers.insert(id.to_owned())
    }

    /// Whether the tier's bit is set (an unknown tier reads as bit 0, retail's default row).
    pub fn tier_unlocked(&self, id: &str, rows: &BTreeMap<String, TierRow>) -> bool {
        let bit = |t: &str| rows.get(t).map_or(0, |r| r.bit);
        let want = bit(id);
        self.tiers.iter().any(|t| bit(t) == want)
    }

    /// Unlock.UnlockTeammate (82839A80): the first locked member 1..4 [inferred: 827CAEE8 not read] joins and sets
    /// its team photo tier; nothing once all five are in. Returns the member index.
    pub fn unlock_teammate(&mut self) -> Option<usize> {
        let i = (1..5).find(|&i| !self.members[i])?;
        self.members[i] = true;
        self.tiers.insert(format!("040{i}_team_photo_teammate_{i}"));
        Some(i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn teammates_join_in_order_up_to_four_and_set_their_tier() {
        let mut t = Team::default();
        assert_eq!((1..=5).map(|_| t.unlock_teammate()).collect::<Vec<_>>(), [Some(1), Some(2), Some(3), Some(4), None]);
        assert!(t.tiers.contains("0403_team_photo_teammate_3"));
    }

    #[test]
    fn tiers_are_idempotent_and_compare_by_bit() {
        let rows = BTreeMap::from([("a".to_owned(), TierRow { bit: 200, ..Default::default() }),
            ("b".to_owned(), TierRow { bit: 200, ..Default::default() }),
            ("c".to_owned(), TierRow { bit: 7, default_unlocked: true, ..Default::default() })]);
        let mut t = Team::default();
        assert!(t.brand_tier("a") && !t.brand_tier("a"));
        assert!(t.tier_unlocked("b", &rows), "b shares a's bit");
        assert!(!t.tier_unlocked("c", &rows));
        t.apply_defaults(&rows);
        assert!(t.tier_unlocked("c", &rows));
    }
}
