//! **Hooking up world audio** (engine-facing surface; doc `docs/hails-additions/15-world-audio.md`).
//!
//! The game has no living world yet (no traffic, pedestrians or AI skaters). Retail's audio for
//! them is ported (`skate_audio::world`, hosted by `game_audio::{world_sources, npc_skaters}`) and
//! waits for publishers. A future engine system (or a Lua mod, `sdk.world_audio`) only adds
//! components to its own entities and sends a few messages; it never sees AEMS, MixMap keys,
//! packets or banks:
//!
//! - a vehicle: [`TrafficAudio`] (engine record name, horn, skid; speed / acceleration optional);
//! - a pedestrian: [`PedAudio`] (voice, shoe class, foot plants, materials, speech value);
//! - an NPC (AI) skater or a remote multiplayer player: [`NpcSkaterAudio`] with an
//!   [`AudioState`] filled like the local player's (`skate_events::skater_audio_state` for a
//!   skater simulated with the player's physics, [`AudioState::rolling`] for anything else);
//! - optional [`AudioVelocity`] for objects that teleport or are kinematic proxies (Doppler and
//!   the 3-D rates read the velocity; by default it is the transform's change per frame).
//!
//! Position and heading come from the entity's `GlobalTransform` (heading = its +Z axis, the
//! game's forward; retail's vehicle `+112` is the world matrix's forward row, recomp gap run G1).
//! **Lifetime = the entity:** insert the component to publish, despawn or remove it to release.
//! Ids are `Entity::to_bits()`, so a reused index is a new owner.
//!
//! **The audio decides who is audible** with retail's limits, applied by the hosts: the 4 nearest
//! vehicles within 40 m (horizontal), the 15 nearest peds within 50 m (footsteps for the 3
//! nearest), one NPC skater within 30 m of the camera (the first in list order, held until 30 m).
//! The opt-in non-retail "more audible" setting raises these to 8 / 24 / 3. The bridge inserts
//! [`WorldAudioInstance`] on the entities that hold an instance, so an engine system can skip
//! per-frame audio work (an NPC's `AudioState`) for the others.
//!
//! Messages: [`PedSpeechEvent`] (a state graph's `SendSpeechEvent`), [`PedTazerEvent`] (a zap:
//! the tazer burst), [`PedBodyFallEvent`] (a knock-down animation's `BodyFallType` key),
//! [`NpcSkaterReactionEvent`] (an NPC skater saw a slam / trick or crashed: its own speech),
//! [`VehicleHorn`] (hold a horn kind for the caller's time), [`VehicleAlarm`] (retail's 8 s alarm),
//! [`VehicleImpact`] (something touched a vehicle: retail's car alarm rule sets a [`VehicleParked`]
//! car's alarm off, tuned by [`CarAlarmRule`], observed through [`VehicleAlarmStarted`]),
//! [`AnnouncerSpeechEvent`] (the announcer channel: a challenge's commentary; set
//! [`LivingWorldAudio::announcer`] while a challenge with an announcer runs). Set [`LivingWorldAudio`]
//! `expected` at map load when the system will publish, so the world banks decode ahead of need.
//!
//! Everything stays inert when nothing is published. `SKATE_AEMS_WORLD=0` /
//! `SKATE_AEMS_NPC_SKATERS=0` turn the hosts off.
// An API for systems the engine does not have yet: not every item has a caller in the game.
#![allow(dead_code)]

use bevy::prelude::*;

pub use skate_audio::player::AudioState;
pub use skate_audio::player::state::LiteSkater;
/// An NPC skater's reaction bytes (what the AI sets per frame; `skate_audio::world::skater_speech`).
pub use skate_audio::world::skater_speech::Reactions as SkaterReactions;

/// A vehicle's horn state (`+156`): what the traffic AI decides. `Honk(1..=5)` = the horn kind
/// (which kind a model uses is the AI's; retail packs it per vehicle), `Alarm` = the car alarm
/// (state 6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HornState {
    #[default]
    None,
    Honk(u8),
    Alarm,
}

impl HornState {
    /// The record word (0 none, 1..=5 kind, 6 alarm).
    pub fn word(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Honk(k) => i32::from(k.clamp(1, 5)),
            Self::Alarm => 6,
        }
    }

    /// From the record word (anything outside 1..=6 = none).
    pub fn from_word(word: i32) -> Self {
        match word {
            1..=5 => Self::Honk(word as u8),
            6 => Self::Alarm,
            _ => Self::None,
        }
    }
}

/// A traffic vehicle's audio (retail `SFXObj_TrafficEngine` / `TrafficHorn` / `TrafficSkids`;
/// the vehicle audio record `+144..+168`, `sub_824B2A28`).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct TrafficAudio {
    /// The vehicle's `aud_traffic_engine` record (`+168`), by record name (`c01_family01` …) or by
    /// the living-world model (`taxi01`, `sedan02`, `suv`, …), which the setup export maps through
    /// retail's attribute chain (entity → vehicle spec → engine record, recomp gap run G1): sedans
    /// / hatchback → `c01_family01`, sports / muscle → `c03_sports01`, taxi / patrol →
    /// `c04_taxi01`, SUV / pickup / minivan → `c05_truck01`. The patch override picks c06 / c07 /
    /// c08 itself. `c00_heavy01` exists but retail never uses it. Unknown or `default` = patch 2,
    /// silent (logged once).
    pub engine: String,
    /// `+148` speed (m/s, ≥ 0). `None` = |velocity|.
    pub speed: Option<f32>,
    /// `+144`: the driver's signed acceleration in m/s² (gap run G1: −15.6 through a hard stop,
    /// up to +3 pulling away, 0 cruising), × 3000 into the engine / skid words. `None` = derived
    /// from the speed change.
    pub load: Option<f32>,
    /// `+156`.
    pub horn: HornState,
    /// `+160`: the tyres skid.
    pub skidding: bool,
}

impl TrafficAudio {
    pub fn new(engine: impl Into<String>) -> Self {
        Self { engine: engine.into(), speed: None, load: None, horn: HornState::None, skidding: false }
    }
}

/// A pedestrian's audio (retail `SFXObj_PedestrianSFX` footsteps and `SFXObj_PedestrianSpeech`
/// requests; the ped audio state S, gap run G2).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct PedAudio {
    /// The model = the speech voice id 41–96 (`S+84`; the clip names' voice). Its
    /// `aud_characteristics` record (setup export `world_tuning.ped_models`) gives the shoe class,
    /// the security kind, the speech type / variant / gender words and the far threshold.
    /// None = no speech, and the defaults below.
    pub voice: Option<u32>,
    /// `S+132` shoe class 2..=5. `None` = the model's (2 for most models and without a voice; 1 =
    /// silent and used by no model).
    pub shoe_class: Option<u8>,
    /// `[obj+28]+144` (1..=5): dynamic per ped in retail (1 beyond ~12 m, 2–5 near; meaning open).
    pub weight: u8,
    /// `S+96 == 64`: a security guard (the close-range footstep levels and the guard's speech
    /// levels). `None` = the model's type bit.
    pub close_range: Option<bool>,
    /// `S+74` / `S+73`: the walk animation's foot plants (A, B).
    pub feet_down: [bool; 2],
    /// `S+140` / `S+144`: the material under each foot (audio surface material). `None` = 0
    /// (retail's pavements read 0 in every recomp line).
    pub foot_materials: Option<[u32; 2]>,
    /// `S+68` footsteps on. `None` = retail's rule: the 3 nearest peds of the list.
    pub footsteps_on: Option<bool>,
    /// `S+148` / `S+156`: distance to the listener and the model's far threshold (the `_f` lines
    /// beyond it). `None` = retail: the 3-D distance and the model's threshold (20 m for regular
    /// peds, 30 for pros).
    pub speech_distance: Option<(f32, f32)>,
    /// `S+136`: the speech value the ped's state graph last sent. Engine systems normally send
    /// [`PedSpeechEvent`] instead; the footstep packets read it too (jump 4/5, collision 6/7).
    pub speech_value: i32,
    /// `S+80`: the ped is tazing (retail: while its state graph's `TazeEntity` state runs):
    /// SFXObj_Tazer holds the `c_tazer` packet (the zap burst). [`PedTazerEvent`] holds it for a
    /// time instead.
    pub tazing: bool,
    /// `S+76`: the ped animation's `BodyFallType` channel (0 = none). Each change to a non-zero
    /// value starts one PedBodyFall sound (8, 9 and any other value have their own sounds).
    /// [`PedBodyFallEvent`] sends one key instead.
    pub body_fall: f32,
}

impl Default for PedAudio {
    fn default() -> Self {
        Self {
            voice: None,
            shoe_class: None,
            weight: 1,
            close_range: None,
            feet_down: [false; 2],
            foot_materials: None,
            footsteps_on: None,
            speech_distance: None,
            speech_value: 0,
            tazing: false,
            body_fall: 0.0,
        }
    }
}

/// An NPC (AI) skater's board audio (the MixMap Player slot's second instance) — or a remote
/// multiplayer player's (non-retail extension, user decision 2026-10-03: another real player
/// nearby takes the same instance; remote players come first in the list order).
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct NpcSkaterAudio {
    /// The skater list position (retail walks its list in order; spawn order is fine). 0 = the
    /// bridge numbers it by spawn order.
    pub list_order: u32,
    /// The full audio state of this frame, filled like the local player's. Needed while the
    /// skater is within 35 m of the camera (a claim at 30 m reads it in the same pass); `None`
    /// = not published this frame.
    pub state: Option<AudioState>,
    /// A remote multiplayer player (sorted before the NPCs).
    pub remote: bool,
    /// The skater's speech voice (its `aud_characteristics` model: the AI skaters' 89–96 speak on
    /// the living-world channel, the pros 1–29 and the special cast 30–38 on the main cast). Its
    /// bail grunt (event 8206 / 115) and its reactions say lines of it. None = no speech.
    pub voice: Option<u32>,
    /// The skater record's reaction bytes this frame (a slam or trick it saw and by whom, its own
    /// crash, the chase flag): its speech process says the matching lines. Held while set, as the
    /// AI holds them; [`NpcSkaterReactionEvent`] raises one for a console frame instead.
    pub reactions: SkaterReactions,
    /// The conditioner's loose-board state (state `+780`: 0 none, 1 upside down, 2 on its side;
    /// `skate_audio::player::rolling::loose_board` from the bail / on-foot flag, the deck contact,
    /// its material and the deck's up against the ground): the board slide (`c_board_slide`) holds
    /// while it is set. Retail computes it per skater entry and the instance bridge copies it.
    pub loose_board: u8,
}

/// Overrides the velocity the bridge derives from the transform (m/s, world).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct AudioVelocity(pub Vec3);

/// Which pool an instance belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WorldAudioSlot {
    Traffic,
    Ped,
    /// The MixMap Player slot (instance ≥ 1; 0 is the local player's).
    PlayerSlot,
}

/// Read back: the entity holds a MixMap instance (it is audible). Inserted and removed by the
/// bridge after the hosts ran.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldAudioInstance {
    pub slot: WorldAudioSlot,
    pub instance: u32,
    /// The instance is the object's own (`OwnAudioInstance`, a private MixMap), not one of retail's.
    pub own: bool,
}

/// A traffic vehicle or ped with this takes its own MixMap instance (doc 16 "L3",
/// `game_audio::mod_world`) instead of competing for retail's pools (4 traffic / 15 pedestrian, the
/// nearest win): it plays whenever it is within retail's list radius (40 m / 50 m) and among the
/// 16 nearest own cars / 16 nearest own peds (the farther ones wait, `WorldAudioStats::own_waiting`).
/// Not retail. **Mods' cars and peds get it by default** (user decision 2026-10-04, doc 16 M3:
/// `sdk.world_audio.spawn(key, 'traffic' | 'ped', …)`; `slots = 'shared'` leaves it off); an
/// engine system publishing the living world does not add it (retail's pools) unless an object of
/// its own must be heard.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OwnAudioInstance;

/// A sound emitter at the entity (world audio extension 2; doc 16 "Mod emitters and reverb zones"):
/// an `.ems` eVolumeType 1 record added to the map's live list (`game_audio::emitters`) with
/// retail's reach test (a sphere when the three extents are equal, else an ellipsoid along
/// `forward` turned by the entity's rotation, up and side; the inner `core` at full level), the
/// falloff curve and the `c_emitter` post with the MixMap Emitter words. By default (user decision
/// 2026-10-04) it has its own instance of the private MixMap, so the map's emitters keep retail's 5
/// emitter states; the setting `"mod_emitter_slots": "shared"` makes such emitters share the 5
/// (first reached, first served) instead.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct WorldEmitter {
    /// The AEMS bank bound to `c_emitter` (retail stem or a content overlay's).
    pub bank: String,
    /// The attribute patch = the `c_emitter` selector (0..=500).
    pub patch: i32,
    pub extent: Vec3,
    /// The ellipsoid's forward axis in the entity's frame.
    pub forward: Vec3,
    pub core: f32,
    /// Attribute volume (level = volume × curve(d)).
    pub volume: f32,
    /// `eVolumeFalloffType`: 0 = (1 − d)², 1 = 1 − d, other = flat.
    pub falloff: i32,
}

/// A reverb zone at the entity (world audio extension 2): an eVolumeType 5 record that joins the
/// zones the reverb selector walks (`game_audio::emitters::reverb_zones`), after the map's own, in
/// the order they are reached. `preset`: an `aud_reverb` key the install has (a zone naming no
/// known preset ends retail's zone walk, so spawning one is refused).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct ReverbZoneVolume {
    pub preset: u64,
    pub extent: Vec3,
    pub forward: Vec3,
    pub core: f32,
}

/// Read back (written by `game_audio::emitters` every frame while any exists): the
/// [`WorldEmitter`] entities playing now and the [`ReverbZoneVolume`] entities holding the
/// listener.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct WorldEmitterStats {
    pub playing: Vec<Entity>,
    pub zones: Vec<Entity>,
}

/// The living world's audio switches.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct LivingWorldAudio {
    /// The living world will publish on this map: the world banks are read and decoded on the
    /// prefetch worker. Set it at map load / spawner start, clear it when the world stops.
    pub expected: bool,
    /// The game flag PedestrianSpeech's photographer repeat needs (retail: a system byte,
    /// `*(0x830CFDC4)+912`, meaning not traced): while set, a ped holding speech value 29
    /// (`PictureTaking`) repeats its request every `world_tuning.ped_objects.photo_repeat` s.
    pub photo_flag: bool,
    /// The same flag raised by mods (`photo_flag` on a mod ped; cleared with the mod's objects).
    pub mod_photo_flag: bool,
    /// The running challenge's announcer character (retail system `+1036`: model 35 or 36, named by
    /// the challenge record). None = free skate: the announcer channel then finds no line for any
    /// request (retail: word 0 stays 0), so a pro's crash near the camera stays silent. A challenge
    /// mode sets it while it runs.
    pub announcer: Option<u32>,
    /// Native speech-system1053, independently enabled by the challenge record.
    /// When set, speech for the announcer model uses the focused output pair.
    pub focus_speech: bool,
    /// The same named by a mod (`sdk.world_audio.announcer`; cleared when the mod stops). The
    /// engine's wins.
    pub mod_announcer: Option<u32>,
}

impl LivingWorldAudio {
    /// The announcer character in effect (the engine's, else a mod's).
    pub fn announcer_character(&self) -> Option<u32> {
        self.announcer.or(self.mod_announcer)
    }
}

/// An announcer event: by its id (24576.. = `0x6000 | n`) or by its `.evt` name (`480_slam_pro`,
/// or just the number before the first `_`, `480`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnnouncerLine {
    Id(u16),
    Name(String),
}

/// Ask the announcer channel for a line (retail's `PlayAnnouncerSpeech` script binding and the
/// challenge modes' commentary call `sub_824AA858` the same way). `words` = the request block from
/// word 0 (missing = 0; word 0 = 0 takes the running challenge's announcer); `pro` = a skater model
/// whose announcer pro id goes into word 2 when it is 0 (as a pro's crash does for
/// `480_slam_pro`). Without an announcer character ([`LivingWorldAudio::announcer`]) nothing plays.
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub struct AnnouncerSpeechEvent {
    pub event: AnnouncerLine,
    pub pro: Option<u32>,
    pub words: Vec<u32>,
}

/// Debug counts (read only; written by the bridge every frame).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct WorldAudioStats {
    pub vehicles: usize,
    pub peds: usize,
    pub skaters: usize,
    /// Holders per pool after the last evaluation.
    pub traffic_held: usize,
    pub peds_held: usize,
    pub skaters_held: usize,
    /// The instance counts (traffic, peds, NPC skaters): retail 4 / 15 / 1.
    pub instances: (usize, usize, usize),
    /// The opt-in non-retail "more audible" layout is on (settings/audio.json).
    pub more_audible: bool,
    /// Speech lines started so far (peds and NPC skaters; `game_audio::world_speech`).
    pub speech_lines: u64,
    /// Inside the list radius but holding none of retail's instances (all held by nearer ones).
    pub waiting: Vec<Entity>,
    /// The objects with their own instance (`OwnAudioInstance`, doc 16 L3 / M3): published cars
    /// and peds, holders, the private MixMap's instance counts (16 / 16) and the ones in reach
    /// waiting for one of them (a full private pool: the nearest play).
    pub own_vehicles: usize,
    pub own_peds: usize,
    pub own_traffic_held: usize,
    pub own_peds_held: usize,
    pub own_instances: (usize, usize),
    pub own_waiting: Vec<Entity>,
}

/// A speech value (`S+136`, the state graphs' `SendSpeechEvent speechvalue=N`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpeechValue(pub i32);

impl SpeechValue {
    /// The warn (`501_warn`): the code sends 53 / 54 (the state graph's `DoWarning` entry, 11,
    /// is commented out in retail, "moved to the code"; 11 is also `LostInterestEndChase`, which
    /// the manager maps to `605_chase_terminate`).
    pub const WARN: Self = Self(53);
    pub const NEARBY_REACTION: Self = Self(10);
    pub const FLEE: Self = Self(20);
    pub const LONG_CHEER: Self = Self(23);
    pub const STOP_CHEER: Self = Self(25);

    /// By name: the state graph's name without the `Pedestrian` prefix, case and `_` ignored
    /// (`DoWarning`, `long_cheer`, `Flee`, …), the short aliases `warn`, `cheer`, `slam`, `flee`,
    /// `knockdown`, `nearby`, or a number.
    pub fn from_name(name: &str) -> Option<Self> {
        let key: String = name.chars().filter(|c| *c != '_' && *c != ' ').flat_map(char::to_lowercase).collect();
        if let Ok(n) = key.parse::<i32>() {
            return (0..=127).contains(&n).then_some(Self(n));
        }
        let alias = match key.as_str() {
            "warn" | "warning" => Some(53),
            "cheer" => Some(23),
            "slam" => Some(25),
            "flee" => Some(20),
            "knockdown" => Some(6),
            "nearby" => Some(10),
            "none" => Some(0),
            _ => None,
        };
        if let Some(v) = alias {
            return Some(Self(v));
        }
        skate_audio::world::speech::SPEECH_VALUES.iter().find_map(|(v, n)| {
            n.split(" / ").any(|part| {
                let p: String = part.trim_start_matches("Pedestrian").trim_start_matches("Pedstrian").chars().flat_map(char::to_lowercase).collect();
                p == key || part.to_lowercase() == key
            })
            .then_some(Self(*v))
        })
    }
}

/// A ped's state graph sent a speech value: the ped's `PedAudio::speech_value` takes it, so
/// PedestrianSpeech sees the change and requests the line.
#[derive(Message, Clone, Copy, Debug)]
pub struct PedSpeechEvent {
    pub ped: Entity,
    pub value: SpeechValue,
    /// A conversation turn's variant and row value (`ped+2472` / `+2476`); no known audio
    /// meaning yet (b43), carried for a later port.
    pub topic: Option<(u8, i32)>,
}

/// Hold horn kind `kind` (1..=5) for `seconds` (the AI's choice; not retail data), then back to
/// the component's own horn state.
#[derive(Message, Clone, Copy, Debug)]
pub struct VehicleHorn {
    pub vehicle: Entity,
    pub kind: u8,
    pub seconds: f32,
}

/// Retail's car alarm: horn state 6 for [`CarAlarmRule`]'s time (8 s, vehicle `+3716`,
/// `audio-specs/npc-livingworld-re.md` §6), unconditionally. A second alarm while one sounds
/// restarts the time. Engine traffic normally sends [`VehicleImpact`] instead, and the car alarm
/// rule decides (only a parked car, only a real contact).
#[derive(Message, Clone, Copy, Debug)]
pub struct VehicleAlarm {
    pub vehicle: Entity,
}

/// The car alarm's length (s): retail's `livingworld_vehicle_characteristics` default (field
/// `E199FC7CEA222809`); the install's setup export (`world_tuning.vehicle_alarm.seconds`) wins.
pub const ALARM_SECONDS: f32 = 8.0;
/// The smallest contact that sets a parked car's alarm off: the length of the collision message's
/// vector must exceed it (retail default, field `543475921FD9E04A`; setup export
/// `world_tuning.vehicle_alarm.min_impact`).
pub const ALARM_MIN_IMPACT: f32 = 0.1;

/// The traffic AI holds the vehicle in retail's `StayingParked` state (vehicle `+3424` bit 0x80,
/// set by the state's begin `sub_82C39120`, cleared by its end `sub_82C391F0`): only then can a
/// contact set its alarm off. Insert while parked, remove when it pulls out. Mod cars: the
/// `parked` option, else parked when the mod stopped updating them.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VehicleParked;

/// Who or what touched a vehicle (retail's callback takes any collider; the source only feeds
/// observers and mods, the rule does not test it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ImpactSource {
    /// The local player (on the board, on foot or in a bail).
    #[default]
    Player,
    /// A ped, an NPC skater or a remote player.
    Character,
    /// Another vehicle.
    Vehicle,
    /// A prop, a physics body, anything else.
    Object,
}

impl ImpactSource {
    pub fn name(self) -> &'static str {
        match self {
            Self::Player => "player",
            Self::Character => "character",
            Self::Vehicle => "vehicle",
            Self::Object => "object",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "player" => Self::Player,
            "character" | "ped" | "skater" => Self::Character,
            "vehicle" | "car" => Self::Vehicle,
            "object" | "prop" => Self::Object,
            _ => return None,
        })
    }
}

/// Something touched a vehicle (retail: the vehicle's collision callback `sub_82C3C150`, slot +32
/// of its contact interface at `+136`). `impact` is the callback message's vector at `+48`, whose
/// length the rule tests: the relative velocity at the contact in m/s (the vehicle's minus the
/// other body's; measured with the hook `VEHHIT`, not mass-weighted: a board, the rider and a
/// pedestrian each give their own speed against the car). Send one per contact (or per frame while touching: a
/// repeat restarts the alarm, as in retail). Engine traffic sends one per skater contact
/// (`living_world::vehicles::apply_vehicle_hits`).
#[derive(Message, Clone, Copy, Debug)]
pub struct VehicleImpact {
    pub vehicle: Entity,
    pub by: ImpactSource,
    pub impact: Vec3,
}

impl VehicleImpact {
    /// An impact of `speed` (the vector's length) without a direction.
    pub fn speed(vehicle: Entity, by: ImpactSource, speed: f32) -> Self {
        Self { vehicle, by, impact: Vec3::new(0.0, 0.0, speed) }
    }
}

/// Read back: the car alarm rule set a parked car's alarm off (`restart` = it was already
/// sounding; retail restarts its timer). Engine traffic reads the sounding alarm back every tick
/// (`Car::alarming`): the parked timer `+3712` stays 0 and the car does not pull out while it
/// sounds (`StayingParked` `82C39138`, pull-out condition `sub_82C3A3A8`).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct VehicleAlarmStarted {
    pub vehicle: Entity,
    pub by: ImpactSource,
    pub restart: bool,
}

/// The car alarm rule's numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AlarmTuning {
    /// The rule runs (false: impacts never set an alarm off; [`VehicleAlarm`] still does).
    pub enabled: bool,
    /// The contact vector's length must exceed this.
    pub min_impact: f32,
    /// The alarm's time after the last qualifying contact (s).
    pub seconds: f32,
}

impl Default for AlarmTuning {
    fn default() -> Self {
        Self { enabled: true, min_impact: ALARM_MIN_IMPACT, seconds: ALARM_SECONDS }
    }
}

impl AlarmTuning {
    /// Retail's rule (`sub_82C3C150`): a parked car, a contact longer than `min_impact`.
    pub fn sets_off(&self, parked: bool, impact: Vec3) -> bool {
        self.enabled && parked && impact.is_finite() && impact.length() > self.min_impact
    }

    /// How long the horn holds state 6: retail adds the frame time to the alarm timer on every AI
    /// update and stops at the first update where it exceeds `seconds` (StopAlarming
    /// `sub_82C3A4D0`, a strict `>`), so at the console's 30 fps the alarm sounds for
    /// `floor(seconds × 30) + 1` frames (8 s → 241 frames, 8.033 s), at any engine frame rate.
    pub fn hold_seconds(&self) -> f32 {
        let dt = skate_audio::mixmap::cadence::CONSOLE_DT;
        if !self.seconds.is_finite() || self.seconds <= 0.0 {
            return dt;
        }
        ((self.seconds / dt + 1e-4).floor() + 1.0) * dt
    }
}

/// The car alarm rule's tuning: retail's values from the install's setup export (else the
/// constants above), and an override (a mod's `sdk.world_audio.alarm_rule`, or engine code).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct CarAlarmRule {
    /// From the setup export (`world_tuning.vehicle_alarm`); None = the retail constants.
    pub setup: Option<AlarmTuning>,
    /// Wins over `setup` while set.
    pub overrides: Option<AlarmTuning>,
    /// The mod that set `overrides` (cleared when it stops); None = engine code.
    pub override_owner: Option<String>,
}

impl CarAlarmRule {
    /// The tuning in effect.
    pub fn tuning(&self) -> AlarmTuning {
        self.overrides.or(self.setup).unwrap_or_default()
    }
}

/// A ped zaps with its tazer: `PedAudio::tazing` is held for `seconds` (None = the state graph's
/// `TazerCycTime`, `world_tuning.ped_objects.tazer_seconds`, 2.0 s), the `c_tazer` burst plays.
#[derive(Message, Clone, Copy, Debug)]
pub struct PedTazerEvent {
    pub ped: Entity,
    pub seconds: Option<f32>,
}

/// A ped animation's `BodyFallType` key (a knock-down's events: 9, other, 9, 9 … in retail):
/// one PedBodyFall sound. Keys are queued; each is held for one console frame with a frame of 0
/// before the next, so repeated values sound.
#[derive(Message, Clone, Copy, Debug)]
pub struct PedBodyFallEvent {
    pub ped: Entity,
    pub kind: f32,
}

/// One NPC skater reaction (the skater record's reaction bytes, `sub_824B6E80`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkaterReaction {
    /// A slam it saw (`+102`), by the skater model `by` (0 = the player).
    Slam,
    /// The second slam reaction (`+103`).
    SlamB,
    /// A trick it saw (`+104`).
    Trick,
    /// Its own crash (`+131`).
    Crash,
    /// The chase flag (living-world voices only).
    Chase,
}

/// An NPC skater reacts: the reaction is held for one console frame (`by` = the other skater's
/// model, 0 = the player; the pro-on-pro lines name it).
#[derive(Message, Clone, Copy, Debug)]
pub struct NpcSkaterReactionEvent {
    pub skater: Entity,
    pub reaction: SkaterReaction,
    pub by: u32,
}

impl SkaterReaction {
    /// `set` with this reaction raised.
    pub fn raise(self, mut set: SkaterReactions, by: u32) -> SkaterReactions {
        match self {
            Self::Slam => (set.slam, set.slam_by) = (true, by),
            Self::SlamB => (set.slam_b, set.slam_b_by) = (true, by),
            Self::Trick => (set.trick, set.trick_by) = (true, by),
            Self::Crash => set.crash = true,
            Self::Chase => set.chase = true,
        }
        set
    }
}

// The NPC bail grunt needs no message: retail's body poster raises it at the bail's first body
// impact (`NpcSkaterAudio::voice` names the speaker).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speech_values_by_name() {
        assert_eq!(SpeechValue::from_name("warn"), Some(SpeechValue::WARN));
        assert_eq!(SpeechValue::WARN, SpeechValue(53));
        assert_eq!(SpeechValue::from_name("DoWarning"), Some(SpeechValue(11)));
        assert_eq!(SpeechValue::from_name("long_cheer"), Some(SpeechValue(23)));
        assert_eq!(SpeechValue::from_name("PedestrianFlee"), Some(SpeechValue(20)));
        assert_eq!(SpeechValue::from_name("PictureTaking"), Some(SpeechValue(29)));
        assert_eq!(SpeechValue::from_name("63"), Some(SpeechValue(63)));
        assert_eq!(SpeechValue::from_name("nonsense"), None);
        assert_eq!(SpeechValue::from_name("400"), None);
    }

    #[test]
    fn horn_words_round_trip() {
        for w in 0..=6 {
            assert_eq!(HornState::from_word(w).word(), w);
        }
        assert_eq!(HornState::Honk(9).word(), 5);
    }
}
