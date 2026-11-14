//! A map's audio as data (doc 16 "Audio modding", R2): which `.ems` files the emitter system
//! loads, extra emitter / reverb-zone records, whose world-painter regions apply (zone ambience,
//! location sets, reverb), box regions, the crossfade bank. Built when the map or the audio
//! content changes, from, in order (later sources override the fields they set and append their
//! records and boxes):
//! 1. retail: the map's database entry, read at run time from the stock collections every
//!    install has (`world` row by `WorldStream` = `DIST_<stem>` → field `99D6E51C9E20A663` →
//!    class `F4917ACACAFAF913`: field `65FA976EF23A314E` = the `.ems` list, `33526BC9D1C36B4A` =
//!    the crossfade bank). No setup refresh;
//! 2. the map's own definition: a `.skate` `AUDO` extension (schema 1, UTF-8 JSON), else a
//!    `<map>.audio.json` sidecar next to the `.skate` (custom maps);
//! 3. audio content overlays' `maps.<stem>` (mods; first owner by mod id per field).
//!
//! The same shape everywhere: `skate_mods::audio_content::MapAudioDef`. Before 2026-10-04 the
//! map → `.ems` list and the crossfade banks were code tables for the ten retail maps; they stay
//! only as the test oracle (`retail_maps_reproduce_the_old_tables`).
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use bevy::prelude::*;

use super::Library;
use super::library::{EmitterRecord, MapJson, RegionTile};

/// A map's audio (see the module docs).
#[derive(Resource, Debug, Default, Clone)]
pub(crate) struct MapAudio {
    /// (map path, map generation, audio content generation) it was built for.
    key: Option<(Option<PathBuf>, u64, u64)>,
    /// The map's `.skate` stem ("" for the test world).
    pub stem: String,
    /// Whose retail region layers apply (the stem unless a definition names another district).
    pub district: String,
    /// The `.ems` files, in the database entry's order.
    pub ems: Vec<String>,
    /// The zone-transition crossfade bank (a bank stem), if the map has one.
    pub crossfade_bank: Option<String>,
    /// A bed for installs without zone data (not retail; `ambience::BEDS` otherwise).
    pub fallback_bed: Option<String>,
    /// Records after the `.ems` files' (definitions and overlays), indexed in order.
    pub emitters: Vec<EmitterRecord>,
    /// Box regions per layer, checked before the district's tiles.
    regions: BTreeMap<String, Vec<RegionTile>>,
    /// Where the definition came from (log / info): "retail", "tag", "sidecar", "mod", "none".
    pub sources: Vec<&'static str>,
}

impl MapAudio {
    /// The key a layer holds at (x, z): a box region of the map's definition first, then the
    /// district's retail tiles.
    pub(crate) fn region_key(&self, library: &Library, layer: &str, x: f32, z: f32) -> Option<u64> {
        if let Some(k) = self.regions.get(layer).and_then(|tiles| tiles.iter().find_map(|t| t.key(x, z))) {
            return Some(k);
        }
        library.region_key(&self.district, layer, x, z)
    }

    /// Every zone key a layer's tiles can return on this map: the map's own boxes and its district's retail tiles.
    pub(crate) fn region_tile_keys<'a>(&'a self, library: &'a Library, layer: &str) -> impl Iterator<Item = &'a str> + 'a {
        let own = self.regions.get(layer).map_or(&[][..], Vec::as_slice);
        own.iter().chain(library.region_tiles(&self.district, layer)).flat_map(|t| t.keys.iter().map(String::as_str))
    }

    /// Every record the emitter system loads, with its file number (the reverb zones' ids keep
    /// retail's `file << 32 | index`): the `.ems` files' records, then the extra records as one
    /// more file.
    pub(crate) fn records<'a>(&'a self, library: &'a Library) -> impl Iterator<Item = (usize, &'a EmitterRecord)> + 'a {
        let extra = self.ems.len();
        self.ems.iter().enumerate().flat_map(move |(f, file)| library.emitters(file).iter().map(move |r| (f, r)))
            .chain(self.emitters.iter().map(move |r| (extra, r)))
    }

    /// Apply one definition: fields it sets replace, records and boxes append.
    fn apply(&mut self, def: &MapJson, library: &Library, source: &'static str) {
        if let Some(d) = &def.district {
            self.district = d.clone();
        }
        if let Some(ems) = &def.ems {
            self.ems = ems.iter().map(|e| e.strip_suffix(".ems").unwrap_or(e).to_owned()).collect();
        }
        if let Some(b) = &def.crossfade_bank {
            self.crossfade_bank = Some(b.clone());
        }
        if let Some(b) = &def.fallback_bed {
            self.fallback_bed = Some(b.clone());
        }
        for r in &def.emitters {
            let mut r = r.clone();
            r.index = self.emitters.len() as u32;
            self.emitters.push(r);
        }
        for (layer, boxes) in &def.regions {
            for b in boxes {
                let key = match u64::from_str_radix(&b.key, 16).ok().filter(|_| b.key.len() == 16) {
                    Some(k) => Some(k),
                    None => match layer.as_str() {
                        "audio_ambience" => library.zone_named(&b.key),
                        "audio_emitters" => library.random_set_named(&b.key).map(|(k, _)| k),
                        _ => None,
                    },
                };
                let Some(key) = key else {
                    warn!("Map audio ({source}): region key {:?} on layer {layer} is not a key or a known name; skipped", b.key);
                    continue;
                };
                self.regions.entry(layer.clone()).or_default().push(RegionTile {
                    r#box: b.r#box,
                    nodes: vec![[super::library::NO_KEY, 0, 0, 0, 0]],
                    keys: vec![format!("{key:016X}")],
                });
            }
        }
        self.sources.push(source);
    }
}

/// The retail map table (stem → `.ems` files and crossfade bank) from the stock collections,
/// read once.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct RetailMap {
    pub ems: Vec<String>,
    pub crossfade_bank: Option<String>,
}

const MAP_CLASS: &str = "Hash_F4917ACACAFAF913";
const EMS_FIELD: &str = "Hash_65FA976EF23A314E";
const CROSSFADE_FIELD: &str = "Hash_33526BC9D1C36B4A";
const WORLD_AUDIO_REF: &str = "Hash_99D6E51C9E20A663";

/// Build the table from the collections: each `world` row with a `WorldStream` of `DIST_<stem>`
/// (parks have several rows: the one named after the stream wins, as `map_starts.py`), its map
/// database reference, that entry's `.ems` list and crossfade bank (`data/audio/<stem>.abk`).
pub(crate) fn retail_table(c: &skate_data::collections::Collections) -> HashMap<String, RetailMap> {
    let mut rows: HashMap<String, (bool, String)> = HashMap::new();
    for row in c.entries().iter().filter(|e| e.class_name == "world") {
        let Ok(stream) = c.field("world", &row.key, "WorldStream") else { continue };
        let stem = crate::park_layout::stream_map(&stream.data);
        if stem.len() == stream.data.len() {
            continue;
        }
        let own = row.key.eq_ignore_ascii_case(&format!("dist_{stem}"));
        let key = stem.to_ascii_lowercase();
        if rows.get(&key).is_none_or(|(was_own, _)| own && !was_own) {
            rows.insert(key, (own, row.key.clone()));
        }
    }
    let mut out = HashMap::new();
    for (stem, (_, row)) in rows {
        let Ok(reference) = c.field("world", &row, WORLD_AUDIO_REF) else { continue };
        let Some(entry) = reference.data.get(16..32) else { continue };
        let entry = format!("Hash_{entry}");
        let ems = c.field(MAP_CLASS, &entry, EMS_FIELD).ok()
            .and_then(|f| f.array.as_ref())
            .map(|a| a.text_items.iter().map(|t| t.strip_suffix(".ems").unwrap_or(t).to_owned()).collect())
            .unwrap_or_default();
        let crossfade_bank = c.field(MAP_CLASS, &entry, CROSSFADE_FIELD).ok()
            .map(|f| f.data.rsplit('/').next().unwrap_or("").trim_end_matches(".abk").to_owned())
            .filter(|s| !s.is_empty());
        out.insert(stem, RetailMap { ems, crossfade_bank });
    }
    out
}

/// The retail table for this install, loaded on first use (None: no stock collections).
fn retail(asset_root: &Path) -> Option<&'static HashMap<String, RetailMap>> {
    static TABLE: OnceLock<Option<HashMap<String, RetailMap>>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let t = std::time::Instant::now();
        match skate_data::collections::Collections::load(asset_root) {
            Ok(c) => {
                let table = retail_table(&c);
                info!("Map audio: {} retail maps from the stock collections ({:.0} ms)", table.len(), t.elapsed().as_secs_f64() * 1000.0);
                Some(table)
            }
            Err(e) => {
                warn!("Map audio: no stock collections ({e}): retail maps get no emitter files or crossfades");
                None
            }
        }
    }).as_ref()
}

/// A map's own definition: the `.skate` `AUDO` extension, else the sidecar.
fn own_definition(map: &crate::map_transition::CurrentMap) -> Option<(MapJson, &'static str)> {
    own_definition_at(map.audio_tag.as_deref(), map.path.as_deref())
}

fn own_definition_at(tag: Option<&[u8]>, map_path: Option<&Path>) -> Option<(MapJson, &'static str)> {
    let (bytes, source, at) = if let Some(tag) = tag {
        (tag.to_vec(), "tag", "AUDO extension".to_owned())
    } else {
        let path = map_path?.with_extension("audio.json");
        let bytes = std::fs::read(&path).ok()?;
        (bytes, "sidecar", path.display().to_string())
    };
    let def = match skate_mods::audio_content::MapAudioDef::parse(&bytes, &at) {
        Ok(d) => d,
        Err(e) => {
            warn!("Map audio: {e}");
            return None;
        }
    };
    match to_map_json(&def) {
        Ok(j) => Some((j, source)),
        Err(e) => {
            warn!("Map audio: {at}: {e}");
            None
        }
    }
}

/// A checked definition as the library's map shape (unset optional fields left out).
pub(crate) fn to_map_json(def: &skate_mods::audio_content::MapAudioDef) -> serde_json::Result<MapJson> {
    fn strip(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(m) => {
                m.retain(|_, x| !x.is_null());
                m.values_mut().for_each(strip);
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut v = serde_json::to_value(def)?;
    strip(&mut v);
    serde_json::from_value(v)
}

/// Assemble a map's audio (see the module docs). `retail`: the table (tests pass their own).
pub(crate) fn build(stem: &str, own: Option<(MapJson, &'static str)>, library: &Library, retail: Option<&HashMap<String, RetailMap>>) -> MapAudio {
    let mut a = MapAudio { stem: stem.to_owned(), district: stem.to_owned(), ..Default::default() };
    if let Some(r) = retail.and_then(|t| t.get(&stem.to_ascii_lowercase())) {
        a.ems = r.ems.clone();
        a.crossfade_bank = r.crossfade_bank.clone();
        a.sources.push("retail");
    }
    if let Some((def, source)) = own {
        a.apply(&def, library, source);
    }
    if let Some(def) = library.mod_map(stem) {
        let def = def.clone();
        a.apply(&def, library, "mod");
    }
    if a.sources.is_empty() {
        a.sources.push("none");
    }
    a
}

/// Per frame, first in the audio pass after `content::frame`: rebuild when the map or the audio
/// content changed.
pub(super) fn update(
    map: Res<crate::map_transition::CurrentMap>,
    library: Option<Res<Library>>,
    content: Res<super::AudioContent>,
    config: Res<crate::config::Config>,
    mut audio: ResMut<MapAudio>,
) {
    let _span = super::hitch_parts::span(super::hitch_parts::MAP_AUDIO);
    let Some(library) = library else { return };
    let key = (map.path.clone(), map.generation, content.generation);
    if audio.key.as_ref() == Some(&key) {
        return;
    }
    let stem = map.path.as_deref().and_then(|p| p.file_stem()).and_then(|s| s.to_str()).unwrap_or("").to_owned();
    let retail = if stem.is_empty() { None } else { retail(&config.asset_root) };
    let mut built = build(&stem, own_definition(&map), &library, retail);
    built.key = Some(key);
    if !stem.is_empty() {
        info!("Map audio {stem}: {:?}, {} .ems files, {} extra records, district {}, crossfade {:?}", built.sources, built.ems.len(), built.emitters.len(), built.district, built.crossfade_bank);
    }
    *audio = built;
}

/// While a map loads: its zone beds (the files `ambience::update` keeps resident) are read on the audio's read-ahead
/// worker, so the first bed is not read on the game thread in the map's first frame (161 ms, desktop session
/// 2026-10-23). Only when the bytes arrive changes. The `.skate` AUDO tag is known only after the load, so a map that
/// names another district there reads the sidecar's guess and its own beds as before.
#[allow(clippy::too_many_arguments)]
pub(super) fn read_ahead_next_map(
    transition: Option<Res<crate::map_transition::MapTransition>>,
    library: Option<ResMut<Library>>,
    native: Option<ResMut<super::native::Native>>,
    owners: Option<Res<super::world_sources::WorldOwners>>,
    config: Res<crate::config::Config>,
    mut requested: Local<Option<Option<std::path::PathBuf>>>,
    // The next map's audio until its sound banks are queued (that waits for its spawn point).
    mut banks: Local<Option<(std::path::PathBuf, MapAudio)>>,
) {
    let Some(entry) = transition.as_deref().and_then(|t| t.loading()) else {
        *requested = None;
        *banks = None;
        // A load that ended without its map change (failed) leaves no banks queued; a committed one handed them over.
        if let Some(mut native) = native.filter(|_| transition.as_deref().is_none_or(|t| !t.busy())) {
            native.prefetch.drop_next();
        }
        return;
    };
    let Some(mut library) = library else { return };
    if requested.as_ref() != Some(&entry.path) {
        *requested = Some(entry.path.clone());
        *banks = None;
        let stem = entry.path.as_deref().and_then(|p| p.file_stem()).and_then(|s| s.to_str()).unwrap_or("").to_owned();
        if stem.is_empty() {
            return;
        }
        let next = build(&stem, own_definition_at(None, entry.path.as_deref()), &library, retail(&config.asset_root));
        let files = library.zone_bed_files(next.region_tile_keys(&library, "audio_ambience"));
        library.warm_files(files.iter());
        info!("AUDIO_AMBIENCE beds read ahead for the next map {stem}: {}", files.len());
        *banks = entry.path.clone().map(|path| (path, next));
    }
    let (Some(spawn), Some(mut native)) = (transition.as_deref().and_then(|t| t.loading_spawn()), native) else { return };
    let Some((path, next)) = banks.take() else { return };
    let emitters = super::emitters::read_ahead_banks(next.records(&library).map(|(_, r)| r), &library, &mut native, &path, Vec3::from(spawn));
    let world = if owners.is_some_and(|o| o.expected) { super::world_sources::read_ahead_banks(&mut native, &library, &path) } else { 0 };
    info!("AUDIO_BANKS read ahead for the next map {}: {emitters} emitter banks near the spawn, {world} world banks", path.display());
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The tables the retail lookup replaced (2026-10-04): kept as the oracle.
    pub(crate) fn old_ems_files(map_stem: &str) -> &'static [&'static str] {
        match map_stem {
            "University" => &["music_university", "sfx_university", "reverb_university", "speakers_university", "crowds_university"],
            "DownTown" => &["music_downtown", "sfx_downtown", "reverb_downtown", "speakers_downtown", "crowds_downtown"],
            "Industrial" => &["music_industrial", "sfx_industrial", "reverb_industrial", "speakers_industrial", "crowds_industrial"],
            "DownTownSkatePark" => &["sfx_dt_skatepark"],
            "IndustrialSkatePark" => &["sfx_ind_skatepark"],
            "MegaPark" => &["sfx_mega_skatepark"],
            "MaloofMoneyCup" => &["sfx_maloof_money_cup"],
            "StartPark" => &["sfx_startpark"],
            "BlackBoxPark" => &["sfx_blackbox_park"],
            "SkateSchool" => &["skateschool"],
            _ => &[],
        }
    }

    pub(crate) fn old_crossfade_bank(district: &str) -> Option<&'static str> {
        match district {
            "DownTown" => Some("Main_Ambience_Crossfade_DT"),
            "Industrial" => Some("Main_Ambience_Crossfade_Ind"),
            "University" => Some("Main_Ambience_Crossfade_Uni"),
            _ => None,
        }
    }

    pub(crate) const MAPS: [&str; 10] = ["University", "DownTown", "Industrial", "DownTownSkatePark", "IndustrialSkatePark", "MegaPark", "MaloofMoneyCup", "StartPark", "BlackBoxPark", "SkateSchool"];

    /// The retail lookup from the stock collections (data-gated: the converted collections of
    /// the user's own disc) gives exactly the old code tables for all ten maps: the same `.ems`
    /// files in the same order (so the same records, the same reverb-zone ids) and the same
    /// crossfade banks.
    #[test]
    #[ignore = "needs the private install data"]
    fn retail_maps_reproduce_the_old_tables() {
        let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
        let Ok(c) = skate_data::collections::Collections::load(root) else { panic!("missing private data: no stock collections") };
        let table = retail_table(&c);
        for stem in MAPS {
            let got = table.get(&stem.to_ascii_lowercase()).unwrap_or_else(|| panic!("{stem}: no retail entry"));
            assert_eq!(got.ems, old_ems_files(stem), "{stem}: .ems files");
            assert_eq!(got.crossfade_bank.as_deref(), old_crossfade_bank(stem), "{stem}: crossfade bank");
        }
    }

    pub(crate) fn json(v: serde_json::Value) -> MapJson {
        let def: skate_mods::audio_content::MapAudioDef = serde_json::from_value(v).unwrap();
        def.check("test").unwrap();
        to_map_json(&def).unwrap()
    }

    /// The sources stack: retail, then the map's own definition, then a mod; box regions come
    /// before the district's tiles and resolve zone / set names.
    #[test]
    fn definitions_override_fields_and_append_records() {
        let (dir, _) = super::super::library::tests::content_fixture("map-audio");
        let library = Library::load(&dir).unwrap();
        let mut table = HashMap::new();
        table.insert("downtown".to_owned(), RetailMap { ems: vec!["sfx_downtown".into(), "reverb_downtown".into()], crossfade_bank: Some("Main_Ambience_Crossfade_DT".into()) });
        let retail = build("DownTown", None, &library, Some(&table));
        assert_eq!((retail.ems.len(), retail.crossfade_bank.as_deref(), retail.district.as_str()), (2, Some("Main_Ambience_Crossfade_DT"), "DownTown"));
        assert_eq!(retail.sources, ["retail"]);
        let custom = build("MyMap", Some((json(serde_json::json!({
            "district": "DownTown", "ems": ["sfx_downtown.ems"],
            "emitters": [{"position": [0, 0, 0], "extent": [4, 4, 4], "bank": "T"}, {"position": [9, 0, 9], "extent": [4, 4, 4], "kind": 5, "reverb": "BEEFC8E3DE04FBAE"}],
            "regions": {"audio_ambience": [{"box": [0, 0, 10, 10], "key": "plaza"}], "audio_reverb": [{"box": [0, 0, 10, 10], "key": "BEEFC8E3DE04FBAE"}, {"box": [0, 0, 1, 1], "key": "unknown_name"}]}
        })), "sidecar")), &library, Some(&table));
        assert_eq!(custom.ems, ["sfx_downtown"]);
        assert_eq!(custom.district, "DownTown");
        assert_eq!((custom.emitters.len(), custom.emitters[1].index, custom.emitters[1].kind), (2, 1, 5));
        assert_eq!(custom.region_key(&library, "audio_ambience", 5.0, 5.0), Some(0xAB), "the zone by name");
        assert_eq!(custom.region_key(&library, "audio_ambience", 50.0, 5.0), None);
        assert_eq!(custom.region_key(&library, "audio_reverb", 0.0, 0.0), Some(0xBEEF_C8E3_DE04_FBAE));
        assert_eq!(custom.sources, ["sidecar"]);
        let records: Vec<_> = custom.records(&library).map(|(f, r)| (f, r.index)).collect();
        assert_eq!(records, [(1, 0), (1, 1)], "the extra records are one more file after the .ems files");
        let none = build("Nowhere", None, &library, Some(&table));
        assert!(none.ems.is_empty() && none.sources == ["none"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A custom map's own definition: a `<map>.audio.json` sidecar next to the `.skate`, or its
    /// `AUDO` extension, which wins over the sidecar. A broken one is skipped (logged).
    #[test]
    fn a_custom_map_reads_its_sidecar_or_its_tag() {
        let dir = std::env::temp_dir().join(format!("skate-map-audio-sidecar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut map = crate::map_transition::CurrentMap { path: Some(dir.join("MyMap.skate")), name: "My map".into(), spawn: [0.0; 3], heading: 0.0, generation: 1, audio_tag: None, layout: None };
        assert!(own_definition(&map).is_none(), "no sidecar, no tag");
        std::fs::write(dir.join("MyMap.audio.json"), r#"{"district": "DownTown", "emitters": [{"position": [0, 0, 0], "extent": [3, 3, 3], "kind": 5, "reverb": "BEEFC8E3DE04FBAE"}]}"#).unwrap();
        let (def, source) = own_definition(&map).unwrap();
        assert_eq!((source, def.district.as_deref(), def.emitters.len()), ("sidecar", Some("DownTown"), 1));
        map.audio_tag = Some(std::sync::Arc::from(&br#"{"district": "University"}"#[..]));
        let (def, source) = own_definition(&map).unwrap();
        assert_eq!((source, def.district.as_deref()), ("tag", Some("University")));
        map.audio_tag = Some(std::sync::Arc::from(&br#"{"district": 5}"#[..]));
        assert!(own_definition(&map).is_none(), "a broken tag is skipped");
        // The sidecar's reverb zone reaches the zone list.
        map.audio_tag = None;
        let (fixture, _) = super::super::library::tests::content_fixture("sidecar-zones");
        let library = Library::load(&fixture).unwrap();
        let built = build("MyMap", own_definition(&map), &library, None);
        let zones: Vec<_> = built.records(&library).filter(|(_, r)| r.kind == 5).collect();
        assert_eq!(zones.len(), 1);
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(fixture);
    }

    /// A mod's `maps.<stem>` applies over retail and the map's own definition.
    #[test]
    fn a_mod_map_section_applies_last() {
        let (dir, mods) = super::super::library::tests::content_fixture("map-audio-mod");
        let o: skate_mods::audio_content::AudioOverlay = serde_json::from_value(serde_json::json!({"version": 1, "maps": {"MyMap": {
            "crossfade_bank": "MOD_fade", "emitters": [{"position": [1, 1, 1], "extent": [2, 2, 2], "bank": "T"}]}}})).unwrap();
        o.validate().unwrap();
        let (library, _) = Library::load_with(&dir, &[super::super::library::OverlaySource { id: "dev.a", root: &mods, overlay: &o }]).unwrap();
        let built = build("MyMap", Some((json(serde_json::json!({"crossfade_bank": "Main_Ambience_Crossfade_DT", "emitters": [{"position": [0, 0, 0], "extent": [4, 4, 4], "bank": "T"}]})), "tag")), &library, None);
        assert_eq!(built.crossfade_bank.as_deref(), Some("MOD_fade"));
        assert_eq!(built.emitters.len(), 2);
        assert_eq!(built.sources, ["tag", "mod"]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
