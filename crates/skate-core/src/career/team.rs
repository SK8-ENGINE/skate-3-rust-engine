//! The player's team: name, logo, teammates (the player is member 0) and the brand tier set (research b870 §2.3,
//! b895 §D, b980; doc 43).
use std::collections::{BTreeMap, BTreeSet};

/// A brand tier row (skatercollections class 939D7B12889CCF21): its bit in retail's 512-bit set, kind and whether a
/// new career starts with it. Rows without a bit inherit `default`'s 0, so tiers can share a bit (retail too).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TierRow {
    pub bit: u32,
    pub kind: u32,
    pub default_unlocked: bool,
}

/// A new team's values (team reset 827CACF8, research b980): retail's strings as data, so a mod can replace them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeamDefaults {
    pub name: String,
    pub teammate_name: String,
    /// Teammate n's character key is `<key_prefix><n>` (retail `"%s%i"` with `"teammate_0"`), the living world row.
    pub key_prefix: String,
    pub teammates: usize,
}

impl Default for TeamDefaults {
    fn default() -> Self {
        Self { name: "MyTeamName".into(), teammate_name: "Teammate".into(), key_prefix: "teammate_0".into(), teammates: 4 }
    }
}

/// Team and teammate names hold 16 characters (retail char[17]).
pub const NAME_MAX: usize = 16;

/// Teammate n (1-based). Retail keeps the joined flag one record later than the name and key (b980 §0.2); here it
/// sits on the teammate it describes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Teammate {
    pub joined: bool,
    pub name: String,
    /// Not saved: rebuilt from the defaults on every load, as retail rebuilds it on every reset.
    pub character: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Team {
    pub name: String,
    /// Logo row id; None = the data's fallback logo (retail D7EDBD362D7D2152).
    pub logo: Option<String>,
    pub teammates: Vec<Teammate>,
    /// Tier ids set (saved as ids; a query compares bits, as retail's set does).
    pub tiers: BTreeSet<String>,
}

impl Default for Team {
    fn default() -> Self {
        Self::new(&TeamDefaults::default())
    }
}

/// The first `NAME_MAX` characters.
pub fn clip_name(name: &str) -> String {
    name.chars().take(NAME_MAX).collect()
}

impl Team {
    pub fn new(d: &TeamDefaults) -> Self {
        let teammates = (1..=d.teammates)
            .map(|n| Teammate { joined: false, name: clip_name(&d.teammate_name), character: format!("{}{n}", d.key_prefix) })
            .collect();
        Self { name: clip_name(&d.name), logo: None, teammates, tiers: BTreeSet::new() }
    }

    /// Joined members, the player included (827CAEE8).
    pub fn count(&self) -> usize {
        1 + self.teammates.iter().filter(|t| t.joined).count()
    }

    /// Joined flags by member, 0 = the player (always in).
    pub fn members(&self) -> Vec<bool> {
        std::iter::once(true).chain(self.teammates.iter().map(|t| t.joined)).collect()
    }

    /// Teammate n, 1-based.
    pub fn teammate_mut(&mut self, n: usize) -> Option<&mut Teammate> {
        n.checked_sub(1).and_then(|i| self.teammates.get_mut(i))
    }

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

    /// Unlock.UnlockTeammate (82839A80): member `count` joins (the first not joined while they join in order, b980)
    /// and sets its team photo tier; nothing once all are in. Returns the teammate number.
    pub fn unlock_teammate(&mut self) -> Option<usize> {
        let i = self.teammates.iter().position(|t| !t.joined)?;
        self.teammates[i].joined = true;
        let n = i + 1;
        self.tiers.insert(format!("040{n}_team_photo_teammate_{n}"));
        Some(n)
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
        assert_eq!(t.count(), 5);
    }

    #[test]
    fn a_new_team_is_retails_reset() {
        let t = Team::default();
        assert_eq!((t.name.as_str(), t.logo.as_deref(), t.count()), ("MyTeamName", None, 1));
        assert_eq!(t.members(), [true, false, false, false, false]);
        assert_eq!(t.teammates.iter().map(|m| (m.name.as_str(), m.character.as_str())).collect::<Vec<_>>(),
            [("Teammate", "teammate_01"), ("Teammate", "teammate_02"), ("Teammate", "teammate_03"), ("Teammate", "teammate_04")]);
        assert_eq!(clip_name("abcdefghijklmnopqrstu"), "abcdefghijklmnop");
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
