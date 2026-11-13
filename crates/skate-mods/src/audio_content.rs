//! Audio content overlays (capability `audio_content` = 1): a mod's `audio.json` replaces or adds
//! retail audio content by identity (bank, sample slot, Splice tree, grain member, wheel stream,
//! ambience bed, emitter record, location set, zone, crossfade, speech take, tuning field, map
//! audio) with files the mod ships. The engine merges overlays over the install's audio manifest,
//! mod first, then the install; with no overlay nothing changes.
//!
//! Everything here is checked before the engine sees it: the schema (`deny_unknown_fields`),
//! counts, numbers, key and path syntax, and every referenced file (PCM16 WAVs, `.abk` banks,
//! `.splc` trees, `.mxb` MixMaps, `.grain` members) is read inside the mod root and parsed.
//! Identities (does the bank / slot / set exist? does the speech index have the clip and take?)
//! need the install and are checked by the merge (`audio_merge`, in the engine and `check_mod
//! --install`); an unknown identity is a warning there, never an error.
use crate::archive::read_bounded;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

/// The overlay file at a mod's root.
pub const FILE: &str = "audio.json";
/// The overlay schema this build reads.
pub const VERSION: u32 = 1;
/// `audio.json` itself.
pub const MAX_JSON_BYTES: u64 = 1024 * 1024;
/// Files one overlay may reference (all kinds together).
pub const MAX_FILES: usize = 1024;
/// Sample slot replacements per overlay (all banks together).
pub const MAX_SAMPLE_REPLACEMENTS: usize = 512;
/// Banks one overlay may replace or add.
pub const MAX_BANKS: usize = 64;
/// Csis projects one overlay may add (doc 16 L4).
pub const MAX_PROJECTS: usize = 8;
/// WAVs per replaced / added bank.
pub const MAX_BANK_SAMPLES: usize = 512;
/// Records (emitters, sets, zones, crossfades, programs, regions) per overlay, all kinds together.
pub const MAX_RECORDS: usize = 4096;
/// Total PCM16 bytes of one overlay's WAVs, and of all running overlays together (the engine).
pub const MAX_PCM_BYTES_PER_MOD: u64 = 64 * 1024 * 1024;
pub const MAX_PCM_BYTES_TOTAL: u64 = 256 * 1024 * 1024;
/// A sample / speech take WAV (bytes, seconds) and an ambience bed WAV.
pub const SAMPLE_LIMITS: (u64, f64) = (crate::audio::MAX_WAV_BYTES, 30.0);
pub const BED_LIMITS: (u64, f64) = (48 * 1024 * 1024, 600.0);
/// The wheel spin recordings are ~15 s streams.
pub const STREAM_LIMITS: (u64, f64) = (16 * 1024 * 1024, 60.0);
/// Binary content files (`.abk`, `.splc`, `.mxb`, `.grain`).
pub const MAX_BINARY_BYTES: u64 = 16 * 1024 * 1024;
/// Payload words of a [`crate::Command::AudioPost`] and handles per mod (the engine also caps them).
pub const MAX_POST_WORDS: usize = 32;

fn one() -> f32 { 1.0 }
fn ten() -> f32 { 10.0 }
fn one_i() -> i32 { 1 }
fn kind_emitter() -> i32 { 1 }
fn forward() -> [f32; 4] { [0.0, 1.0, 0.0, 0.0] }
fn default_min_level() -> f32 { 0.2 }
fn default_min_interval() -> f32 { 10.0 }
fn default_max_interval() -> f32 { 20.0 }

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioOverlay {
    pub version: u32,
    #[serde(default)]
    pub replace: Replace,
    #[serde(default)]
    pub add: Add,
    #[serde(default)]
    pub tuning: Tuning,
    /// Map stem → that map's audio (custom maps; also a retail map's extras).
    #[serde(default)]
    pub maps: BTreeMap<String, MapAudioDef>,
    /// Mute / replace / layer rules on the event sites (`audio_rules`; capability `audio_content`
    /// = 2): applied while the mod runs, without an audio restart.
    #[serde(default)]
    pub rules: BTreeMap<String, crate::audio_rules::Rule>,
}

/// A WAV: a path, or a path with the loop start (frames) for a slot that loops.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SampleFile {
    Path(String),
    Detailed(SampleDetail),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleDetail {
    pub file: String,
    #[serde(default)]
    pub loop_start: Option<u32>,
}

impl SampleFile {
    pub fn file(&self) -> &str {
        match self {
            Self::Path(p) => p,
            Self::Detailed(d) => &d.file,
        }
    }
    pub fn loop_start(&self) -> Option<u32> {
        match self {
            Self::Path(_) => None,
            Self::Detailed(d) => d.loop_start,
        }
    }
}

/// The volume group a mod bank's voices play in (the menu's Effects or Ambience volume).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Group {
    Player,
    World,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BankDef {
    /// The AEMS module bank (patch programs); without it a replaced bank keeps the install's.
    #[serde(default)]
    pub abk: Option<String>,
    /// The bank's WAVs by S10A slot.
    #[serde(default)]
    pub samples: Vec<SampleFile>,
    /// Load at audio start and keep across map changes (banks bound to always-present classes).
    #[serde(default)]
    pub preload: bool,
    #[serde(default)]
    pub group: Option<Group>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrainDef {
    /// The whole recording (WAV) and its `.grain` member (seek table, durations).
    pub file: String,
    pub grain: String,
}

/// An `.ems` record (the manifest's emitter row): a sound emitter (`kind` 1) or a reverb zone
/// (`kind` 5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmitterDef {
    #[serde(default)]
    pub flags: u32,
    pub position: [f32; 3],
    pub extent: [f32; 3],
    /// Inner core, then the forward axis.
    #[serde(default = "forward")]
    pub scalars: [f32; 4],
    #[serde(default = "kind_emitter")]
    pub kind: i32,
    #[serde(default = "one")]
    pub volume: f32,
    /// 0 = (1 − d)², 1 = 1 − d, 2 = flat.
    #[serde(default)]
    pub falloff: i32,
    #[serde(default)]
    pub bank: Option<String>,
    #[serde(default)]
    pub patch: i32,
    /// The reverb preset key (16 hex digits) of a reverb zone.
    #[serde(default)]
    pub reverb: Option<String>,
    #[serde(default)]
    pub sound_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SoundDef {
    pub bank: String,
    #[serde(default = "one")]
    pub volume: f32,
    /// The post's timeout (seconds).
    #[serde(default = "ten")]
    pub seconds: f32,
    #[serde(default = "one_i")]
    pub weight: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RandomSetDef {
    #[serde(default)]
    pub name: Option<String>,
    pub sounds: Vec<SoundDef>,
    #[serde(default = "default_min_level")]
    pub min_level: f32,
    #[serde(default = "one")]
    pub max_level: f32,
    #[serde(default = "default_min_interval")]
    pub min_interval: f32,
    #[serde(default = "default_max_interval")]
    pub max_interval: f32,
    /// The level grid: a level is drawn as a whole step of 1 / `level_steps` between the two levels (retail 10).
    #[serde(default = "default_level_steps")]
    pub level_steps: u32,
}
fn default_level_steps() -> u32 { 10 }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZoneDef {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub bed: Option<String>,
    #[serde(default = "one")]
    pub volume: f32,
    /// Fade-out when leaving, fade-in when entering (seconds).
    #[serde(default = "one")]
    pub time_a: f32,
    #[serde(default = "one")]
    pub time_b: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrossfadeDef {
    pub from: String,
    pub to: String,
    pub group: u32,
    #[serde(default = "one")]
    pub level: f32,
}

/// A layer of a location-set post as the interim Bevy-voice player plays it (until the location
/// sets run on the native evaluator): `sample` = a slot or `"shuffle"`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerDef {
    #[serde(default)]
    pub delay: f32,
    pub sample: LayerSample,
    #[serde(default = "one")]
    pub level: f32,
    #[serde(default)]
    pub pan_sweep: f32,
    #[serde(default)]
    pub looping: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LayerSample {
    Slot(u32),
    Named(String),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Replace {
    /// Bank stem → S10A slot (decimal) → WAV.
    #[serde(default)]
    pub samples: BTreeMap<String, BTreeMap<String, SampleFile>>,
    #[serde(default)]
    pub banks: BTreeMap<String, BankDef>,
    /// Bank stem → `.splc` patch tree (expert).
    #[serde(default)]
    pub splice: BTreeMap<String, String>,
    #[serde(default)]
    pub grains: BTreeMap<String, GrainDef>,
    #[serde(default)]
    pub wheels: BTreeMap<String, String>,
    #[serde(default)]
    pub ambience: BTreeMap<String, String>,
    /// The whole MixMap (expert).
    #[serde(default)]
    pub mixmap: Option<String>,
    /// `.ems` file stem → record index (decimal) → record.
    #[serde(default)]
    pub emitters: BTreeMap<String, BTreeMap<String, EmitterDef>>,
    /// Location set by key (16 hex digits) or name.
    #[serde(default)]
    pub random_sets: BTreeMap<String, RandomSetDef>,
    /// Zone ambience by key (16 hex digits) or name.
    #[serde(default)]
    pub zones: BTreeMap<String, ZoneDef>,
    /// By zone pair (either order).
    #[serde(default)]
    pub crossfades: Vec<CrossfadeDef>,
    /// Speech archive → clip name → take (decimal) → WAV.
    #[serde(default)]
    pub speech: BTreeMap<String, BTreeMap<String, BTreeMap<String, String>>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Add {
    #[serde(default)]
    pub banks: BTreeMap<String, BankDef>,
    /// Csis projects (`.csi`: new classes, functions, globals; doc 16 L4), installed after the
    /// install's in mod-id order. Their symbol names must not be the install's or an earlier mod's
    /// (checked when the overlays merge: the overlay is left out otherwise).
    #[serde(default)]
    pub projects: Vec<String>,
    /// `.ems` file stem → records appended (a new stem makes a new file a map can list).
    #[serde(default)]
    pub emitters: BTreeMap<String, Vec<EmitterDef>>,
    /// New location sets / zones by key (16 hex digits).
    #[serde(default)]
    pub random_sets: BTreeMap<String, RandomSetDef>,
    #[serde(default)]
    pub zones: BTreeMap<String, ZoneDef>,
    #[serde(default)]
    pub crossfades: Vec<CrossfadeDef>,
    /// Speech archive → clip name → extra takes (WAVs) after the clip's own.
    #[serde(default)]
    pub speech: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    /// Bank stem → the layers a location-set post of the bank plays (interim player).
    #[serde(default)]
    pub location_programs: BTreeMap<String, Vec<LayerDef>>,
    /// Crossfade bank stem → group (decimal 1..64) → the voices the group plays. For a bank
    /// without a `c_main_ambience_crossfade` program (a mod bank of WAVs): banks with one get
    /// their layout from the program, as retail; a declared layout wins over the program.
    #[serde(default)]
    pub crossfade_layouts: BTreeMap<String, BTreeMap<String, Vec<CrossfadeVoiceDef>>>,
}

/// One looping voice of a crossfade group: a sample slot of the crossfade bank, its direction
/// around the listener (degrees, 0 = ahead, 90 = right) and its level (1 = full).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrossfadeVoiceDef {
    pub sample: u32,
    #[serde(default)]
    pub pan: f32,
    #[serde(default = "one")]
    pub level: f32,
}

/// Voices per declared crossfade group (retail's groups have four).
pub const MAX_CROSSFADE_VOICES: usize = 8;
/// The highest group a crossfade pair or a declared layout may name.
pub const MAX_CROSSFADE_GROUP: u32 = 64;

/// Field merges onto the install's tuning sections (only fields the install has; arrays by index
/// as decimal keys). The engine checks every field against the install.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tuning {
    #[serde(default)]
    pub player: Option<Value>,
    #[serde(default)]
    pub world: Option<Value>,
    #[serde(default)]
    pub bus: Option<Value>,
    #[serde(default)]
    pub grain: Option<Value>,
}

/// A map's audio: which `.ems` files (retail names), extra emitter / reverb-zone records, box
/// regions for the zone ambience (`audio_ambience`), location sets (`audio_emitters`) and reverb
/// (`audio_reverb`), the crossfade bank, and whose retail regions it uses (`district`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapAudioDef {
    #[serde(default)]
    pub district: Option<String>,
    #[serde(default)]
    pub ems: Option<Vec<String>>,
    #[serde(default)]
    pub crossfade_bank: Option<String>,
    /// A bed played when the map has no zone data (not retail).
    #[serde(default)]
    pub fallback_bed: Option<String>,
    #[serde(default)]
    pub emitters: Vec<EmitterDef>,
    #[serde(default)]
    pub regions: BTreeMap<String, Vec<BoxRegion>>,
}

/// An axis-aligned box on the ground plane: centre x, z and half sizes x, z; `key` = a zone /
/// location set / reverb preset key (16 hex digits) or a zone / set name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoxRegion {
    pub r#box: [f32; 4],
    pub key: String,
}

/// The region layers a map can carry.
pub const REGION_LAYERS: [&str; 3] = ["audio_ambience", "audio_emitters", "audio_reverb"];

/// A 16-hex-digit identity key.
pub fn hex_key(s: &str) -> bool {
    s.len() == 16 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A retail-style name (bank / clip / emitter file / set / zone / map stem).
pub fn valid_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b' '))
        && !s.starts_with('.') && !s.contains("..")
}

/// A mod-relative content path with one of `exts` (lower case, without the dot).
pub fn valid_content_path(path: &str, exts: &[&str]) -> bool {
    let lower = path.to_ascii_lowercase();
    !path.is_empty() && path.len() <= 256
        && exts.iter().any(|e| lower.ends_with(&format!(".{e}")))
        && !path.chars().any(|c| matches!(c, '\\' | ':' | '#' | '?') || c.is_control())
        && path.split('/').all(|s| !s.is_empty() && s != "." && s != "..")
}

/// The speech archives the engine plays (`livingworld` = peds and NPC skaters, `maincast` = the
/// pros and the special cast).
pub const SPEECH_ARCHIVES: [&str; 2] = ["livingworld", "maincast"];

/// A speech clip name as the archives name them (`<event>_<voice>[_<voice name>]_<line>`, with
/// or without `.dat`): the speech index's own parser reads it.
pub fn speech_clip_name(clip: &str) -> bool {
    skate_audio::world::speech::parse_name(clip).is_some_and(|(_, _, _, line)| !line.is_empty())
}

/// The clips of the install's speech archives (archive → clip name without `.dat` → take count),
/// read from each archive's index (`speech.<archive>.index` in the manifest). The merge checks
/// speech takes against it (`audio_merge::merge_one_with`).
#[derive(Clone, Debug, Default)]
pub struct SpeechClips {
    pub archives: BTreeMap<String, BTreeMap<String, usize>>,
}

impl SpeechClips {
    /// Read the indexes the manifest names under `audio_root` (the install's `private/audio`).
    /// Archives whose index is missing or unreadable are left out (their takes then warn).
    pub fn load(audio_root: &Path, manifest: &Value) -> Self {
        #[derive(Deserialize)]
        struct Index {
            clips: Vec<IndexClip>,
        }
        #[derive(Deserialize)]
        struct IndexClip {
            name: String,
            #[serde(default)]
            takes: Vec<serde::de::IgnoredAny>,
        }
        let mut archives = BTreeMap::new();
        for (archive, entry) in manifest.get("speech").and_then(Value::as_object).into_iter().flatten() {
            let Some(index) = entry.get("index").and_then(Value::as_str) else { continue };
            if !valid_content_path(index, &["json"]) {
                continue;
            }
            let Ok(bytes) = std::fs::read(audio_root.join(index)) else { continue };
            let Ok(index) = serde_json::from_slice::<Index>(&bytes) else { continue };
            let clips = index.clips.into_iter().map(|c| (c.name.strip_suffix(".dat").unwrap_or(&c.name).to_owned(), c.takes.len())).collect();
            archives.insert(archive.clone(), clips);
        }
        Self { archives }
    }

    /// Whether an overlay names speech at all (only then is the index worth reading).
    pub fn needed(overlay: &AudioOverlay) -> bool {
        !overlay.replace.speech.is_empty() || !overlay.add.speech.is_empty()
    }

    /// The archive's clip → take count, if the install has the archive.
    pub fn archive(&self, archive: &str) -> Option<&BTreeMap<String, usize>> {
        self.archives.get(archive)
    }

    /// A close clip name to suggest for an unknown one: the same name in another case, else the
    /// first clip of the same event and voice.
    pub fn suggest(&self, archive: &str, clip: &str) -> Option<&str> {
        let clips = self.archive(archive)?;
        if let Some((name, _)) = clips.iter().find(|(c, _)| c.eq_ignore_ascii_case(clip)) {
            return Some(name);
        }
        let (event, voice, ..) = skate_audio::world::speech::parse_name(clip)?;
        clips.keys().find(|c| skate_audio::world::speech::parse_name(c).is_some_and(|(e, v, ..)| e == event && v == voice)).map(String::as_str)
    }
}

fn index(s: &str, max: u32) -> bool {
    !s.is_empty() && s.len() <= 6 && s.bytes().all(|b| b.is_ascii_digit()) && s.parse::<u32>().is_ok_and(|v| v <= max)
}

fn finite(v: &[f32]) -> bool {
    v.iter().all(|x| x.is_finite() && x.abs() <= 1.0e6)
}

/// What kind of file a path is, for its checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Sample,
    Bed,
    Stream,
    Speech,
    Abk,
    Splice,
    MixMap,
    Grain,
    /// A Csis project (doc 16 L4).
    Csi,
}

impl FileKind {
    fn exts(self) -> &'static [&'static str] {
        match self {
            Self::Sample | Self::Bed | Self::Stream | Self::Speech => &["wav"],
            Self::Abk => &["abk"],
            Self::Splice => &["splc"],
            Self::MixMap => &["mxb"],
            Self::Grain => &["grain"],
            Self::Csi => &["csi"],
        }
    }
}

impl EmitterDef {
    fn check(&self, at: &str) -> Result<(), String> {
        if !finite(&self.position) || !finite(&self.extent) || !finite(&self.scalars) || !self.volume.is_finite() {
            return Err(format!("{at}: numbers must be finite"));
        }
        if self.extent.iter().any(|e| *e <= 0.0) {
            return Err(format!("{at}: extents must be > 0"));
        }
        if !(0.0..=4.0).contains(&self.volume) || !(0..=2).contains(&self.falloff) || !(0..=500).contains(&self.patch) {
            return Err(format!("{at}: volume 0..4, falloff 0..2, patch 0..500"));
        }
        if !matches!(self.kind, 1 | 5) {
            return Err(format!("{at}: kind is 1 (sound emitter) or 5 (reverb zone)"));
        }
        if self.kind == 1 && !self.bank.as_deref().is_some_and(valid_name) {
            return Err(format!("{at}: a sound emitter needs its bank"));
        }
        if self.kind == 5 && !self.reverb.as_deref().is_some_and(hex_key) {
            return Err(format!("{at}: a reverb zone needs its reverb preset key (16 hex digits)"));
        }
        if self.bank.as_deref().is_some_and(|b| !valid_name(b)) || self.sound_id.as_deref().is_some_and(|k| !hex_key(k)) {
            return Err(format!("{at}: bad bank name or sound_id"));
        }
        Ok(())
    }
}

impl RandomSetDef {
    fn check(&self, at: &str) -> Result<(), String> {
        if self.sounds.len() > 64 || self.sounds.iter().any(|s| !valid_name(&s.bank) || !(0.0..=4.0).contains(&s.volume)
            || !(0.0..=600.0).contains(&s.seconds) || !(0..=10_000).contains(&s.weight)) {
            return Err(format!("{at}: up to 64 sounds with a bank, volume 0..4, seconds 0..600, weight 0..10000"));
        }
        if !finite(&[self.min_level, self.max_level, self.min_interval, self.max_interval])
            || !(0.0..=4.0).contains(&self.min_level) || self.max_level < self.min_level || self.max_level > 4.0
            || !(0.0..=3600.0).contains(&self.min_interval) || self.max_interval < self.min_interval || self.max_interval > 3600.0 {
            return Err(format!("{at}: levels 0..4 and intervals 0..3600 s, min ≤ max"));
        }
        if !(1..=1000).contains(&self.level_steps) {
            return Err(format!("{at}: level_steps 1..1000"));
        }
        if self.name.as_deref().is_some_and(|n| !valid_name(n)) {
            return Err(format!("{at}: bad name"));
        }
        Ok(())
    }
}

impl ZoneDef {
    fn check(&self, at: &str) -> Result<(), String> {
        if !finite(&[self.volume, self.time_a, self.time_b]) || !(0.0..=4.0).contains(&self.volume)
            || !(0.0..=60.0).contains(&self.time_a) || !(0.0..=60.0).contains(&self.time_b) {
            return Err(format!("{at}: volume 0..4, fades 0..60 s"));
        }
        if self.name.as_deref().is_some_and(|n| !valid_name(n)) || self.bed.as_deref().is_some_and(|n| !valid_name(n)) {
            return Err(format!("{at}: bad name or bed"));
        }
        Ok(())
    }
}

impl CrossfadeDef {
    fn check(&self, at: &str) -> Result<(), String> {
        if !hex_key(&self.from) || !hex_key(&self.to) || self.from == self.to || self.group > MAX_CROSSFADE_GROUP || !self.level.is_finite() || !(0.0..=4.0).contains(&self.level) {
            return Err(format!("{at}: two different zone keys (16 hex digits), group ≤ 64, level 0..4"));
        }
        Ok(())
    }
}

impl LayerDef {
    fn check(&self, at: &str) -> Result<(), String> {
        if !finite(&[self.delay, self.level, self.pan_sweep]) || !(0.0..=60.0).contains(&self.delay) || !(0.0..=4.0).contains(&self.level)
            || self.pan_sweep.abs() > 3600.0 {
            return Err(format!("{at}: delay 0..60 s, level 0..4, pan sweep ±3600 °/s"));
        }
        match &self.sample {
            LayerSample::Slot(s) if *s < MAX_BANK_SAMPLES as u32 => Ok(()),
            LayerSample::Named(n) if n == "shuffle" => Ok(()),
            _ => Err(format!("{at}: sample is a slot number or \"shuffle\"")),
        }
    }
}

impl MapAudioDef {
    /// The map section's own checks (also used for `<map>.audio.json` sidecars and `.skate` tags).
    pub fn check(&self, at: &str) -> Result<usize, String> {
        if self.district.as_deref().is_some_and(|d| !valid_name(d)) || self.crossfade_bank.as_deref().is_some_and(|d| !valid_name(d))
            || self.fallback_bed.as_deref().is_some_and(|d| !valid_name(d)) {
            return Err(format!("{at}: bad district / crossfade_bank / fallback_bed name"));
        }
        if let Some(ems) = &self.ems {
            if ems.len() > 16 || ems.iter().any(|e| !valid_name(e)) {
                return Err(format!("{at}: up to 16 .ems file names"));
            }
        }
        let mut records = self.emitters.len();
        for (i, e) in self.emitters.iter().enumerate() {
            e.check(&format!("{at}.emitters[{i}]"))?;
        }
        for (layer, boxes) in &self.regions {
            if !REGION_LAYERS.contains(&layer.as_str()) {
                return Err(format!("{at}.regions: unknown layer {layer:?} (audio_ambience, audio_emitters, audio_reverb)"));
            }
            records += boxes.len();
            for (i, b) in boxes.iter().enumerate() {
                if !finite(&b.r#box) || b.r#box[2] <= 0.0 || b.r#box[3] <= 0.0 || !(hex_key(&b.key) || valid_name(&b.key)) {
                    return Err(format!("{at}.regions.{layer}[{i}]: box = [x, z, half x, half z] (half sizes > 0) and a key or name"));
                }
            }
        }
        Ok(records)
    }

    /// Parse and check a map's own audio definition (sidecar / `.skate` tag).
    pub fn parse(bytes: &[u8], at: &str) -> Result<Self, String> {
        let def: Self = serde_json::from_slice(bytes).map_err(|e| format!("{at}: {e}"))?;
        let records = def.check(at)?;
        if records > MAX_RECORDS {
            return Err(format!("{at}: more than {MAX_RECORDS} records"));
        }
        Ok(def)
    }
}

fn object(v: &Option<Value>, at: &str) -> Result<(), String> {
    match v {
        None => Ok(()),
        Some(Value::Object(_)) => {
            fn depth(v: &Value, d: usize) -> bool {
                d <= 8 && match v {
                    Value::Object(m) => m.len() <= 4096 && m.values().all(|x| depth(x, d + 1)),
                    Value::Array(a) => a.len() <= 4096 && a.iter().all(|x| depth(x, d + 1)),
                    Value::Number(n) => n.as_f64().is_some_and(f64::is_finite),
                    _ => true,
                }
            }
            if depth(v.as_ref().unwrap(), 0) { Ok(()) } else { Err(format!("{at}: tuning nests at most 8 deep with finite numbers")) }
        }
        Some(_) => Err(format!("{at}: tuning sections are objects of fields")),
    }
}

impl AudioOverlay {
    /// The file list (path, kind) in a stable order: what [`load`] reads and checks.
    pub fn files(&self) -> Vec<(&str, FileKind)> {
        let mut out = Vec::new();
        for slots in self.replace.samples.values() {
            out.extend(slots.values().map(|s| (s.file(), FileKind::Sample)));
        }
        for b in self.replace.banks.values().chain(self.add.banks.values()) {
            out.extend(b.abk.as_deref().map(|a| (a, FileKind::Abk)));
            out.extend(b.samples.iter().map(|s| (s.file(), FileKind::Sample)));
        }
        out.extend(self.replace.splice.values().map(|f| (f.as_str(), FileKind::Splice)));
        for g in self.replace.grains.values() {
            out.push((g.file.as_str(), FileKind::Stream));
            out.push((g.grain.as_str(), FileKind::Grain));
        }
        out.extend(self.replace.wheels.values().map(|f| (f.as_str(), FileKind::Stream)));
        out.extend(self.replace.ambience.values().map(|f| (f.as_str(), FileKind::Bed)));
        out.extend(self.replace.mixmap.as_deref().map(|f| (f, FileKind::MixMap)));
        for clips in self.replace.speech.values() {
            for takes in clips.values() {
                out.extend(takes.values().map(|f| (f.as_str(), FileKind::Speech)));
            }
        }
        for clips in self.add.speech.values() {
            for takes in clips.values() {
                out.extend(takes.iter().map(|f| (f.as_str(), FileKind::Speech)));
            }
        }
        out.extend(self.rules.values().filter_map(|r| r.play.as_ref()).map(|p| (p.path.as_str(), FileKind::Sample)));
        out.extend(self.add.projects.iter().map(|f| (f.as_str(), FileKind::Csi)));
        out
    }

    /// Whether the overlay changes content (anything but `rules`): only such an overlay restarts
    /// the game's sound when it comes or goes.
    pub fn has_content(&self) -> bool {
        let content = AudioOverlay { version: self.version, rules: BTreeMap::new(), ..self.clone() };
        content != AudioOverlay { version: self.version, ..Default::default() }
    }

    /// Schema-level checks: version, counts, numbers, key and path syntax.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != VERSION {
            return Err(format!("audio.json version {} (this build reads {VERSION})", self.version));
        }
        let r = &self.replace;
        let a = &self.add;
        let samples: usize = r.samples.values().map(BTreeMap::len).sum();
        if samples > MAX_SAMPLE_REPLACEMENTS {
            return Err(format!("more than {MAX_SAMPLE_REPLACEMENTS} sample replacements"));
        }
        if r.banks.len() + a.banks.len() > MAX_BANKS {
            return Err(format!("more than {MAX_BANKS} banks"));
        }
        if a.projects.len() > MAX_PROJECTS {
            return Err(format!("more than {MAX_PROJECTS} Csis projects"));
        }
        for (bank, slots) in &r.samples {
            if !valid_name(bank) {
                return Err(format!("replace.samples: bad bank name {bank:?}"));
            }
            for (slot, file) in slots {
                if !index(slot, MAX_BANK_SAMPLES as u32 * 8) {
                    return Err(format!("replace.samples.{bank}: slot {slot:?} is not a slot number"));
                }
                if file.loop_start().is_some_and(|l| l > 48_000 * 600) {
                    return Err(format!("replace.samples.{bank}.{slot}: loop_start out of range"));
                }
            }
        }
        for (section, banks) in [("replace.banks", &r.banks), ("add.banks", &a.banks)] {
            for (stem, b) in banks {
                if !valid_name(stem) {
                    return Err(format!("{section}: bad bank name {stem:?}"));
                }
                if b.samples.len() > MAX_BANK_SAMPLES {
                    return Err(format!("{section}.{stem}: more than {MAX_BANK_SAMPLES} samples"));
                }
                if b.abk.is_none() && b.samples.is_empty() {
                    return Err(format!("{section}.{stem}: give an abk and / or samples"));
                }
            }
        }
        for (section, names) in [("replace.splice", r.splice.keys()), ("replace.wheels", r.wheels.keys()), ("replace.ambience", r.ambience.keys())] {
            for n in names {
                if !valid_name(n) {
                    return Err(format!("{section}: bad name {n:?}"));
                }
            }
        }
        for n in r.grains.keys() {
            if !valid_name(n) {
                return Err(format!("replace.grains: bad name {n:?}"));
            }
        }
        let mut records = 0usize;
        for (file, recs) in &r.emitters {
            if !valid_name(file) {
                return Err(format!("replace.emitters: bad file name {file:?}"));
            }
            for (i, e) in recs {
                if !index(i, 1 << 20) {
                    return Err(format!("replace.emitters.{file}: {i:?} is not a record index"));
                }
                e.check(&format!("replace.emitters.{file}.{i}"))?;
            }
            records += recs.len();
        }
        for (file, recs) in &a.emitters {
            if !valid_name(file) {
                return Err(format!("add.emitters: bad file name {file:?}"));
            }
            for (i, e) in recs.iter().enumerate() {
                e.check(&format!("add.emitters.{file}[{i}]"))?;
            }
            records += recs.len();
        }
        for (key, set) in &r.random_sets {
            if !(hex_key(key) || valid_name(key)) {
                return Err(format!("replace.random_sets: {key:?} is not a key or name"));
            }
            set.check(&format!("replace.random_sets.{key}"))?;
        }
        for (key, set) in &a.random_sets {
            if !hex_key(key) {
                return Err(format!("add.random_sets: a new set's key is 16 hex digits ({key:?})"));
            }
            set.check(&format!("add.random_sets.{key}"))?;
        }
        for (key, zone) in &r.zones {
            if !(hex_key(key) || valid_name(key)) {
                return Err(format!("replace.zones: {key:?} is not a key or name"));
            }
            zone.check(&format!("replace.zones.{key}"))?;
        }
        for (key, zone) in &a.zones {
            if !hex_key(key) {
                return Err(format!("add.zones: a new zone's key is 16 hex digits ({key:?})"));
            }
            zone.check(&format!("add.zones.{key}"))?;
        }
        for (i, c) in r.crossfades.iter().enumerate() {
            c.check(&format!("replace.crossfades[{i}]"))?;
        }
        for (i, c) in a.crossfades.iter().enumerate() {
            c.check(&format!("add.crossfades[{i}]"))?;
        }
        for (bank, layers) in &a.location_programs {
            if !valid_name(bank) || layers.is_empty() || layers.len() > 16 {
                return Err(format!("add.location_programs.{bank}: a bank name and 1..16 layers"));
            }
            for (i, l) in layers.iter().enumerate() {
                l.check(&format!("add.location_programs.{bank}[{i}]"))?;
            }
            records += layers.len();
        }
        for (bank, groups) in &a.crossfade_layouts {
            if !valid_name(bank) || groups.is_empty() {
                return Err(format!("add.crossfade_layouts.{bank}: a bank name and at least one group"));
            }
            for (group, voices) in groups {
                let at = format!("add.crossfade_layouts.{bank}.{group}");
                if !index(group, MAX_CROSSFADE_GROUP) || group.parse::<u32>() == Ok(0) {
                    return Err(format!("{at}: the group is a number 1..{MAX_CROSSFADE_GROUP}"));
                }
                if voices.is_empty() || voices.len() > MAX_CROSSFADE_VOICES {
                    return Err(format!("{at}: 1..{MAX_CROSSFADE_VOICES} voices"));
                }
                for (i, v) in voices.iter().enumerate() {
                    if v.sample >= MAX_BANK_SAMPLES as u32 || !finite(&[v.pan, v.level]) || v.pan.abs() > 360.0 || !(0.0..=1.0).contains(&v.level) {
                        return Err(format!("{at}[{i}]: sample is a slot number, pan ±360 °, level 0..1"));
                    }
                }
                records += voices.len();
            }
        }
        for (section, archive, clips) in r.speech.iter().map(|(a, c)| ("replace.speech", a, c.keys().collect::<Vec<_>>()))
            .chain(a.speech.iter().map(|(a, c)| ("add.speech", a, c.keys().collect::<Vec<_>>()))) {
            if !SPEECH_ARCHIVES.contains(&archive.as_str()) {
                return Err(format!("{section}: unknown archive {archive:?} (one of {})", SPEECH_ARCHIVES.join(", ")));
            }
            for clip in clips {
                if !valid_name(clip) || !speech_clip_name(clip) {
                    return Err(format!("{section}.{archive}: {clip:?} is not a speech clip name (<event>_<voice>[_<voice name>]_<line>[.dat], e.g. 501_41_adtm1_Warn_n)"));
                }
            }
        }
        for (archive, clips) in &r.speech {
            for (clip, takes) in clips {
                if takes.keys().any(|t| !index(t, 254)) {
                    return Err(format!("replace.speech.{archive}.{clip}: takes are numbers 0..254"));
                }
            }
        }
        for (archive, clips) in &a.speech {
            for (clip, takes) in clips {
                if takes.is_empty() || takes.len() > 64 {
                    return Err(format!("add.speech.{archive}.{clip}: 1..64 takes"));
                }
            }
        }
        object(&self.tuning.player, "tuning.player")?;
        object(&self.tuning.world, "tuning.world")?;
        object(&self.tuning.bus, "tuning.bus")?;
        object(&self.tuning.grain, "tuning.grain")?;
        for (stem, map) in &self.maps {
            if !valid_name(stem) {
                return Err(format!("maps: bad map stem {stem:?}"));
            }
            records += map.check(&format!("maps.{stem}"))?;
        }
        records += r.random_sets.len() + a.random_sets.len() + r.zones.len() + a.zones.len() + r.crossfades.len() + a.crossfades.len();
        if records > MAX_RECORDS {
            return Err(format!("more than {MAX_RECORDS} records"));
        }
        if self.rules.len() > crate::audio_rules::MAX_RULES_PER_MOD {
            return Err(format!("more than {} rules", crate::audio_rules::MAX_RULES_PER_MOD));
        }
        for (key, rule) in &self.rules {
            if !crate::schema::valid_id(key) || !rule.validate() {
                return Err(format!("rules.{key}: a rule needs a known match (tag / kind / source / class / slot / id), an action (mute, replace, layer) and, for replace / layer, a play with a mod WAV"));
            }
        }
        let files = self.files();
        if files.len() > MAX_FILES {
            return Err(format!("more than {MAX_FILES} files"));
        }
        for (path, kind) in &files {
            if !valid_content_path(path, kind.exts()) {
                return Err(format!("bad {:?} path {path:?} (mod-relative, /-separated, .{})", kind, kind.exts()[0]));
            }
        }
        Ok(())
    }

    /// One line per kind of change, for check_mod and the log.
    pub fn summary(&self) -> Vec<String> {
        let r = &self.replace;
        let a = &self.add;
        let mut out = Vec::new();
        let mut line = |n: usize, what: &str| {
            if n > 0 {
                out.push(format!("{what}: {n}"));
            }
        };
        line(r.samples.values().map(BTreeMap::len).sum(), "replaced samples");
        line(r.banks.len(), "replaced banks");
        line(a.banks.len(), "added banks");
        line(a.projects.len(), "added Csis projects");
        line(r.splice.len(), "replaced Splice trees");
        line(r.grains.len(), "replaced grain members");
        line(r.wheels.len(), "replaced wheel streams");
        line(r.ambience.len(), "replaced ambience beds");
        line(usize::from(r.mixmap.is_some()), "replaced MixMap");
        line(r.emitters.values().map(BTreeMap::len).sum(), "replaced emitter records");
        line(a.emitters.values().map(Vec::len).sum(), "added emitter records");
        line(r.random_sets.len(), "replaced location sets");
        line(a.random_sets.len(), "added location sets");
        line(r.zones.len(), "replaced zones");
        line(a.zones.len(), "added zones");
        line(r.crossfades.len(), "replaced crossfades");
        line(a.crossfades.len(), "added crossfades");
        line(r.speech.values().flat_map(|c| c.values()).map(BTreeMap::len).sum(), "replaced speech takes");
        line(a.speech.values().flat_map(|c| c.values()).map(Vec::len).sum(), "added speech takes");
        line(a.location_programs.len(), "location programs");
        line(a.crossfade_layouts.len(), "crossfade layouts");
        let t = &self.tuning;
        line([&t.player, &t.world, &t.bus, &t.grain].iter().filter(|v| v.is_some()).count(), "tuning sections");
        line(self.maps.len(), "maps");
        line(self.rules.len(), "rules");
        out
    }
}

/// A checked overlay and what its files cost.
#[derive(Clone, Debug)]
pub struct Loaded {
    pub overlay: AudioOverlay,
    /// PCM16 bytes of its WAVs (the budget).
    pub pcm_bytes: u64,
}

/// Check one content file inside the mod root: WAVs through the canonical PCM16 check (with the
/// kind's limits), banks / trees / MixMaps / grain members through their parsers. Returns the
/// PCM bytes of a WAV (0 for other kinds).
pub fn check_file(root: &Path, path: &str, kind: FileKind) -> Result<u64, String> {
    let (bytes_limit, seconds) = match kind {
        FileKind::Sample | FileKind::Speech => SAMPLE_LIMITS,
        FileKind::Bed => BED_LIMITS,
        FileKind::Stream => STREAM_LIMITS,
        _ => (MAX_BINARY_BYTES, 0.0),
    };
    let bytes = read_bounded(root, path, bytes_limit).map_err(|e| format!("{path}: {e}"))?;
    match kind {
        FileKind::Sample | FileKind::Bed | FileKind::Stream | FileKind::Speech => {
            let (canonical, _) = crate::audio::canonical_pcm_wav_limited(&bytes, bytes_limit, seconds).map_err(|e| format!("{path}: {e}"))?;
            Ok(canonical.len().saturating_sub(44) as u64)
        }
        FileKind::Abk => skate_audio::formats::Bank::parse(path, bytes).map(|_| 0).map_err(|e| format!("{path}: {e}")),
        FileKind::Splice => skate_audio::splice::SpliceBank::parse(&bytes).map(|_| 0).map_err(|e| format!("{path}: {e}")),
        FileKind::MixMap => skate_audio::mixmap::MixMapFile::parse(&bytes).map(|_| 0).map_err(|e| format!("{path}: {e}")),
        FileKind::Grain => skate_audio::grain::GrainFile::parse(&bytes).map(|_| 0).map_err(|e| format!("{path}: {e}")),
        FileKind::Csi => project_symbols(&bytes, path).and_then(|s| if s.is_empty() { Err(format!("{path}: a project without symbols")) } else { Ok(0) }),
    }
}

/// A Csis project's symbols as (table, name): 0 functions, 1 classes, 2 globals.
pub fn project_symbols(bytes: &[u8], name: &str) -> Result<Vec<(u8, String)>, String> {
    let p = skate_audio::formats::Project::parse(name, bytes).map_err(|e| format!("{name}: {e}"))?;
    Ok(p.tables.iter().enumerate().flat_map(|(t, syms)| syms.iter().map(move |s| (t as u8, s.name.clone()))).collect())
}

/// Doc 16 L4: the symbol names already taken (the install's projects, then the overlays merged so
/// far) and a mod's projects: the first symbol of the mod that is taken, as a message. A mod's
/// symbol may not shadow one the game or another mod posts by name.
pub fn project_clash(taken: &std::collections::BTreeSet<(u8, String)>, symbols: &[(u8, String)]) -> Option<String> {
    let kind = |t: u8| ["function", "class", "global"].get(usize::from(t)).copied().unwrap_or("symbol");
    symbols.iter().find(|s| taken.contains(*s)).map(|(t, n)| format!("add.projects: the {} {n} is already defined (by the install or an earlier mod); the overlay is left out", kind(*t)))
}

/// Read, parse and check a mod's `audio.json` and every file it names. `Ok(None)`: the mod has
/// no overlay.
pub fn load(root: &Path) -> Result<Option<Loaded>, String> {
    if !root.join(FILE).is_file() {
        return Ok(None);
    }
    let bytes = read_bounded(root, FILE, MAX_JSON_BYTES)?;
    let overlay: AudioOverlay = serde_json::from_slice(&bytes).map_err(|e| format!("{FILE}: {e}"))?;
    overlay.validate().map_err(|e| format!("{FILE}: {e}"))?;
    let mut pcm_bytes = 0u64;
    let mut seen = std::collections::BTreeSet::new();
    for (path, kind) in overlay.files() {
        if !seen.insert((path, kind as u8)) {
            continue;
        }
        pcm_bytes += check_file(root, path, kind).map_err(|e| format!("{FILE}: {e}"))?;
        if pcm_bytes > MAX_PCM_BYTES_PER_MOD {
            return Err(format!("{FILE}: the WAVs exceed {} MiB of PCM", MAX_PCM_BYTES_PER_MOD / (1024 * 1024)));
        }
    }
    Ok(Some(Loaded { overlay, pcm_bytes }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn wav(frames: usize, rate: u32, channels: u16) -> Vec<u8> {
        let data = frames * 2 * usize::from(channels);
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data as u32).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&channels.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
        b.extend_from_slice(&(channels * 2).to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(data as u32).to_le_bytes());
        for i in 0..frames * usize::from(channels) {
            b.extend_from_slice(&(((i as f32 * 0.1).sin() * 8000.0) as i16).to_le_bytes());
        }
        b
    }

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("skate-audio-content-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("audio")).unwrap();
        d
    }

    #[test]
    fn a_full_overlay_parses_validates_and_summarises() {
        let v = json!({
            "version": 1,
            "replace": {
                "samples": {"Skate_Collisions": {"3": "audio/pop.wav", "4": {"file": "audio/loop.wav", "loop_start": 10}}},
                "banks": {"C04_taxi01": {"abk": "audio/taxi.abk", "samples": ["audio/t0.wav"]}},
                "splice": {"Skate_Collisions": "audio/c.splc"},
                "grains": {"wood_ramp_hard": {"file": "audio/wood.wav", "grain": "audio/wood.grain"}},
                "wheels": {"Whls_spins_Jump_1": "audio/spin.wav"},
                "ambience": {"04_dt_main": "audio/dt.wav"},
                "mixmap": "audio/MixMapSK8.mxb",
                "emitters": {"sfx_downtown": {"9": {"position": [0, 0, 0], "extent": [5, 5, 5], "bank": "Baby_Cry_1"}}},
                "random_sets": {"e_dwtn_spillway_brewery": {"sounds": [{"bank": "Siren_city_8"}]}},
                "zones": {"int_tunnel": {"bed": "22_interior_tunnel_amb"}},
                "crossfades": [{"from": "1F741D87EB58E84F", "to": "B6CE4BDEB63B6639", "group": 2}],
                "speech": {"livingworld": {"501_59_busm1_Warn_n.dat": {"3": "audio/warn3.wav"}}}
            },
            "add": {
                "banks": {"MOD_siren": {"abk": "audio/MOD_siren.abk", "samples": ["audio/s0.wav"], "preload": true, "group": "world"}},
                "emitters": {"sfx_mymap": [{"position": [1, 2, 3], "extent": [4, 4, 4], "bank": "MOD_siren", "patch": 1},
                                           {"position": [1, 2, 3], "extent": [9, 9, 9], "kind": 5, "reverb": "BEEFC8E3DE04FBAE"}]},
                "random_sets": {"00000000000000AA": {"name": "my_set", "sounds": [{"bank": "MOD_siren", "weight": 3}]}},
                "zones": {"00000000000000BB": {"name": "my_zone", "bed": "04_dt_main"}},
                "speech": {"livingworld": {"501_59_busm1_Warn_n.dat": ["audio/w.wav"]}},
                "location_programs": {"MOD_siren": [{"sample": "shuffle"}, {"delay": 0.5, "sample": 0, "level": 0.5}]}
            },
            "tuning": {"player": {"grind": {"3": {"v": {"0": 0.8}}}}, "world": {"traffic_engine": {"c04_taxi01": {"idle_rpm": 900}}}},
            "maps": {"MyMap": {"district": "DownTown", "ems": ["sfx_mymap"], "regions": {"audio_ambience": [{"box": [0, 0, 50, 50], "key": "my_zone"}]}}}
        });
        let o: AudioOverlay = serde_json::from_value(v.clone()).unwrap();
        o.validate().unwrap();
        assert_eq!(o.replace.samples["Skate_Collisions"]["4"].loop_start(), Some(10));
        assert_eq!(o.add.banks["MOD_siren"].group, Some(Group::World));
        let s = o.summary().join("; ");
        assert!(s.contains("replaced samples: 2") && s.contains("added banks: 1") && s.contains("maps: 1"), "{s}");
        assert_eq!(o.files().len(), 14);
        // The serialized form reads back the same (the engine takes it as data).
        let back: AudioOverlay = serde_json::from_value(serde_json::to_value(&o).unwrap()).unwrap();
        assert_eq!(back, o);
    }

    #[test]
    fn bad_overlays_are_rejected() {
        let base = || json!({"version": 1});
        let cases = [
            json!({"version": 2}),
            json!({"version": 1, "typo": {}}),
            json!({"version": 1, "replace": {"sample": {}}}),
            json!({"version": 1, "replace": {"samples": {"x": {"a": "s.wav"}}}}),
            json!({"version": 1, "replace": {"samples": {"x": {"1": "../s.wav"}}}}),
            json!({"version": 1, "replace": {"samples": {"x": {"1": "C:/s.wav"}}}}),
            json!({"version": 1, "replace": {"samples": {"x": {"1": "s.ogg"}}}}),
            json!({"version": 1, "replace": {"samples": {"x": {"1": {"file": "s.wav", "extra": 1}}}}}),
            json!({"version": 1, "replace": {"banks": {"x": {}}}}),
            json!({"version": 1, "replace": {"mixmap": "m.wav"}}),
            json!({"version": 1, "add": {"random_sets": {"name": {"sounds": []}}}}),
            json!({"version": 1, "add": {"emitters": {"f": [{"position": [0, 0, 0], "extent": [0, 1, 1], "bank": "b"}]}}}),
            json!({"version": 1, "add": {"emitters": {"f": [{"position": [0, 0, 0], "extent": [1, 1, 1]}]}}}),
            json!({"version": 1, "add": {"emitters": {"f": [{"position": [0, 0, 0], "extent": [1, 1, 1], "kind": 5}]}}}),
            json!({"version": 1, "add": {"crossfades": [{"from": "00000000000000AA", "to": "00000000000000AA", "group": 1}]}}),
            json!({"version": 1, "add": {"location_programs": {"b": [{"sample": "every"}]}}}),
            json!({"version": 1, "tuning": {"player": 3}}),
            json!({"version": 1, "maps": {"m": {"regions": {"audio_music": []}}}}),
            json!({"version": 1, "maps": {"m": {"regions": {"audio_reverb": [{"box": [0, 0, 0, 1], "key": "x"}]}}}}),
            json!({"version": 1, "replace": {"speech": {"livingworld": {"501_41_adtm1_Warn_n": {"300": "a.wav"}}}}}),
            // Speech clip names are checked with the speech index's own parser, archives by name.
            json!({"version": 1, "replace": {"speech": {"livingworld": {"c": {"0": "a.wav"}}}}}),
            json!({"version": 1, "add": {"speech": {"livingworld": {"warn_line": ["a.wav"]}}}}),
            json!({"version": 1, "add": {"speech": {"livingworld": {"501_41": ["a.wav"]}}}}),
            json!({"version": 1, "add": {"speech": {"crowd": {"501_41_adtm1_Warn_n": ["a.wav"]}}}}),
            // Declared crossfade layouts: groups 1..64, 1..8 voices, levels 0..1.
            json!({"version": 1, "add": {"crossfade_layouts": {"b": {"0": [{"sample": 0}]}}}}),
            json!({"version": 1, "add": {"crossfade_layouts": {"b": {"65": [{"sample": 0}]}}}}),
            json!({"version": 1, "add": {"crossfade_layouts": {"b": {"1": []}}}}),
            json!({"version": 1, "add": {"crossfade_layouts": {"b": {"1": [{"sample": 0, "level": 1.5}]}}}}),
            json!({"version": 1, "add": {"crossfade_layouts": {"b": {"1": [{"sample": 0, "pan": 400}]}}}}),
            json!({"version": 1, "add": {"crossfade_layouts": {"b": {}}}}),
        ];
        assert!(serde_json::from_value::<AudioOverlay>(base()).unwrap().validate().is_ok());
        for case in cases {
            let ok = serde_json::from_value::<AudioOverlay>(case.clone()).map_err(|e| e.to_string()).and_then(|o| o.validate());
            assert!(ok.is_err(), "accepted {case}");
        }
    }

    #[test]
    fn load_checks_every_file_inside_the_mod() {
        let d = dir("load");
        std::fs::write(d.join("audio/pop.wav"), wav(4800, 48000, 1)).unwrap();
        std::fs::write(d.join(FILE), json!({"version": 1, "replace": {"samples": {"Skate_Collisions": {"3": "audio/pop.wav"}}}}).to_string()).unwrap();
        let loaded = load(&d).unwrap().unwrap();
        assert_eq!(loaded.pcm_bytes, 9600);
        // A missing file, a corrupt WAV, a non-bank `.abk` and a path escaping the root fail.
        for (overlay, why) in [
            (json!({"version": 1, "replace": {"samples": {"b": {"1": "audio/missing.wav"}}}}), "missing"),
            (json!({"version": 1, "replace": {"samples": {"b": {"1": "audio/bad.wav"}}}}), "corrupt"),
            (json!({"version": 1, "add": {"banks": {"b": {"abk": "audio/bad.abk"}}}}), "bank"),
            (json!({"version": 1, "replace": {"mixmap": "audio/bad.mxb"}}), "mixmap"),
        ] {
            std::fs::write(d.join("audio/bad.wav"), b"RIFF0000WAVEjunk").unwrap();
            std::fs::write(d.join("audio/bad.abk"), b"not a bank").unwrap();
            std::fs::write(d.join("audio/bad.mxb"), b"not a mixmap").unwrap();
            std::fs::write(d.join(FILE), overlay.to_string()).unwrap();
            assert!(load(&d).is_err(), "{why}");
        }
        // A bed may be longer than a sample.
        std::fs::write(d.join("audio/bed.wav"), wav(48000 * 40, 48000, 1)).unwrap();
        std::fs::write(d.join(FILE), json!({"version": 1, "replace": {"ambience": {"04_dt_main": "audio/bed.wav"}}}).to_string()).unwrap();
        assert!(load(&d).is_ok(), "a 40 s bed");
        std::fs::write(d.join(FILE), json!({"version": 1, "replace": {"samples": {"b": {"0": "audio/bed.wav"}}}}).to_string()).unwrap();
        assert!(load(&d).is_err(), "a 40 s sample");
        std::fs::remove_file(d.join(FILE)).unwrap();
        assert!(load(&d).unwrap().is_none(), "no overlay");
        let _ = std::fs::remove_dir_all(d);
    }

    /// The overlays the repo ships (the SDK example and the dev test mod) pass the deep check, and
    /// their packages validate.
    #[test]
    fn the_shipped_overlays_validate() {
        for dir in ["../../sdk/examples/audio-example", "../../mods/audio-content-test"] {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(dir);
            let loaded = load(&root).unwrap_or_else(|e| panic!("{dir}: {e}")).unwrap_or_else(|| panic!("{dir}: no audio.json"));
            assert!(!loaded.overlay.summary().is_empty());
            let (manifest, audio) = crate::validate_package_content(&root).unwrap_or_else(|e| panic!("{dir}: {e}"));
            assert!(!manifest.enabled_by_default, "{dir}: audio mods ship off by default");
            assert!(audio.is_some());
        }
    }

    #[test]
    fn map_definitions_parse_on_their_own() {
        let def = MapAudioDef::parse(br#"{"ems": ["sfx_downtown"], "crossfade_bank": "Main_Ambience_Crossfade_DT",
            "emitters": [{"position": [0, 0, 0], "extent": [3, 3, 3], "bank": "Baby_Cry_1"}],
            "regions": {"audio_emitters": [{"box": [10, 10, 5, 5], "key": "e_dwtn_spillway_brewery"}]}}"#, "x.audio.json").unwrap();
        assert_eq!(def.ems.as_deref(), Some(&["sfx_downtown".to_owned()][..]));
        assert!(MapAudioDef::parse(br#"{"ems": "sfx"}"#, "x").is_err());
        assert!(MapAudioDef::parse(br#"{"unknown": 1}"#, "x").is_err());
    }

    /// `rules` in audio.json (capability `audio_content` = 2): checked in depth (the rule shape and
    /// its WAV, counted in the PCM budget); an overlay of rules only has no content (no restart).
    #[test]
    fn rules_in_audio_json_are_checked_and_carry_no_content() {
        let d = dir("rules");
        std::fs::write(d.join("audio/pop.wav"), wav(4800, 48000, 1)).unwrap();
        let only = json!({"version": 1, "rules": {"my_pop": {"match": {"tag": "pop"}, "action": "replace", "play": {"path": "audio/pop.wav"}}}});
        std::fs::write(d.join(FILE), only.to_string()).unwrap();
        let loaded = load(&d).unwrap().unwrap();
        assert!(!loaded.overlay.has_content(), "rules only: no restart");
        assert_eq!(loaded.pcm_bytes, 9600, "the rule's WAV counts");
        assert!(loaded.overlay.summary().contains(&"rules: 1".to_owned()));
        let both = json!({"version": 1, "replace": {"samples": {"x": {"0": "audio/pop.wav"}}}, "rules": {"q": {"match": {"tag": "land"}, "action": "mute"}}});
        let o: AudioOverlay = serde_json::from_value(both).unwrap();
        assert!(o.has_content() && o.validate().is_ok());
        for bad in [
            json!({"version": 1, "rules": {"x": {"match": {"tag": "pop"}, "action": "replace", "play": {"path": "audio/missing.wav"}}}}),
            json!({"version": 1, "rules": {"x": {"match": {}, "action": "mute"}}}),
            json!({"version": 1, "rules": {"bad key": {"match": {"tag": "pop"}, "action": "mute"}}}),
        ] {
            std::fs::write(d.join(FILE), bad.to_string()).unwrap();
            assert!(load(&d).is_err(), "accepted {bad}");
        }
        let many: serde_json::Map<String, Value> = (0..33).map(|i| (format!("r{i}"), json!({"match": {"tag": "pop"}, "action": "mute"}))).collect();
        let o: AudioOverlay = serde_json::from_value(json!({"version": 1, "rules": many})).unwrap();
        assert!(o.validate().is_err(), "32 rules at most");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Doc 16 L4: `add.projects` names `.csi` files, read through the Csis parser (a project without
    /// symbols, a broken file or another extension is refused); a project's symbols that the
    /// install or an earlier mod already defines are a clash; the merge appends the project after
    /// the install's.
    #[test]
    fn mod_csis_projects_are_checked_and_merged() {
        use skate_audio::formats::{Project, csi::Symbol};
        let d = std::env::temp_dir().join(format!("skate-audio-csi-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("audio")).unwrap();
        let s = |name: &str| Symbol { name: name.into(), name_id: 1, default: 0 };
        std::fs::write(d.join("audio/mod.csi"), Project { name: "m".into(), id: 7, tables: [vec![], vec![s("c_mod")], vec![s("g_mod")]] }.to_bytes()).unwrap();
        std::fs::write(d.join("audio/empty.csi"), Project { name: "e".into(), id: 7, tables: Default::default() }.to_bytes()).unwrap();
        std::fs::write(d.join("audio/bad.csi"), b"nope").unwrap();
        let write = |o: serde_json::Value| std::fs::write(d.join(FILE), o.to_string()).unwrap();
        write(json!({"version": 1, "add": {"projects": ["audio/mod.csi"]}}));
        let loaded = load(&d).unwrap().unwrap();
        assert!(loaded.overlay.has_content() && loaded.overlay.summary().contains(&"added Csis projects: 1".to_owned()));
        for bad in ["audio/empty.csi", "audio/bad.csi", "audio/mod.abk", "../mod.csi"] {
            write(json!({"version": 1, "add": {"projects": [bad]}}));
            assert!(load(&d).is_err(), "{bad}");
        }
        write(json!({"version": 1, "add": {"projects": vec!["audio/mod.csi"; MAX_PROJECTS + 1]}}));
        assert!(load(&d).is_err(), "too many");
        let symbols = project_symbols(&std::fs::read(d.join("audio/mod.csi")).unwrap(), "mod.csi").unwrap();
        assert_eq!(symbols, [(1, "c_mod".to_owned()), (2, "g_mod".to_owned())]);
        let mut taken = std::collections::BTreeSet::from([(1u8, "c_emitter".to_owned())]);
        assert_eq!(project_clash(&taken, &symbols), None);
        taken.insert((2, "g_mod".into()));
        assert!(project_clash(&taken, &symbols).is_some_and(|m| m.contains("global g_mod")));
        assert_eq!(project_clash(&std::collections::BTreeSet::from([(0u8, "g_mod".to_owned())]), &symbols), None, "another table");
        let mut m = json!({"aems": {"projects": ["aems/a.csi"]}});
        let o: AudioOverlay = serde_json::from_value(json!({"version": 1, "add": {"projects": ["audio/mod.csi"]}})).unwrap();
        crate::audio_merge::merge(&mut m, &[crate::audio_merge::Source { id: "me", overlay: &o }]);
        assert_eq!(m["aems"]["projects"], json!(["aems/a.csi", "mod:me/audio/mod.csi"]));
        let _ = std::fs::remove_dir_all(&d);
    }
}
