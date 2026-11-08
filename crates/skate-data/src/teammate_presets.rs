//! Teammate presets (research b986, b988, b990, b994): the leaf rows of vault class progression_preset_teammate, each
//! naming `data/cacrecipes/SavedRecipe<id>.bin`, read into a customiser profile at runtime from the installed files
//! (no setup step). Retail lists the rows sorted by their localised name (`sorted_by`). Details:
//! docs/hails-additions/43-career.md
use crate::collections::Collections;
use serde_json::{json, Map, Value};
use std::path::Path;

const PRESET_CLASS: &str = "Hash_12BCDBD69BA7D33D";
const PRESET_ID: &str = "Hash_3F1DD58F86CAAB1E";
const PRESET_NAME: &str = "Hash_8063E6A0DB33E680";
const MORPH_CLASS: &str = "Hash_7F47CA62F4100F41";
const MORPH_TARGET: &str = "Hash_AAC0F30B119BD14F";
const MORPH_ORDER: &str = "Hash_0D381DFA82C751E1";
const MORPHS: usize = 19;

#[derive(Clone, Debug, PartialEq)]
pub struct PresetRow {
    /// Row key (`preset00`.. when the name resolves, else the hashed key).
    pub key: String,
    pub id: u32,
    /// Text id of the name (`ID_TEAMMATE_PRESET_01`..).
    pub name_text: String,
}

/// The leaf rows (a row that is another row's parent, such as `default`, is not a preset; 828DA0D8), by id.
pub fn preset_rows(c: &Collections) -> Vec<PresetRow> {
    use crate::attrib_hash::numeric_name;
    let class = numeric_name(PRESET_CLASS);
    let rows: Vec<_> = c.entries().iter().filter(|e| numeric_name(&e.class_name) == class).collect();
    let parents: std::collections::BTreeSet<String> =
        rows.iter().filter(|e| !e.parent.is_empty()).map(|e| numeric_name(&e.parent)).collect();
    let names: Vec<String> = (0..100).map(|i| format!("preset{i:02}")).collect();
    let mut out: Vec<PresetRow> = rows.iter()
        .filter(|e| !parents.contains(&numeric_name(&e.key)))
        .map(|e| {
            let h = numeric_name(&e.key);
            PresetRow {
                key: names.iter().find(|n| numeric_name(n) == h).cloned().unwrap_or_else(|| e.key.clone()),
                id: c.integer(PRESET_CLASS, &e.key, PRESET_ID).unwrap_or(0),
                name_text: c.field(PRESET_CLASS, &e.key, PRESET_NAME).map(|f| f.data.clone()).unwrap_or_default(),
            }
        })
        .collect();
    out.sort_by(|a, b| (a.id, &a.key).cmp(&(b.id, &b.key)));
    out
}

/// Retail's list order: by localised name (8255A698).
pub fn sorted_by(mut rows: Vec<PresetRow>, localise: impl Fn(&str) -> String) -> Vec<PresetRow> {
    rows.sort_by_cached_key(|r| localise(&r.name_text));
    rows
}

/// The 19 recipe morph names by the table index (vault `target_order`, 8253CA60); empty if the rows are incomplete.
pub fn morph_order(c: &Collections) -> Vec<String> {
    use crate::attrib_hash::numeric_name;
    let class = numeric_name(MORPH_CLASS);
    let mut names = vec![String::new(); MORPHS];
    for e in c.entries().iter().filter(|e| numeric_name(&e.class_name) == class && e.key != "default") {
        let Ok(order) = c.integer(MORPH_CLASS, &e.key, MORPH_ORDER) else { continue };
        if let (Some(slot), Ok(target)) = (names.get_mut(order as usize), c.field(MORPH_CLASS, &e.key, MORPH_TARGET)) {
            slot.clone_from(&target.data);
        }
    }
    if names.iter().any(String::is_empty) { Vec::new() } else { names }
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn bytes(&self, at: usize, n: usize) -> Result<&[u8], String> {
        self.data.get(at..at + n).ok_or_else(|| format!("recipe ends at {at:#x}"))
    }
    fn byte(&self, at: usize) -> Result<u8, String> {
        Ok(self.bytes(at, 1)?[0])
    }
    fn u32(&self, at: usize) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.bytes(at, 4)?.try_into().unwrap()))
    }
    fn f32(&self, at: usize) -> Result<f32, String> {
        Ok(f32::from_bits(self.u32(at)?))
    }
    fn hex(&self, at: usize, n: usize) -> Result<String, String> {
        Ok(self.bytes(at, n)?.iter().map(|b| format!("{b:02x}")).collect())
    }
}

/// A `SavedRecipe<id>.bin` (the serialised slot record, b988) as a customiser profile: gender, the first asset and
/// material per slot, truck / wheel, gestures, morphs by name, and the tint and tattoo records kept as
/// `retail_tints` / `retail_tattoos` (their mapping onto the customiser is open, b990).
pub fn recipe_profile(file: &[u8], morph_names: &[String]) -> Result<Map<String, Value>, String> {
    let outer = Reader { data: file, at: 0 };
    if outer.u32(0)? != 8 {
        return Err("recipe slot stream version is not 8".into());
    }
    let inner_version = outer.u32(8)?;
    let state_len = outer.u32(0xC)? as usize;
    let mut r = Reader { data: outer.bytes(0x10, state_len)?, at: 0 };
    let name_len = r.byte(7)? as usize;
    let full_payload = r.byte(0x13 + name_len)? == 2;
    r.at = 0x18 + name_len;
    let lists = r.byte(r.at - 1)?;
    let mut selections = Map::new();
    for _ in 0..lists {
        let len = r.byte(r.at + 3)? as usize;
        let slot = String::from_utf8_lossy(r.bytes(r.at + 4, len)?).into_owned();
        r.at += 8 + len;
        let assets = r.byte(r.at - 1)?;
        for a in 0..assets {
            let asset_id = r.hex(r.at, 8)?;
            let models = r.byte(r.at + 11)?;
            r.at += 12;
            for m in 0..models {
                let block = r.at;
                let (loops, textures) = (r.byte(block + 0x14)?, r.byte(block + 0x24)?);
                if a == 0 && m == 0 {
                    selections.insert(slot.clone(), json!({"asset_id": asset_id, "material_id": r.hex(block + 0x19, 8)?}));
                }
                r.at += 0x25;
                for _ in 0..textures {
                    r.at += 12 + r.byte(r.at + 3)? as usize;
                }
                for _ in 1..loops.max(1) {
                    r.at += 0x10;
                    for _ in 0..r.byte(r.at - 1)? {
                        r.at += 12 + r.byte(r.at + 3)? as usize;
                    }
                }
            }
        }
    }
    let mut profile = Map::new();
    profile.insert("selections".into(), Value::Object(selections));
    if full_payload {
        r.at += 5;
        profile.insert("gender".into(), json!(if r.byte(r.at)? == 0 { "female" } else { "male" }));
        r.at += 9;
        let mut tints = Vec::new();
        for _ in 0..r.byte(r.at - 1)? {
            let rgb = |o: usize| -> Result<Vec<f32>, String> { (0..3).map(|i| r.f32(r.at + o + i * 4)).collect() };
            tints.push(json!({"index": r.u32(r.at)?, "a": rgb(4)?, "b": rgb(16)?,
                "asset_id": r.hex(r.at + 28, 8)?, "material_id": r.hex(r.at + 36, 8)?}));
            r.at += 0x2C;
        }
        r.at += 8;
        let mut tattoos = Vec::new();
        for _ in 0..r.byte(r.at - 1)? {
            let len = r.byte(r.at + 3)? as usize;
            let ids = (0..3).map(|i| r.hex(r.at + 4 + len + i * 8, 8)).collect::<Result<Vec<_>, _>>()?;
            tattoos.push(json!({"material": String::from_utf8_lossy(r.bytes(r.at + 4, len)?), "ids": ids,
                "bytes": r.hex(r.at + 28 + len, 2)?}));
            r.at += 30 + len;
        }
        r.at += 4;
        let floats = (0..MORPHS).map(|i| r.f32(r.at + i * 4)).collect::<Result<Vec<_>, _>>()?;
        if morph_names.len() == MORPHS {
            let morphs: Map<String, Value> = morph_names.iter().zip(&floats).map(|(n, v)| (n.clone(), json!(v))).collect();
            profile.insert("morphs".into(), Value::Object(morphs));
        }
        profile.insert("retail_tints".into(), Value::Array(tints));
        profile.insert("retail_tattoos".into(), Value::Array(tattoos));
    }
    // After the state: truck, wheel, three u32 (four from inner version 11), then the four gestures (82DDE8E0).
    let tail = 0x10 + state_len;
    profile.insert("truck".into(), json!(outer.f32(tail)?));
    profile.insert("wheel".into(), json!(outer.f32(tail + 4)?));
    let gestures_at = tail + 8 + if inner_version > 10 { 16 } else { 12 };
    let gestures: Map<String, Value> =
        (0..4).map(|i| Ok((i.to_string(), json!(outer.u32(gestures_at + i * 4)?)))).collect::<Result<_, String>>()?;
    profile.insert("gestures".into(), Value::Object(gestures));
    Ok(profile)
}

/// Preset `row`'s profile from the installed recipe files.
pub fn load_profile(asset_root: &Path, row: &PresetRow, morph_names: &[String]) -> Result<Map<String, Value>, String> {
    let path = asset_root.join(format!("private/stock/data/cacrecipes/SavedRecipe{}.bin", row.id));
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    recipe_profile(&bytes, morph_names)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal recipe in the retail layout: one slot (one asset, one model, one texture), male, one tint, one
    /// tattoo, 19 morph floats, then truck / wheel / ints / gestures.
    fn recipe() -> Vec<u8> {
        let mut s = vec![0, 0, 0, 7, 0, 0, 0, 6];
        s.extend(b"cas_db");
        s.extend([0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 1]);
        s.extend([0, 0, 0, 4]);
        s.extend(b"Hair");
        s.extend([0, 0, 0, 1]);
        s.extend([0xAA; 8]);
        s.extend([0, 0, 0, 1]);
        let mut block = [0u8; 0x25];
        block[0x14] = 1;
        block[0x19..0x21].copy_from_slice(&[0xBB; 8]);
        block[0x24] = 1;
        s.extend(block);
        s.extend([0, 0, 0, 7]);
        s.extend(b"diffuse");
        s.extend([0xCC; 8]);
        s.extend([0, 0, 0, 0, 0, 1, 0, 0, 0, 6, 0, 0, 0, 1]);
        s.extend(5u32.to_be_bytes());
        for v in [0.1f32, 0.2, 0.3, 0.76, 0.57, 0.51] {
            s.extend(v.to_be_bytes());
        }
        s.extend([0x11; 8]);
        s.extend([0x22; 8]);
        s.extend([0, 0, 0, 0, 0, 0, 0, 1]);
        s.extend([0, 0, 0, 3]);
        s.extend(b"tat");
        s.extend([0x33; 24]);
        s.extend([0, 3]);
        s.extend([0; 4]);
        for i in 0..19 {
            s.extend((i as f32 / 100.0).to_be_bytes());
        }
        let mut f = Vec::new();
        f.extend(8u32.to_be_bytes());
        f.extend((s.len() as u32).to_be_bytes());
        f.extend(12u32.to_be_bytes());
        f.extend((s.len() as u32).to_be_bytes());
        f.extend(&s);
        for v in [0.7f32, 0.3] {
            f.extend(v.to_be_bytes());
        }
        f.extend([0u8; 16]);
        for g in [4u32, 9, 2, 34] {
            f.extend(g.to_be_bytes());
        }
        f
    }

    #[test]
    fn a_recipe_reads_into_a_profile() {
        let names: Vec<String> = (0..19).map(|i| format!("m{i}")).collect();
        let p = recipe_profile(&recipe(), &names).unwrap();
        assert_eq!(p["selections"]["Hair"], json!({"asset_id": "aa".repeat(8), "material_id": "bb".repeat(8)}));
        assert_eq!(p["gender"], "male");
        assert_eq!(p["retail_tints"][0]["index"], 5);
        assert_eq!(p["retail_tints"][0]["asset_id"], "11".repeat(8));
        assert_eq!(p["retail_tattoos"][0]["material"], "tat");
        assert_eq!(p["retail_tattoos"][0]["bytes"], "0003");
        assert!((p["morphs"]["m3"].as_f64().unwrap() - 0.03).abs() < 1e-6);
        assert!((p["truck"].as_f64().unwrap() - 0.7).abs() < 1e-6);
        assert_eq!(p["gestures"], json!({"0": 4, "1": 9, "2": 2, "3": 34}));
        assert!(recipe_profile(&recipe()[..40], &names).is_err());
    }

    #[test]
    #[ignore = "requires the converted vault and recipes (SKATE3_ASSET_ROOT)"]
    fn the_retail_presets() {
        let root = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").expect("asset root"));
        let c = Collections::load(&root).unwrap();
        let rows = preset_rows(&c);
        assert_eq!(rows.iter().map(|r| (r.key.as_str(), r.id)).collect::<Vec<_>>(),
            (0..10).map(|i| (format!("preset{i:02}"), i)).collect::<Vec<_>>().iter().map(|(k, i)| (k.as_str(), *i)).collect::<Vec<_>>());
        assert_eq!(rows[0].name_text, "ID_TEAMMATE_PRESET_01");
        let names = morph_order(&c);
        assert_eq!((names[0].as_str(), names[1].as_str(), names[2].as_str()), ("fat", "thin", "local_brows_rotation"));
        for row in &rows {
            let p = load_profile(&root, row, &names).unwrap_or_else(|e| panic!("{}: {e}", row.key));
            assert_eq!(p["retail_tints"].as_array().unwrap().len(), 13, "{}", row.key);
            assert!(p["selections"].as_object().unwrap().contains_key("SkateBoard"), "{}", row.key);
            assert!(p["gestures"].as_object().unwrap().values().all(|g| g.as_u64().unwrap() < 37), "{}", row.key);
            assert!(["truck", "wheel"].iter().all(|k| (0.0..=1.0).contains(&p[*k].as_f64().unwrap())), "{}", row.key);
            assert!(p["morphs"].as_object().unwrap().values().all(|v| (0.0..=0.5).contains(&v.as_f64().unwrap())), "{}", row.key);
        }
    }
}
