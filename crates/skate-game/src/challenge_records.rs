//! The challenge records kept across sessions (`settings/challenges.json` beside the install): per challenge id the
//! best completion, the high score and the discovery status. Retail keeps the first two in the career save's
//! `ChallengeStatusData` and the third in `cChallengeDiscoveryManager`, both keyed by a hash of the challenge name
//! (research b846); ours keys by the id string, so a mod's challenges (their own ids) keep their records too, and a
//! record whose challenge is not loaded (a disabled mod) stays in the file untouched.
use serde::{Deserialize, Serialize};
use skate_core::challenges::{Completion, Record};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Retail `Discovery.eDiscoveryStatus` (828388E0 / 828389A0).
pub(crate) const NOT_DISCOVERED: u32 = 0;
pub(crate) const DISCOVERED: u32 = 1;
pub(crate) const SEEN_INTRO: u32 = 2;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Saved {
    /// "owned" or "killed"; absent before the first completion (abandoned is never stored).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<String>,
    #[serde(default)]
    pub high_score: i32,
    #[serde(default)]
    pub discovery: u32,
    /// A race's best time in seconds (retail SetHighScore with the f32 time, race_shared cleanup_heat; research b982).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best_time: Option<f32>,
}

impl Saved {
    pub fn record(&self) -> Record {
        let completion = match self.completion.as_deref() {
            Some("owned") => Some(Completion::Owned),
            Some("killed") => Some(Completion::Killed),
            _ => None,
        };
        Record { completion, high_score: self.high_score }
    }

    pub fn set_record(&mut self, record: Record) {
        self.completion = match record.completion {
            Some(Completion::Owned) => Some("owned".into()),
            Some(Completion::Killed) => Some("killed".into()),
            Some(Completion::Abandoned) | None => None,
        };
        self.high_score = record.high_score;
    }
}

#[derive(Serialize, Deserialize)]
struct File {
    format: String,
    version: u32,
    records: BTreeMap<String, Saved>,
}

const FORMAT: &str = "skate3rust.challenge-records";

#[derive(Default)]
pub(crate) struct Records {
    pub entries: BTreeMap<String, Saved>,
    path: Option<PathBuf>,
}

impl Records {
    /// The records at `path`; a missing or unreadable file gives none (logged), so a bad file never stops play.
    pub fn load(path: PathBuf) -> Self {
        let entries = match std::fs::read(&path) {
            Err(_) => BTreeMap::new(),
            Ok(bytes) => match serde_json::from_slice::<File>(&bytes) {
                Ok(f) if f.format == FORMAT && f.version == 1 => f.records,
                Ok(f) => {
                    bevy::log::warn!("SKATE_CHALLENGES {}: unsupported {} v{}", path.display(), f.format, f.version);
                    BTreeMap::new()
                }
                Err(e) => {
                    bevy::log::warn!("SKATE_CHALLENGES {}: {e}", path.display());
                    BTreeMap::new()
                }
            },
        };
        Self { entries, path: Some(path) }
    }

    pub fn get(&self, id: &str) -> Saved {
        self.entries.get(id).cloned().unwrap_or_default()
    }

    /// Change one record; true when it changed (the caller saves).
    pub fn update(&mut self, id: &str, change: impl FnOnce(&mut Saved)) -> bool {
        let before = self.get(id);
        let mut after = before.clone();
        change(&mut after);
        if after == before {
            return false;
        }
        self.entries.insert(id.to_owned(), after);
        true
    }

    fn bytes(&self) -> Option<Vec<u8>> {
        serde_json::to_vec_pretty(&File { format: FORMAT.into(), version: 1, records: self.entries.clone() }).ok()
    }

    /// Writes the file on a short-lived thread (atomic: `.part`, then rename), so play never waits on the disk.
    pub fn save_in_background(&self) {
        if let (Some(path), Some(bytes)) = (self.path.clone(), self.bytes()) {
            let writing = crate::save_tag::Writing::begin();
            let spawned = std::thread::Builder::new().name("challenge-records-save".into()).spawn(move || { let _writing = writing; write(&path, &bytes) });
            if spawned.is_err() {
                bevy::log::warn!("SKATE_CHALLENGES could not start the record save");
            }
        }
    }
}

fn write(path: &Path, bytes: &[u8]) -> bool {
    let part = path.with_extension("json.part");
    let written = path.parent().is_none_or(|p| std::fs::create_dir_all(p).is_ok())
        && std::fs::write(&part, bytes).is_ok()
        && std::fs::rename(&part, path).is_ok();
    if !written {
        bevy::log::warn!("SKATE_CHALLENGES could not write {}", path.display());
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip_and_keep_unknown_challenges() {
        let dir = std::env::temp_dir().join(format!("skate-challenge-records-{}", std::process::id()));
        let path = dir.join("settings/challenges.json");
        let mut records = Records::load(path.clone());
        assert!(records.entries.is_empty());
        assert!(records.update("ots_dwtn_01", |s| {
            let mut r = s.record();
            r.complete(Completion::Owned);
            r.high_score = 1200;
            s.set_record(r);
            s.discovery = SEEN_INTRO;
        }));
        assert!(!records.update("ots_dwtn_01", |s| s.discovery = SEEN_INTRO), "no change, no save");
        records.entries.insert("some_mod.spot".into(), Saved { completion: Some("killed".into()), high_score: 9, discovery: DISCOVERED, best_time: None });
        assert!(write(&path, &records.bytes().unwrap()));
        let back = Records::load(path);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(back.get("ots_dwtn_01").record(), Record { completion: Some(Completion::Owned), high_score: 1200 });
        assert_eq!(back.get("ots_dwtn_01").discovery, SEEN_INTRO);
        assert_eq!(back.get("some_mod.spot").high_score, 9);
        assert_eq!(back.get("missing").discovery, NOT_DISCOVERED);
    }

    #[test]
    fn a_bad_file_loads_as_no_records() {
        let dir = std::env::temp_dir().join(format!("skate-challenge-records-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("challenges.json");
        std::fs::write(&path, b"{not json").unwrap();
        let records = Records::load(path);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(records.entries.is_empty());
    }
}
