//! World tuning writes (capability `world_tuning` = 1): typed patches a mod applies to the living
//! world, dynamic props and prop carrying while it runs (`sdk.world.set_tuning(domain, patch)`).
//!
//! Every field is optional; an absent field keeps the game's value (retail by default). Patches are
//! held per mod; when several mods patch the same field the first writer wins, and a mod that stops
//! (or sends `nil`) gives its fields back. The schema lives here so the command is validated at the
//! serde boundary; the engine (`skate-game` `world_tuning`) applies it.
//!
//! Domains:
//! - `living_world`: `npc_draw_distance`, `skater_fade {fade_in_seconds, fade_seconds,
//!   despawn_alpha}`, `skater_line_chain {radius, max_candidates, blend_seconds, keep_facing, facing_rule,
//!   steer_dead_zone_deg, steer_full_deg, fakie_high_speed, fakie_low_speed, fakie_slow_seconds,
//!   fakie_spawn_seconds}` (NPC skater line end: retail
//!   4 m / 16; root blend onto the new line after a branch or chain, engine default 0.2 s, 0 = cut), `ped_fade {distance = {near, far}, fade_in_seconds, enabled}`,
//!   `skater_clips {[<phase> or <phase>.<style>] = <stock clip name>}` (NPC skater puppet clip per
//!   replay phase; phases in [`NPC_SKATER_PHASES`]; a clip whose name holds `_CYC` loops),
//!   `skater_blend_seconds {[<phase> or default] = seconds}` (NPC skater crossfade into a phase's
//!   clip; stock graph default 0.2 s, 0 = cut; also `trick_takeoff` 0.05 s / `trick_air` 0.1 s),
//!   `skater_clips["trick.<scorable name>"]` = a trick animation base (`<base>_G` / `<base>_A`),
//!   `skater_clips["fakie_channel"]` = the stock tree overlaid while riding fakie (`B_FAKIE_CHANNEL`),
//!   `skater_stance {[<record id hex> or <record name>] = "regular" | "goofy"}` (NPC skater natural
//!   stance, read once at spawn; retail values from the measured table, unknown records goofy),
//!   `skater_stance_events {board_backward, mirrored, switch = <clip attribute name>}` (the
//!   trick clip attributes that toggle an NPC skater's stance bits; retail `animboardbackward` /
//!   `mirrored` / `switch`, empty = that toggle off),
//!   `ped_obstacles {enabled, min_half_extent, moving_speed, recut_fraction, detour_margin,
//!   step_height, held_is_obstacle, moving_solid}` (props and mod bodies as ped navigation
//!   obstacles; retail on / 0.2 / 0.4 / 0.25 / held props stay obstacles; `moving_solid` is our
//!   stand-in for NavPower's moving avoider, default on),
//!   `npc_skater_props {enabled}` (NPC skaters push dynamic props like the player; retail on),
//!   `ped_vehicle_contact {enabled, push}` (traffic cars touching peds; retail on / on: the ped is
//!   pushed out of the car, no knock-down),
//!   `npc_tricks {mode, gate_window, min_air_frames}` (the trick an NPC skater does at a recorded
//!   ollie / flip slot: `"profile"` = retail, re-picked from the character's profile table when
//!   the recorded air is long enough; `"recorded"` = the line's own trick; `"none"` = no ollies /
//!   flips; retail windows 300 / 50 recorded 60 Hz frames),
//!   `skater_trick_profiles {[<character key> or <ai_skater_profiles name>] = {regular, nollie}}`
//!   (each a list of `{trick = <EScorableID 0..332>, weight}`; replaces that table, an absent table
//!   keeps the disc's; a character key wins over a profile name),
//!   `skaters` / `pedestrians` / `vehicles {enabled, density}` (per kind; density 1 = retail, 0..=4,
//!   scales the census caps), `ambient_skaters` (NPC skaters offline, retail 3, 0..=8),
//!   `free_play {traffic, pedestrians, ai_skaters}` (retail Free Play mode: traffic and peds 0..1 scale
//!   the caps, 0 removes them at once, `ai_skaters` on / off; absent = career free roam),
//!   `npc_simulated {enabled, radius, max, respawn_seconds, respawn_min, respawn_max}` (NPC
//!   skaters near the player as full physics skaters driven by their AI record; default off
//!   until play-tested, 40 m, 3; bail respawn after 5 s clamped to 1.5..7.9 s; mode 7 walk back
//!   `walk_back` (retail on), `walk_arrive_distance` (2.4 m, 0..=20), `walk_stuck_ticks` (600, 0..=36000)),
//!   `npc_avoid {enabled, max_entries, skitch_cooldown_ticks, radius_skater, radius_pedestrian,
//!   radius_vehicle, radius_prop, cone, wide_cone, wide_cone_distance, skater_radius,
//!   speed_margin, stop_gap, stop_gap_far, stop_cone, stop_gap_prop, side_on_angle,
//!   floor_headroom, skitch_cos, low_prop_height, low_prop_time, step_off_cap, step_off_time}`
//!   (the NPC skaters' retail obstacle avoider, `skate_core::living_world::avoid`; metres,
//!   radians, m/s, seconds).
//! - `props`: `default` and `by_template[<MOBJ template name>]`, each a [`PropTuningPatch`].
//! - `carry`: `grab_bit`, `placement_bit`, `grab_range`, and the Move Object tuning while
//!   holding a prop (retail defaults from attribute class 3EDA5B140604613D): `push_speed`,
//!   `pull_speed`, `side_speed` (target m/s at full left stick, retail 3.0 / 2.0 / 2.5),
//!   `turn_rate` (constant yaw gain replacing the retail inertia curve), `grip_reach` (m between
//!   the grab edge and the skater), `linear_clamp` / `yaw_clamp` (command clamps, 20 / 6),
//!   `relatch` (rad, 0.1), `slew_per_tick` (4), `yaw_rate_feedback` (60: the per-tick facing
//!   change times this is the measured yaw rate the yaw controller tracks; 0 = off),
//!   `linear_controller` / `yaw_controller`
//!   (`[p, filtered, d, filter]`, 20 / 0 / 40 / 0.1), the curves `lever_rotation`, `lever_yaw`,
//!   `mass_speed`, `inertia_yaw_gain` (`[[8 x], [8 y]]`), `let_go_distance` (m, 0 = off = retail), `drop_board`
//!   (grabbing a prop drops a carried board, retail true), `follow_step` (0.1 m), `hold_angle_limit` /
//!   `hold_max_angle_to_horizontal` (80 / 50 deg), `hold_box_extents` ([0.9, 0.8, 1.01]),
//!   `record_272_speed_scale` (2.0), `grab_end_exclusion` (0.25 m), the hand IK `hand_ik_enter` (0.7216, 0 = never), `hand_ik_curve` (`[[8 x], [8 y]]`), `hand_ik_rate`
//!   (0.2 per tick), `hand_ik_reach` (0.65 m) and per template `record_272`. `grip_reach` sets the retail follow
//!   reach (0.65 m). Contact material blocks: `commanded_material` ([0.03, 0.02] static / dynamic
//!   friction), `upright_cos` (0.65) and per template `material_held`, `material_free`,
//!   `material_free_upright`, `upright_pair`, `restitution`, `linear_drag`, `angular_drag` (per
//!   second, retail DMO data +308 / +336 of the type), `mass` (kg, +304), `maximum_linear_velocity` /
//!   `maximum_angular_velocity` (+292 / +296) and `inertia_scale` / `inertia_offset` (+16 / +32).
//! - `shadows`: `world_floor = {r, g, b}`, the lightest a dynamic object's shadow can make the baked
//!   world (each 0..=1, in the shader's squared lightmap space). Retail {0.05, 0.09, 0.13}: the
//!   constant every retail world receiver shader adds to its shadow-map visibility before taking
//!   the minimum with the baked lightmap.
//! - `backdrop`: `visible` (bool), the district's global presentation model (Industrial's sea, the
//!   far sea planes, distant tree walls). Retail draws it (true). `proxy_terrain` (bool), the
//!   far-proxy hills retail leaves drawn where no full-detail cell pairs with them (Industrial's
//!   south hills under the tree wall). Retail true.
//! - `respawn`: `air_timeout_ticks` (integer, 1..=[`MAX_AIR_TIMEOUT_TICKS`]), the fixed 1/60 s
//!   ticks a skater may spend in the air before the checkpoint respawn (retail 300 = 5 s,
//!   `CalcSuggestedState` `count > 300`), e.g. after falling off the map.
//! - `exposure`: the auto-exposure meter. `meter_weights = {r, g, b}` (each 0..=1; retail
//!   {0.3, 0.4, 0.3}, the channel weights retail's bloom downsample dots its tone-mapped value with)
//!   and `meter_scale` (0..=[`MAX_METER_SCALE`]; retail 2.515, the evaluator's average scale).
//! - `ghost`: the skater fade-in after every placement (respawn, teleport, marker return, spawn).
//!   `enabled` (bool, retail true), `fade_in_seconds` (0..=[`MAX_GHOST_FADE_IN_SECONDS`], retail
//!   1.0; 0 = no fade) and `hold_alpha` (0..=1, retail 0.68, the opacity the fade waits at while
//!   retail's hold condition is set; that condition is not decoded yet, so it has no effect now).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DOMAINS: [&str; 9] = ["living_world", "props", "carry", "shadows", "backdrop", "respawn", "exposure", "ghost", "decals"];

/// Longest skater fade-in after a placement a mod may set (s).
pub const MAX_GHOST_FADE_IN_SECONDS: f32 = 60.0;

/// Largest accepted exposure `meter_scale`.
pub const MAX_METER_SCALE: f32 = 100.0;
/// Longest air timeout a mod may set: one hour of 1/60 s ticks.
pub const MAX_AIR_TIMEOUT_TICKS: u32 = 216_000;
/// Upper bound for every number (keeps a typo from building a 1e30 m fade range).
pub const MAX_NUMBER: f32 = 100_000.0;
/// Stable NPC skater replay phase ids (`skate_core::living_world::replay::ReplayPhase::name`).
pub const NPC_SKATER_PHASES: [&str; 6] = ["rolling", "crouched", "air", "air_trick", "ground_trick", "off_board"];
/// `skater_clips` key group for a recorded trick's animation: `trick.<EScorableID name>` (e.g.
/// `trick.kickflip`) = an animation base the NPC plays as `<base>_G` / `<base>_A` (fix 21).
pub const NPC_SKATER_TRICK_GROUP: &str = "trick";
/// `skater_clips` key for the stock tree the NPC overlays while riding fakie (retail
/// `B_FAKIE_CHANNEL`, `FakieHeadChannel82BAC778`).
pub const NPC_SKATER_FAKIE_CHANNEL: &str = "fakie_channel";
/// `skater_line_chain.facing_rule` values (`skate_core::living_world::replay::FacingRule::NAMES`).
pub const NPC_SKATER_FACING_RULES: [&str; 2] = ["riding_entry", "per_node"];
/// Extra `skater_blend_seconds` keys: into a trick's ground clip (retail 0.05 s) and into its air
/// clip when no ground clip ran before it (retail 0.1 s).
pub const NPC_SKATER_TRICK_BLENDS: [&str; 2] = ["trick_takeoff", "trick_air"];
/// `npc_tricks.mode` values (`skate_core::living_world::npc_tricks::TrickMode::name`).
pub const NPC_SKATER_TRICK_MODES: [&str; 3] = ["recorded", "profile", "none"];
/// Entries one trick table may carry.
pub const MAX_TRICK_TABLE: usize = 64;
/// Longest trick gate window a mod may set (recorded 60 Hz frames).
pub const MAX_TRICK_WINDOW: u32 = 36_000;
/// `skater_stance` values (`skate_core::living_world::stance::NaturalStance::name`).
pub const NPC_SKATER_STANCES: [&str; 2] = ["regular", "goofy"];
/// `skater_stance_events` keys (`skate_core::living_world::stance::StanceEvents::KEYS`).
pub const NPC_SKATER_STANCE_EVENTS: [&str; 3] = ["board_backward", "mirrored", "switch"];
/// Longest NPC skater crossfade a mod may set (s).
pub const MAX_BLEND_SECONDS: f32 = 10.0;
/// Per-template entries one patch may carry.
pub const MAX_TEMPLATES: usize = 256;

fn finite(v: Option<f32>) -> bool {
    v.is_none_or(|v| v.is_finite() && v.abs() <= MAX_NUMBER)
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkaterFadePatch {
    pub fade_in_seconds: Option<f32>,
    pub fade_seconds: Option<f32>,
    pub despawn_alpha: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PedFadePatch {
    /// Fallback camera distance pair (near, far); a model record's own pair still wins.
    pub distance: Option<[f32; 2]>,
    pub fade_in_seconds: Option<f32>,
    pub enabled: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LivingWorldPatch {
    pub npc_draw_distance: Option<f32>,
    pub skater_fade: Option<SkaterFadePatch>,
    pub skater_line_chain: Option<SkaterLineChainPatch>,
    pub ped_fade: Option<PedFadePatch>,
    /// NPC skater clip per phase id, or per `<phase>.<style>` (style = the pro's animation style).
    pub skater_clips: Option<BTreeMap<String, String>>,
    /// NPC skater crossfade time (s) into a phase's clip, per phase id or `default`.
    pub skater_blend_seconds: Option<BTreeMap<String, f32>>,
    /// NPC skater natural stance per character record id (16 hex digits) or record name:
    /// `"regular"` or `"goofy"` (retail values from the stance table; read at spawn).
    pub skater_stance: Option<BTreeMap<String, String>>,
    /// Clip attribute name per NPC skater stance toggle (`board_backward` / `mirrored` /
    /// `switch`); empty = off (retail `animboardbackward` / `mirrored` / `switch`).
    pub skater_stance_events: Option<BTreeMap<String, String>>,
    /// Props and mod bodies as ped navigation obstacles (fix 11).
    pub ped_obstacles: Option<PedObstaclesPatch>,
    /// NPC skaters pushing dynamic props (fix 19).
    pub npc_skater_props: Option<NpcSkaterPropsPatch>,
    /// Traffic cars touching peds (`skate_core::living_world::peds::VehicleContactParams`).
    pub ped_vehicle_contact: Option<PedVehicleContactPatch>,
    /// NPC skater trick choice (`skate_core::living_world::npc_tricks`).
    pub npc_tricks: Option<NpcTricksPatch>,
    /// NPC skater trick tables per character key or `ai_skater_profiles` name.
    pub skater_trick_profiles: Option<BTreeMap<String, TrickTablesPatch>>,
    /// Simulated NPC skaters (`skate-game` `living_world::npc_sim`).
    pub npc_simulated: Option<NpcSimulatedPatch>,
    /// The NPC skaters' obstacle avoider (retail values by default).
    pub npc_avoid: Option<NpcAvoidPatch>,
    /// The ped behaviour runtime (stock ped AI graph on each ped's brain).
    pub ped_brain: Option<PedBrainPatch>,
    /// Traffic horn values per driver record (`default`, `driver_fast`, `driver_normal`, `driver_reckless`,
    /// `driver_taxi`, or `all`, applied first).
    pub traffic_horn: Option<BTreeMap<String, TrafficHornPatch>>,
    /// Per-kind switch and density.
    pub skaters: Option<KindPatch>,
    pub pedestrians: Option<KindPatch>,
    pub vehicles: Option<KindPatch>,
    /// NPC skaters offline (retail 3).
    pub ambient_skaters: Option<u32>,
    /// Retail Free Play options (`skate_core::living_world::FreePlay`).
    pub free_play: Option<FreePlayPatch>,
    /// The zombie cheat (free skate: every ped follows and attacks the skater, no traffic or NPC skaters).
    pub zombie: Option<bool>,
}

/// One living-world kind: `enabled`, `density` (0..=4, 1 = retail).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KindPatch {
    pub enabled: Option<bool>,
    pub density: Option<f32>,
}

/// Free Play: `traffic` and `pedestrians` 0..=1 (retail steps of 0.1 behind No / Low / Medium / High),
/// `ai_skaters`. An absent field is the mode block's reset value (1, 1, on).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FreePlayPatch {
    pub traffic: Option<f32>,
    pub pedestrians: Option<f32>,
    pub ai_skaters: Option<bool>,
}

/// Simulated NPC skaters: `enabled`, `radius` (m, 0..=500), `max` (0..=16), the bail respawn
/// delay `respawn_seconds` and its clamp `respawn_min` / `respawn_max` (s, 0..=60; retail 5.0,
/// 1.5, 7.9).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NpcSimulatedPatch {
    pub enabled: Option<bool>,
    pub radius: Option<f32>,
    pub max: Option<u32>,
    pub respawn_seconds: Option<f32>,
    pub respawn_min: Option<f32>,
    pub respawn_max: Option<f32>,
    /// AI trick signals: anticipation distance (m, 0..=50), look-ahead (recorded frames,
    /// 0..=600), max nodes crossed per tick for an event (0..=60); retail 3.0, 60, 3.
    pub anticipation_distance: Option<f32>,
    pub anticipation_frames: Option<u32>,
    pub max_crossed_nodes: Option<u32>,
    /// Mode 7: step off and walk back to the line past a blocking prop (controller B, retail on), the
    /// horizontal arrival distance (m, 0..=20; retail 2.4) and the ticks on one waypoint before it asks to bail
    /// (0..=36000; retail 600).
    pub walk_back: Option<bool>,
    pub walk_arrive_distance: Option<f32>,
    pub walk_stuck_ticks: Option<u32>,
}

/// The NPC skaters' obstacle avoider (`skate_core::living_world::avoid::AvoidSettings`): every
/// value optional; distances 0..=500 m, angles 0..=pi, speeds and times 0..=100.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NpcAvoidPatch {
    pub enabled: Option<bool>,
    pub max_entries: Option<u32>,
    pub skitch_cooldown_ticks: Option<u32>,
    pub radius_skater: Option<f32>,
    pub radius_pedestrian: Option<f32>,
    pub radius_vehicle: Option<f32>,
    pub radius_prop: Option<f32>,
    pub cone: Option<f32>,
    pub wide_cone: Option<f32>,
    pub wide_cone_distance: Option<f32>,
    pub skater_radius: Option<f32>,
    pub speed_margin: Option<f32>,
    pub stop_gap: Option<f32>,
    pub stop_gap_far: Option<f32>,
    pub stop_cone: Option<f32>,
    pub stop_gap_prop: Option<f32>,
    pub side_on_angle: Option<f32>,
    pub floor_headroom: Option<f32>,
    pub skitch_cos: Option<f32>,
    pub low_prop_height: Option<f32>,
    pub low_prop_time: Option<f32>,
    pub step_off_cap: Option<f32>,
    pub step_off_time: Option<f32>,
}

impl NpcAvoidPatch {
    /// Every float field with its name (validation, application).
    pub fn floats(&self) -> [(&'static str, Option<f32>); 20] {
        [("radius_skater", self.radius_skater), ("radius_pedestrian", self.radius_pedestrian), ("radius_vehicle", self.radius_vehicle), ("radius_prop", self.radius_prop), ("cone", self.cone), ("wide_cone", self.wide_cone), ("wide_cone_distance", self.wide_cone_distance), ("skater_radius", self.skater_radius), ("speed_margin", self.speed_margin), ("stop_gap", self.stop_gap), ("stop_gap_far", self.stop_gap_far), ("stop_cone", self.stop_cone), ("stop_gap_prop", self.stop_gap_prop), ("side_on_angle", self.side_on_angle), ("floor_headroom", self.floor_headroom), ("skitch_cos", self.skitch_cos), ("low_prop_height", self.low_prop_height), ("low_prop_time", self.low_prop_time), ("step_off_cap", self.step_off_cap), ("step_off_time", self.step_off_time)]
    }
    pub fn validate(&self) -> bool {
        self.max_entries.is_none_or(|n| n <= 64)
            && self.skitch_cooldown_ticks.is_none_or(|n| n <= 3600)
            && self.floats().into_iter().all(|(name, v)| {
                v.is_none_or(|v| {
                    let max = if name.starts_with("radius") || name.contains("gap") || name.ends_with("distance") { 500.0 } else if name.contains("cone") || name.ends_with("angle") { core::f32::consts::PI } else { 100.0 };
                    v.is_finite() && (0.0..=max).contains(&v)
                })
            })
    }
}

/// The ped behaviour runtime: `enabled`, `mood` (the mood system raises wants), `wander_speed` (m/s), `warn_seconds`,
/// `know_about_seconds`, `conversation_turn_seconds`, `conversation_gather_seconds` (s); 0..=100 each (retail 2.0,
/// 3.5, 30.0, 3.0, 30.0); `run_from_honker_distance` (m sideways of a honking car's line) and
/// `run_from_honker_speed` (m/s), 0..=100 (retail 10.0, 6.0); `warn_speech`, the warn's speech value 0..=127 (retail 53).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PedBrainPatch {
    pub enabled: Option<bool>,
    pub mood: Option<bool>,
    pub wander_speed: Option<f32>,
    pub warn_seconds: Option<f32>,
    pub know_about_seconds: Option<f32>,
    pub conversation_turn_seconds: Option<f32>,
    pub conversation_gather_seconds: Option<f32>,
    pub run_from_honker_distance: Option<f32>,
    pub run_from_honker_speed: Option<f32>,
    pub warn_speech: Option<i32>,
    /// Hand props: the attack throw's speed and look-ahead (retail 10.0 m/s, 0.8333 s), its jitter (0.25 m) and lift
    /// (1.0 m), the light (bin) throw's speed (5.0); 0..=100. `hand_prop_skater_contact` (thrown props hit the
    /// skater) and `starting_hand_props` (peds start out carrying), retail true.
    pub attack_throw_speed: Option<f32>,
    pub attack_throw_lead_seconds: Option<f32>,
    pub attack_throw_jitter: Option<f32>,
    pub attack_throw_lift: Option<f32>,
    pub light_throw_speed: Option<f32>,
    pub hand_prop_skater_contact: Option<bool>,
    pub starting_hand_props: Option<bool>,
    /// ZombieFollow: follow distance (8 m), sprint distance (15 m), ring around the player (1..8 m), sprint and walk
    /// speeds (8, 3 m/s); 0..=100.
    pub zombie_follow_distance: Option<f32>,
    pub zombie_sprint_distance: Option<f32>,
    pub zombie_ring_min: Option<f32>,
    pub zombie_ring_max: Option<f32>,
    pub zombie_sprint_speed: Option<f32>,
    pub zombie_walk_speed: Option<f32>,
}

/// One traffic driver's horn: `blocked_time`, `obstacle_time` (s, retail 4 / taxi 1, 2), `approach_speed_kmh`
/// (retail 5, taxi 10), `approach_seconds` (time to the obstacle for the approach horn, retail 2), each 0..=100;
/// `enabled_chance` / `blocked_long_chance` (0..=1, the per-car rolls of driver bits 0x01 / 0x02; retail 1.0 / 0.5,
/// normal 0.3 and reckless 0.8 long). Read when a car spawns.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrafficHornPatch {
    pub blocked_time: Option<f32>,
    pub obstacle_time: Option<f32>,
    pub approach_speed_kmh: Option<f32>,
    pub approach_seconds: Option<f32>,
    pub enabled_chance: Option<f32>,
    pub blocked_long_chance: Option<f32>,
}

impl TrafficHornPatch {
    pub fn validate(&self) -> bool {
        [self.blocked_time, self.obstacle_time, self.approach_speed_kmh, self.approach_seconds].into_iter().all(|v| v.is_none_or(|v| v.is_finite() && (0.0..=100.0).contains(&v)))
            && [self.enabled_chance, self.blocked_long_chance].into_iter().all(|v| v.is_none_or(|v| v.is_finite() && (0.0..=1.0).contains(&v)))
    }
}

/// NPC skater trick choice: `mode` (one of [`NPC_SKATER_TRICK_MODES`], retail `"profile"`),
/// `gate_window` / `min_air_frames` (retail 300 / 50 recorded 60 Hz frames).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NpcTricksPatch {
    pub mode: Option<String>,
    pub gate_window: Option<u32>,
    pub min_air_frames: Option<u32>,
}

/// One trick table entry.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrickWeight {
    pub trick: i16,
    pub weight: f32,
}

/// A character's trick tables: `regular` (flips and the ollie) and `nollie` (nollie flips and the
/// nollie); an absent table keeps the disc's.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrickTablesPatch {
    pub regular: Option<Vec<TrickWeight>>,
    pub nollie: Option<Vec<TrickWeight>>,
}

impl TrickTablesPatch {
    pub fn validate(&self) -> bool {
        [&self.regular, &self.nollie].into_iter().flatten().all(|t| {
            t.len() <= MAX_TRICK_TABLE && t.iter().all(|e| (0..332).contains(&e.trick) && e.weight.is_finite() && (0.0..=MAX_NUMBER).contains(&e.weight))
        })
    }
}

/// Traffic cars touching peds: `enabled` (detection, the event and the log; retail on), `push`
/// (the ped is shoved out of the car's box; retail on, the only response retail has).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PedVehicleContactPatch {
    pub enabled: Option<bool>,
    pub push: Option<bool>,
}

/// NPC skaters against dynamic props: their board and body push a prop by the prop's own push
/// tuning (`props` domain), like the player's (retail: NPC skaters are full skaters; on).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NpcSkaterPropsPatch {
    pub enabled: Option<bool>,
}

/// Ped obstacle rules (`skate_core::living_world::peds::ObstacleParams`; retail: on, 0.2 m minimum
/// half extent, no cut above 0.4 m/s, re-cut after 0.25 x the smallest half extent; ours: 0.1 m
/// detour margin, 0 m step height). `held_is_obstacle`: a prop held by Move Object (or an attached
/// mod body) stays an obstacle (retail true). `moving_solid`: a moving object blocks a ped's step
/// (NOT RETAIL YET stand-in for NavPower's moving avoider; default true).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PedObstaclesPatch {
    pub enabled: Option<bool>,
    pub min_half_extent: Option<f32>,
    pub moving_speed: Option<f32>,
    pub recut_fraction: Option<f32>,
    pub detour_margin: Option<f32>,
    pub step_height: Option<f32>,
    pub held_is_obstacle: Option<bool>,
    pub moving_solid: Option<bool>,
}

/// NPC skater line end (`skate_core::living_world::replay::ChainConfig`): continue on an unused
/// line whose start node is within `radius` m (retail 4.0; 0 = fade out at every line end), among
/// at most `max_candidates` (retail 16); `blend_seconds`: the drawn root moves onto the new line
/// over this time after a branch or chain (engine default 0.2 s, 0 = cut, at most 10 s);
/// `keep_facing`: mod option, not retail (default false): the skater keeps the way it faces
/// (forward or fakie) across a branch or chain by riding the new line turned round (fix 16).
/// `facing_rule`: one of [`NPC_SKATER_FACING_RULES`]: `riding_entry` (default, retail: the recorded
/// skater frame, turned while a flip latched on entering riding is set, held across switches; a
/// body against its travel on the ground is drawn riding fakie with the stock fakie channel) or
/// `per_node` (not retail, the fix 23 rule: each node folded onto the board's riding direction). `steer_dead_zone_deg` / `steer_full_deg`: retail AI
/// steer ramp (`ai_skater` 2 / 10 deg), data for the simulated tier (the replay tier does not steer).
/// `fakie_high_speed` / `fakie_low_speed` (m/s), `fakie_slow_seconds`, `fakie_spawn_seconds` (s):
/// retail's riding-fakie rule (stock motion graph `UpdateRidingFakie`: 1.0 / 0.5 / 0.2 / 1.0): the
/// NPC is drawn riding fakie (the stock fakie channel over its riding clip) when it rolls against
/// its board's forward above the high speed, or above the low speed for longer than the slow time,
/// never in the first spawn seconds. A very high speed turns the fakie drawing off.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkaterLineChainPatch {
    pub radius: Option<f32>,
    pub max_candidates: Option<u32>,
    pub blend_seconds: Option<f32>,
    pub keep_facing: Option<bool>,
    pub facing_rule: Option<String>,
    pub steer_dead_zone_deg: Option<f32>,
    pub steer_full_deg: Option<f32>,
    pub fakie_high_speed: Option<f32>,
    pub fakie_low_speed: Option<f32>,
    pub fakie_slow_seconds: Option<f32>,
    pub fakie_spawn_seconds: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropBoxPatch {
    pub center: [f32; 3],
    pub half_extents: [f32; 3],
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropTuningPatch {
    pub contact_padding: Option<f32>,
    pub penetration_slop: Option<f32>,
    pub penetration_correction: Option<f32>,
    pub max_depenetration_per_tick: Option<f32>,
    pub restitution_threshold: Option<f32>,
    pub skater_push_mass: Option<f32>,
    pub push_transfer: Option<f32>,
    pub body_push_speed: Option<f32>,
    pub board_push_speed: Option<f32>,
    pub penetration_push_speed: Option<f32>,
    pub stuck_release_ticks: Option<u32>,
    pub collision_box: Option<PropBoxPatch>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropsPatch {
    pub default: Option<PropTuningPatch>,
    #[serde(default)]
    pub by_template: BTreeMap<String, PropTuningPatch>,
    /// Island settings shared by every prop (retail DMO simulation block).
    pub solver: Option<PropSolverPatch>,
    /// Self-righting window of Upright (retail cMsgUprightDMO, 82C4B8C0 / 82C56780 / 82C573D0).
    pub upright: Option<PropUprightPatch>,
}

/// Upright self-righting (doc 27, Upright); retail defaults: `window_seconds` 2.0,
/// `tick_seconds` 1/60, `stop_angle_deg` 10, `max_angle_deg` 70, `dead_band_deg` 5, `gain_min` 3,
/// `gain_max` 5, `gain_blend_start` 1.1, `off_axis_spin` 0.1, `command_rate` 60,
/// `fallback_angle_deg` 120, `block_yaw` true. Numbers finite and >= 0; the window and tick > 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropUprightPatch {
    pub window_seconds: Option<f32>,
    pub tick_seconds: Option<f32>,
    pub stop_angle_deg: Option<f32>,
    pub max_angle_deg: Option<f32>,
    pub dead_band_deg: Option<f32>,
    pub gain_min: Option<f32>,
    pub gain_max: Option<f32>,
    pub gain_blend_start: Option<f32>,
    pub off_axis_spin: Option<f32>,
    pub command_rate: Option<f32>,
    pub fallback_angle_deg: Option<f32>,
    pub block_yaw: Option<bool>,
}

/// Prop contact solver and sleep rule (retail DMO simulation, 8275DCC8 ->
/// 82DC2840): `row_solver` true = retail row solver (false = the engine's older
/// impulse pass); `iterations` (retail 25, 1..=256); `sleep_energy` (retail 1e-5);
/// `sleep_frames` (retail 2, 1..=10000); `max_sleeps_per_step` (retail 100);
/// `rest_snap` (engine snap, not retail, default false).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropSolverPatch {
    pub row_solver: Option<bool>,
    pub iterations: Option<u32>,
    pub sleep_energy: Option<f32>,
    pub sleep_frames: Option<u32>,
    pub max_sleeps_per_step: Option<u32>,
    pub rest_snap: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarryPatch {
    /// Packed controller button bit (0..=31) held to grab and carry (retail 28, RB).
    pub grab_bit: Option<u32>,
    /// Bit whose rising edge toggles placement (retail 20, B).
    pub placement_bit: Option<u32>,
    /// Pickup reach in metres to the prop's surface (engine default 2.0).
    pub grab_range: Option<f32>,
    /// Move Object speed pushing forward, m/s at full stick (engine default 1.4).
    pub push_speed: Option<f32>,
    /// Move Object speed pulling back, m/s at full stick (engine default 1.0).
    pub pull_speed: Option<f32>,
    /// Move Object side step, m/s at full stick (engine default 0.8).
    pub side_speed: Option<f32>,
    /// Move Object turn, rad/s at full right stick X (engine default 1.6).
    pub turn_rate: Option<f32>,
    /// Gap in metres between the grab edge and the skater (engine default 0.35).
    pub grip_reach: Option<f32>,
    /// Linear command clamp (retail 20).
    pub linear_clamp: Option<f32>,
    /// Yaw command clamp (retail 6).
    pub yaw_clamp: Option<f32>,
    /// Heading re-latch threshold in rad (retail 0.1).
    pub relatch: Option<f32>,
    /// Max change of the linear command per tick (retail 4).
    pub slew_per_tick: Option<f32>,
    /// Yaw-rate feedback factor (retail 60 = 1/dt, 0x822F860C): facing change per
    /// tick x this = measured yaw rate subtracted from the yaw target; 0 = off.
    pub yaw_rate_feedback: Option<f32>,
    /// Linear controller `[p, filtered, d, filter]` (retail 20, 0, 40, 0.1).
    pub linear_controller: Option<[f32; 4]>,
    /// Yaw controller `[p, filtered, d, filter]` (retail 20, 0, 40, 0.1).
    pub yaw_controller: Option<[f32; 4]>,
    /// Curve |lever| -> rotation demand, `[[8 x], [8 y]]`.
    pub lever_rotation: Option<[[f32; 8]; 2]>,
    /// Curve |lever| -> yaw-rate factor.
    pub lever_yaw: Option<[[f32; 8]; 2]>,
    /// Curve mass -> speed scale.
    pub mass_speed: Option<[[f32; 8]; 2]>,
    /// Curve yaw inertia -> yaw gain.
    pub inertia_yaw_gain: Option<[[f32; 8]; 2]>,
    /// Metres the skater may fall behind its grab point before letting go (engine rule for mods; default 0 = off, retail lets go by record qualification).
    pub let_go_distance: Option<f32>,
    /// Grabbing a prop drops a carried board (retail true, 82D442D0 -> LetGoOfSkateboard 82D75440).
    pub drop_board: Option<bool>,
    /// Max change of the skater follow step per tick in m (retail 0.1, 82BD41B0 in 82D44A10).
    pub follow_step: Option<f32>,
    /// Hold qualification approach angle limit in degrees (retail GrabSplineAngleLimitGrabbing 80).
    pub hold_angle_limit: Option<f32>,
    /// Hold qualification edge slope limit in degrees (retail GrabSplineMaxAngleToHorizontalGrabbing 50).
    pub hold_max_angle_to_horizontal: Option<f32>,
    /// Hold qualification grab box half extents (retail GrabBoxSizeGrabbing 0.9, 0.8, 1.01).
    pub hold_box_extents: Option<[f32; 3]>,
    /// Target speed scale for prop types with record+272 set (retail 2.0).
    pub record_272_speed_scale: Option<f32>,
    /// Grip end exclusion on a prop's authored grab record in m (retail GrabSplineEndExclusion 0.25, 82D444A0).
    pub grab_end_exclusion: Option<f32>,
    /// Hand IK enter value (retail 5E35DB02BE697A58 = 0.7216; 0 = the hands never go to IK, 82D46610).
    pub hand_ik_enter: Option<f32>,
    /// Hand IK curve over the state time (retail 702F25BA3A5AAA56): IK turns on once 1 - y passes 0.1.
    pub hand_ik_curve: Option<[[f32; 8]; 2]>,
    /// Hand IK weight change per tick (retail 0.2, 82D45008).
    pub hand_ik_rate: Option<f32>,
    /// Hand IK reach around the animated hands in m (retail 0.65, 82BD9728).
    pub hand_ik_reach: Option<f32>,
    /// Friction pair `[static, dynamic]` every held (commanded) prop switches to (retail [0.03, 0.02],
    /// 82C53EF8); combined with the other side by max / max / min (82763078).
    pub commanded_material: Option<[f32; 2]>,
    /// Upright test on the prop's up axis y for the upright free pair (retail 0.65, 82C54B00; -1..1).
    pub upright_cos: Option<f32>,
    /// Linear command at the centre of mass (retail true; false = at the grip point, lever torque).
    pub apply_at_com: Option<bool>,
    /// Yaw command replaces the prop's angular accumulator (retail true; false = added).
    pub yaw_replaces_torque: Option<bool>,
    /// Vertical command dropped (retail true).
    pub ignore_vertical: Option<bool>,
    /// Every command wakes the prop (retail true; false = only a non-zero command).
    pub wake_on_command: Option<bool>,
    /// Per prop type held / free parameter blocks, keyed by the MOBJ template name or by the
    /// type's vault record name (`livingworld_dynamicobject_characteristics`, e.g.
    /// `dt_garbagebin`, logged as `type=` in HELD_PROP); the template name entry wins. Each
    /// field overrides the type's retail value; unset fields keep it.
    #[serde(default)]
    pub by_template: BTreeMap<String, CarryMaterialPatch>,
}

/// Per prop type contact material blocks of `carry.by_template` (friction pairs `[static, dynamic]`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarryMaterialPatch {
    /// Friction pair while held (default: `commanded_material`).
    pub material_held: Option<[f32; 2]>,
    /// Free friction pair (retail DMO data +320 / +328 of the type; the prop's authored friction
    /// only when its type data is missing).
    pub material_free: Option<[f32; 2]>,
    /// Free friction pair while upright (retail DMO data +316 / +324; default: `material_free`),
    /// used only when `upright_pair` is set.
    pub material_free_upright: Option<[f32; 2]>,
    /// The free pair depends on the upright test (retail DMO data +312 of the type).
    pub upright_pair: Option<bool>,
    /// Restitution of this type's blocks (retail DMO data +272 of the type; the authored
    /// restitution only when its type data is missing).
    pub restitution: Option<f32>,
    /// Record+272 for this prop type: Move Object target speeds x `record_272_speed_scale`
    /// (retail: set when the type's DMO data +312 is set, 82C4B960).
    pub record_272: Option<bool>,
    /// Linear drag of this prop type's body, per second (retail DMO data +308 `LinearDrag`; the
    /// integrator keeps `1 - drag * dt` of the velocity each fixed step, 60 or more stops it).
    pub linear_drag: Option<f32>,
    /// Angular drag, per second (retail DMO data +336 `AngularDrag`, same rule).
    pub angular_drag: Option<f32>,
    /// Body mass in kg (retail DMO data +304 of the type; the box inertia follows it).
    pub mass: Option<f32>,
    /// Linear speed cap in m/s (retail DMO data +292; the integrator shortens faster velocities).
    pub maximum_linear_velocity: Option<f32>,
    /// Angular speed cap in rad/s (retail DMO data +296, same rule).
    pub maximum_angular_velocity: Option<f32>,
    /// Box inertia shape: the body's half extents x `inertia_scale` + `inertia_offset` (retail DMO
    /// data +16 / +32 of the type; class default 1.2 / 0).
    pub inertia_scale: Option<[f32; 3]>,
    pub inertia_offset: Option<[f32; 3]>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShadowsPatch {
    /// Dynamic shadow floor on the baked world, RGB 0..=1 (retail 0.05, 0.09, 0.13).
    pub world_floor: Option<[f32; 3]>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackdropPatch {
    /// Draw the district's global presentation model (retail true).
    pub visible: Option<bool>,
    /// Draw the unpaired far-proxy terrain cells (retail true).
    pub proxy_terrain: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExposurePatch {
    /// Meter channel weights R, G, B, each 0..=1 (retail 0.3, 0.4, 0.3).
    pub meter_weights: Option<[f32; 3]>,
    /// Meter average scale (retail 2.515).
    pub meter_scale: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GhostPatch {
    /// Fade the skater in after placements (retail true).
    pub enabled: Option<bool>,
    /// Seconds to fully opaque (retail 1.0).
    pub fade_in_seconds: Option<f32>,
    /// Opacity held while the hold condition is set (retail 0.68).
    pub hold_alpha: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecalsPatch {
    /// Strength of every world decal over its base surface, 0..=1 (retail 1.0: the decal programs
    /// blend at the decal texture's own alpha).
    pub opacity: Option<f32>,
}

impl DecalsPatch {
    pub fn validate(&self) -> bool {
        self.opacity.is_none_or(|v| v.is_finite() && (0.0..=1.0).contains(&v))
    }
}

impl GhostPatch {
    pub fn validate(&self) -> bool {
        self.fade_in_seconds.is_none_or(|v| v.is_finite() && (0.0..=MAX_GHOST_FADE_IN_SECONDS).contains(&v))
            && self.hold_alpha.is_none_or(|v| v.is_finite() && (0.0..=1.0).contains(&v))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RespawnPatch {
    /// Air ticks before the checkpoint respawn (retail 300).
    pub air_timeout_ticks: Option<u32>,
}

impl RespawnPatch {
    pub fn validate(&self) -> bool {
        self.air_timeout_ticks.is_none_or(|t| (1..=MAX_AIR_TIMEOUT_TICKS).contains(&t))
    }
}

/// Field-wise "first writer wins": `self` keeps its fields, `later` fills the gaps.
pub trait Merge {
    fn merge(&mut self, later: &Self);
}

macro_rules! merge_opts {
    ($a:ident, $b:ident; $($f:ident),*) => { $( if $a.$f.is_none() { $a.$f = $b.$f.clone(); } )* };
}

impl Merge for SkaterFadePatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; fade_in_seconds, fade_seconds, despawn_alpha);
    }
}
impl Merge for PedObstaclesPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; enabled, min_half_extent, moving_speed, recut_fraction, detour_margin, step_height, held_is_obstacle, moving_solid);
    }
}
impl Merge for PedVehicleContactPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; enabled, push);
    }
}
impl Merge for NpcSkaterPropsPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; enabled);
    }
}
impl Merge for SkaterLineChainPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; radius, max_candidates, blend_seconds, keep_facing, facing_rule, steer_dead_zone_deg, steer_full_deg, fakie_high_speed, fakie_low_speed, fakie_slow_seconds, fakie_spawn_seconds);
    }
}
impl Merge for PedFadePatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; distance, fade_in_seconds, enabled);
    }
}
fn merge_nested<T: Merge + Clone>(a: &mut Option<T>, b: &Option<T>) {
    match (a.as_mut(), b) {
        (Some(a), Some(b)) => a.merge(b),
        (None, Some(b)) => *a = Some(b.clone()),
        _ => {}
    }
}
impl Merge for LivingWorldPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; npc_draw_distance);
        merge_nested(&mut self.skater_fade, &b.skater_fade);
        merge_nested(&mut self.skater_line_chain, &b.skater_line_chain);
        merge_nested(&mut self.ped_fade, &b.ped_fade);
        merge_nested(&mut self.ped_obstacles, &b.ped_obstacles);
        merge_nested(&mut self.npc_skater_props, &b.npc_skater_props);
        merge_nested(&mut self.ped_vehicle_contact, &b.ped_vehicle_contact);
        merge_nested(&mut self.npc_tricks, &b.npc_tricks);
        merge_nested(&mut self.npc_simulated, &b.npc_simulated);
        merge_nested(&mut self.npc_avoid, &b.npc_avoid);
        merge_nested(&mut self.ped_brain, &b.ped_brain);
        merge_nested(&mut self.skaters, &b.skaters);
        merge_nested(&mut self.pedestrians, &b.pedestrians);
        merge_nested(&mut self.vehicles, &b.vehicles);
        merge_nested(&mut self.free_play, &b.free_play);
        merge_opts!(self, b; ambient_skaters, zombie);
        match (self.traffic_horn.as_mut(), &b.traffic_horn) {
            (Some(a), Some(b)) => b.iter().for_each(|(k, v)| {
                a.entry(k.clone()).or_insert_with(|| v.clone());
            }),
            (None, Some(b)) => self.traffic_horn = Some(b.clone()),
            _ => {}
        }
        match (self.skater_trick_profiles.as_mut(), &b.skater_trick_profiles) {
            (Some(a), Some(b)) => b.iter().for_each(|(k, v)| {
                a.entry(k.clone()).or_insert_with(|| v.clone());
            }),
            (None, Some(b)) => self.skater_trick_profiles = Some(b.clone()),
            _ => {}
        }
        match (self.skater_clips.as_mut(), &b.skater_clips) {
            (Some(a), Some(b)) => b.iter().for_each(|(k, v)| {
                a.entry(k.clone()).or_insert_with(|| v.clone());
            }),
            (None, Some(b)) => self.skater_clips = Some(b.clone()),
            _ => {}
        }
        match (self.skater_blend_seconds.as_mut(), &b.skater_blend_seconds) {
            (Some(a), Some(b)) => b.iter().for_each(|(k, v)| {
                a.entry(k.clone()).or_insert(*v);
            }),
            (None, Some(b)) => self.skater_blend_seconds = Some(b.clone()),
            _ => {}
        }
        match (self.skater_stance.as_mut(), &b.skater_stance) {
            (Some(a), Some(b)) => b.iter().for_each(|(k, v)| {
                a.entry(k.clone()).or_insert_with(|| v.clone());
            }),
            (None, Some(b)) => self.skater_stance = Some(b.clone()),
            _ => {}
        }
        match (self.skater_stance_events.as_mut(), &b.skater_stance_events) {
            (Some(a), Some(b)) => b.iter().for_each(|(k, v)| {
                a.entry(k.clone()).or_insert_with(|| v.clone());
            }),
            (None, Some(b)) => self.skater_stance_events = Some(b.clone()),
            _ => {}
        }
    }
}
impl Merge for PropTuningPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; contact_padding, penetration_slop, penetration_correction, max_depenetration_per_tick,
            restitution_threshold, skater_push_mass, push_transfer, body_push_speed, board_push_speed,
            penetration_push_speed, stuck_release_ticks, collision_box);
    }
}
impl Merge for PropSolverPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; row_solver, iterations, sleep_energy, sleep_frames, max_sleeps_per_step, rest_snap);
    }
}
impl Merge for PropUprightPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; window_seconds, tick_seconds, stop_angle_deg, max_angle_deg, dead_band_deg, gain_min, gain_max,
            gain_blend_start, off_axis_spin, command_rate, fallback_angle_deg, block_yaw);
    }
}
impl Merge for PropsPatch {
    fn merge(&mut self, b: &Self) {
        merge_nested(&mut self.default, &b.default);
        merge_nested(&mut self.solver, &b.solver);
        merge_nested(&mut self.upright, &b.upright);
        for (k, v) in &b.by_template {
            self.by_template.entry(k.clone()).and_modify(|a| a.merge(v)).or_insert_with(|| v.clone());
        }
    }
}
impl Merge for CarryPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; grab_bit, placement_bit, grab_range, push_speed, pull_speed, side_speed, turn_rate, grip_reach,
            linear_clamp, yaw_clamp, relatch, slew_per_tick, yaw_rate_feedback, linear_controller, yaw_controller, lever_rotation, lever_yaw,
            mass_speed, inertia_yaw_gain, let_go_distance, drop_board, follow_step, hold_angle_limit, hold_max_angle_to_horizontal,
            hold_box_extents, record_272_speed_scale, grab_end_exclusion, hand_ik_enter, hand_ik_curve, hand_ik_rate, hand_ik_reach, commanded_material, upright_cos,
            apply_at_com, yaw_replaces_torque, ignore_vertical, wake_on_command);
        for (k, v) in &b.by_template {
            self.by_template.entry(k.clone()).and_modify(|a| { merge_opts!(a, v; material_held, material_free, material_free_upright, upright_pair, restitution, record_272, linear_drag, angular_drag, mass, maximum_linear_velocity, maximum_angular_velocity, inertia_scale, inertia_offset); }).or_insert_with(|| v.clone());
        }
    }
}

impl Merge for KindPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; enabled, density);
    }
}

impl Merge for FreePlayPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; traffic, pedestrians, ai_skaters);
    }
}

impl Merge for NpcSimulatedPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; enabled, radius, max, respawn_seconds, respawn_min, respawn_max, anticipation_distance, anticipation_frames, max_crossed_nodes, walk_back, walk_arrive_distance, walk_stuck_ticks);
    }
}

impl Merge for PedBrainPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; enabled, mood, wander_speed, warn_seconds, know_about_seconds, conversation_turn_seconds, conversation_gather_seconds, run_from_honker_distance, run_from_honker_speed, warn_speech, attack_throw_speed, attack_throw_lead_seconds, attack_throw_jitter, attack_throw_lift, light_throw_speed, hand_prop_skater_contact, starting_hand_props, zombie_follow_distance, zombie_sprint_distance, zombie_ring_min, zombie_ring_max, zombie_sprint_speed, zombie_walk_speed);
    }
}

impl Merge for NpcAvoidPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; enabled, max_entries, skitch_cooldown_ticks, radius_skater, radius_pedestrian, radius_vehicle, radius_prop, cone, wide_cone, wide_cone_distance, skater_radius, speed_margin, stop_gap, stop_gap_far, stop_cone, stop_gap_prop, side_on_angle, floor_headroom, skitch_cos, low_prop_height, low_prop_time, step_off_cap, step_off_time);
    }
}

impl Merge for NpcTricksPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; mode, gate_window, min_air_frames);
    }
}

impl Merge for BackdropPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; visible, proxy_terrain);
    }
}

impl Merge for RespawnPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; air_timeout_ticks);
    }
}

impl Merge for ExposurePatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; meter_weights, meter_scale);
    }
}

impl Merge for DecalsPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; opacity);
    }
}

impl Merge for GhostPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; enabled, fade_in_seconds, hold_alpha);
    }
}

impl Merge for ShadowsPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; world_floor);
    }
}

impl LivingWorldPatch {
    pub fn validate(&self) -> bool {
        finite(self.npc_draw_distance)
            && self.skater_fade.as_ref().is_none_or(|f| finite(f.fade_in_seconds) && finite(f.fade_seconds) && finite(f.despawn_alpha))
            && self.skater_line_chain.as_ref().is_none_or(|c| finite(c.radius) && c.max_candidates.is_none_or(|n| n <= 256) && c.blend_seconds.is_none_or(|v| v.is_finite() && (0.0..=10.0).contains(&v))
                && c.facing_rule.as_deref().is_none_or(|r| NPC_SKATER_FACING_RULES.contains(&r))
                && [c.steer_dead_zone_deg, c.steer_full_deg].into_iter().all(|v| v.is_none_or(|v| v.is_finite() && (0.0..=180.0).contains(&v)))
                && [c.fakie_high_speed, c.fakie_low_speed, c.fakie_slow_seconds, c.fakie_spawn_seconds].into_iter().all(|v| v.is_none_or(|v| v.is_finite() && (0.0..=MAX_NUMBER).contains(&v))))
            && self.ped_fade.as_ref().is_none_or(|f| finite(f.fade_in_seconds) && f.distance.is_none_or(|d| d.iter().all(|v| finite(Some(*v)))))
            && self.ped_obstacles.as_ref().is_none_or(|o| {
                [o.min_half_extent, o.moving_speed, o.recut_fraction, o.detour_margin, o.step_height].into_iter().all(finite)
            })
            && self.skater_clips.as_ref().is_none_or(|m| {
                m.len() <= MAX_TEMPLATES
                    && m.iter().all(|(k, v)| {
                        let phase = k.split_once('.').map_or(k.as_str(), |(p, style)| if style.is_empty() || style.len() > 64 { "" } else { p });
                        (NPC_SKATER_PHASES.contains(&phase) || k == NPC_SKATER_FAKIE_CHANNEL || (phase == NPC_SKATER_TRICK_GROUP && k.contains('.'))) && !v.is_empty() && v.len() <= 128 && v.bytes().all(|b| b.is_ascii_graphic())
                    })
            })
            && self.skater_blend_seconds.as_ref().is_none_or(|m| {
                m.iter().all(|(k, v)| (k == "default" || NPC_SKATER_PHASES.contains(&k.as_str()) || NPC_SKATER_TRICK_BLENDS.contains(&k.as_str())) && v.is_finite() && (0.0..=MAX_BLEND_SECONDS).contains(v))
            })
            && self.skater_stance.as_ref().is_none_or(|m| {
                m.len() <= MAX_TEMPLATES
                    && m.iter().all(|(k, v)| !k.is_empty() && k.len() <= 64 && k.bytes().all(|b| b.is_ascii_graphic()) && NPC_SKATER_STANCES.contains(&v.as_str()))
            })
            && self.skater_stance_events.as_ref().is_none_or(|m| {
                m.iter().all(|(k, v)| NPC_SKATER_STANCE_EVENTS.contains(&k.as_str()) && v.len() <= 64 && v.bytes().all(|b| b.is_ascii_graphic()))
            })
            && self.npc_tricks.as_ref().is_none_or(|t| {
                t.mode.as_deref().is_none_or(|m| NPC_SKATER_TRICK_MODES.contains(&m))
                    && [t.gate_window, t.min_air_frames].into_iter().all(|v| v.is_none_or(|v| v <= MAX_TRICK_WINDOW))
            })
            && [&self.skaters, &self.pedestrians, &self.vehicles].into_iter().flatten().all(|k| k.density.is_none_or(|d| d.is_finite() && (0.0..=4.0).contains(&d)))
            && self.ambient_skaters.is_none_or(|n| n <= 8)
            && self.free_play.as_ref().is_none_or(|f| [f.traffic, f.pedestrians].into_iter().all(|v| v.is_none_or(|v| v.is_finite() && (0.0..=1.0).contains(&v))))
            && self.npc_simulated.as_ref().is_none_or(|n| {
                n.radius.is_none_or(|r| r.is_finite() && (0.0..=500.0).contains(&r))
                    && n.max.is_none_or(|m| m <= 16)
                    && [n.respawn_seconds, n.respawn_min, n.respawn_max].into_iter().all(|v| v.is_none_or(|v| v.is_finite() && (0.0..=60.0).contains(&v)))
                    && n.anticipation_distance.is_none_or(|v| v.is_finite() && (0.0..=50.0).contains(&v))
                    && n.anticipation_frames.is_none_or(|v| v <= 600)
                    && n.max_crossed_nodes.is_none_or(|v| v <= 60)
                    && n.walk_arrive_distance.is_none_or(|v| v.is_finite() && (0.0..=20.0).contains(&v))
                    && n.walk_stuck_ticks.is_none_or(|v| v <= 36000)
            })
            && self.npc_avoid.as_ref().is_none_or(NpcAvoidPatch::validate)
            && self.ped_brain.as_ref().is_none_or(|p| {
                [p.wander_speed, p.warn_seconds, p.know_about_seconds, p.conversation_turn_seconds, p.conversation_gather_seconds, p.run_from_honker_distance, p.run_from_honker_speed, p.attack_throw_speed, p.attack_throw_lead_seconds, p.attack_throw_jitter, p.attack_throw_lift, p.light_throw_speed, p.zombie_follow_distance, p.zombie_sprint_distance, p.zombie_ring_min, p.zombie_ring_max, p.zombie_sprint_speed, p.zombie_walk_speed].into_iter().all(|v| v.is_none_or(|v| v.is_finite() && (0.0..=100.0).contains(&v)))
                    && p.warn_speech.is_none_or(|v| (0..=127).contains(&v))
            })
            && self.traffic_horn.as_ref().is_none_or(|m| {
                m.len() <= 16 && m.iter().all(|(k, v)| !k.is_empty() && k.len() <= 64 && k.bytes().all(|b| b.is_ascii_graphic()) && v.validate())
            })
            && self.skater_trick_profiles.as_ref().is_none_or(|m| {
                m.len() <= MAX_TEMPLATES && m.iter().all(|(k, v)| !k.is_empty() && k.len() <= 64 && k.bytes().all(|b| b.is_ascii_graphic()) && v.validate())
            })
    }
}
impl PropTuningPatch {
    pub fn validate(&self) -> bool {
        [self.contact_padding, self.penetration_slop, self.penetration_correction, self.max_depenetration_per_tick,
            self.restitution_threshold, self.skater_push_mass, self.push_transfer, self.body_push_speed,
            self.board_push_speed, self.penetration_push_speed]
            .into_iter()
            .all(finite)
            && self.collision_box.is_none_or(|b| {
                b.center.iter().all(|v| finite(Some(*v))) && b.half_extents.iter().all(|v| v.is_finite() && *v > 0.0 && *v <= MAX_NUMBER)
            })
    }
}
impl PropSolverPatch {
    pub fn validate(&self) -> bool {
        self.iterations.is_none_or(|n| (1..=256).contains(&n))
            && self.sleep_energy.is_none_or(|e| e.is_finite() && (0.0..=MAX_NUMBER).contains(&e))
            && self.sleep_frames.is_none_or(|n| (1..=10_000).contains(&n))
            && self.max_sleeps_per_step.is_none_or(|n| n >= 1)
    }
}
impl PropUprightPatch {
    pub fn validate(&self) -> bool {
        let ok = |v: Option<f32>| v.is_none_or(|v| v.is_finite() && (0.0..=MAX_NUMBER).contains(&v));
        let positive = |v: Option<f32>| v.is_none_or(|v| v.is_finite() && v > 0.0 && v <= MAX_NUMBER);
        positive(self.window_seconds)
            && positive(self.tick_seconds)
            && [self.stop_angle_deg, self.max_angle_deg, self.dead_band_deg, self.gain_min, self.gain_max,
                self.gain_blend_start, self.off_axis_spin, self.command_rate, self.fallback_angle_deg]
                .into_iter()
                .all(ok)
    }
}
impl PropsPatch {
    pub fn validate(&self) -> bool {
        self.default.as_ref().is_none_or(PropTuningPatch::validate)
            && self.solver.as_ref().is_none_or(PropSolverPatch::validate)
            && self.upright.as_ref().is_none_or(PropUprightPatch::validate)
            && self.by_template.len() <= MAX_TEMPLATES
            && self.by_template.iter().all(|(k, v)| !k.is_empty() && k.len() <= 128 && v.validate())
    }
}
impl CarryPatch {
    pub fn validate(&self) -> bool {
        self.grab_bit.is_none_or(|b| b < 32)
            && self.placement_bit.is_none_or(|b| b < 32)
            && finite(self.grab_range)
            && [self.push_speed, self.pull_speed, self.side_speed, self.turn_rate, self.grip_reach,
                self.linear_clamp, self.yaw_clamp, self.relatch, self.slew_per_tick, self.yaw_rate_feedback, self.let_go_distance,
                self.follow_step, self.hold_angle_limit, self.hold_max_angle_to_horizontal, self.record_272_speed_scale, self.grab_end_exclusion,
                self.hand_ik_enter, self.hand_ik_rate, self.hand_ik_reach]
                .into_iter()
                .all(|v| finite(v) && v.is_none_or(|v| v >= 0.0))
            && [self.linear_controller, self.yaw_controller]
                .into_iter()
                .flatten()
                .all(|g| g.iter().all(|v| v.is_finite()) && (0.0..=1.0).contains(&g[3]))
            && [self.lever_rotation, self.lever_yaw, self.mass_speed, self.inertia_yaw_gain, self.hand_ik_curve]
                .into_iter()
                .flatten()
                .all(|c| c.iter().flatten().all(|v| v.is_finite()) && c[0].windows(2).all(|p| p[0] <= p[1]))
            && self.hold_box_extents.is_none_or(|e| e.iter().all(|v| v.is_finite() && *v >= 0.0 && *v <= MAX_NUMBER))
            && self.commanded_material.is_none_or(material_block)
            && self.upright_cos.is_none_or(|v| (-1.0..=1.0).contains(&v))
            && self.by_template.len() <= MAX_TEMPLATES
            && self.by_template.iter().all(|(k, v)| {
                !k.is_empty()
                    && k.len() <= 128
                    && [v.material_held, v.material_free, v.material_free_upright].into_iter().flatten().all(material_block)
                    && [v.restitution, v.linear_drag, v.angular_drag, v.maximum_linear_velocity, v.maximum_angular_velocity].into_iter().flatten().all(|r| r.is_finite() && (0.0..=MAX_NUMBER).contains(&r))
                    && v.mass.is_none_or(|m| m.is_finite() && m > 0.0 && m <= MAX_NUMBER)
                    && [v.inertia_scale, v.inertia_offset].into_iter().flatten().flatten().all(|x| x.is_finite() && x.abs() <= MAX_NUMBER)
            })
    }
}

/// A friction pair: two finite, non-negative values.
fn material_block(b: [f32; 2]) -> bool {
    b.iter().all(|v| v.is_finite() && (0.0..=MAX_NUMBER).contains(v))
}

impl ExposurePatch {
    pub fn validate(&self) -> bool {
        self.meter_weights.is_none_or(|c| c.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)))
            && self.meter_scale.is_none_or(|v| v.is_finite() && (0.0..=MAX_METER_SCALE).contains(&v))
    }
}

impl ShadowsPatch {
    pub fn validate(&self) -> bool {
        self.world_floor.is_none_or(|c| c.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)))
    }
}

/// A parsed patch of one domain.
#[derive(Clone, Debug, PartialEq)]
pub enum Patch {
    LivingWorld(LivingWorldPatch),
    Props(PropsPatch),
    Carry(CarryPatch),
    Shadows(ShadowsPatch),
    Backdrop(BackdropPatch),
    Respawn(RespawnPatch),
    Exposure(ExposurePatch),
    Ghost(GhostPatch),
    Decals(DecalsPatch),
}

/// Parse and validate a patch for `domain` (`None` = unknown domain, unknown field or bad value).
pub fn parse(domain: &str, patch: &Value) -> Option<Patch> {
    let p = match domain {
        "living_world" => Patch::LivingWorld(serde_json::from_value(patch.clone()).ok()?),
        "props" => Patch::Props(serde_json::from_value(patch.clone()).ok()?),
        "carry" => Patch::Carry(serde_json::from_value(patch.clone()).ok()?),
        "shadows" => Patch::Shadows(serde_json::from_value(patch.clone()).ok()?),
        "backdrop" => Patch::Backdrop(serde_json::from_value(patch.clone()).ok()?),
        "respawn" => Patch::Respawn(serde_json::from_value(patch.clone()).ok()?),
        "exposure" => Patch::Exposure(serde_json::from_value(patch.clone()).ok()?),
        "ghost" => Patch::Ghost(serde_json::from_value(patch.clone()).ok()?),
        "decals" => Patch::Decals(serde_json::from_value(patch.clone()).ok()?),
        _ => return None,
    };
    let ok = match &p {
        Patch::LivingWorld(p) => p.validate(),
        Patch::Props(p) => p.validate(),
        Patch::Carry(p) => p.validate(),
        Patch::Shadows(p) => p.validate(),
        Patch::Backdrop(_) => true,
        Patch::Respawn(p) => p.validate(),
        Patch::Exposure(p) => p.validate(),
        Patch::Ghost(p) => p.validate(),
        Patch::Decals(p) => p.validate(),
    };
    ok.then_some(p)
}

pub fn valid_patch(domain: &str, patch: &Value) -> bool {
    parse(domain, patch).is_some()
}

/// `sdk.engine.inspect(key, "world_tuning:<domain>")`.
pub fn valid_inspect(system: &str) -> bool {
    system.strip_prefix("world_tuning:").is_some_and(|d| DOMAINS.contains(&d))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn patches_parse_validate_and_reject_unknown_fields() {
        assert!(valid_patch("living_world", &json!({"npc_draw_distance": 2.0, "skater_fade": {"fade_seconds": 3.0}, "ped_fade": {"distance": [80, 100], "enabled": false}})));
        assert!(valid_patch("props", &json!({"default": {"push_transfer": 0.5}, "by_template": {"bench01": {"collision_box": {"center": [0, 0.4, 0], "half_extents": [1, 0.4, 0.3]}}}})));
        assert!(valid_patch("carry", &json!({"grab_bit": 21, "grab_range": 3.5})));
        // The NPC skaters' obstacle avoider.
        assert!(valid_patch("living_world", &json!({"npc_avoid": {"enabled": false, "speed_margin": 2.0, "radius_pedestrian": 12.0, "cone": 1.0, "max_entries": 8}})));
        assert!(!valid_patch("living_world", &json!({"npc_avoid": {"cone": 4.0}})));
        assert!(valid_patch("living_world", &json!({"ped_brain": {"warn_speech": 54}})));
        assert!(valid_patch("living_world", &json!({"traffic_horn": {"driver_taxi": {"enabled_chance": 0.0, "blocked_time": 2.0}}})));
        assert!(!valid_patch("living_world", &json!({"traffic_horn": {"all": {"enabled_chance": 2.0}}})));
        assert!(!valid_patch("living_world", &json!({"traffic_horn": {"all": {"honk": 1}}})));
        assert!(!valid_patch("living_world", &json!({"ped_brain": {"warn_speech": 128}})));
        assert!(!valid_patch("living_world", &json!({"npc_avoid": {"radius_vehicle": -1.0}})));
        assert!(!valid_patch("living_world", &json!({"npc_avoid": {"max_entries": 100}})));
        assert!(!valid_patch("living_world", &json!({"npc_avoid": {"swerve": 1.0}})));
        assert!(valid_patch("living_world", &json!({"ped_brain": {"enabled": false, "warn_seconds": 5.0}})));
        assert!(!valid_patch("living_world", &json!({"ped_brain": {"wander_speed": -1.0}})));
        assert!(valid_patch("living_world", &json!({"zombie": true, "ped_brain": {"zombie_sprint_speed": 6.0, "attack_throw_jitter": 0.0, "starting_hand_props": false}})));
        assert!(!valid_patch("living_world", &json!({"ped_brain": {"zombie_ring_max": -1.0}})));
        assert!(valid_patch("living_world", &json!({"npc_simulated": {"respawn_seconds": 3.0, "respawn_min": 0.0}})));
        assert!(!valid_patch("living_world", &json!({"npc_simulated": {"respawn_seconds": 90.0}})));
        assert!(valid_patch("living_world", &json!({"npc_simulated": {"anticipation_distance": 5.0, "anticipation_frames": 90, "max_crossed_nodes": 4}})));
        assert!(!valid_patch("living_world", &json!({"npc_simulated": {"anticipation_frames": 9000}})));
        assert!(valid_patch("living_world", &json!({"skater_clips": {"rolling": "R_IDLE_RIDE_N_0_CYC", "rolling.Aggressive": "X"}})));
        assert!(!valid_patch("living_world", &json!({"skater_clips": {"flying": "X"}})));
        assert!(!valid_patch("living_world", &json!({"skater_clips": {"air": ""}})));
        assert!(valid_patch("living_world", &json!({"skater_blend_seconds": {"default": 0.3, "air": 0.0}})));
        assert!(!valid_patch("living_world", &json!({"skater_blend_seconds": {"flying": 0.2}})));
        assert!(!valid_patch("living_world", &json!({"skater_blend_seconds": {"air": -0.1}})));
        // Fix 21: trick animations per recorded trick and the trick transition times.
        assert!(valid_patch("living_world", &json!({"skater_clips": {"trick.kickflip": "B_HEELFLIP_IN"}})));
        assert!(!valid_patch("living_world", &json!({"skater_clips": {"trick": "B_OLLIE"}})));
        assert!(valid_patch("living_world", &json!({"skater_blend_seconds": {"trick_takeoff": 0.1, "trick_air": 0.0}})));
        assert!(valid_patch("living_world", &json!({"skater_line_chain": {"blend_seconds": 0.5}})));
        assert!(!valid_patch("living_world", &json!({"skater_line_chain": {"blend_seconds": -0.5}})));
        assert!(!valid_patch("living_world", &json!({"skater_line_chain": {"blend_seconds": 11.0}})));
        assert!(valid_patch("living_world", &json!({"skater_line_chain": {"keep_facing": false}})));
        assert!(!valid_patch("living_world", &json!({"skater_line_chain": {"keep_facing": 1}})));
        assert!(valid_patch("living_world", &json!({"skater_line_chain": {"facing_rule": "per_node", "steer_dead_zone_deg": 2.0, "steer_full_deg": 10.0}})));
        assert!(!valid_patch("living_world", &json!({"skater_line_chain": {"facing_rule": "backwards"}})));
        assert!(!valid_patch("living_world", &json!({"skater_line_chain": {"steer_full_deg": -1.0}})));
        assert!(valid_patch("living_world", &json!({"skater_line_chain": {"fakie_high_speed": 2.0, "fakie_low_speed": 1.0, "fakie_slow_seconds": 0.5, "fakie_spawn_seconds": 0.0}})));
        assert!(!valid_patch("living_world", &json!({"skater_line_chain": {"fakie_low_speed": -1.0}})));
        assert!(valid_patch("living_world", &json!({"skater_clips": {"fakie_channel": "B_FAKIE_CHANNEL"}})));
        assert!(!valid_patch("living_world", &json!({"skater_clips": {"fakie_channel": ""}})));
        assert!(valid_patch("living_world", &json!({"skater_stance": {"CD56C7FE01EBE665": "regular", "danny_way": "goofy"}})));
        assert!(!valid_patch("living_world", &json!({"skater_stance": {"josh_kalis": "sideways"}})));
        assert!(!valid_patch("living_world", &json!({"skater_stance": {"": "goofy"}})));
        assert!(!valid_patch("living_world", &json!({"skater_stance": {"josh_kalis": true}})));
        assert!(valid_patch("living_world", &json!({"skater_stance_events": {"mirrored": "my_mirror", "switch": ""}})));
        assert!(!valid_patch("living_world", &json!({"skater_stance_events": {"fakie": "x"}})));
        assert!(!valid_patch("living_world", &json!({"skater_stance_events": {"mirrored": "a b"}})));
        assert!(!valid_patch("living_world", &json!({"draw": 2.0})));
        assert!(!valid_patch("living_world", &json!({"skater_fade": {"fade_seconds": 1e9}})));
        assert!(!valid_patch("props", &json!({"by_template": {"b": {"collision_box": {"center": [0, 0, 0], "half_extents": [0, 1, 1]}}}})));
        assert!(!valid_patch("carry", &json!({"grab_bit": 32})));
        assert!(valid_patch("carry", &json!({"push_speed": 2.0, "turn_rate": 0.5})));
        assert!(!valid_patch("carry", &json!({"pull_speed": -1.0})));
        assert!(valid_patch("carry", &json!({"grip_reach": 0.5})));
        assert!(!valid_patch("carry", &json!({"grip_reach": -0.1})));
        assert!(valid_patch("carry", &json!({"commanded_material": [0.1, 0.02], "apply_at_com": false, "wake_on_command": true})));
        assert!(!valid_patch("carry", &json!({"commanded_material": [-0.1, 0.02]})));
        assert!(valid_patch("carry", &json!({"upright_cos": 0.65, "by_template": {"t": {"material_free_upright": [0.9, 0.7], "upright_pair": true, "restitution": 0.2}}})));
        assert!(!valid_patch("carry", &json!({"upright_cos": 1.5})));
        assert!(!valid_patch("carry", &json!({"by_template": {"t": {"material_free_upright": [0.9, -0.7]}}})));
        assert!(!valid_patch("carry", &json!({"by_template": {"t": {"restitution": -0.1}}})));
        assert!(valid_patch("carry", &json!({"by_template": {"bin": {"material_held": [0.2, 0.0], "material_free": [0.5, 0.1]}}})));
        assert!(!valid_patch("carry", &json!({"by_template": {"bin": {"material_held": [0.2]}}})));
        assert!(!valid_patch("carry", &json!({"by_template": {"bin": {"friction": 1.0}}})));
        assert!(!valid_patch("carry", &json!({"apply_at_com": 1})));
        assert!(valid_patch("shadows", &json!({"world_floor": [0.05, 0.09, 0.13]})));
        assert!(!valid_patch("shadows", &json!({"world_floor": [0.05, 0.09]})));
        assert!(!valid_patch("shadows", &json!({"world_floor": [0.05, 0.09, 1.5]})));
        assert!(!valid_patch("shadows", &json!({"floor": [0.0, 0.0, 0.0]})));
        assert!(valid_patch("exposure", &json!({"meter_weights": [0.3, 0.4, 0.3], "meter_scale": 2.515})));
        assert!(!valid_patch("exposure", &json!({"meter_weights": [0.3, 0.4]})));
        assert!(!valid_patch("exposure", &json!({"meter_weights": [0.3, 1.4, 0.3]})));
        assert!(!valid_patch("exposure", &json!({"meter_scale": -1.0})));
        assert!(!valid_patch("exposure", &json!({"target": 0.25})));
        assert!(valid_patch("backdrop", &json!({"visible": false})));
        assert!(!valid_patch("backdrop", &json!({"visible": 0})));
        assert!(valid_patch("backdrop", &json!({"proxy_terrain": false})));
        assert!(!valid_patch("backdrop", &json!({"proxy_terrain": "off"})));
        assert!(!valid_patch("backdrop", &json!({"hidden": true})));
        assert!(valid_patch("respawn", &json!({"air_timeout_ticks": 300})));
        assert!(valid_patch("respawn", &json!({"air_timeout_ticks": 216_000})));
        assert!(!valid_patch("respawn", &json!({"air_timeout_ticks": 0})));
        assert!(!valid_patch("respawn", &json!({"air_timeout_ticks": 216_001})));
        assert!(!valid_patch("respawn", &json!({"air_timeout_ticks": 2.5})));
        assert!(!valid_patch("respawn", &json!({"air_timeout_ticks": -1})));
        assert!(!valid_patch("respawn", &json!({"air_timeout": 300})));
        assert!(valid_inspect("world_tuning:respawn"));
        assert!(!valid_patch("roads", &json!({})));
        assert!(valid_inspect("world_tuning:carry") && !valid_inspect("world_tuning:x"));
    }

    #[test]
    fn first_writer_wins_per_field() {
        let Some(Patch::LivingWorld(mut a)) = parse("living_world", &json!({"skater_fade": {"fade_seconds": 2.0}})) else { panic!() };
        let Some(Patch::LivingWorld(b)) = parse("living_world", &json!({"npc_draw_distance": 3.0, "skater_fade": {"fade_seconds": 5.0, "despawn_alpha": 0.1}})) else { panic!() };
        a.merge(&b);
        assert_eq!(a.npc_draw_distance, Some(3.0));
        let f = a.skater_fade.unwrap();
        assert_eq!((f.fade_seconds, f.despawn_alpha, f.fade_in_seconds), (Some(2.0), Some(0.1), None));
    }
}
