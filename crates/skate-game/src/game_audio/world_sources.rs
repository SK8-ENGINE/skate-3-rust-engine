//! World sound sources on the native runtime (`skate_audio::world`): traffic vehicles and
//! pedestrians as retail's `SFXObj_Traffic*` / `SFXObj_Pedestrian*` owners. **Inert until a game
//! system publishes owners**: the engine has no living world (peds, traffic) yet, so
//! [`WorldOwners`] stays empty and [`frame`] returns at once. `SKATE_AEMS_WORLD=0` turns it off
//! even with owners.
//!
//! The hook for a future ped / vehicle system: each frame, write every live object into
//! [`WorldOwners`] (`vehicles` / `peds`, keyed by a stable id) and remove the ones that despawn.
//! Everything else (instances, MixMap inputs, posts, banks) happens here:
//! - on the first frame with owners the world banks load (`skate_audio::world::{TRAFFIC_BANKS,
//!   PED_BANKS}`; retail loads them all when the living world starts). Set
//!   [`WorldOwners::expected`] as soon as the system knows it will publish owners on this map
//!   (map load, its spawner starting): the banks are then read and decoded on the prefetch worker
//!   (`native::prefetch`) and `load_bank` at the first owner takes the decoded data instead of
//!   decoding on the game thread (`SKATE_AEMS_WORLD_PREFETCH=0`: game-thread loads as before);
//! - per pass, as retail: before the MixMap ticks ([`frame`] → [`pre`], between
//!   `native::mixmap_frame` and `native::mixmap_tick`) the instances go to the nearest owners
//!   within retail's list radii (`owners::Pool`), and `process` writes the 3DObjPos blocks and
//!   inputs and posts; after the ticks ([`frame_post`] → [`post`]) each instance's objects `update`
//!   from this pass's outputs. (Before 2026-10-03 both ran after the ticks: the inputs a tick saw
//!   were one console frame old.)
//! - ped speech requests go to the speech host (`world_speech`: the speech manager, the library and
//!   the living world's two streams, levels from each speaker's PedestrianSpeech outputs).
use std::collections::HashMap;

use bevy::prelude::*;
use serde::Deserialize;
use skate_audio::eval::NodeId;
use skate_audio::mixmap::cadence::CONSOLE_DT;
use skate_audio::player::objpos::Listener;
use skate_audio::world::owners::{Pool, Positions};
use skate_audio::world::peds::{PedBodyFall, PedFootstepTuning, PedObjectTuning, PedSfx, PedSpeech, PedState, PedTazer};
use skate_audio::player::Outputs as _;
use skate_audio::world::traffic::{EngineRecord, OutputsSnapshot, Vehicle, VehicleState};
use skate_audio::world::{Lcg, PED_BANKS, TRAFFIC_BANKS, WorldCommand, WorldSlot, keys};

use super::Library;
use super::native::Native;

/// What the living world publishes each frame (empty: nothing plays).
#[derive(Resource, Default)]
pub(crate) struct WorldOwners {
    pub(crate) vehicles: HashMap<u64, VehicleState>,
    pub(crate) peds: HashMap<u64, PedState>,
    /// The living world will publish owners on this map: prefetch the world banks (decode only;
    /// see the module docs). Clearing it drops the prefetched banks no owner has used yet.
    pub(crate) expected: bool,
    /// The game flag PedestrianSpeech's photographer repeat needs (system byte
    /// `*(0x830CFDC4)+912`; meaning not traced): `LivingWorldAudio::photo_repeat`.
    pub(crate) photo_flag: bool,
}

/// The install's world tuning (`audio_manifest.json` `world_tuning`, setup
/// `tools/asset_pipeline/world_audio.py`; absent in older installs: the defaults apply).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct WorldTuningJson {
    /// `aud_traffic_engine` records by name (`default`, `c01_family01`, …).
    traffic_engine: HashMap<String, EngineJson>,
    ped_footsteps: Option<PedFootstepsJson>,
    /// The speech manager's event tuning per speech bank (`"1"` = the living world) and event id.
    speech_tuning: HashMap<String, HashMap<String, SpeechTuningJson>>,
    /// Per ped model (= speech voice id): `aud_characteristics` (spec §7.3 G2).
    ped_models: HashMap<String, PedModelJson>,
    /// Traffic model (living-world entity name: `taxi01`, `sedan02`, …) → `aud_traffic_engine`
    /// record (spec §7.3 G1).
    traffic_models: HashMap<String, String>,
    /// The ped one-shot objects (`skate_audio::world::peds::PedObjectTuning`): PedBodyFall's
    /// containers and eEQChain bus, the phone ring, the tazer hold.
    ped_objects: Option<PedObjectsJson>,
    /// The speech stream voice's PEAK curves (`skate_audio::world::speech_player::SpeechVoiceTuning`).
    speech_voice: Option<SpeechVoiceJson>,
    /// The car alarm rule (`livingworld_vehicle_characteristics` default: `min_impact`, `seconds`).
    vehicle_alarm: Option<VehicleAlarmJson>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct VehicleAlarmJson {
    min_impact: Option<f32>,
    seconds: Option<f32>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct PedObjectsJson {
    body_fall_ids: Vec<u32>,
    body_fall_bank: Option<String>,
    body_fall_eq: Option<u8>,
    ring_bank: Option<String>,
    ring_id: Option<u32>,
    ring_answer: Option<i32>,
    photo_repeat: Option<f32>,
    tazer_seconds: Option<f32>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct GraphJson {
    x: Vec<f32>,
    y: Vec<f32>,
}

impl GraphJson {
    fn graph8(&self) -> Option<skate_audio::world::speech_player::Graph8> {
        Some(skate_audio::world::speech_player::Graph8 { x: self.x.as_slice().try_into().ok()?, y: self.y.as_slice().try_into().ok()? })
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct SpeechVoiceJson {
    peak_freq: Option<GraphJson>,
    peak_gain: Option<GraphJson>,
    peak_q: Option<GraphJson>,
    /// The echo delay's camera-distance factor and its refresh (console frames).
    delay_factor: Option<f32>,
    delay_frames: Option<u32>,
    /// The announcer: an NPC pro's crash asks for `480_slam_pro` within this camera distance (m),
    /// and the stream level multipliers by language group (`other` = English) and challenge byte.
    announcer_crash_m: Option<f32>,
    announcer_level: HashMap<String, Vec<f32>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct SpeechTuningJson {
    gap: f32,
    flags_12: Vec<u8>,
    priority: i32,
    probability: Option<f32>,
    repeat: f32,
    /// The main cast's repeat time for speaker slots 31 / 30 (record `+52` / `+56`), and (speaker, s)
    /// pairs a mod's tuning adds (they win over the two).
    repeat_speaker_31: Option<f32>,
    repeat_speaker_30: Option<f32>,
    speaker_repeat: Vec<(u32, f32)>,
    min_player_kmh: f32,
    max_player_kmh: f32,
    timer_40: f32,
    timer_44: f32,
    flags_48: Vec<u8>,
    zombie: bool,
    not_follow: Vec<(i64, f32)>,
    challenges: Vec<i32>,
}

/// A ped model's audio fields (`aud_characteristics`, setup `world_audio.ped_models`).
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct PedModelJson {
    /// `SPCH1Type_CharID`: the voice variant bit (`S+88`).
    pub(crate) variant: u32,
    /// The speaker type bit (`S+96`; 64 = security).
    pub(crate) kind: u32,
    /// `S+124`: 1 female, 2 male.
    pub(crate) gender: u32,
    /// `S+132`.
    pub(crate) shoe_class: u8,
    /// `S+156`: the far line threshold (m).
    pub(crate) far: f32,
    /// `S+152`: the per-voice float (`2087A3290483BB4F`, 0.8–1.2): the speech stream's gain and
    /// sends are multiplied by it (`sub_82C5CEF0`). 0 = absent (1.0).
    pub(crate) pitch: f32,
    /// The main cast's words (`S+84` / `+88`, the other-skater word `D6EA428C2B43E23A`): the cast bit
    /// (`6F2933E977CF40DD`: the pros 1–29), the cast word (`14FD437D190677C8`: the special cast) and
    /// the word another pro's line names this one by. A model with no type bit (`kind` 0) and a
    /// cast bit or word speaks on the main-cast channel.
    pub(crate) cast_bit: u32,
    pub(crate) cast_word: u32,
    pub(crate) cast_word2: u32,
    /// `SPCH3Type_char_ID_Ann` (`6F9C8A27E4CD37DC`): the announcer characters' word (model 35 → 1,
    /// 36 → 2); `SPCH3Type_pro_id_ANN` (`14B23B4527AF919E`): the word `480_slam_pro` names this pro
    /// by (0 = no announcer line about this model).
    pub(crate) announcer_id: u32,
    pub(crate) announcer_pro: u32,
}

impl PedModelJson {
    /// The main-cast words (bit, word, other-skater word) when this model speaks on the main cast.
    pub(crate) fn main_cast(&self) -> Option<(u32, u32, u32)> {
        (self.kind == 0 && (self.cast_bit != 0 || self.cast_word != 0)).then_some((self.cast_bit, self.cast_word, self.cast_word2))
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default)]
struct EngineJson {
    idle_rpm: f32,
    max_rpm: f32,
    patch: i32,
    wobble_limit: f32,
    wobble_rate: f32,
    rise: f32,
    fall: f32,
    slew: f32,
    gear_speed: f32,
    gears: i32,
    rear_bias: i32,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct PedFootstepsJson {
    speed_curve_x: Vec<f32>,
    speed_curve_y: Vec<f32>,
    speeds: Vec<f32>,
    step_ids: Vec<i32>,
    tail: Vec<i32>,
    eq_chain: Option<i32>,
}

impl WorldTuningJson {
    /// An engine record by name (the vehicle system names its models' records). Unused until a
    /// vehicle system publishes owners.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn engine(&self, name: &str) -> Option<EngineRecord> {
        let e = self.traffic_engine.get(name)?;
        Some(EngineRecord {
            idle_rpm: e.idle_rpm,
            max_rpm: e.max_rpm,
            patch: e.patch,
            wobble_limit: e.wobble_limit,
            wobble_rate: e.wobble_rate,
            rise: e.rise,
            fall: e.fall,
            slew: e.slew,
            gear_speed: e.gear_speed,
            gears: e.gears,
            rear_bias: e.rear_bias,
        })
    }

    /// A ped model's fields by its voice id (None: not in the export).
    pub(crate) fn ped_model(&self, voice: u32) -> Option<PedModelJson> {
        self.ped_models.get(&voice.to_string()).copied()
    }

    /// The car alarm rule's retail numbers from the export (None: an install from before it; the
    /// engine's constants are the same values).
    pub(crate) fn vehicle_alarm(&self) -> Option<crate::world_audio::AlarmTuning> {
        let v = self.vehicle_alarm.as_ref()?;
        let mut t = crate::world_audio::AlarmTuning::default();
        if let Some(m) = v.min_impact.filter(|m| m.is_finite() && *m >= 0.0) {
            t.min_impact = m;
        }
        if let Some(s) = v.seconds.filter(|s| s.is_finite() && *s >= 0.0) {
            t.seconds = s;
        }
        Some(t)
    }

    /// A traffic model's engine record name (`taxi01` → `c04_taxi01`).
    pub(crate) fn traffic_model(&self, model: &str) -> Option<&str> {
        self.traffic_models.get(model).map(String::as_str)
    }

    /// The living world's speech tuning (bank 1) by event id.
    pub(crate) fn speech_tuning(&self) -> HashMap<u16, skate_audio::world::speech_manager::EventTuning> {
        self.speech_tuning_bank(1)
    }

    /// One speech bank's tuning by event id (0 = the main cast, 1 = the living world).
    pub(crate) fn speech_tuning_bank(&self, bank: u32) -> HashMap<u16, skate_audio::world::speech_manager::EventTuning> {
        let Some(bank) = self.speech_tuning.get(&bank.to_string()) else { return HashMap::new() };
        bank.iter()
            .filter_map(|(id, t)| {
                let flags = |i: usize| t.flags_12.get(i).is_some_and(|b| *b != 0);
                let blocked = |i: usize| t.flags_48.get(i).is_some_and(|b| *b != 0);
                Some((
                    id.parse().ok()?,
                    skate_audio::world::speech_manager::EventTuning {
                        gap: t.gap,
                        priority: t.priority,
                        interrupt: flags(1),
                        interrupt_when_full: flags(2),
                        probability: t.probability.unwrap_or(100.0),
                        repeat: t.repeat,
                        // A mod's pairs first: the first match wins.
                        speaker_repeat: t.speaker_repeat.iter().copied().chain(t.repeat_speaker_31.map(|s| (31, s))).chain(t.repeat_speaker_30.map(|s| (30, s))).collect(),
                        min_player_kmh: t.min_player_kmh,
                        max_player_kmh: t.max_player_kmh,
                        timer_40: t.timer_40,
                        timer_44: t.timer_44,
                        blocked_by: [blocked(1), blocked(2), blocked(3)],
                        zombie: t.zombie,
                        not_follow: t.not_follow.iter().filter_map(|&(e, s)| Some((u16::try_from(e).ok()?, s))).collect(),
                        challenges: t.challenges.clone(),
                    },
                ))
            })
            .collect()
    }

    /// The ped one-shot objects' tuning (the retail defaults where the export lacks a field).
    pub(crate) fn ped_objects(&self) -> PedObjectTuning {
        let mut t = PedObjectTuning::default();
        let Some(p) = &self.ped_objects else { return t };
        if let [a, b, c] = p.body_fall_ids[..] {
            t.body_fall_ids = [a, b, c];
        }
        if let Some(v) = &p.body_fall_bank {
            t.body_fall_bank = v.clone();
        }
        if let Some(v) = p.body_fall_eq {
            t.body_fall_eq = v.min(7);
        }
        if let Some(v) = &p.ring_bank {
            t.ring_bank = v.clone();
        }
        if let Some(v) = p.ring_id {
            t.ring_id = v;
        }
        if let Some(v) = p.ring_answer {
            t.ring_answer = v;
        }
        if let Some(v) = p.photo_repeat.filter(|v| v.is_finite() && *v > 0.0) {
            t.photo_repeat = v;
        }
        if let Some(v) = p.tazer_seconds.filter(|v| v.is_finite() && *v >= 0.0) {
            t.tazer_seconds = v;
        }
        t
    }

    /// The speech stream voice's tuning (the shipped curves where the export lacks one).
    pub(crate) fn speech_voice(&self) -> skate_audio::world::speech_player::SpeechVoiceTuning {
        let mut t = skate_audio::world::speech_player::SpeechVoiceTuning::default();
        let Some(v) = &self.speech_voice else { return t };
        if let Some(g) = v.peak_freq.as_ref().and_then(GraphJson::graph8) {
            t.peak_freq = g;
        }
        if let Some(g) = v.peak_gain.as_ref().and_then(GraphJson::graph8) {
            t.peak_gain = g;
        }
        if let Some(g) = v.peak_q.as_ref().and_then(GraphJson::graph8) {
            t.peak_q = g;
        }
        if let Some(f) = v.delay_factor.filter(|f| f.is_finite() && *f >= 0.0) {
            t.delay_factor = f;
        }
        if let Some(n) = v.delay_frames.filter(|n| *n >= 1) {
            t.delay_frames = n;
        }
        t
    }

    /// The announcer's crash distance and level multipliers (the shipped English values where the
    /// export lacks them).
    pub(crate) fn announcer_level(&self) -> skate_audio::world::announcer::AnnouncerLevel {
        let mut t = skate_audio::world::announcer::AnnouncerLevel::default();
        let Some(v) = &self.speech_voice else { return t };
        if let Some(d) = v.announcer_crash_m.filter(|d| d.is_finite() && *d >= 0.0) {
            t.crash_distance = d;
        }
        let three = |k: &str| v.announcer_level.get(k).and_then(|a| <[f32; 3]>::try_from(a.as_slice()).ok()).filter(|a| a.iter().all(|x| x.is_finite() && *x >= 0.0));
        if let Some(a) = three("other") {
            t.scale = a;
        }
        if let Some(a) = three("other_challenge") {
            t.scale_challenge = a;
        }
        t
    }

    pub(crate) fn ped_footsteps(&self) -> PedFootstepTuning {
        let mut t = PedFootstepTuning::default();
        let Some(p) = &self.ped_footsteps else { return t };
        if p.speed_curve_x.len() == 16 && p.speed_curve_y.len() == 16 {
            t.speed_curve.x.copy_from_slice(&p.speed_curve_x);
            t.speed_curve.y.copy_from_slice(&p.speed_curve_y);
        }
        if let [a, b] = p.speeds[..] {
            t.speeds = [a, b];
        }
        if let [a, b, c] = p.step_ids[..] {
            t.step_ids = [a, b, c];
        }
        if let [a, b, c] = p.tail[..] {
            t.tail = [a, b, c];
        }
        if let Some(eq) = p.eq_chain {
            t.eq_chain = eq;
        }
        t
    }
}

/// `SKATE_AEMS_WORLD=0` keeps the world owners off.
fn requested() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("SKATE_AEMS_WORLD").is_ok_and(|v| v == "0"))
}

/// `SKATE_AEMS_WORLD_PREFETCH=0` keeps the world banks off the prefetch worker (they load on the
/// game thread at the first owner, as before).
fn prefetch_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("SKATE_AEMS_WORLD_PREFETCH").is_ok_and(|v| v == "0"))
}

/// The world banks this host asked the prefetch worker for.
#[derive(Default)]
struct WorldPrefetch {
    /// `Prefetch::clears` when `requested` was last valid: a map change clears the prefetch and
    /// unloads the world banks, so they are asked for again.
    epoch: Option<u64>,
    /// Requested since the last clear (whether still queued, decoded, failed or already taken).
    requested: Vec<&'static str>,
    /// Not in the install (`ensure_bank` reports them at the first owner, as before).
    unavailable: Vec<&'static str>,
}

/// While the living world is `expected`, queue every world bank that is neither loaded nor
/// requested on the prefetch worker; once it is not, drop the requested banks no owner loaded.
/// Only reading and decoding move: `Native::ensure_bank` still runs `load_bank` at the first
/// owner (it takes the worker's data, waits for a running decode, or loads a queued / failed bank
/// itself), so the runtime sees the same calls in the same order. Touches no runtime state.
fn prefetch_world_banks(p: &mut WorldPrefetch, native: &mut Native, library: &Library, expected: bool) {
    let clears = native.prefetch.clears();
    if p.epoch != Some(clears) {
        p.epoch = Some(clears);
        p.requested.clear();
    }
    if expected {
        for &stem in TRAFFIC_BANKS.iter().chain(PED_BANKS) {
            if p.requested.contains(&stem) || p.unavailable.contains(&stem) || native.bank_loaded(stem) {
                continue;
            }
            match library.bank_source(stem) {
                Ok(source) => {
                    native.prefetch.request(source);
                    p.requested.push(stem);
                }
                Err(_) => p.unavailable.push(stem),
            }
        }
    } else {
        for stem in p.requested.drain(..) {
            if !native.bank_loaded(stem) {
                native.prefetch.drop_bank(stem);
            }
        }
    }
}

#[derive(Resource)]
pub(crate) struct WorldHost {
    /// None: not tried yet; Some(false): some world banks are missing (warned once).
    banks: Option<bool>,
    traffic: Pool,
    peds: Pool,
    vehicles: HashMap<u64, (Vehicle, Positions)>,
    ped_objects: HashMap<u64, PedObjects>,
    nodes: HashMap<(u64, WorldSlot), NodeId>,
    classes: HashMap<&'static str, usize>,
    /// This frame's pass between [`pre`] and [`post`]: the camera and the seconds it covers.
    pass: Option<([f32; 3], f32)>,
    rng: Lcg,
    ped_tuning: Option<PedFootstepTuning>,
    /// The ped one-shot objects' tuning (`world_tuning.ped_objects`, read with the banks).
    object_tuning: PedObjectTuning,
    /// The phone ring's Splice bank is loaded (without it the answer follows the value at once:
    /// a missing-data fallback, logged once).
    ring_bank: Option<bool>,
    player_tuning: Option<skate_audio::player::tuning::PlayerTuning>,
    /// The camera at the last evaluation and the host's cut count then (`Native::cuts`: no
    /// velocity across a teleport / map change).
    last_camera: Option<([f32; 3], u64)>,
    prefetch: WorldPrefetch,
    /// `Native::map_epoch` this host last ran in (None: never ran; [`WorldHost::reset`]).
    epoch: Option<u64>,
    /// Packets posted (the summary log).
    posts: u64,
    /// The ped speech requests of the last evaluation (PedestrianSpeech process), with the speaker
    /// words and level selection: `frame` hands them to the speech host (`world_speech`).
    pub(crate) speech_requests: Vec<super::world_speech::PedRequest>,
    /// Audio event rows while some mod subscribes (`mod_audio::events_frame`).
    pub(crate) events: super::mod_audio::EventBuf,
    /// The mods' mute / replace / layer rules (`mod_rules`); None without rules.
    pub(crate) rules: Option<std::sync::Arc<super::mod_rules::RuleSet>>,
    /// The owners (cars, peds) inside the list radius that hold no instance after the last
    /// assignment: every instance is held by a nearer one; each takes one as soon as it is among
    /// the nearest (read back, doc 16 M3; bookkeeping only).
    pub(crate) waiting: (Vec<u64>, Vec<u64>),
}

impl Default for WorldHost {
    fn default() -> Self {
        Self {
            banks: None,
            traffic: Pool::new(keys::TRAFFIC_INSTANCES),
            peds: Pool::new(keys::PEDESTRIAN_INSTANCES),
            vehicles: HashMap::new(),
            ped_objects: HashMap::new(),
            nodes: HashMap::new(),
            classes: HashMap::new(),
            pass: None,
            rng: Lcg(0x5EED),
            ped_tuning: None,
            object_tuning: PedObjectTuning::default(),
            ring_bank: None,
            player_tuning: None,
            last_camera: None,
            prefetch: WorldPrefetch::default(),
            epoch: None,
            posts: 0,
            speech_requests: Vec::new(),
            events: None,
            rules: None,
            waiting: (Vec::new(), Vec::new()),
        }
    }
}

//// A held ped's objects (the Pedestrian slot's four: Speech 5.0, SFX 5.1, BodyFall 5.2, Tazer 5.3)
/// and its 3DObjPos block.
pub(crate) struct PedObjects {
    sfx: PedSfx,
    speech: PedSpeech,
    body_fall: PedBodyFall,
    tazer: PedTazer,
    pos: Positions,
    /// The missing-ring fallback answered this value (once per value change).
    ring_fallback: Option<i32>,
}

impl PedObjects {
    fn new(g: u32) -> Self {
        Self {
            sfx: PedSfx::default(),
            speech: PedSpeech::default(),
            body_fall: PedBodyFall::default(),
            tazer: PedTazer::default(),
            pos: Positions::new(&[keys::ped_pos(g)]),
            ring_fallback: None,
        }
    }

    /// The ped lost its instance: every packet released, every Splice sound stopped.
    fn release(&mut self, owner: u64, splice: &mut dyn skate_audio::player::contacts::SpliceHost) -> Vec<WorldCommand> {
        let mut cmds = self.sfx.release(owner, splice);
        cmds.extend(self.tazer.release(owner));
        self.body_fall.release(splice);
        self.speech.release(splice);
        cmds
    }
}

/// Which owners hold a MixMap instance after the last evaluation (the hosts write it; the
/// engine-facing bridge turns it into `world_audio::WorldAudioInstance`).
#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub(crate) struct WorldHeld {
    /// (owner, instance) of the Traffic / Pedestrian pools.
    pub(crate) traffic: Vec<(u64, u32)>,
    pub(crate) peds: Vec<(u64, u32)>,
    /// (owner, Player-slot instance ≥ 1) of the NPC / remote skaters (`npc_skaters.rs`).
    pub(crate) skaters: Vec<(u64, u32)>,
    /// (owner, instance) of the objects with their own instance (`mod_world`, doc 16 L3).
    pub(crate) own_traffic: Vec<(u64, u32)>,
    pub(crate) own_peds: Vec<(u64, u32)>,
    /// Owners inside the list radius waiting for an instance (all held by nearer ones): retail's
    /// pools, and the own-instance pools (`mod_world`, doc 16 M3).
    pub(crate) waiting_traffic: Vec<u64>,
    pub(crate) waiting_peds: Vec<u64>,
    pub(crate) own_waiting_traffic: Vec<u64>,
    pub(crate) own_waiting_peds: Vec<u64>,
    /// Running counts for the summary log: packets the world host posted, packets / Splice starts
    /// of the NPC skater instances, and speech lines started.
    pub(crate) posts: u64,
    pub(crate) npc_posts: u64,
    pub(crate) speech_lines: u64,
}

/// Retail's traffic list is cut at 40 m horizontal distance to the listener (vehicle record
/// `+152`, `sub_824B2A28`; recomp gap run G1: never above 39.994 m), and its 4 instances went to
/// the 4 nearest (horizontal) in 311 / 311 holder-seconds.
pub(crate) const TRAFFIC_LIST_RADIUS: f32 = 40.0;
/// Retail's ped list is sorted nearest first (3-D distance, `+148`) and cut at 50 m; the 15
/// instances = its first 15 (manager `sub_824F2890`, gap run G2).
pub(crate) const PED_LIST_RADIUS: f32 = 50.0;

pub(crate) fn register(app: &mut App) {
    app.init_resource::<WorldOwners>()
        .init_resource::<WorldHost>()
        .init_resource::<WorldHeld>()
        .add_systems(Update, frame_post.after(super::native::mixmap_tick));
}

fn apply(host: &mut WorldHost, rt: &mut skate_audio::runtime::Runtime, cmds: Vec<WorldCommand>) {
    for cmd in cmds {
        match cmd {
            WorldCommand::Post { owner, slot, class, words } => {
                let id = *host.classes.entry(class).or_insert_with(|| rt.eval.class_id(class).unwrap_or(usize::MAX));
                if id == usize::MAX {
                    continue;
                }
                if let Some(old) = host.nodes.remove(&(owner, slot)) {
                    rt.release(old);
                }
                let muted = host.rules.as_deref().is_some_and(|r| {
                    let (name, index) = super::mod_audio::world_slot(&slot);
                    r.mutes(&super::mod_audio::EventRow { kind: super::mod_audio::EventKind::Post, source: super::mod_audio::Source::World, class, slot: name, id: index, owner, hit: None })
                });
                if !muted {
                    host.nodes.insert((owner, slot), rt.post(id, &words));
                }
                host.posts += 1;
                if host.events.is_some() {
                    let (name, index) = super::mod_audio::world_slot(&slot);
                    super::mod_audio::record(&mut host.events, super::mod_audio::EventRow { kind: super::mod_audio::EventKind::Post, source: super::mod_audio::Source::World, class, slot: name, id: index, owner, hit: None });
                }
            }
            WorldCommand::Redeliver { owner, slot, words } => {
                if let Some(&node) = host.nodes.get(&(owner, slot)) {
                    rt.redeliver(node, &words);
                }
            }
            WorldCommand::Release { owner, slot } => {
                if let Some(node) = host.nodes.remove(&(owner, slot)) {
                    rt.release(node);
                    if host.events.is_some() {
                        let (name, index) = super::mod_audio::world_slot(&slot);
                        super::mod_audio::record(&mut host.events, super::mod_audio::EventRow { kind: super::mod_audio::EventKind::Release, source: super::mod_audio::Source::World, class: "", slot: name, id: index, owner, hit: None });
                    }
                }
            }
        }
    }
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn horizontal(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The candidates inside the list radius that hold no instance (sorted by id).
fn waiting(into: &mut Vec<u64>, candidates: &[(u64, f32)], pool: &Pool) {
    into.clear();
    into.extend(candidates.iter().filter(|c| c.1.is_finite() && pool.instance(c.0).is_none()).map(|c| c.0));
    into.sort_unstable();
    into.dedup();
}

/// A candidate inside the list radius keeps its distance; one outside never gets an instance.
fn within(d: f32, radius: f32) -> f32 {
    if d < radius { d } else { f32::INFINITY }
}

/// The seconds one MixMap evaluation covers (`native::mixmap_frame`: the console cadence, 1/30).
pub(super) fn evaluation_dt() -> f32 {
    CONSOLE_DT
}

impl WorldHost {
    /// The world generator's state (`seed.rs`, doc 16 L5).
    pub(crate) fn rng_state(&self) -> u32 {
        self.rng.0
    }
    pub(crate) fn set_rng_state(&mut self, state: u32) {
        self.rng.0 = state;
    }

    /// A runtime tuning write changed the world or player tuning (`tuning.rs`): the cached copies
    /// follow (only once the host has read them; before, it reads the new values itself).
    pub(crate) fn retune(&mut self, library: &super::Library, player: Option<&skate_audio::player::tuning::PlayerTuning>) {
        if self.banks.is_some() {
            self.ped_tuning = Some(library.world_tuning().ped_footsteps());
            self.object_tuning = library.world_tuning().ped_objects();
            if let Some(p) = player {
                self.player_tuning = Some(p.clone());
            }
        }
    }

    /// The map changed (`Native::map_epoch`, bumped by `unload_map_banks`), or the host runs for
    /// the first time: the unload destroyed every instance of the world banks, so every held node
    /// is released (harmless on a dead node; it frees them), the pools forget their holders, the
    /// held instances' 3DObjPos blocks go inactive, the ped Splice steps stop, and the per-owner
    /// objects are dropped. An owner that is still published is claimed again and posts afresh.
    /// The banks reload at the next owner (`ensure_bank`, through the prefetch when expected).
    /// The pools take the MixMap's instance counts (`Native::world`).
    fn reset(&mut self, native: &mut Native, mixmap: Option<&mut skate_audio::mixmap::MixMap>, pools: (usize, usize)) {
        self.epoch = Some(native.map_epoch);
        let traffic = self.traffic.clear();
        let peds = self.peds.clear();
        let shared = &native.shared;
        if !self.nodes.is_empty() || !self.ped_objects.is_empty() {
            if let Ok(mut runtime) = super::timing::lock(shared, &super::timing::GAME_LOCK) {
                let rt = &mut *runtime;
                for (owner, mut o) in std::mem::take(&mut self.ped_objects) {
                    let _ = o.release(owner, &mut rt.splice_host());
                }
                for (_, node) in self.nodes.drain() {
                    rt.release(node);
                }
            }
        }
        self.ped_objects.clear();
        self.vehicles.clear();
        self.nodes.clear();
        if let Some(m) = mixmap {
            let l = Listener::default();
            for (_, g) in traffic {
                Positions::new(&[keys::traffic_pos(g as u32, 1), keys::traffic_pos(g as u32, 2), keys::traffic_pos(g as u32, 3)]).deactivate(m, &l);
            }
            for (_, g) in peds {
                Positions::new(&[keys::ped_pos(g as u32)]).deactivate(m, &l);
            }
        }
        self.pass = None;
        if self.traffic.len() != pools.0 {
            self.traffic = Pool::new(pools.0);
        }
        if self.peds.len() != pools.1 {
            self.peds = Pool::new(pools.1);
        }
        self.banks = None;
        self.last_camera = None;
        self.speech_requests.clear();
        self.waiting.0.clear();
        self.waiting.1.clear();
    }

    /// A pass began in `pre` and waits for `post`.
    pub(crate) fn has_pass(&self) -> bool {
        self.pass.is_some()
    }

    pub(crate) fn held(&self) -> (Vec<(u64, u32)>, Vec<(u64, u32)>) {
        (self.traffic.holders().map(|(g, o)| (o, g as u32)).collect(), self.peds.holders().map(|(g, o)| (o, g as u32)).collect())
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn frame(
    native: Option<ResMut<Native>>,
    owners: Res<WorldOwners>,
    mut host: ResMut<WorldHost>,
    mut held: ResMut<WorldHeld>,
    library: Option<Res<Library>>,
    cues: Res<super::skate_events::Cues>,
    mut speech: ResMut<super::world_speech::WorldSpeech>,
    listener: Query<&GlobalTransform, With<super::GameAudioListener>>,
) {
    // Inert: nothing published, expected, held or prefetched.
    let idle = owners.vehicles.is_empty() && owners.peds.is_empty() && host.vehicles.is_empty() && host.ped_objects.is_empty();
    if idle && !owners.expected && host.prefetch.requested.is_empty() {
        return;
    }
    let (Some(mut native), Some(library)) = (native, library) else { return };
    if !requested() {
        return;
    }
    let camera = listener.single().ok().map(|t| (t.translation().to_array(), t.forward().as_vec3().to_array()));
    let calls = native.pending.map_or(0, |p| p.calls);
    pre(&mut host, &owners, &mut native, &library, camera, &cues.riding.audio, calls);
    if !host.speech_requests.is_empty() {
        speech.peds.append(&mut host.speech_requests);
    }
    // The speaking peds' positions (the speech echo's delay reads the camera distance).
    if !speech.ped_positions.is_empty() || !owners.peds.is_empty() {
        speech.ped_positions.clear();
        for (id, _) in host.peds.holders().map(|(g, o)| (o, g)) {
            if let Some(p) = owners.peds.get(&id) {
                speech.ped_positions.insert(id, p.position);
            }
        }
    }
    let (traffic, peds) = host.held();
    if held.traffic != traffic || held.peds != peds {
        held.traffic = traffic;
        held.peds = peds;
    }
    if held.waiting_traffic != host.waiting.0 || held.waiting_peds != host.waiting.1 {
        held.waiting_traffic.clone_from(&host.waiting.0);
        held.waiting_peds.clone_from(&host.waiting.1);
    }
    if held.posts != host.posts {
        held.posts = host.posts;
    }
}

/// The update after the MixMap ticks (`native::mixmap_tick`): [`post`].
pub(super) fn frame_post(native: Option<ResMut<Native>>, owners: Res<WorldOwners>, mut host: ResMut<WorldHost>) {
    if host.pass.is_none() {
        return;
    }
    let Some(mut native) = native else { return };
    post(&mut host, &owners, &mut native);
}

/// The host's first half of a frame, before the MixMap ticks (retail's process): the prefetch, the
/// map-change reset, the banks, and with a pass this frame (`calls` console evaluations, from
/// `Native::pending`) the instance assignment, the 3DObjPos blocks and inputs, and the objects'
/// process (posts, Splice steps, speech requests). [`post`] runs the update after the ticks.
/// `camera` = the listener (position, forward); `local` = the local player's audio state (the
/// followed point).
pub(crate) fn pre(host: &mut WorldHost, owners: &WorldOwners, native: &mut Native, library: &Library, camera: Option<([f32; 3], [f32; 3])>, local: &skate_audio::player::AudioState, calls: usize) {
    // The game's MixMap and its instance pools (the more-audible setting); `mod_world` runs the
    // same host on a private MixMap (doc 16 L3).
    let pools = (native.world.traffic, native.world.peds);
    let mut m = native.mixmap.take();
    pre_in(host, owners, native, m.as_mut(), pools, library, camera, local, calls);
    native.mixmap = m;
}

/// [`pre`] on a given MixMap with given instance pools (traffic, peds).
#[allow(clippy::too_many_arguments)]
pub(crate) fn pre_in(host: &mut WorldHost, owners: &WorldOwners, native: &mut Native, mut mix: Option<&mut skate_audio::mixmap::MixMap>, pools: (usize, usize), library: &Library, camera: Option<([f32; 3], [f32; 3])>, local: &skate_audio::player::AudioState, calls: usize) {
    if prefetch_on() {
        prefetch_world_banks(&mut host.prefetch, native, library, owners.expected);
    }
    if host.epoch != Some(native.map_epoch) {
        host.reset(native, mix.as_deref_mut(), pools);
    }
    let idle = owners.vehicles.is_empty() && owners.peds.is_empty() && host.vehicles.is_empty() && host.ped_objects.is_empty();
    if idle {
        host.waiting.0.clear();
        host.waiting.1.clear();
        return;
    }
    // The world banks (cheap when loaded; a map change unloads them, `native::unload_map_banks`).
    let mut missing = false;
    for stem in TRAFFIC_BANKS.iter().chain(PED_BANKS) {
        if let Err(e) = native.ensure_bank(library, stem) {
            if host.banks.is_none() {
                warn!("Game audio: world sources: {e} (rerun setup to refresh the audio)");
            }
            missing = true;
        }
    }
    if host.banks.is_none() {
        host.ped_tuning = Some(library.world_tuning().ped_footsteps());
        host.object_tuning = library.world_tuning().ped_objects();
        host.player_tuning = native.player.as_ref().map(|p| p.tuning.clone());
        info!("AUDIO_WORLD on: {} vehicles, {} peds published", owners.vehicles.len(), owners.peds.len());
    }
    host.banks = Some(!missing);
    // The phone ring's Splice bank (once: Splice banks stay loaded across maps).
    if host.ring_bank.is_none() && !owners.peds.is_empty() {
        let stem = host.object_tuning.ring_bank.clone();
        let loaded = match library.splice_bank(&stem) {
            Some((bank, pcm)) => match super::timing::lock(&native.shared, &super::timing::GAME_LOCK) {
                Ok(mut runtime) => {
                    let rt = &mut *runtime;
                    rt.splice.load_bank(&stem, bank, pcm, &mut rt.mixer);
                    true
                }
                Err(_) => false,
            },
            None => {
                warn!("AUDIO_WORLD {stem} has no patch tree in this install: phone calls answer without the ring (rerun setup)");
                false
            }
        };
        host.ring_bank = Some(loaded);
    }
    let Some((cam, view)) = camera else { return };
    if calls == 0 {
        return;
    }
    let native = &mut *native;
    let Some(m) = mix else { return };
    let dt = evaluation_dt() * calls.min(4) as f32;
    let cam_velocity = host.last_camera.filter(|l| l.1 == native.cuts).map_or([0.0; 3], |(last, _)| std::array::from_fn(|i| (cam[i] - last[i]) / dt));
    host.last_camera = Some((cam, native.cuts));
    host.pass = Some((cam, dt));
    let s = local;
    let l = Listener {
        camera: cam,
        view,
        camera_velocity: cam_velocity,
        followed: s.com_position,
        facing: s.com_velocity,
        followed_velocity: s.com_velocity,
    };
    let Ok(mut runtime) = super::timing::lock(&native.shared, &super::timing::GAME_LOCK) else { return };
    let rt = &mut *runtime;
    let ped_tuning = host.ped_tuning.take().unwrap_or_default();

    // Instances: the nearest N inside retail's list radii (traffic: horizontal, 40 m; peds: 3-D,
    // 50 m). `owners::Pool`.
    let traffic_candidates: Vec<(u64, f32)> = owners.vehicles.iter().map(|(&id, v)| (id, within(horizontal(v.position, cam), TRAFFIC_LIST_RADIUS))).collect();
    let ped_candidates: Vec<(u64, f32)> = owners.peds.iter().map(|(&id, p)| (id, within(distance(p.position, cam), PED_LIST_RADIUS))).collect();
    let traffic = host.traffic.assign(&traffic_candidates);
    let peds = host.peds.assign(&ped_candidates);
    waiting(&mut host.waiting.0, &traffic_candidates, &host.traffic);
    waiting(&mut host.waiting.1, &ped_candidates, &host.peds);
    for (owner, _) in traffic.released {
        if let Some((mut vehicle, mut pos)) = host.vehicles.remove(&owner) {
            pos.deactivate(m, &l);
            let cmds = vehicle.release(owner);
            apply(host, rt, cmds);
        }
    }
    for (owner, _) in peds.released {
        if let Some(mut o) = host.ped_objects.remove(&owner) {
            o.pos.deactivate(m, &l);
            let cmds = o.release(owner, &mut rt.splice_host());
            apply(host, rt, cmds);
        }
    }
    for (owner, g) in traffic.claimed {
        let g = g as u32;
        host.vehicles.insert(owner, (Vehicle::default(), Positions::new(&[keys::traffic_pos(g, 1), keys::traffic_pos(g, 2), keys::traffic_pos(g, 3)])));
    }
    for (owner, g) in peds.claimed {
        host.ped_objects.insert(owner, PedObjects::new(g as u32));
    }

    // Process: the 3DObjPos blocks and inputs for this pass's evaluations, then the posts.
    let holders: Vec<(usize, u64)> = host.traffic.holders().collect();
    for (g, owner) in holders {
        let Some(v) = owners.vehicles.get(&owner) else { continue };
        let Some((mut vehicle, mut pos)) = host.vehicles.remove(&owner) else { continue };
        // The record's body / front / rear points (`sub_824B2A28`) and TrafficCarPhysics.in0.
        let pts = skate_audio::world::traffic::record_points(v);
        pos.write(m, &l, &[Some((pts[0], v.velocity)), Some((pts[1], v.velocity)), Some((pts[2], v.velocity))]);
        vehicle.write_inputs(g as u32, v, m, cam_velocity, dt);
        let cmds = vehicle.process(owner, v, &mut host.rng);
        apply(host, rt, cmds);
        host.vehicles.insert(owner, (vehicle, pos));
    }
    let holders: Vec<(usize, u64)> = host.peds.holders().collect();
    for (_, owner) in holders {
        let Some(p) = owners.peds.get(&owner) else { continue };
        let Some(mut o) = host.ped_objects.remove(&owner) else { continue };
        o.pos.write(m, &l, &[Some((p.position, p.velocity))]);
        // Retail's object order in the Pedestrian slot: Speech, SFX, BodyFall, Tazer. The Splice
        // starts (ring, foot plants, body falls) are recorded for mods (observe only).
        let request = o.speech.process(owner, p, dt, owners.photo_flag, &host.object_tuning, &mut super::mod_audio::Observed::new(&mut rt.splice_host(), &mut host.events, super::mod_audio::Source::World, owner).rules(host.rules.as_deref()).slot("ring"));
        if let Some(r) = request {
            host.speech_requests.push(super::world_speech::PedRequest { request: r, voice: p.voice, speaker: p.speaker, level: p.level_select });
        } else if p.speech_value == 49 && o.speech.ring.is_none() && host.ring_bank == Some(false) && o.ring_fallback != Some(p.speech_value) {
            // No ring bank in the install: answer at once (missing-data fallback).
            host.speech_requests.push(super::world_speech::PedRequest {
                request: skate_audio::world::peds::SpeechRequest { owner, value: host.object_tuning.ring_answer, flag: PedSpeech::flag(p) },
                voice: p.voice,
                speaker: p.speaker,
                level: p.level_select,
            });
        }
        o.ring_fallback = (p.speech_value == 49).then_some(49);
        let mut cmds = o.sfx.process(owner, p, &ped_tuning, &mut super::mod_audio::Observed::new(&mut rt.splice_host(), &mut host.events, super::mod_audio::Source::World, owner).rules(host.rules.as_deref()), dt);
        o.body_fall.process(p, &host.object_tuning, &mut super::mod_audio::Observed::new(&mut rt.splice_host(), &mut host.events, super::mod_audio::Source::World, owner).rules(host.rules.as_deref()).slot("body_fall"));
        cmds.extend(o.tazer.process(owner, p, local.global_224));
        apply(host, rt, cmds);
        host.ped_objects.insert(owner, o);
    }
    host.ped_tuning = Some(ped_tuning);
}

/// The host's second half, after the MixMap ticks (retail's update): every held object's packets
/// from this pass's outputs. Nothing without a pass ([`pre`]).
pub(crate) fn post(host: &mut WorldHost, owners: &WorldOwners, native: &mut Native) {
    let mut m = native.mixmap.take();
    post_in(host, owners, native, m.as_mut());
    native.mixmap = m;
}

/// [`post`] on a given MixMap (`mod_world`, doc 16 L3).
pub(crate) fn post_in(host: &mut WorldHost, owners: &WorldOwners, native: &mut Native, mix: Option<&mut skate_audio::mixmap::MixMap>) {
    let Some((cam, dt)) = host.pass.take() else { return };
    let native = &mut *native;
    let Some(m) = mix else { return };
    let Ok(mut runtime) = super::timing::lock(&native.shared, &super::timing::GAME_LOCK) else { return };
    let rt = &mut *runtime;
    // Taken out for the frame (no per-frame clone) and put back at the end.
    let ped_tuning = host.ped_tuning.take().unwrap_or_default();
    let player_tuning = host.player_tuning.take().unwrap_or_default();
    let holders: Vec<(usize, u64)> = host.traffic.holders().collect();
    for (g, owner) in holders {
        let Some(v) = owners.vehicles.get(&owner) else { continue };
        let Some((mut vehicle, pos)) = host.vehicles.remove(&owner) else { continue };
        let cmds = vehicle.update(owner, g as u32, v, m, cam, dt);
        apply(host, rt, cmds);
        host.vehicles.insert(owner, (vehicle, pos));
    }
    let holders: Vec<(usize, u64)> = host.peds.holders().collect();
    for (g, owner) in holders {
        let Some(p) = owners.peds.get(&owner) else { continue };
        let Some(mut o) = host.ped_objects.remove(&owner) else { continue };
        let g = g as u32;
        if o.speech.ring.is_some() {
            let out = OutputsSnapshot::take(m, keys::ped_speech(g), &skate_audio::world::speech_player::PED_FILTERS);
            let main = out.level(skate_audio::world::speech_player::ped_level_ids(p.level_select, 0, false).0);
            if let Some(r) = o.speech.update(owner, p, main, &out, dt, &host.object_tuning, &mut rt.splice_host()) {
                host.speech_requests.push(super::world_speech::PedRequest { request: r, voice: p.voice, speaker: p.speaker, level: p.level_select });
            }
        }
        let out = OutputsSnapshot::take(m, keys::ped_sfx(g), &[7, 8]);
        let mut cmds = o.sfx.update(owner, p, &ped_tuning, &player_tuning, &out, &mut rt.splice_host(), dt);
        if o.body_fall.sounding() {
            o.body_fall.update(&OutputsSnapshot::take(m, keys::ped_body_fall(g), &[]), dt, &mut rt.splice_host());
        } else {
            o.body_fall.latch_env(m.level(keys::ped_body_fall(g), 7));
        }
        if o.tazer.packet.is_some() {
            cmds.extend(o.tazer.update(owner, &OutputsSnapshot::take(m, keys::ped_tazer(g), &[])));
        }
        apply(host, rt, cmds);
        host.ped_objects.insert(owner, o);
    }
    host.ped_tuning = Some(ped_tuning);
    host.player_tuning = Some(player_tuning);
}

/// One whole pass for tests and tools: [`pre`] with one console evaluation, the tick, [`post`] (the
/// caller sets the MixMap's globals first, as `native::mixmap_frame` does).
#[cfg(test)]
pub(crate) fn run(host: &mut WorldHost, owners: &WorldOwners, native: &mut Native, library: &Library, camera: Option<([f32; 3], [f32; 3])>, local: &skate_audio::player::AudioState) {
    pre(host, owners, native, library, camera, local, 1);
    if host.pass.is_some()
        && let Some(m) = native.mixmap.as_mut()
    {
        m.tick(CONSOLE_DT);
    }
    post(host, owners, native);
}

#[cfg(test)]
#[path = "world_oneshot_tests.rs"]
mod oneshot_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_tuning_reads_the_setup_export() {
        let json = r#"{"traffic_engine": {"c04_taxi01": {"idle_rpm": 1500, "max_rpm": 3500, "patch": 4, "wobble_limit": 8,
            "wobble_rate": 4, "rise": 0.5, "fall": 2, "slew": 2000, "gear_speed": 7, "gears": 4, "rear_bias": 20000}},
            "ped_footsteps": {"speeds": [7.5, 2.5], "step_ids": [62, 63, 64], "tail": [32767, 7000, 25000], "eq_chain": 2}}"#;
        let t: WorldTuningJson = serde_json::from_str(json).unwrap();
        let e = t.engine("c04_taxi01").unwrap();
        assert_eq!((e.idle_rpm, e.max_rpm, e.patch, e.gears), (1500.0, 3500.0, 4, 4));
        assert!(t.engine("c99").is_none());
        assert_eq!(t.ped_footsteps(), PedFootstepTuning::default());
        let empty: WorldTuningJson = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.ped_footsteps(), PedFootstepTuning::default());
        assert_eq!(empty.vehicle_alarm(), None, "an install from before the export");
        let alarm: WorldTuningJson = serde_json::from_str(r#"{"vehicle_alarm": {"min_impact": 0.25, "seconds": 12}}"#).unwrap();
        let a = alarm.vehicle_alarm().unwrap();
        assert_eq!((a.enabled, a.min_impact, a.seconds), (true, 0.25, 12.0));
        let retail: WorldTuningJson = serde_json::from_str(r#"{"vehicle_alarm": {"min_impact": 0.1, "seconds": 8}}"#).unwrap();
        assert_eq!(retail.vehicle_alarm(), Some(crate::world_audio::AlarmTuning::default()), "the export = the engine's retail constants");
    }

    #[test]
    fn main_cast_speaker_repeat_times_come_from_setup_and_mods() {
        // Event 0 of the main cast's tuning: +24 = 20 s, +52 (slot 31) = 30 s, +56 (slot 30) = 10 s.
        let json = r#"{"speech_tuning": {"0": {"0": {"repeat": 20, "repeat_speaker_31": 30, "repeat_speaker_30": 10},
            "3": {"repeat": 25, "repeat_speaker_31": 30, "repeat_speaker_30": 10, "speaker_repeat": [[31, 5], [7, 2]]},
            "9": {"repeat": 10}}}}"#;
        let t: WorldTuningJson = serde_json::from_str(json).unwrap();
        let bank = t.speech_tuning_bank(0);
        assert_eq!((bank[&0].repeat, bank[&0].speaker_repeat.clone()), (20.0, vec![(31, 30.0), (30, 10.0)]));
        assert_eq!(bank[&3].speaker_repeat, vec![(31, 5.0), (7, 2.0), (31, 30.0), (30, 10.0)], "a mod's pairs first: they win");
        assert!(bank[&9].speaker_repeat.is_empty(), "an install from before the export: +24 for everyone");
    }

    #[test]
    fn no_owners_means_no_work() {
        // The default host holds nothing: `frame` returns before touching the runtime.
        let host = WorldHost::default();
        let owners = WorldOwners::default();
        assert!(owners.vehicles.is_empty() && owners.peds.is_empty() && host.vehicles.is_empty() && host.ped_objects.is_empty());
        assert_eq!(host.traffic.len(), 4);
        assert_eq!(host.peds.len(), 15);
        assert!(!owners.expected && host.prefetch.requested.is_empty(), "nothing expected: no prefetch either");
        assert!(host.waiting.0.is_empty() && host.waiting.1.is_empty());
    }

    /// Doc 16 M3: a full pool's nearest hold the instances; the others inside the list radius
    /// wait (out of reach is not waiting), and a waiting one takes the instance a nearer one frees.
    #[test]
    fn a_full_pool_leaves_the_farther_ones_in_reach_waiting() {
        let mut pool = Pool::new(2);
        let mut out = Vec::new();
        let candidates = [(1, 5.0), (2, 1.0), (3, 3.0), (4, f32::INFINITY)];
        pool.assign(&candidates);
        waiting(&mut out, &candidates, &pool);
        assert_eq!(out, vec![1], "2 and 3 hold, 1 waits, 4 is out of reach");
        let candidates = [(1, 5.0), (3, 3.0), (4, f32::INFINITY)];
        pool.assign(&candidates);
        waiting(&mut out, &candidates, &pool);
        assert!(out.is_empty() && pool.instance(1).is_some(), "2 went: 1 takes its instance");
    }

    /// The world banks on the prefetch worker (data-gated): nothing is asked for until the world
    /// is expected; then every world bank in the install is queued once, the worker's data equals
    /// the game thread's own load bit for bit, the first owner's `ensure_bank` decodes nothing on
    /// the calling thread, the runtime's random state and blocks are untouched by the requests, a
    /// map change (`unload_map_banks`) makes them ask again, and an unexpected world drops the
    /// unused ones.
    #[test]
    #[ignore = "needs the private install data"]
    fn world_banks_are_prefetched_when_expected_and_load_without_decoding() {
        use super::super::library::WAV_DECODES;
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
        let Ok(library) = Library::load(root) else { panic!("missing private data: no audio install") };
        let Ok(mut native) = Native::start(&library) else { panic!("missing private data: no AEMS install") };
        let stems: Vec<&str> = TRAFFIC_BANKS.iter().chain(PED_BANKS).copied().filter(|s| library.bank_source(s).is_ok()).collect();
        if stems.is_empty() {
            panic!("missing private data: no world banks in the install");
        }
        let snapshot = |n: &Native| {
            let rt = n.shared.lock().unwrap();
            (rt.eval.rng, rt.blocks)
        };
        let before = snapshot(&native);
        let mut p = WorldPrefetch::default();
        prefetch_world_banks(&mut p, &mut native, &library, false);
        assert_eq!(native.prefetch.stems().count(), 0, "not expected: nothing requested");
        prefetch_world_banks(&mut p, &mut native, &library, true);
        for stem in &stems {
            assert!(native.prefetch.contains(stem), "{stem} requested");
        }
        assert_eq!(native.prefetch.stems().count(), stems.len(), "only the installed world banks");
        assert_eq!(p.unavailable.len(), TRAFFIC_BANKS.len() + PED_BANKS.len() - stems.len());
        prefetch_world_banks(&mut p, &mut native, &library, true);
        assert_eq!(p.requested.len(), stems.len(), "asked for once");

        // Identity: the worker's bank and PCM against the game thread's own load.
        let mut q = super::super::native::prefetch::Prefetch::default();
        for stem in &stems {
            q.request(library.bank_source(stem).unwrap());
        }
        for stem in &stems {
            q.wait(stem);
            let (bank, pcm) = q.take(stem).expect("prefetched");
            let (want_bank, want_pcm) = library.bank_source(stem).unwrap().load().unwrap();
            assert_eq!(format!("{bank:?}"), format!("{want_bank:?}"), "{stem}: bank");
            assert_eq!(pcm.len(), want_pcm.len(), "{stem}: samples");
            for (a, b) in pcm.iter().zip(&want_pcm) {
                match (a, b) {
                    (None, None) => {}
                    (Some(a), Some(b)) => {
                        assert_eq!(a.rate, b.rate, "{stem}");
                        assert_eq!(a.channels.len(), b.channels.len(), "{stem}");
                        for (x, y) in a.channels.iter().zip(&b.channels) {
                            assert!(x.len() == y.len() && x.iter().zip(y).all(|(x, y)| x.to_bits() == y.to_bits()), "{stem}: pcm");
                        }
                    }
                    _ => panic!("{stem}: a sample present on one side only"),
                }
            }
        }

        // The first owner: `ensure_bank` takes the decoded banks.
        for stem in &stems {
            native.prefetch.wait(stem);
        }
        assert_eq!(snapshot(&native), before, "the requests never touch the runtime");
        let decodes = WAV_DECODES.with(|n| n.get());
        for stem in &stems {
            native.ensure_bank(&library, stem).unwrap();
            assert!(native.bank_loaded(stem));
        }
        assert_eq!(WAV_DECODES.with(|n| n.get()), decodes, "no decode on the game thread");
        assert_eq!(native.prefetch.stems().count(), 0, "all taken");
        prefetch_world_banks(&mut p, &mut native, &library, true);
        assert_eq!(native.prefetch.stems().count(), 0, "loaded banks are not asked for again");

        // Map change: the world banks are unloaded and asked for again.
        native.unload_map_banks();
        assert!(stems.iter().all(|s| !native.bank_loaded(s)));
        prefetch_world_banks(&mut p, &mut native, &library, true);
        assert_eq!(native.prefetch.stems().count(), stems.len(), "re-requested after the clear");
        // No longer expected: the unused ones go.
        prefetch_world_banks(&mut p, &mut native, &library, false);
        assert_eq!(native.prefetch.stems().count(), 0, "dropped");
        assert!(p.requested.is_empty());
        // Without the prefetch the same bank loads on the game thread (the old path, its decodes).
        let decodes = WAV_DECODES.with(|n| n.get());
        native.ensure_bank(&library, stems[0]).unwrap();
        assert_eq!(WAV_DECODES.with(|n| n.get()) - decodes, library.bank_pcm(stems[0]).len() as u64, "decoded here without a prefetch");
    }

    /// The traffic record against the recomp (data-gated; gap run G1 `gapg1_20261003_152507`, hook
    /// `VEHAUD`, ≤ 4 lines per vehicle per second): between two logged samples of one vehicle the
    /// port steps at the console's 1/30 s with the speed interpolated and the listener's velocity
    /// from its logged positions, starting from the first sample's RPM / `+176`, and must land on
    /// the next sample's TrafficEngine RPM (`obj+52`, the wobble's ±8 RPM and the sampling allowed)
    /// and on the record's `+176` relative speed (TrafficCarPhysics.in0's value). Prints the
    /// agreement.
    #[test]
    #[ignore = "needs the private install data and the recomp gap run G1"]
    fn traffic_rpm_and_relative_speed_follow_the_recomp() {
        let base = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let Ok(library) = Library::load(&base.join("assets")) else { panic!("missing private data: no audio install") };
        let sessions = std::env::var_os("SKATE_RECOMP_SESSIONS").filter(|v| !v.is_empty()).map(std::path::PathBuf::from);
        let Some(Ok(text)) = sessions.map(|s| std::fs::read_to_string(s.join("gapg1_20261003_152507/trace.tsv"))) else { panic!("missing private data: no gap run G1") };
        let v3 = |s: &str| -> [f32; 3] {
            let v: Vec<f32> = s.split(' ').filter_map(|x| x.parse().ok()).collect();
            [v[0], v[1], v[2]]
        };
        // (ms, obj, id, key, patch, rpm, pos, fwd, f144, speed, rel176, listener)
        let mut tracks: HashMap<(String, String, String), Vec<(f64, i32, f32, [f32; 3], f32, f32, [f32; 3])>> = HashMap::new();
        for line in text.lines() {
            let f: Vec<&str> = line.split('\t').collect();
            if f.first() != Some(&"VEHAUD") || f.len() != 17 {
                continue;
            }
            let speed: f32 = f[11].parse().unwrap_or(0.0);
            if f[5] == "0000000000000000" || speed <= 0.0 {
                continue;
            }
            let row = (f[1].parse().unwrap(), f[6].parse().unwrap(), f[7].parse().unwrap(), v3(f[9]), speed, f[15].parse().unwrap(), v3(f[16]));
            tracks.entry((f[2].to_owned(), f[4].to_owned(), f[5].to_owned())).or_default().push(row);
        }
        let record = |patch: i32| match patch {
            1 | 7 | 8 => "c01_family01",
            3 | 6 => "c03_sports01",
            4 => "c04_taxi01",
            _ => "c05_truck01",
        };
        struct Zero;
        impl skate_audio::player::Outputs for Zero {
            fn level(&self, _: usize) -> i32 {
                0
            }
            fn raw(&self, _: usize) -> i32 {
                0
            }
            fn pitch(&self, _: usize) -> i32 {
                4096
            }
        }
        let (mut rpm_ok, mut rel_ok, mut n, mut stale) = (0, 0, 0, 0);
        let (mut moving_ok, mut moving_n) = (0, 0);
        let mut worst: Vec<f32> = Vec::new();
        for rows in tracks.values() {
            let Some(engine) = library.world_tuning().engine(record(rows[0].1)) else { panic!("missing private data: no world tuning") };
            let mut e = skate_audio::world::traffic::Engine::default();
            let v0 = VehicleState { speed: rows[0].4, engine, ..Default::default() };
            e.process(1, &v0, &mut || 0u32);
            e.rpm = rows[0].2;
            let mut rel = rows[0].5;
            for w in rows.windows(2) {
                let (a, b) = (&w[0], &w[1]);
                let span = ((b.0 - a.0) / 1000.0) as f32;
                // A held vehicle that left the 40 m list keeps its frozen record (G1 "stale
                // records"): speed, RPM and +176 all unchanged.
                if a.4 == b.4 && a.2 == b.2 && a.5 == b.5 {
                    stale += 1;
                    continue;
                }
                if !(0.05..=0.6).contains(&span) {
                    e.rpm = b.2;
                    rel = b.5;
                    continue;
                }
                let steps = (span * 30.0).round().max(1.0) as usize;
                let lv: [f32; 3] = std::array::from_fn(|i| (b.6[i] - a.6[i]) / span);
                for k in 1..=steps {
                    let t = k as f32 / steps as f32;
                    let speed = a.4 + (b.4 - a.4) * t;
                    let v = VehicleState { speed, direction: a.3, engine, ..Default::default() };
                    e.update(1, &v, &Zero, [0.0; 3], 1.0 / 30.0, None);
                    skate_audio::world::traffic::relative_speed_word(&mut rel, &v, lv, span / steps as f32);
                }
                let d = (e.rpm - b.2).abs();
                worst.push(d);
                rpm_ok += usize::from(d <= 60.0);
                // `[listener+48]` is the listener record's own velocity (not logged): judged while the
                // listener stands; while it moves the difference of its logged positions stands in.
                if lv.iter().map(|x| x * x).sum::<f32>() < 0.25 {
                    n += 1;
                    rel_ok += usize::from((rel - b.5).abs() <= 0.5);
                } else {
                    moving_n += 1;
                    moving_ok += usize::from((rel - b.5).abs() <= 2.0);
                }
                if std::env::var_os("SKATE_VERIFY_VERBOSE").is_some() && (d > 60.0 || (rel - b.5).abs() > 0.5) {
                    eprintln!("  {:.1}s span {span:.3} speed {:.2}->{:.2} rpm {:.0}->{:.0} ours {:.0} | rel {:.2}->{:.2} ours {rel:.2} lv {:?}", b.0 / 1000.0, a.4, b.4, a.2, b.2, e.rpm, a.5, b.5, lv);
                }
                // Resynchronise (the log is the truth at each sample).
                e.rpm = b.2;
                rel = b.5;
            }
        }
        worst.sort_by(f32::total_cmp);
        let q = |p: f32| worst.get(((worst.len() as f32 - 1.0) * p) as usize).copied().unwrap_or(0.0);
        let pairs = worst.len();
        eprintln!(
            "G1: {} vehicle tracks, {pairs} live sample pairs ({stale} stale left out): RPM within 60 in {rpm_ok} ({:.1} %; |diff| p50 {:.1} p90 {:.1} max {:.1}); +176 within 0.5 m/s in {rel_ok} of {n} with the listener standing ({:.1} %), within 2 m/s in {moving_ok} of {moving_n} with it moving",
            tracks.len(),
            100.0 * rpm_ok as f32 / pairs.max(1) as f32,
            q(0.5),
            q(0.9),
            q(1.0),
            100.0 * rel_ok as f32 / n.max(1) as f32
        );
        assert!(pairs > 100 && n > 100, "enough sample pairs");
        assert!(rpm_ok * 10 >= pairs * 9, "the RPM model follows the recomp");
        assert!(rel_ok * 10 >= n * 9, "the relative speed follows the recomp");
    }

    /// The process / update move (spec §3.8, data-gated): the world host now writes its inputs and
    /// posts before the MixMap tick and updates after it (retail's order); before, both ran after
    /// the tick. Driving the same car past the camera both ways (the old order = tick, update,
    /// process; the new = process, tick, update) through the real MixMap must give the same
    /// TrafficEngine / TrafficSkids / TrafficHorn outputs shifted by exactly one console evaluation
    /// (the new pass's tick sees this frame's position, the old one the previous frame's) and
    /// nothing else.
    #[test]
    #[ignore = "needs the private install data"]
    fn process_before_the_tick_shifts_the_outputs_by_one_evaluation() {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
        let Ok(library) = Library::load(root) else { panic!("missing private data: no audio install") };
        let Some(engine) = library.world_tuning().engine("c04_taxi01") else { panic!("missing private data: no world tuning") };
        let local = skate_audio::player::AudioState::default();
        let camera = Some(([0.0, 1.5, 0.0], [0.0, 0.0, 1.0]));
        let frames = 90;
        let render = |old: bool| -> Vec<Vec<i32>> {
            let Ok(mut native) = Native::start(&library) else { panic!("missing private data: no AEMS install") };
            let mut host = WorldHost::default();
            let mut owners = WorldOwners::default();
            let mut out = Vec::new();
            if !old {
                // The old order's first pass ticked once before any input existed: the same history.
                let m = native.mixmap.as_mut().unwrap();
                for id in 1..=4 {
                    m.set_input(skate_audio::mixmap::keys::MASTER, id, 32767);
                }
                for id in [1, 2, 5] {
                    m.set_input(skate_audio::mixmap::keys::MUSIC, id, 32767);
                }
                m.set_input(skate_audio::mixmap::keys::REVERB, 5, 32767);
                m.tick(CONSOLE_DT);
            }
            for f in 0..frames {
                let z = -30.0 + f as f32 * 0.4;
                owners.vehicles.insert(7, VehicleState { position: [3.0, 0.5, z], velocity: [0.0, 0.0, 12.0], direction: [0.0, 0.0, 1.0], speed: 12.0, engine, ..Default::default() });
                {
                    let m = native.mixmap.as_mut().unwrap();
                    for id in 1..=4 {
                        m.set_input(skate_audio::mixmap::keys::MASTER, id, 32767);
                    }
                    for id in [1, 2, 5] {
                        m.set_input(skate_audio::mixmap::keys::MUSIC, id, 32767);
                    }
                    m.set_input(skate_audio::mixmap::keys::REVERB, 5, 32767);
                }
                if old {
                    // Before 2026-10-03: the tick, then the update and the process.
                    native.mixmap.as_mut().unwrap().tick(CONSOLE_DT);
                    host.pass = Some((camera.unwrap().0, CONSOLE_DT));
                    post(&mut host, &owners, &mut native);
                    pre(&mut host, &owners, &mut native, &library, camera, &local, 1);
                    host.pass = None;
                } else {
                    run(&mut host, &owners, &mut native, &library, camera, &local);
                }
                let m = native.mixmap.as_ref().unwrap();
                let mut row = Vec::new();
                for key in [keys::traffic_engine(0), keys::traffic_skids(0), keys::traffic_horn(0)] {
                    for id in 0..11 {
                        row.extend([m.level(key, id), m.raw(key, id), m.pitch_4096(key, id), m.filter_hz(key, id)]);
                    }
                }
                out.push(row);
            }
            out
        };
        let (old, new) = (render(true), render(false));
        let shifted = (0..frames - 1).filter(|&k| new[k] == old[k + 1]).count();
        let same = (0..frames).filter(|&k| new[k] == old[k]).count();
        for k in (0..frames - 1).filter(|&k| new[k] != old[k + 1]) {
            let diff: Vec<(usize, i32, i32)> = new[k].iter().zip(&old[k + 1]).enumerate().filter(|(_, (a, b))| a != b).map(|(i, (a, b))| (i, *a, *b)).collect();
            eprintln!("  evaluation {k}: {} fields differ (index = object * 44 + output * 4 + kind): {:?}", diff.len(), &diff[..diff.len().min(8)]);
        }
        eprintln!("process before the tick: new[k] == old[k + 1] on {shifted} of {} evaluations; unshifted equal on {same}", frames - 1);
        assert_eq!(shifted, frames - 1, "exactly a one-evaluation shift");
        assert!(same < frames - 1, "the outputs move (a car drives past)");
    }

    /// The map-change regression (spec §2.1, data-gated): a vehicle and a ped publish, their
    /// instances post and the C04 engine sounds; `unload_map_banks` destroys the world banks'
    /// instances; the same ids keep publishing and must post again (before the epoch reset the
    /// engine kept redelivering to its dead node and stayed silent for the rest of its life).
    /// Also: with no owners a map change costs nothing but the epoch bookkeeping.
    /// Audio event tags on the world host's real posts (R5, data-gated): a car's horn (horn
    /// state 2) and alarm (state 6), a ped's footsteps; the rows carry the owner.
    #[test]
    #[ignore = "needs the private install data"]
    fn world_event_tags_fire_on_real_posts() {
        use skate_audio::world::peds::PedState;
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
        let Ok(library) = Library::load(root) else { panic!("missing private data: no audio install") };
        let Ok(mut native) = Native::start(&library) else { panic!("missing private data: no AEMS install") };
        let Some(engine) = library.world_tuning().engine("c04_taxi01") else { panic!("missing private data: no world tuning") };
        let mut host = WorldHost { events: Some(Vec::new()), ..Default::default() };
        let mut owners = WorldOwners::default();
        let local = skate_audio::player::AudioState::default();
        let camera = Some(([0.0, 1.5, 0.0], [0.0, 0.0, 1.0]));
        let tags = super::super::mod_audio::Tags::default();
        let mut seen = std::collections::BTreeMap::<&str, Vec<u64>>::new();
        for f in 0..240usize {
            let horn = match f { 20..80 => 2, 140..200 => 6, _ => 0 };
            owners.vehicles.insert(7, VehicleState { position: [3.0, 0.5, 8.0], velocity: [0.0, 0.0, 0.0], speed: 0.0, engine, horn, ..Default::default() });
            owners.peds.insert(9, PedState { position: [1.5, 0.0, 4.0], velocity: [0.0, 0.0, 1.3], speed: 1.3, feet: [f % 20 < 10, f % 20 >= 10], class: 2, weight: 1, ..Default::default() });
            // A standing ped that tazes, then falls (the world-gaps one-shots).
            let fall = match f { 100..110 => 8.0, 160..170 => 10.0, _ => 0.0 };
            owners.peds.insert(11, PedState { position: [-1.5, 0.0, 4.0], class: 2, weight: 1, tazing: (30..90).contains(&f), body_fall: fall, ..Default::default() });
            let m = native.mixmap.as_mut().unwrap();
            for id in 1..=4 {
                m.set_input(skate_audio::mixmap::keys::MASTER, id, 32767);
            }
            for id in [1, 2, 5] {
                m.set_input(skate_audio::mixmap::keys::MUSIC, id, 32767);
            }
            m.set_input(skate_audio::mixmap::keys::REVERB, 5, 32767);
            run(&mut host, &owners, &mut native, &library, camera, &local);
            for r in host.events.as_mut().unwrap().drain(..) {
                if let Some(t) = tags.tag(&r) {
                    seen.entry(t).or_default().push(r.owner);
                }
            }
        }
        println!("world tags: {:?}", seen.iter().map(|(k, v)| (k, v.len())).collect::<Vec<_>>());
        assert!(seen.get("horn").is_some_and(|o| o.contains(&7)), "{seen:?}");
        assert!(seen.get("alarm").is_some_and(|o| o.contains(&7)), "{seen:?}");
        assert!(seen.get("footstep").is_some_and(|o| o.contains(&9)), "{seen:?}");
        assert!(seen.get("tazer").is_some_and(|o| o.contains(&11)), "{seen:?}");
        assert_eq!(seen.get("body_fall").map(Vec::as_slice), Some(&[11u64, 11][..]), "{seen:?}");
    }

    /// Rules at the world host's post site (data-gated): a car honks for 60 frames and a ped walks.
    /// Without rules the horn is posted; `mute` on the horn tag drops the post (no horn node, the
    /// ped's footsteps untouched) while subscribers still see the request; `replace` drops it and
    /// queues the rule's sound once (min_interval); `layer` keeps the post and queues the sound.
    #[test]
    #[ignore = "needs the private install data"]
    fn rules_mute_replace_and_layer_the_world_hosts_posts() {
        use skate_audio::world::peds::PedState;
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
        let Ok(library) = Library::load(root) else { panic!("missing private data: no audio install") };
        let Some(engine) = library.world_tuning().engine("c04_taxi01") else { panic!("missing private data: no world tuning") };
        let rule = |v: serde_json::Value| -> skate_mods::audio_rules::Rule { serde_json::from_value(v).unwrap() };
        let run_with = |rules: Option<std::sync::Arc<super::super::mod_rules::RuleSet>>| {
            let Ok(mut native) = Native::start(&library) else { panic!("missing private data: no AEMS install") };
            let mut host = WorldHost { events: Some(Vec::new()), rules: rules.clone(), ..Default::default() };
            let mut owners = WorldOwners::default();
            let local = skate_audio::player::AudioState::default();
            let camera = Some(([0.0, 1.5, 0.0], [0.0, 0.0, 1.0]));
            let tags = super::super::mod_audio::Tags::default();
            let (mut horn_frames, mut horn_rows, mut steps) = (0, 0, 0);
            for f in 0..120usize {
                let horn = if (20..80).contains(&f) { 2 } else { 0 };
                owners.vehicles.insert(7, VehicleState { position: [3.0, 0.5, 8.0], engine, horn, ..Default::default() });
                owners.peds.insert(9, PedState { position: [1.5, 0.0, 4.0], velocity: [0.0, 0.0, 1.3], speed: 1.3, feet: [f % 20 < 10, f % 20 >= 10], class: 2, weight: 1, ..Default::default() });
                let m = native.mixmap.as_mut().unwrap();
                for id in 1..=4 {
                    m.set_input(skate_audio::mixmap::keys::MASTER, id, 32767);
                }
                for id in [1, 2, 5] {
                    m.set_input(skate_audio::mixmap::keys::MUSIC, id, 32767);
                }
                m.set_input(skate_audio::mixmap::keys::REVERB, 5, 32767);
                run(&mut host, &owners, &mut native, &library, camera, &local);
                horn_frames += usize::from(host.nodes.contains_key(&(7, WorldSlot::Horn)));
                for r in host.events.as_mut().unwrap().drain(..) {
                    match tags.tag(&r) {
                        Some("horn") => horn_rows += 1,
                        Some("footstep") => steps += 1,
                        _ => {}
                    }
                }
            }
            let plays = rules.map(|r| r.take_plays(&mut 0).len()).unwrap_or(0);
            (horn_frames, horn_rows, steps, plays)
        };
        let plain = run_with(None);
        assert!(plain.0 > 0 && plain.1 > 0 && plain.2 > 0, "{plain:?}");
        let set = |r| super::super::mod_rules::RuleSet::for_test(&[("dev.a", "horn", r)], Default::default());
        let muted = run_with(Some(set(rule(serde_json::json!({"match": {"tag": "horn"}, "action": "mute"})))));
        assert_eq!(muted.0, 0, "no horn node while muted");
        assert_eq!((muted.1, muted.2), (plain.1, plain.2), "the requests are still reported; the steps untouched");
        let replaced = run_with(Some(set(rule(serde_json::json!({"match": {"tag": "horn"}, "action": "replace", "play": {"path": "honk.wav"}, "min_interval": 10})))));
        assert_eq!((replaced.0, replaced.3), (0, 1), "dropped, one sound (min_interval)");
        let layered = run_with(Some(set(rule(serde_json::json!({"match": {"source": "world", "slot": "horn"}, "action": "layer", "play": {"path": "honk.wav"}, "min_interval": 10})))));
        assert_eq!((layered.0, layered.3), (plain.0, 1), "kept, plus one sound");
    }

    #[test]
    #[ignore = "needs the private install data"]
    fn world_owners_post_again_after_a_map_change() {
        use skate_audio::world::peds::PedState;
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
        let Ok(library) = Library::load(root) else { panic!("missing private data: no audio install") };
        let Ok(mut native) = Native::start(&library) else { panic!("missing private data: no AEMS install") };
        let Some(engine) = library.world_tuning().engine("c04_taxi01") else { panic!("missing private data: no world tuning") };
        let mut host = WorldHost::default();
        let mut owners = WorldOwners::default();
        let local = skate_audio::player::AudioState::default();
        let camera = Some(([0.0, 1.5, 0.0], [0.0, 0.0, 1.0]));
        // Zero owners: the first run only takes the epoch.
        run(&mut host, &owners, &mut native, &library, camera, &local);
        assert_eq!(host.epoch, Some(native.map_epoch));
        assert!(host.nodes.is_empty() && host.vehicles.is_empty());
        let (car, ped) = (7u64, 9u64);
        let engine_voices = |native: &Native| {
            let Some(bank) = native.bank_id("C04_taxi01") else { return 0 };
            native.shared.lock().unwrap().mixer.snapshot().iter().filter(|v| v.bank == bank).count()
        };
        let step = |native: &mut Native, host: &mut WorldHost, owners: &mut WorldOwners, frames: usize| {
            let mut most = 0;
            for f in 0..frames {
                let z = 8.0 + (f % 60) as f32 * 0.2;
                owners.vehicles.insert(car, VehicleState { position: [3.0, 0.5, z], velocity: [0.0, 0.0, 12.0], speed: 12.0, engine, ..Default::default() });
                owners.peds.insert(ped, PedState { position: [1.5, 0.0, 4.0], velocity: [0.0, 0.0, 1.3], speed: 1.3, feet: [f % 20 < 10, f % 20 >= 10], class: 2, weight: 1, ..Default::default() });
                let m = native.mixmap.as_mut().unwrap();
                // As `native::mixmap_frame`: the category gains, then the tick.
                for id in 1..=4 {
                    m.set_input(skate_audio::mixmap::keys::MASTER, id, 32767);
                }
                for id in [1, 2, 5] {
                    m.set_input(skate_audio::mixmap::keys::MUSIC, id, 32767);
                }
                m.set_input(skate_audio::mixmap::keys::REVERB, 5, 32767);
                // `run` = the host's process, the tick, its update (as `mixmap_frame` / `mixmap_tick`).
                run(host, owners, native, &library, camera, &local);
                {
                    let mut rt = native.shared.lock().unwrap();
                    for _ in 0..7 {
                        rt.render_block();
                    }
                }
                most = most.max(engine_voices(native));
            }
            most
        };
        assert!(step(&mut native, &mut host, &mut owners, 40) > 0, "the engine sounds before the map change");
        assert!(host.nodes.contains_key(&(car, WorldSlot::Engine)));
        let before = host.nodes[&(car, WorldSlot::Engine)];
        native.unload_map_banks();
        assert!(!native.bank_loaded("C04_taxi01"));
        assert!(step(&mut native, &mut host, &mut owners, 40) > 0, "the same owner sounds again after the map change");
        assert_ne!(host.nodes[&(car, WorldSlot::Engine)], before, "posted afresh, not redelivered to the dead node");
        assert_eq!(host.peds.instance(ped), Some(0), "the ped holds an instance again");
        assert!(host.nodes.keys().any(|k| k.0 == ped), "the ped's footsteps posted again");
        // The owners go: everything is released.
        owners.vehicles.clear();
        owners.peds.clear();
        run(&mut host, &owners, &mut native, &library, camera, &local);
        assert!(host.nodes.is_empty() && host.vehicles.is_empty() && host.ped_objects.is_empty());
        // A map change with nothing held.
        native.unload_map_banks();
        run(&mut host, &owners, &mut native, &library, camera, &local);
        assert_eq!(host.epoch, Some(native.map_epoch));
    }
}
