//! The unlock store (research b872; career M1 step 9; doc 43): grants by retail unlock kind and name, the "new" flag
//! the customizer shows, and the two progression summary lists the reward screens read (big rewards: graphs'
//! UnlockProgressionReward with TRUE; others: challenge results and FALSE). Everything answers "unlocked" while
//! `unlock_all` is on, the default until the user turns gating on (USER-ASKS history 93).
use std::collections::BTreeMap;

/// Retail eUNLOCK_CATEGORY kinds (b872).
pub mod kind {
    pub const LOCATIONS: u32 = 0;
    pub const CHARACTERS: u32 = 1;
    pub const REPLAY: u32 = 2;
    pub const EGGS: u32 = 3;
    pub const GESTURES: u32 = 4;
    pub const CLOTHING_UNLOCK: u32 = 5;
    pub const CLOTHING_SPONSOR: u32 = 6;
    pub const SKATEPARK_SET: u32 = 7;
    pub const ENTITLEMENTS: u32 = 8;
    pub const WORLDS: u32 = 9;
    pub const CHALLENGES: u32 = 10;
    pub const MOVIE: u32 = 11;
    pub const DLC_PRODUCT: u32 = 12;
}

/// The unlock name hash (82503BD8): FNV-1 32, multiply then xor, over the name's bytes as given.
pub fn fnv1(name: &str) -> u32 {
    name.bytes().fold(0x811C_9DC5u32, |h, b| h.wrapping_mul(0x0100_0193) ^ u32::from(b))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unlocks {
    /// (kind, name) -> still "new" (not yet seen in its menu).
    pub granted: BTreeMap<(u32, String), bool>,
    pub unlock_all: bool,
    /// Reward ids for the reward summary: big rewards (summary +0x08) and the others (+0x2C).
    pub summary_big: Vec<String>,
    pub summary_other: Vec<String>,
}

impl Default for Unlocks {
    fn default() -> Self {
        Self { granted: BTreeMap::new(), unlock_all: true, summary_big: Vec::new(), summary_other: Vec::new() }
    }
}

impl Unlocks {
    /// Grants an unlock (82503BD8 accepts any kind below 13). True when it was not granted before.
    pub fn grant(&mut self, kind: u32, name: &str) -> bool {
        if kind >= 13 {
            return false;
        }
        let key = (kind, name.to_owned());
        if self.granted.contains_key(&key) {
            return false;
        }
        self.granted.insert(key, true);
        true
    }

    /// A progression reward: its unlock plus the summary list entry (`big` = the graph's TRUE flag).
    pub fn reward(&mut self, reward: &str, kind: u32, name: &str, big: bool) -> bool {
        let new = self.grant(kind, name);
        let list = if big { &mut self.summary_big } else { &mut self.summary_other };
        list.push(reward.to_owned());
        new
    }

    pub fn is_unlocked(&self, kind: u32, name: &str) -> bool {
        self.unlock_all || self.granted.contains_key(&(kind, name.to_owned()))
    }

    /// Retail's stored check (82503D50): a (kind, FNV-1 of the name) pair was granted. Data rows such as the
    /// summary groups' tUnlock store the hash, not the name. `unlock_all` does not apply: retail's bypass (+0x24) is
    /// off for players, so these rows follow real grants.
    pub fn granted_hash(&self, kind: u32, hash: u32) -> bool {
        self.granted.keys().any(|(k, name)| *k == kind && fnv1(name) == hash)
    }

    pub fn is_new(&self, kind: u32, name: &str) -> bool {
        self.granted.get(&(kind, name.to_owned())).copied().unwrap_or(false)
    }

    pub fn mark_seen(&mut self, kind: u32, name: &str) {
        if let Some(new) = self.granted.get_mut(&(kind, name.to_owned())) {
            *new = false;
        }
    }

    /// The reward screens took the lists.
    pub fn take_summary(&mut self) -> (Vec<String>, Vec<String>) {
        (std::mem::take(&mut self.summary_big), std::mem::take(&mut self.summary_other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_is_unlocked_until_gating_is_turned_on() {
        let mut u = Unlocks::default();
        assert!(u.is_unlocked(kind::WORLDS, "industrial"));
        u.unlock_all = false;
        assert!(!u.is_unlocked(kind::WORLDS, "industrial"));
        assert!(u.grant(kind::WORLDS, "industrial"));
        assert!(!u.grant(kind::WORLDS, "industrial"), "once");
        assert!(u.is_unlocked(kind::WORLDS, "industrial"));
        assert!(!u.grant(13, "x"), "kinds stop at 12");
    }

    #[test]
    fn stored_checks_use_the_fnv1_name_hash_and_ignore_unlock_all() {
        assert_eq!(fnv1("megapark"), 0x6599_5D27, "the disc's dist_megapark world row stores this");
        let mut u = Unlocks::default();
        assert!(!u.granted_hash(kind::WORLDS, 0x6599_5D27), "unlock_all does not open stored checks");
        u.grant(kind::WORLDS, "megapark");
        assert!(u.granted_hash(kind::WORLDS, 0x6599_5D27));
        assert!(!u.granted_hash(kind::LOCATIONS, 0x6599_5D27), "the kind must match");
    }

    #[test]
    fn rewards_feed_the_summary_lists_and_the_new_flag() {
        let mut u = Unlocks::default();
        u.reward("milestone_01", kind::DLC_PRODUCT, "milestone_placeholder", true);
        u.reward("skatepark_unlock_ots_dwtn_01", kind::SKATEPARK_SET, "skatepark_unlock_ots_dwtn_01", false);
        assert!(u.is_new(kind::SKATEPARK_SET, "skatepark_unlock_ots_dwtn_01"));
        u.mark_seen(kind::SKATEPARK_SET, "skatepark_unlock_ots_dwtn_01");
        assert!(!u.is_new(kind::SKATEPARK_SET, "skatepark_unlock_ots_dwtn_01"));
        assert_eq!(u.take_summary(), (vec!["milestone_01".into()], vec!["skatepark_unlock_ots_dwtn_01".into()]));
        assert_eq!(u.take_summary(), (vec![], vec![]));
    }
}
