//! The saved skater looks `settings/profiles/<player>/cac.json`: retail's own "CAC" save record beside the career
//! (research b983). Slot 0 is the player (whose look stays in `settings/character.json`), slot n teammate n. A slot
//! with a look opens teammate n's spawn gate (retail: the slot's state length != 0, b977). An unreadable slot loads
//! as empty, as retail resets a slot whose read fails. Details: docs/hails-additions/43-career.md
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const FORMAT: &str = "skate3rust.cac";
pub const VERSION: u32 = 1;
pub const SLOTS: usize = 5;

/// Where a look came from: a preset row key, the customiser, or a mod (cleared when that mod stops).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LookSource {
    Preset { id: String },
    Custom,
    Mod { id: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlotLook {
    pub source: LookSource,
    /// A customiser profile (`settings/character.json` form). Retail parts the customiser does not apply yet (tint
    /// records, stamps) ride along as extra keys.
    pub profile: Map<String, Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CacFile {
    pub format: String,
    pub version: u32,
    #[serde(default)]
    pub slots: Vec<Option<SlotLook>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for CacFile {
    fn default() -> Self {
        Self { format: FORMAT.into(), version: VERSION, slots: vec![None; SLOTS], extra: Map::new() }
    }
}

impl CacFile {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut value: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let slots = value.as_object_mut().and_then(|o| o.remove("slots"));
        let mut file: Self = serde_json::from_value(value).map_err(|e| e.to_string())?;
        if file.format != FORMAT {
            return Err(format!("not a look save ({})", file.format));
        }
        file.slots = slots.as_ref().and_then(Value::as_array).map_or_else(Vec::new, |a| {
            a.iter().map(|s| serde_json::from_value::<SlotLook>(s.clone()).ok().filter(|l| !l.profile.is_empty())).collect()
        });
        if file.slots.len() < SLOTS {
            file.slots.resize(SLOTS, None);
        }
        Ok(file)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    pub fn look(&self, slot: usize) -> Option<&SlotLook> {
        self.slots.get(slot).and_then(Option::as_ref)
    }

    /// The spawn gate of teammate `slot`.
    pub fn has_look(&self, slot: usize) -> bool {
        self.look(slot).is_some()
    }

    /// Writes or clears a slot; an empty profile clears it (retail's gate is a non-empty state).
    pub fn set(&mut self, slot: usize, look: Option<SlotLook>) {
        if self.slots.len() <= slot {
            self.slots.resize(slot + 1, None);
        }
        self.slots[slot] = look.filter(|l| !l.profile.is_empty());
    }

    /// Clears the looks a mod set (its gate closes, as a failed read resets a retail slot). Returns the slots.
    pub fn clear_mod(&mut self, id: &str) -> Vec<usize> {
        let mut cleared = Vec::new();
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if slot.as_ref().is_some_and(|l| matches!(&l.source, LookSource::Mod { id: m } if m == id)) {
                *slot = None;
                cleared.push(i);
            }
        }
        cleared
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn look(source: LookSource) -> SlotLook {
        SlotLook { source, profile: json!({"gender": "female", "tints": [[0, 0.76, 0.57, 0.51]]}).as_object().unwrap().clone(), extra: Map::new() }
    }

    #[test]
    fn looks_round_trip_and_open_the_gate() {
        let mut f = CacFile::default();
        assert!(!f.has_look(2));
        f.set(2, Some(look(LookSource::Preset { id: "teammate_preset_03".into() })));
        f.set(3, Some(SlotLook { profile: Map::new(), ..look(LookSource::Custom) }));
        let again = CacFile::parse(&f.to_json()).unwrap();
        assert!(again.has_look(2) && !again.has_look(3));
        assert_eq!(again.look(2).unwrap().profile["tints"], json!([[0, 0.76, 0.57, 0.51]]));
        assert_eq!(again.slots.len(), SLOTS);
    }

    #[test]
    fn an_unreadable_slot_loads_empty_and_the_rest_stays() {
        let text = json!({"format": FORMAT, "version": 1, "future": 7,
            "slots": [null, {"source": {"kind": "nope"}, "profile": {"a": 1}}, {"source": {"kind": "custom"}, "profile": {"a": 1}}]});
        let f = CacFile::parse(&text.to_string()).unwrap();
        assert!(!f.has_look(1) && f.has_look(2));
        assert_eq!(f.extra["future"], 7);
        assert!(CacFile::parse(r#"{"format": "x", "version": 1}"#).is_err());
    }

    #[test]
    fn a_stopped_mod_loses_only_its_own_looks() {
        let mut f = CacFile::default();
        f.set(1, Some(look(LookSource::Mod { id: "a".into() })));
        f.set(2, Some(look(LookSource::Mod { id: "b".into() })));
        f.set(3, Some(look(LookSource::Custom)));
        assert_eq!(f.clear_mod("a"), [1]);
        assert!(!f.has_look(1) && f.has_look(2) && f.has_look(3));
    }
}
