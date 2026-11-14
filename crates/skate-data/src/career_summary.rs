//! The Skate Profile's progression summary rows (class progression_summary_group, research b873 §2.1 / b895 §A),
//! read from the installed stock collections; mods change them through collection overrides like any stock row.
use crate::challenge_map::{key_hash, ICON_CLASS, ICON_NAME};
use crate::collections::Collections;
use std::collections::BTreeMap;

const CLASS: &str = "Hash_E5CC5838BC2A40E9";
const MILESTONE: &str = "Hash_60B0B3B5BE2768AA";
const TITLE: &str = "Hash_E2A50788B6C20C92";
const ICON: &str = "Hash_786255B8FF426A3D";
const ORDER: &str = "Hash_692F97FF95563BB4";
const ACTION: &str = "Hash_51ADE02D62CB4254";
const GROUPS: &str = "Hash_4ACBEE23F4D5A7B5";
const UNLOCK: &str = "Hash_34A3DFA6690CC33D";
const FLAGS: [&str; 3] = ["Hash_5170A82CAB5EC30E", "Hash_F0657539821075ED", "Hash_98BEF8D8CA30738D"];

#[derive(Clone, Debug, PartialEq)]
pub struct SummaryRow {
    pub key: String,
    /// true: list 0 (milestone entry, 8 slots); false: list 1 (challenge entry, 6 slots).
    pub milestone: bool,
    pub title: String,
    /// The fe_map_icons record's texture name.
    pub icon: String,
    pub order: i16,
    /// The progression action whose first trigger is the row's threshold (milestone rows).
    pub action: Option<String>,
    /// progression_group keys as stored (`Hash_..` when the name is unknown), counted in turn.
    pub groups: Vec<String>,
    /// Sk8::BE::tUnlock words {kind, name}; the row shows only once unlocked.
    pub unlock: Option<[u32; 2]>,
    /// The movie's mSkaterIcon / mParkIcon / mChallengeIcon.
    pub flags: [bool; 3],
}

/// Every titled summary row (default and the bare parent rows have no title [inferred]), sorted by order then key.
pub fn summary_rows(c: &Collections) -> Vec<SummaryRow> {
    let icons: BTreeMap<u64, String> = c
        .entries()
        .iter()
        .filter(|e| key_hash(&e.class_name) == key_hash(ICON_CLASS))
        .filter_map(|e| Some((key_hash(&e.key), c.field(ICON_CLASS, &e.key, ICON_NAME).ok()?.data.clone())))
        .collect();
    let mut rows: Vec<SummaryRow> = c
        .entries()
        .iter()
        .filter(|e| key_hash(&e.class_name) == key_hash(CLASS))
        .filter_map(|e| {
            let field = |name: &str| c.field(CLASS, &e.key, name).ok();
            let title = field(TITLE).map(|f| f.data.clone()).filter(|t| !t.is_empty() && t != "None")?;
            let bool_of = |name: &str| c.boolean(CLASS, &e.key, name).unwrap_or(false);
            let first_key = |name: &str| field(name).and_then(|f| u64::from_str_radix(f.data.get(0..16)?, 16).ok());
            let action_key = first_key(ACTION).filter(|&k| k != 0);
            Some(SummaryRow {
                key: e.key.clone(),
                milestone: bool_of(MILESTONE),
                title,
                icon: first_key(ICON).and_then(|k| icons.get(&k).cloned()).unwrap_or_default(),
                order: field(ORDER).and_then(|f| u16::from_str_radix(f.data.get(0..4)?, 16).ok()).unwrap_or(0) as i16,
                action: action_key.map(|k| format!("Hash_{k:016X}")),
                groups: field(GROUPS)
                    .and_then(|f| f.array.as_ref())
                    .map(|a| a.items.iter().filter_map(|i| Some(format!("Hash_{}", i.get(0..16)?))).collect())
                    .unwrap_or_default(),
                unlock: field(UNLOCK).and_then(|f| {
                    Some([u32::from_str_radix(f.data.get(0..8)?, 16).ok()?, u32::from_str_radix(f.data.get(8..16)?, 16).ok()?])
                }),
                flags: FLAGS.map(bool_of),
            })
        })
        .collect();
    rows.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.key.cmp(&b.key)));
    rows
}

const TAB_CLASS: &str = "Hash_B5D4474F18223090";
const TAB_TITLE: &str = "Hash_AA753A536859233D";
const TAB_LIST: &str = "Hash_C83BB50EC9A5A801";
const TAB_ORDER: &str = "Hash_804F77ACECCF24CB";
const TAB_KINDS: &str = "Hash_17E926A1AA38ACC4";

/// A reward summary tab (progression_reward_tabs, research b873 §4): the unlock kinds it lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RewardTab {
    pub key: String,
    pub title: String,
    /// IsCategoryShownAsList.
    pub list: bool,
    pub order: i16,
    pub kinds: Vec<u32>,
}

/// Every titled reward tab, sorted by order then key.
pub fn reward_tabs(c: &Collections) -> Vec<RewardTab> {
    let mut tabs: Vec<RewardTab> = c
        .entries()
        .iter()
        .filter(|e| key_hash(&e.class_name) == key_hash(TAB_CLASS))
        .filter_map(|e| {
            let field = |name: &str| c.field(TAB_CLASS, &e.key, name).ok();
            let title = field(TAB_TITLE).map(|f| f.data.clone()).filter(|t| !t.is_empty() && t != "None")?;
            Some(RewardTab {
                key: e.key.clone(),
                title,
                list: c.boolean(TAB_CLASS, &e.key, TAB_LIST).unwrap_or(false),
                order: field(TAB_ORDER).and_then(|f| u16::from_str_radix(f.data.get(0..4)?, 16).ok()).unwrap_or(0) as i16,
                kinds: field(TAB_KINDS)
                    .and_then(|f| f.array.as_ref())
                    .map(|a| a.items.iter().filter_map(|i| u32::from_str_radix(i.get(0..8)?, 16).ok()).collect())
                    .unwrap_or_default(),
            })
        })
        .collect();
    tabs.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.key.cmp(&b.key)));
    tabs
}

const TIER_CLASS: &str = "Hash_939D7B12889CCF21";
const TIER_BIT: &str = "Hash_4632B6D0D476D3AB";
const TIER_KIND: &str = "Hash_5F64C810E0CBABD3";
const TIER_DEFAULT: &str = "Hash_F256F51F547BECD6";
const ACHIEVEMENT_CLASS: &str = "Hash_9CB9012080F727B9";
const ACHIEVEMENT_ID: &str = "Hash_4042D2F96C4E4B8C";
const ACHIEVEMENT_ENABLED: &str = "Hash_CE07C47E53576879";

/// A class's rows keyed by the matching name from `names` (stored keys are mostly hashed), else the stored key.
fn rows_of<T>(c: &Collections, class: &str, names: &[String], read: impl Fn(&str) -> T) -> BTreeMap<String, T> {
    c.entries()
        .iter()
        .filter(|e| key_hash(&e.class_name) == key_hash(class) && e.key != "default")
        .map(|e| {
            let h = key_hash(&e.key);
            let id = names.iter().find(|n| key_hash(n) == h).cloned().unwrap_or_else(|| e.key.clone());
            (id, read(&e.key))
        })
        .collect()
}

/// Brand tier rows (b870 §2.3; bit from TeamBrand.UnlockTier 827D0140).
pub fn tier_rows(c: &Collections, names: &[String]) -> BTreeMap<String, skate_core::career::team::TierRow> {
    rows_of(c, TIER_CLASS, names, |key| skate_core::career::team::TierRow {
        bit: c.integer(TIER_CLASS, key, TIER_BIT).unwrap_or(0),
        kind: c.field(TIER_CLASS, key, TIER_KIND).ok().and_then(|f| u32::from_str_radix(f.data.get(0..8)?, 16).ok()).unwrap_or(0),
        default_unlocked: c.boolean(TIER_CLASS, key, TIER_DEFAULT).unwrap_or(false),
    })
}

/// Achievement rows (b881).
pub fn achievement_rows(c: &Collections, names: &[String]) -> BTreeMap<String, skate_core::career::achievements::AchievementRow> {
    rows_of(c, ACHIEVEMENT_CLASS, names, |key| skate_core::career::achievements::AchievementRow {
        id: c.integer(ACHIEVEMENT_CLASS, key, ACHIEVEMENT_ID).unwrap_or(0),
        enabled: c.boolean(ACHIEVEMENT_CLASS, key, ACHIEVEMENT_ENABLED).unwrap_or(false),
    })
}

/// The overall bars' rows (825B0F20 / 825B0F38 read these keys).
pub const OVERALL: [&str; 2] = ["allmilestones", "allchallenges"];

/// The rows for career::profile, references resolved to progression ids (unknown ones stay `Hash_..`), and the
/// overall rows' keys. `unlocked` answers a row's unlock words.
pub fn summary_groups(rows: &[SummaryRow], data: &skate_core::career::Progression, unlocked: impl Fn([u32; 2]) -> bool)
    -> (Vec<skate_core::career::profile::SummaryGroup>, [String; 2]) {
    fn resolve<'a>(hashed: &str, mut names: impl Iterator<Item = &'a String>) -> String {
        let h = key_hash(hashed);
        names.find(|n| key_hash(n) == h).cloned().unwrap_or_else(|| hashed.to_owned())
    }
    let groups = rows
        .iter()
        .map(|r| skate_core::career::profile::SummaryGroup {
            key: r.key.clone(),
            milestone: r.milestone,
            title: r.title.clone(),
            icon: r.icon.clone(),
            action: r.action.as_deref().map(|a| resolve(a, data.actions.keys())),
            groups: r.groups.iter().map(|g| resolve(g, data.groups.keys())).collect(),
            locked_out: r.unlock.is_some_and(|u| !unlocked(u)),
            flags: r.flags,
        })
        .collect();
    let overall = OVERALL.map(|name| {
        rows.iter().find(|r| key_hash(&r.key) == key_hash(name)).map_or_else(|| name.to_owned(), |r| r.key.clone())
    });
    (groups, overall)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Installed data (b873): 7 milestone rows (allmilestones + milestone_01..06), 16 challenge rows incl. the
    /// unlock-gated blackbox group; allchallenges counts 14 groups.
    #[test]
    #[ignore = "requires the converted vault (SKATE3_ASSET_ROOT with private/stock/skater-collections.json)"]
    fn the_retail_summary_rows() {
        let root = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").expect("asset root"));
        let rows = summary_rows(&Collections::load(&root).unwrap());
        assert_eq!(rows.iter().filter(|r| r.milestone).count(), 7);
        assert!(rows.iter().filter(|r| !r.milestone).count() >= 16, "16 on the disc, DLC packs add their own");
        let all = rows.iter().find(|r| r.title == "ID_PROGRESSION_PROFILE_CHALLENGES").unwrap();
        assert_eq!(all.groups.len(), 14);
        assert!(rows.iter().any(|r| r.unlock.is_some() && r.title == "ID_WORLD_BLACKBOXPARK"));
        let m1 = rows.iter().find(|r| r.title == "ID_PROGRESSION_MILESTONE_1").unwrap();
        assert!(m1.milestone && m1.action.is_some() && m1.flags == [true, false, false]);
        assert!(rows.iter().all(|r| !r.icon.is_empty() || r.milestone), "challenge rows have icons");
    }

    /// Installed data (b870 / b881): 81 tier rows besides default, the teammate tiers resolve by name; 47 enabled
    /// achievements besides default.
    #[test]
    #[ignore = "requires the converted vault (SKATE3_ASSET_ROOT with private/stock/skater-collections.json)"]
    fn the_retail_tier_and_achievement_rows() {
        let root = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").expect("asset root"));
        let c = Collections::load(&root).unwrap();
        let names: Vec<String> = (1..5).map(|i| format!("040{i}_team_photo_teammate_{i}")).collect();
        let tiers = tier_rows(&c, &names);
        assert!(names.iter().all(|n| tiers.contains_key(n)), "{:?}", tiers.keys().take(5).collect::<Vec<_>>());
        let achievements = achievement_rows(&c, &[]);
        assert_eq!(achievements.len(), 47);
        assert!(achievements.values().all(|a| a.enabled && (1..=47).contains(&a.id)));
    }
}
