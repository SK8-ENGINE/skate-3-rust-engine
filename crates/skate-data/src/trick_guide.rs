//! The Trick Guide data (doc 31 "Trick Guide") from the installed skater collections and the menu tables: the
//! scoring_trickcategory / scoring_tricksubcategory / scoring_trick records by Attrib key, the category order and the
//! slot name tables. No game data lives here; everything comes from the user's disc at setup.
use crate::{collections::Collections, menu_tables::TrickGuideTables};
use skate_core::trick_guide::{Category, Entry, Key, Slot, Subcategory, Trick, TrickGuide};

const CATEGORY: &str = "Hash_8BD17173D2F48577";
const SUBCATEGORY: &str = "Hash_187447EFFFDF8563";
const TRICK: &str = "Hash_6918469984A8C596";
const NAME: &str = "Hash_7296FFD6010D73EF";
const DESCRIPTION: &str = "Hash_273DF7D4EE7A4C1C";
const TRICK_LABEL: &str = "Hash_843613E915014627";
const TRICK_DESCRIPTION: &str = "Hash_4663ADFED2CD5EF7";
const ANIMATION: &str = "Hash_3B71EA27D53D7C4F";
const SLOTS: &str = "Hash_03ACC9F200807B87";
const SUBCATEGORIES: &str = "Hash_57035D975F71D506";
const TRICK_SUBCATEGORY: &str = "Hash_06513CC28A532524";
const HAS_TRICKS: &str = "Hash_A990472B974734C3";
const ORDER: &str = "Hash_F9E5D2F69066E925";
const CAMERA_TRACK: &str = "Hash_3D3A79F16DCB54BB";
/// `trickbook_camera_tracks` and its one field (Int16 at offset 0).
const CAMERA_TRACKS: &str = "Hash_7CCE51F922C0591D";
const CAMERA_TRACK_INDEX: &str = "Hash_FB37E863C5C1700D";

fn key_of(name: &str) -> Option<Key> {
    u64::from_str_radix(crate::attrib_hash::numeric_name(name).strip_prefix("Hash_")?, 16).ok()
}

fn hex_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len() / 2).filter_map(|i| u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()).collect()
}

fn word(bytes: &[u8], at: usize) -> u32 {
    bytes.get(at..at + 4).map_or(0, |b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// A ClassRefSpec (record key, pointer; the class is in the type): the record key.
fn class_ref_key(hex: &str) -> Option<Key> {
    let b = hex_bytes(hex);
    Some(u64::from_be_bytes(b.get(..8)?.try_into().ok()?)).filter(|k| *k != 0)
}

/// A RefSpec (class key, record key, pointer): the record key.
fn ref_key(hex: &str) -> Option<Key> {
    let b = hex_bytes(hex);
    let key = b.get(8..16)?;
    Some(u64::from_be_bytes(key.try_into().ok()?)).filter(|k| *k != 0)
}

struct Reader<'a> {
    data: &'a Collections,
    cameras: &'a std::collections::HashMap<Key, i16>,
    class: &'static str,
    key: &'a str,
}

impl Reader<'_> {
    fn text(&self, field: &str) -> String {
        self.data.field(self.class, self.key, field).map(|f| f.data.clone()).unwrap_or_default()
    }
    fn items(&self, field: &str) -> Vec<String> {
        self.data.field(self.class, self.key, field).ok().and_then(|f| f.array.as_ref()).map(|a| a.items.clone()).unwrap_or_default()
    }
    fn slots(&self) -> Vec<Slot> {
        self.items(SLOTS)
            .iter()
            .map(|hex| {
                let b = hex_bytes(hex);
                Slot { gesture: word(&b, 0), button: word(&b, 4), connector: word(&b, 8) }
            })
            .collect()
    }
    fn entry(&self, label: &str, description: &str) -> Entry {
        // sub_826188B8: the referenced record's Int16; no ref (or no record) reads 0.
        let camera_track = self.data.field(self.class, self.key, CAMERA_TRACK).ok().and_then(|f| class_ref_key(&f.data))
            .and_then(|k| self.cameras.get(&k).copied()).unwrap_or(0);
        Entry { label: self.text(label), description: self.text(description), slots: self.slots(), animation: self.text(ANIMATION), camera_track }
    }
}

/// Builds the guide; records that are missing or malformed read as empty (the screen shows what there is).
pub fn load(data: &Collections, tables: &TrickGuideTables) -> TrickGuide {
    let mut guide = TrickGuide {
        categories: tables.categories.iter().filter_map(|k| u64::from_str_radix(k, 16).ok()).collect(),
        gestures: tables.gestures.clone(),
        buttons: tables.buttons.clone(),
        ..Default::default()
    };
    let cameras: std::collections::HashMap<Key, i16> = data.entries().iter()
        .filter(|row| crate::attrib_hash::numeric_name(&row.class_name) == CAMERA_TRACKS)
        .filter_map(|row| Some((key_of(&row.key)?, {
            // EA::Reflection::Int16, big-endian hex.
            let field = data.field(CAMERA_TRACKS, &row.key, CAMERA_TRACK_INDEX).ok()?;
            Some(i16::from_be_bytes(hex_bytes(&field.data).get(..2)?.try_into().ok()?))
        }?)))
        .collect();
    for row in data.entries() {
        let class = crate::attrib_hash::numeric_name(&row.class_name);
        let Some(key) = key_of(&row.key) else { continue };
        if class == CATEGORY {
            let r = Reader { data, cameras: &cameras, class: CATEGORY, key: &row.key };
            let subcategories = r.items(SUBCATEGORIES).iter().filter_map(|i| ref_key(i)).collect();
            guide.category.insert(key, Category { entry: r.entry(NAME, DESCRIPTION), subcategories });
        } else if class == SUBCATEGORY {
            let r = Reader { data, cameras: &cameras, class: SUBCATEGORY, key: &row.key };
            let has_tricks = data.boolean(SUBCATEGORY, &row.key, HAS_TRICKS).unwrap_or(false);
            guide.subcategory.insert(key, Subcategory { entry: r.entry(NAME, DESCRIPTION), has_tricks });
        } else if class == TRICK {
            let r = Reader { data, cameras: &cameras, class: TRICK, key: &row.key };
            let subcategory = data.field(TRICK, &row.key, TRICK_SUBCATEGORY).ok().and_then(|f| ref_key(&f.data));
            let order = data.integer(TRICK, &row.key, ORDER).map_or(0, |v| v as i32);
            guide.tricks.push((key, Trick { entry: r.entry(TRICK_LABEL, TRICK_DESCRIPTION), subcategory, order }));
        }
    }
    guide
}
