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
//! Messages: [`PedSpeechEvent`] (a state graph's `SendSpeechEvent`), [`VehicleHorn`] (hold a horn
//! kind for the caller's time), [`VehicleAlarm`] (retail's 8 s alarm). Set [`LivingWorldAudio`]
//! `expected` at map load when the system will publish, so the world banks decode ahead of need.
//!
//! Everything stays inert when nothing is published. `SKATE_AEMS_WORLD=0` /
//! `SKATE_AEMS_NPC_SKATERS=0` turn the hosts off.
// An API for systems the engine does not have yet: not every item has a caller in the game.
#![allow(dead_code)]

use bevy::prelude::*;

pub use skate_audio::player::AudioState;
pub use skate_audio::player::state::LiteSkater;

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
    /// The model's `aud_traffic_engine` record name (`+168`): `c01_family01` (sedans, hatchback),
    /// `c03_sports01` (sports, muscle), `c04_taxi01` (taxi, patrol), `c05_truck01` (SUV, pickup,
    /// minivan); the patch override picks c06 / c07 / c08 itself. `c00_heavy01` exists but retail
    /// never uses it. Unknown or `default` = patch 2, silent (logged once).
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
    /// The model = the speech voice id 41–96 (`S+84`; the clip names' voice). None = no speech.
    pub voice: Option<u32>,
    /// `S+132` shoe class 2..=5 (`aud_characteristics` per model; 2 for most, 1 = silent and used
    /// by no model).
    pub shoe_class: u8,
    /// `[obj+28]+144` (1..=5): dynamic per ped in retail (1 beyond ~12 m, 2–5 near; meaning open).
    pub weight: u8,
    /// `S+96 == 64`: a security guard (the close-range footstep levels).
    pub close_range: bool,
    /// `S+74` / `S+73`: the walk animation's foot plants (A, B).
    pub feet_down: [bool; 2],
    /// `S+140` / `S+144`: the material under each foot (audio surface material). `None` = 0
    /// (retail's pavements read 0 in every recomp line).
    pub foot_materials: Option<[u32; 2]>,
    /// `S+68` footsteps on. `None` = retail's rule: the 3 nearest peds of the list.
    pub footsteps_on: Option<bool>,
    /// `S+148` / `S+156`: distance to the listener and the model's far threshold (the `_f` lines
    /// beyond it). `None` = retail: the 3-D distance and 20 m.
    pub speech_distance: Option<(f32, f32)>,
    /// `S+136`: the speech value the ped's state graph last sent. Engine systems normally send
    /// [`PedSpeechEvent`] instead; the footstep packets read it too (jump 4/5, collision 6/7).
    pub speech_value: i32,
}

impl Default for PedAudio {
    fn default() -> Self {
        Self {
            voice: None,
            shoe_class: 2,
            weight: 1,
            close_range: false,
            feet_down: [false; 2],
            foot_materials: None,
            footsteps_on: None,
            speech_distance: None,
            speech_value: 0,
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
}

/// The living world's audio switches.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct LivingWorldAudio {
    /// The living world will publish on this map: the world banks are read and decoded on the
    /// prefetch worker. Set it at map load / spawner start, clear it when the world stops.
    pub expected: bool,
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
}

/// A speech value (`S+136`, the state graphs' `SendSpeechEvent speechvalue=N`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpeechValue(pub i32);

impl SpeechValue {
    pub const WARN: Self = Self(11);
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
            "warn" | "warning" => Some(11),
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
}

/// Hold horn kind `kind` (1..=5) for `seconds` (the AI's choice; not retail data), then back to
/// the component's own horn state.
#[derive(Message, Clone, Copy, Debug)]
pub struct VehicleHorn {
    pub vehicle: Entity,
    pub kind: u8,
    pub seconds: f32,
}

/// Retail's car alarm: horn state 6 for 8 s (vehicle `+3716`, `npc-livingworld-re.md` §6).
#[derive(Message, Clone, Copy, Debug)]
pub struct VehicleAlarm {
    pub vehicle: Entity,
}

/// The car alarm's length (s).
pub const ALARM_SECONDS: f32 = 8.0;

// Reserved for the next pass (spec §3.4, P3): `PedKnockDown` (PedBodyFall), `PedTazer` (Tazer,
// needs AEMS op 38), `NpcSkaterBail` (the NPC bail grunt through its SkaterSpeech).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speech_values_by_name() {
        assert_eq!(SpeechValue::from_name("warn"), Some(SpeechValue::WARN));
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
