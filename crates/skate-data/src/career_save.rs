//! The career save `settings/profiles/<player>/career.json` (career M1 step 5, design research b887 section 3; doc 43):
//! challenge statuses (completion, high score, discovery), the progression counters, the action tracker lists and the
//! career flags. Every level keeps keys it does not know, so a newer engine's or a mod's fields survive a save. The
//! first load imports the older `settings/challenges.json` records once, as statuses only: wins from before the
//! career save do not pay (user decision 2026-11-06).
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub const FORMAT: &str = "skate3rust.career";
pub const VERSION: u32 = 1;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChallengeEntry {
    /// "owned" or "killed"; absent before the first completion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<String>,
    #[serde(default)]
    pub high_score: i32,
    #[serde(default)]
    pub discovery: u32,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Counters {
    #[serde(default)]
    pub sales_owned: u64,
    #[serde(default)]
    pub sales_killed: u64,
    #[serde(default)]
    pub sales_bonus: u64,
    #[serde(default)]
    pub sales_set: u64,
    #[serde(default)]
    pub money: u64,
    #[serde(default)]
    pub money_earned: u64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Actions {
    /// Armed actions, front first (retail arm order).
    #[serde(default)]
    pub armed: Vec<String>,
    /// Auto-started actions already fired (retail tracker +0x14).
    #[serde(default)]
    pub fired: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Earned rewards not presented yet (retail saves its RewardTracker; b895 §C).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rewards {
    #[serde(default)]
    pub milestones_pending: u32,
    #[serde(default)]
    pub teammates_pending: u32,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// The team (b870): members joined (0 = the player) and the brand tier ids set.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TeamFile {
    #[serde(default)]
    pub members: Vec<bool>,
    #[serde(default)]
    pub tiers: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// The profile's new career flags (retail ProfileBackEnd::ProfileData +0x68 / +0x6C, research b1001): `pending` = the
/// first challenge is still to be queued (set for a new career, absent = false for a career saved before it).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BootFile {
    #[serde(default)]
    pub pending: bool,
    #[serde(default)]
    pub progress: u32,
}

/// One granted unlock (retail kind, name) and whether its menu has not shown it yet.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UnlockEntry {
    pub kind: u32,
    pub name: String,
    #[serde(default)]
    pub new: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `settings/profiles/local/achievements.json`: kept across a new career (b895 §D).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AchievementsFile {
    pub format: String,
    pub version: u32,
    /// name -> grant time (Unix seconds).
    #[serde(default)]
    pub granted: BTreeMap<String, u64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

pub const ACHIEVEMENTS_FORMAT: &str = "skate3rust.achievements";

impl Default for AchievementsFile {
    fn default() -> Self {
        Self { format: ACHIEVEMENTS_FORMAT.into(), version: 1, granted: BTreeMap::new(), extra: Map::new() }
    }
}

impl AchievementsFile {
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if file.format != ACHIEVEMENTS_FORMAT || file.version != 1 {
            return Err(format!("not an achievements file: {} v{}", file.format, file.version));
        }
        Ok(file)
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CareerFile {
    pub format: String,
    pub version: u32,
    #[serde(default)]
    pub challenges: BTreeMap<String, ChallengeEntry>,
    #[serde(default)]
    pub counters: Counters,
    #[serde(default)]
    pub actions: Actions,
    #[serde(default)]
    pub rewards: Rewards,
    #[serde(default)]
    pub team: TeamFile,
    #[serde(default)]
    pub unlocks: Vec<UnlockEntry>,
    /// Challenges the career activated (gating).
    #[serde(default)]
    pub active: Vec<String>,
    #[serde(default, skip_serializing_if = "is_default_boot")]
    pub boot: BootFile,
    /// Career flags: absent = unset (retail nil), so a flag never set is not `false` (research b889).
    #[serde(default)]
    pub flags: BTreeMap<String, bool>,
    /// Where the challenge statuses were imported from, once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for CareerFile {
    fn default() -> Self {
        Self { format: FORMAT.into(), version: VERSION, challenges: BTreeMap::new(), counters: Counters::default(),
            actions: Actions::default(), rewards: Rewards::default(), team: TeamFile::default(), unlocks: Vec::new(), active: Vec::new(), flags: BTreeMap::new(),
            imported: None, boot: BootFile::default(),
            extra: Map::new() }
    }
}

impl CareerFile {
    /// Parses a saved career; anything that is not this format gives an error (the caller keeps the file aside).
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if file.format != FORMAT {
            return Err(format!("not a career save ({})", file.format));
        }
        Ok(file)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Imports the older challenge records (`skate3rust.challenge-records`: `{"challenges": {id: {...}}}`) once, as
    /// statuses only: nothing is paid and no counter changes. Entries already in the career are kept.
    pub fn import_challenge_records(&mut self, text: &str, source: &str) -> Result<usize, String> {
        if self.imported.is_some() {
            return Ok(0);
        }
        let value: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let records = value.get("challenges").and_then(Value::as_object).ok_or("no challenges object")?;
        let mut n = 0;
        for (id, entry) in records {
            if self.challenges.contains_key(id) {
                continue;
            }
            if let Ok(entry) = serde_json::from_value::<ChallengeEntry>(entry.clone()) {
                self.challenges.insert(id.clone(), entry);
                n += 1;
            }
        }
        self.imported = Some(source.to_owned());
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_keys_survive_a_round_trip() {
        let text = r#"{"format":"skate3rust.career","version":1,"future":{"a":1},
            "challenges":{"ots_a":{"completion":"owned","high_score":1200,"discovery":2,"mod_note":"x"}},
            "counters":{"sales_owned":10000,"new_counter":5},"actions":{"armed":["m1"],"fired":["starting_action"],"queue":[]},
            "flags":{"CompletedBasicSchool":true}}"#;
        let file = CareerFile::parse(text).unwrap();
        assert_eq!(file.counters.sales_owned, 10_000);
        let back = CareerFile::parse(&file.to_json()).unwrap();
        assert_eq!(back, file);
        let json: Value = serde_json::from_str(&file.to_json()).unwrap();
        assert_eq!(json["future"]["a"], 1);
        assert_eq!(json["challenges"]["ots_a"]["mod_note"], "x");
        assert_eq!(json["counters"]["new_counter"], 5);
        assert!(json["actions"]["queue"].is_array());
        assert!(CareerFile::parse(r#"{"format":"other","version":1}"#).is_err());
    }

    #[test]
    fn old_records_import_once_as_statuses_without_pay() {
        let old = r#"{"format":"skate3rust.challenge-records","version":1,
            "challenges":{"ots_a":{"completion":"owned","high_score":900,"discovery":2},"ots_b":{"discovery":1}}}"#;
        let mut file = CareerFile::default();
        file.challenges.insert("ots_b".into(), ChallengeEntry { completion: Some("killed".into()), ..Default::default() });
        assert_eq!(file.import_challenge_records(old, "settings/challenges.json"), Ok(1));
        assert_eq!(file.challenges["ots_a"].completion.as_deref(), Some("owned"));
        assert_eq!(file.challenges["ots_b"].completion.as_deref(), Some("killed"), "career entries win");
        assert_eq!(file.counters, Counters::default(), "imported wins pay nothing");
        assert_eq!(file.import_challenge_records(old, "again"), Ok(0), "only once");
    }

    #[test]
    fn a_missing_flag_is_unset_not_false() {
        let file = CareerFile::parse(r#"{"format":"skate3rust.career","version":1,"flags":{"A":false}}"#).unwrap();
        assert_eq!(file.flags.get("A"), Some(&false));
        assert_eq!(file.flags.get("B"), None);
    }

    #[test]
    fn a_career_saved_before_the_boot_flags_is_not_pending() {
        let file = CareerFile::parse(r#"{"format":"skate3rust.career","version":1}"#).unwrap();
        assert!(!file.boot.pending);
        let mut fresh = CareerFile::default();
        fresh.boot.pending = true;
        let back = CareerFile::parse(&fresh.to_json()).unwrap();
        assert!(back.boot.pending);
    }
}

fn is_default_boot(b: &BootFile) -> bool {
    *b == BootFile::default()
}
