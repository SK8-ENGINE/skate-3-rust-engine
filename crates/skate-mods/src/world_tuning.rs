//! World tuning writes (capability `world_tuning` = 1): typed `sdk.world.set_tuning(domain, patch)` patches.
//! Domains, fields and retail defaults: docs/hails-additions/26a-living-world-overview.md#world-tuning-domains

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DOMAINS: [&str; 12] = ["living_world", "props", "carry", "shadows", "backdrop", "respawn", "exposure", "ghost", "decals", "video_screens", "prop_sounds", "vehicles"];

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
    /// A bailed skater fades out before its respawn (retail true).
    pub bail_fade: Option<bool>,
    /// Seconds before the respawn that fade starts (retail 1.0).
    pub bail_fade_lead_seconds: Option<f32>,
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
    /// NPC skater grind board-to-CoG height 0..1 (retail rest: the setup data's `max_grind_disttocog`, 0.85).
    pub npc_grind_disttocog: Option<f32>,
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
    /// Peds as solid bodies the skater collides with.
    pub ped_body: Option<PedBodyPatch>,
    /// Ped navigation: the walking steer rate, the turn bucket edges, the pinned-ped watch and the crosswalk rule.
    pub ped_nav: Option<PedNavPatch>,
    /// Traffic cars touching peds (`skate_core::living_world::peds::VehicleContactParams`).
    pub ped_vehicle_contact: Option<PedVehicleContactPatch>,
    /// Traffic cars are ground for the on-foot skater (retail on): off, an on-foot jump lands through a roof.
    pub on_foot_vehicle_ground: Option<bool>,
    /// NPC skater trick choice (`skate_core::living_world::npc_tricks`).
    pub npc_tricks: Option<NpcTricksPatch>,
    /// NPC skater trick tables per character key or `ai_skater_profiles` name.
    pub skater_trick_profiles: Option<BTreeMap<String, TrickTablesPatch>>,
    /// Simulated NPC skaters (`skate-game` `living_world::npc_sim`).
    pub npc_simulated: Option<NpcSimulatedPatch>,
    /// The NPC skaters' obstacle avoider (retail values by default).
    pub npc_avoid: Option<NpcAvoidPatch>,
    /// The NPC skaters' walk-back controller (controller B); applied after `npc_simulated`, so it wins over the
    /// `walk_arrive_distance` / `walk_stuck_ticks` aliases.
    pub npc_walk_back: Option<NpcWalkBackPatch>,
    /// The NPC skaters' line controller (stuck tracker, re-snap, reposition, safe node).
    pub npc_line_controller: Option<NpcLineControllerPatch>,
    /// The simulated NPC skaters' path follower: line end trigger and lost tests.
    pub skater_path: Option<SkaterPathPatch>,
    /// The ped behavior runtime (stock ped AI graph on each ped's brain).
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
    /// After a placement the skater holds (no steer, no trick) this long (s, 0..=10; default 0.5).
    pub placement_hold_seconds: Option<f32>,
    /// The hold after a bail respawn (s, 0..=10; default 8 ticks, retail `8246EA90`).
    pub respawn_hold_seconds: Option<f32>,
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
    /// The takeoff board yaw toward the recorded board angle (`BoardAdjustAngle` / `BoardAdjustMag` in the air, so a
    /// recorded slide lands as recorded; research b113; retail on), its dead zone (rad, 0..=pi/2; retail 0.31416) and
    /// magnitude scale (0..=100; retail 2/pi = 0.63662).
    pub board_yaw: Option<bool>,
    pub board_yaw_dead_zone: Option<f32>,
    pub board_yaw_mag_scale: Option<f32>,
    /// The air steer toward the recorded skater forward (`sub_8246B358`, b123; retail on), its dead zone and full-stick
    /// angle (deg, 0..=180; retail 2 and 10).
    pub air_steer: Option<bool>,
    pub air_steer_dead_zone_deg: Option<f32>,
    pub air_steer_full_deg: Option<f32>,
    /// While avoid-steering, the far aim's fallback anchor ahead of the cursor, recorded ticks (0..=600; retail 15).
    pub avoid_anchor_ticks: Option<f32>,
    /// The `Manual` value an NPC holds a manual with (+ nosemanual, - tailmanual; 0..=1; retail 0.5).
    pub manual_value: Option<f32>,
    /// The player and simulated NPCs bump as full skaters (a hard hit knocks one down; retail on).
    pub bump: Option<bool>,
    /// NPC skaters react to wipeouts near them (slam / slam_b / crash lines; retail on) within `reaction_radius` m
    /// (retail 15, 0..=100).
    pub reactions: Option<bool>,
    pub reaction_radius: Option<f32>,
    /// AI skaters push while slower than their line (retail on); push fields and retail values: docs/hails-additions/26a-living-world-overview.md#ai-skater-push-fields
    pub push: Option<bool>,
    pub push_factor: Option<f32>,
    pub push_reduce: Option<f32>,
    pub push_fast_speed: Option<f32>,
    pub push_downhill_slope: Option<f32>,
    pub push_max_heading_deg: Option<f32>,
    pub push_right_foot: Option<bool>,
    /// OURS, opt-in (retail off): NPC skaters pump down slopes with the player's auto pump intents.
    pub auto_pump: Option<bool>,
    /// AI skaters brake far above their line's speed (retail on): when speed x `brake_factor` (0..=1, retail 0.3)
    /// beats the line's speed, ramping to full over `brake_ramp` m/s (0..=100, retail 0.1).
    pub brake: Option<bool>,
    pub brake_factor: Option<f32>,
    pub brake_ramp: Option<f32>,
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

/// Ped behavior runtime fields (0..=100 each) and retail values: docs/hails-additions/26a-living-world-overview.md#ped-behavior-fields
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PedBrainPatch {
    pub enabled: Option<bool>,
    pub mood: Option<bool>,
    pub wander_speed: Option<f32>,
    pub warn_seconds: Option<f32>,
    pub know_about_seconds: Option<f32>,
    pub mood_want_memory_seconds: Option<f32>,
    pub conversation_turn_seconds: Option<f32>,
    pub conversation_gather_seconds: Option<f32>,
    /// CannotReachChaseTarget: the radius (retail 5.0 m) and how often the mover checks reachability (retail 10
    /// mover updates, 1/3 s at 30 fps); 0..=100.
    pub unreachable_radius: Option<f32>,
    /// The chase reach check's snap window along X and Z, m (0..100; retail 0.65 = 0.5 x 1.3).
    pub reach_snap_window: Option<f32>,
    /// The ped scatter alarm's length after the player's D-pad Down gesture, s (0..3600; 0 = off; retail 180).
    pub scatter_seconds: Option<f32>,
    pub scatter_arrival_radius: Option<f32>,
    pub scatter_channel_fade: Option<f32>,
    /// Hold the scatter alarm raised (ours; released, the retail timer runs out).
    pub scatter_hold: Option<bool>,
    pub reach_check_seconds: Option<f32>,
    pub run_from_honker_distance: Option<f32>,
    pub run_from_honker_speed: Option<f32>,
    pub warn_speech: Option<i32>,
    /// A moved bench with a seat in water stops offering its seats (ours, default true) and how deep under a water
    /// surface still counts (m, 0..=100, default 10.0) and how far over it (`water_above`, m, 0..=100, default 0.8).
    pub water_disables_seats: Option<bool>,
    pub water_max_depth: Option<f32>,
    pub water_above: Option<f32>,
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
    /// speeds (8, 3 m/s), goal reached distance (2 m); 0..=100.
    pub zombie_follow_distance: Option<f32>,
    pub zombie_arrive_distance: Option<f32>,
    pub zombie_sprint_distance: Option<f32>,
    pub zombie_ring_min: Option<f32>,
    pub zombie_ring_max: Option<f32>,
    pub zombie_sprint_speed: Option<f32>,
    pub zombie_walk_speed: Option<f32>,
    /// The reaction a failed takedown's trip stores: `trip_kind` "Knockdown" (retail) or "Standing", `trip_direction`
    /// "FromBack" (retail), "FromFront", "FromLeft" or "FromRight".
    pub trip_kind: Option<String>,
    pub trip_direction: Option<String>,
    /// SkaterAvoidance (a ped reacting to a skater coming at it; retail values): movers within `skater_avoid_sense`
    /// (10 m) and faster than `skater_avoid_min_speed` (1 m/s) whose closest approach comes within
    /// `skater_avoid_look_ahead` (0.75 s) and `skater_avoid_radius` (3 m); a Jump when closer than
    /// `skater_avoid_jump_radius` (1 m) and faster than `skater_avoid_jump_speed` (5 m/s), else a Step; Forward / Back
    /// sectors `skater_avoid_sector` rad either side (0.5236); tested every `skater_avoid_period` s (1/3); 0..=100.
    pub skater_avoid_sense: Option<f32>,
    pub skater_avoid_min_speed: Option<f32>,
    pub skater_avoid_look_ahead: Option<f32>,
    pub skater_avoid_radius: Option<f32>,
    pub skater_avoid_jump_radius: Option<f32>,
    pub skater_avoid_jump_speed: Option<f32>,
    pub skater_avoid_sector: Option<f32>,
    pub skater_avoid_period: Option<f32>,
    /// Standing and walking peds dodge too (retail true); false keeps the dodge to seated peds.
    pub skater_avoid_standing: Option<bool>,
    /// A prop moving at a ped makes it flee (retail on): within `object_flee_radius` m of the prop (8.0), the prop
    /// at least `object_flee_min_speed` m/s (0.75), the ped inside its swept lane plus `object_flee_lane_margin` m
    /// (0.75); 0..=100.
    pub object_flee: Option<bool>,
    pub object_flee_radius: Option<f32>,
    pub object_flee_min_speed: Option<f32>,
    pub object_flee_lane_margin: Option<f32>,
    /// A car near a ped moving along the line between them makes it flee (the Ped Vehicle Flee opt-in, off by
    /// default): within `vehicle_flee_radius` m (10.0), the flat dot of ped-to-car and the car's velocity past
    /// `vehicle_flee_min_dot` (3.0) away (`vehicle_flee_away`, retail's code test) or toward (`vehicle_flee_toward`).
    pub vehicle_flee: Option<bool>,
    pub vehicle_flee_radius: Option<f32>,
    pub vehicle_flee_min_dot: Option<f32>,
    pub vehicle_flee_away: Option<bool>,
    pub vehicle_flee_toward: Option<bool>,
    /// Retail's gates before the flee test (`82E2C0C8`, b821): the ped stands on a road (`vehicle_flee_require_on_road`,
    /// ped vf200) and the car is flagged, held / skitched (`vehicle_flee_require_flagged`, IVehicle vf0x60); both true.
    pub vehicle_flee_require_on_road: Option<bool>,
    pub vehicle_flee_require_flagged: Option<bool>,
    /// The flee leg (b8 / b650): `flee_leg` m away from the threat (15.0), a new leg within `flee_arrival_radius` m of
    /// the goal (2.0); a threat at least `flee_threat_speed_gate` m/s (1.0) bends the leg to the ped's side by
    /// `flee_side_blend` (0.5); 0..=100.
    pub flee_leg: Option<f32>,
    pub flee_arrival_radius: Option<f32>,
    pub flee_threat_speed_gate: Option<f32>,
    pub flee_side_blend: Option<f32>,
    /// A new flee leg when the ped's path ends or fails short of the goal (retail, true).
    pub flee_releg_on_nav_done: Option<bool>,
}

/// The names `PedBrainPatch::trip_kind` / `trip_direction` take (skate-core `ReactionKind` / `ReactionDirection`).
pub const TRIP_KINDS: [&str; 2] = ["Knockdown", "Standing"];
pub const TRIP_DIRECTIONS: [&str; 4] = ["FromFront", "FromBack", "FromLeft", "FromRight"];

/// One traffic driver's horn (read when a car spawns); fields and retail values: docs/hails-additions/26a-living-world-overview.md#traffic-horn-fields
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

/// Ped navigation fields (`peds::wander`) and retail / our values: docs/hails-additions/26a-living-world-overview.md#ped-navigation-fields
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PedNavPatch {
    pub steer_rate: Option<f32>,
    pub turn_edges: Option<[f32; 4]>,
    pub stall_seconds: Option<f32>,
    pub stuck_distance: Option<f32>,
    pub stuck_seconds: Option<f32>,
    pub give_up_seconds: Option<f32>,
    pub yield_patience: Option<f32>,
    /// How far an unreachable wander / route target may move to the nearest reachable navmesh point (meters).
    pub rewrite_range: Option<f32>,
    pub crosswalk: Option<String>,
}

/// Peds as solid bodies in the skater's contact solve (retail collision group 11, `sub_82BD4A30`): the skater is
/// stopped or deflected and a hard hit can bail it. `parts` replaces the whole part list.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PedBodyPatch {
    pub enabled: Option<bool>,
    pub parts: Option<Vec<PedBodyPartPatch>>,
    pub look_ahead: Option<f32>,
    pub range: Option<f32>,
    /// Index into `parts` of the part whose speed decides the ped's reaction (retail 0, the torso).
    pub reaction_part: Option<usize>,
    /// Body A, the movement proxy props are stopped by: `proxy` on / off (retail on), its capsule `proxy_radius` /
    /// `proxy_half_length` (m, retail 0.3125 / 0.5375) and `proxy_mass` (kg, retail 10).
    pub proxy: Option<bool>,
    pub proxy_radius: Option<f32>,
    pub proxy_half_length: Option<f32>,
    pub proxy_mass: Option<f32>,
}

/// One capsule between two rig bones: centered at `centre` (0 = `from`, 1 = `to`), along `from -> to`; m / kg.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PedBodyPartPatch {
    pub from: String,
    pub to: String,
    pub centre: f32,
    pub radius: f32,
    pub half_length: f32,
    pub mass: f32,
    /// The part's contact friction, 0..=2 (absent: retail 0.0).
    #[serde(default)]
    pub friction: Option<f32>,
}

/// Ped obstacle rules (`peds::ObstacleParams`) and retail / our values: docs/hails-additions/26a-living-world-overview.md#ped-obstacle-fields
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PedObstaclesPatch {
    pub enabled: Option<bool>,
    pub min_half_extent: Option<f32>,
    pub moving_speed: Option<f32>,
    pub recut_fraction: Option<f32>,
    pub detour_margin: Option<f32>,
    pub target_snap: Option<f32>,
    pub step_height: Option<f32>,
    pub held_is_obstacle: Option<bool>,
    pub moving_solid: Option<bool>,
    /// Player skaters are ped obstacles (retail on, b135) and their box half extents (NOT RETAIL YET size).
    pub skaters: Option<bool>,
    pub skater_half_extents: Option<[f32; 3]>,
    /// Contact slide round a moving body (`82E9D0C0`): fraction of the push (0.7) and the heading-in dot (-0.3).
    pub contact_slide: Option<f32>,
    pub contact_slide_dot: Option<f32>,
    /// Predictive side-step (`82E9CE90` / `82E9CBD0`): look-ahead s (3.0, 0 = off), ray scale (0.99), acceleration
    /// base (3.0) x NavPower scale (1.3), head-on (-0.7) and same-direction (0.7) dots.
    pub avoid_horizon: Option<f32>,
    pub avoid_ray_scale: Option<f32>,
    pub avoid_accel: Option<f32>,
    pub avoid_scale: Option<f32>,
    pub avoid_head_on_dot: Option<f32>,
    pub avoid_crossing_dot: Option<f32>,
    /// A moving avoider's extents `{a, t a, t a}`, `a = avoid_scale x avoider_extent` (0.25, t 0.2).
    pub avoider_extent: Option<f32>,
    pub avoider_extent_tail: Option<f32>,
    /// The path turn term that pulls a side-step back (`82E9DF70`, research b716): accel m/s^2 (40), full below
    /// `path_turn_full_deg` (5), boosted above `path_turn_boost_deg` (10) by `path_turn_boost_rate` per rad (4.5837) up
    /// to `path_turn_boost_max` (5), none from `path_turn_max_deg` (120), capped by `v^2 / (2 path_turn_radius)` (0.35 m)
    /// and the sideways speed over `controller_step` (1 / 30 s).
    pub path_turn_accel: Option<f32>,
    pub path_turn_full_deg: Option<f32>,
    pub path_turn_boost_deg: Option<f32>,
    pub path_turn_boost_rate: Option<f32>,
    pub path_turn_boost_max: Option<f32>,
    pub path_turn_max_deg: Option<f32>,
    pub path_turn_radius: Option<f32>,
    pub controller_step: Option<f32>,
    /// The contact slide's speed tests: the slow fraction (0.5) of the path speed `contact_max_speed` (2.0 m/s).
    pub contact_slow_fraction: Option<f32>,
    pub contact_max_speed: Option<f32>,
    /// Braking at the path end: deceleration (80, 0 = off), goal radius (2.0), body radius (1.225), away fraction (0.25).
    pub brake_decel: Option<f32>,
    pub brake_goal_radius: Option<f32>,
    pub brake_body_radius: Option<f32>,
    pub brake_away_fraction: Option<f32>,
}

/// NPC skater line end, facing rule and riding-fakie fields (`replay::ChainConfig`): docs/hails-additions/26a-living-world-overview.md#skater-line-chain-fields
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
    /// The mode 1 fallback takes the nearest invalid start when the nearest valid one is at least this far (m).
    pub fallback_valid_m: Option<f32>,
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
    /// Better Water: the prop type's density in water, kg/m3 (10..=20000; unset = by its template name).
    pub water_density: Option<f32>,
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

/// Upright self-righting fields and retail defaults: docs/hails-additions/26a-living-world-overview.md#upright-fields
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

/// Prop contact solver, sleep and bail clamp fields (8275DCC8 -> 82DC2840, 82C530F0): docs/hails-additions/26a-living-world-overview.md#prop-solver-fields
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropSolverPatch {
    pub row_solver: Option<bool>,
    pub iterations: Option<u32>,
    pub sleep_energy: Option<f32>,
    pub sleep_frames: Option<u32>,
    pub max_sleeps_per_step: Option<u32>,
    pub rest_snap: Option<bool>,
    pub bail_clamp: Option<bool>,
    pub bail_linear_factor: Option<f32>,
    pub bail_angular_factor: Option<f32>,
    pub bail_heavy_mass: Option<f32>,
    pub bail_blend: Option<f32>,
    pub walkable_mass: Option<f32>,
    pub skater_push_box: Option<bool>,
    /// Skater mesh contacts as rows in the prop solve (retail, b617; default on) and the board side's mass (kg, 0..=1000;
    /// default 8).
    pub skater_rows: Option<bool>,
    pub board_row_mass: Option<f32>,
    pub ground_reduction: Option<bool>,
    pub reduction_normal_dot: Option<f32>,
    pub reduction_plane_distance: Option<f32>,
    pub reduction_chunk: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarryPatch {
    /// Packed controller button bit (0..=31) held to grab and carry (retail 28, RB).
    pub grab_bit: Option<u32>,
    /// Bit whose rising edge toggles placement (retail 20, B).
    pub placement_bit: Option<u32>,
    /// Pickup reach in meters to the prop's surface (engine default 2.0).
    pub grab_range: Option<f32>,
    /// Move Object speed pushing forward, m/s at full stick (engine default 1.4).
    pub push_speed: Option<f32>,
    /// Move Object speed pulling back, m/s at full stick (engine default 1.0).
    pub pull_speed: Option<f32>,
    /// Move Object side step, m/s at full stick (engine default 0.8).
    pub side_speed: Option<f32>,
    /// Move Object turn, rad/s at full right stick X (engine default 1.6).
    pub turn_rate: Option<f32>,
    /// Gap in meters between the grab edge and the skater (engine default 0.35).
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
    /// Meters the skater may fall behind its grab point before letting go (engine rule for mods; default 0 = off, retail lets go by record qualification).
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
    /// Root yaw ease over the time held (retail 023D73966B8669A5, 82D46610): share of the angle to the grab edge
    /// the skater turns per tick, `[[8 x], [8 y]]` (0.18 at the grab, 1.0 from 0.5 s; all y 1.0 = snap).
    pub root_yaw_gain: Option<[[f32; 8]; 2]>,
    /// Hand IK weight change per tick (retail 0.2, 82D45008).
    pub hand_ik_rate: Option<f32>,
    /// Hand IK reach around the animated hands in m (retail 0.65, 82BD9728).
    pub hand_ik_reach: Option<f32>,
    /// Friction pair `[static, dynamic]` every held (commanded) prop switches to (retail [0.03, 0.02],
    /// 82C53EF8); combined with the other side by max / max / min (82763078).
    pub commanded_material: Option<[f32; 2]>,
    /// Upright test on the prop's up axis y for the upright free pair (retail 0.65, 82C54B00; -1..1).
    pub upright_cos: Option<f32>,
    /// Linear command at the center of mass (retail true; false = at the grip point, lever torque).
    pub apply_at_com: Option<bool>,
    /// Yaw command replaces the prop's angular accumulator (retail true; false = added).
    pub yaw_replaces_torque: Option<bool>,
    /// Vertical command dropped (retail true).
    pub ignore_vertical: Option<bool>,
    /// Every command wakes the prop (retail true; false = only a non-zero command).
    pub wake_on_command: Option<bool>,
    /// Per prop type held / free blocks by MOBJ template or vault record name (template wins); unset keeps retail.
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
    /// Ped plugins on this prop type stop being offered (seated peds evicted) past these: per-axis rotation
    /// tolerance from the authored pose, degrees (retail (2.5, 15, 2.5)), and max distance moved (retail 5 m).
    pub plugin_rotation_tolerance: Option<[f32; 3]>,
    pub plugin_max_distance: Option<f32>,
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

/// Traffic cars (ask 611): which cars the census spawns and what they look like. Names are the export's stable ones
/// (`vehicles.json`: census categories, entities, `livingworld_models` records). Empty = retail.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VehiclesPatch {
    /// New entity -> the existing entity it starts as (its spec, follower values, colors and size).
    #[serde(default)]
    pub entities: BTreeMap<String, String>,
    /// Census category -> the entities it spawns, in pick order (replaces the category's list; `[]` spawns none).
    #[serde(default)]
    pub spawn_tables: BTreeMap<String, Vec<String>>,
    /// Entity -> the `livingworld_models` record it draws.
    #[serde(default)]
    pub models: BTreeMap<String, String>,
    /// Model record -> a GLB asset path drawn instead of the export's.
    #[serde(default)]
    pub glbs: BTreeMap<String, String>,
    /// Scale on both tint colors (retail 1).
    pub paint_gain: Option<f32>,
}

impl VehiclesPatch {
    fn validate(&self) -> bool {
        let named = |s: &String| !s.is_empty() && s.len() <= 128;
        self.paint_gain.is_none_or(|g| g.is_finite() && (0.0..=8.0).contains(&g))
            && self.entities.iter().all(|(k, v)| named(k) && named(v) && k != v)
            && self.spawn_tables.iter().all(|(k, v)| named(k) && v.len() <= 256 && v.iter().all(named))
            && self.models.iter().chain(&self.glbs).all(|(k, v)| named(k) && named(v))
    }
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

/// The park videoscreens' live feed: docs/hails-additions/35-world-shading-parity.md#videoscreen-live-feed
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoScreensPatch {
    /// Live feed (true) or the static screen texture (false).
    pub enabled: Option<bool>,
    /// Feed picture size in pixels, 16..=MAX_VIDEO_FEED_SIZE.
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Feed updates per second, 1..=240.
    pub rate_hz: Option<f32>,
    /// The feed renders only while a screen is in view within this distance (m), 1..=10000.
    pub max_distance: Option<f32>,
}

/// Largest accepted feed width / height.
pub const MAX_VIDEO_FEED_SIZE: u32 = 2048;

/// The dynamic props' impact sounds: docs/hails-additions/27-dynamic-props.md#prop-sounds
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropSoundsPatch {
    /// Impact sounds on (retail true).
    pub enabled: Option<bool>,
    /// Per prop type key (the characteristics record name, e.g. `dumpster`): fields over its retail record.
    #[serde(default)]
    pub types: BTreeMap<String, PropSoundPatch>,
}

/// One prop type's impact record fields (unset = the retail record; a type without one starts silent).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropSoundPatch {
    /// Collision materials 0..=143 (143 = none).
    pub material: Option<i32>,
    pub secondary: Option<i32>,
    pub use_contact_material: Option<bool>,
    /// Impact range (closing speed, m/s), 0..=MAX_NUMBER.
    pub impact_min: Option<f32>,
    pub impact_max: Option<f32>,
    /// Console evaluations between posts, 0..=600.
    pub cooldown: Option<i32>,
    /// Attenuation limits (m/s, rad/s), above 0.
    pub speed_attenuation: Option<f32>,
    pub spin_attenuation: Option<f32>,
    /// The loop banks' patch (w9; 0..=1000) and the speed of a full loop speed word (km/h, above 0).
    pub patch: Option<i32>,
    pub loop_max_kmh: Option<f32>,
}

impl PropSoundsPatch {
    pub fn validate(&self) -> bool {
        let material = |v: Option<i32>| v.is_none_or(|v| (0..=143).contains(&v));
        let number = |v: Option<f32>| v.is_none_or(|v| v.is_finite() && (0.0..=MAX_NUMBER).contains(&v));
        let limit = |v: Option<f32>| v.is_none_or(|v| v.is_finite() && v > 0.0 && v <= MAX_NUMBER);
        self.types.iter().all(|(key, t)| {
            !key.is_empty()
                && material(t.material)
                && material(t.secondary)
                && number(t.impact_min)
                && number(t.impact_max)
                && t.cooldown.is_none_or(|v| (0..=600).contains(&v))
                && limit(t.speed_attenuation)
                && limit(t.spin_attenuation)
                && t.patch.is_none_or(|v| (0..=1000).contains(&v))
                && limit(t.loop_max_kmh)
        })
    }
}

impl VideoScreensPatch {
    pub fn validate(&self) -> bool {
        let size = |v: Option<u32>| v.is_none_or(|v| (16..=MAX_VIDEO_FEED_SIZE).contains(&v));
        size(self.width)
            && size(self.height)
            && self.rate_hz.is_none_or(|v| v.is_finite() && (1.0..=240.0).contains(&v))
            && self.max_distance.is_none_or(|v| v.is_finite() && (1.0..=10_000.0).contains(&v))
    }
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
        merge_opts!(self, b; fade_in_seconds, fade_seconds, despawn_alpha, bail_fade, bail_fade_lead_seconds);
    }
}
impl Merge for PedNavPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; steer_rate, turn_edges, stall_seconds, stuck_distance, stuck_seconds, give_up_seconds, yield_patience, rewrite_range, crosswalk);
    }
}

impl Merge for PedObstaclesPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; enabled, min_half_extent, moving_speed, recut_fraction, detour_margin, target_snap, step_height, held_is_obstacle, moving_solid, skaters, skater_half_extents, contact_slide, contact_slide_dot, avoid_horizon, avoid_ray_scale, avoid_accel, avoid_scale, avoid_head_on_dot, avoid_crossing_dot, avoider_extent, avoider_extent_tail, path_turn_accel, path_turn_full_deg, path_turn_boost_deg, path_turn_boost_rate, path_turn_boost_max, path_turn_max_deg, path_turn_radius, controller_step, contact_slow_fraction, contact_max_speed, brake_decel, brake_goal_radius, brake_body_radius, brake_away_fraction);
    }
}
impl Merge for VehiclesPatch {
    fn merge(&mut self, b: &Self) {
        for (mine, later) in [(&mut self.entities, &b.entities), (&mut self.models, &b.models), (&mut self.glbs, &b.glbs)] {
            for (k, v) in later {
                mine.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
        for (k, v) in &b.spawn_tables {
            self.spawn_tables.entry(k.clone()).or_insert_with(|| v.clone());
        }
        merge_opts!(self, b; paint_gain);
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
impl Merge for PedBodyPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; enabled, parts, look_ahead, range, reaction_part, proxy, proxy_radius, proxy_half_length, proxy_mass);
    }
}
impl Merge for SkaterLineChainPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; radius, max_candidates, blend_seconds, keep_facing, facing_rule, steer_dead_zone_deg, steer_full_deg, fakie_high_speed, fakie_low_speed, fakie_slow_seconds, fakie_spawn_seconds, fallback_valid_m);
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
        merge_opts!(self, b; npc_draw_distance, npc_grind_disttocog, on_foot_vehicle_ground);
        merge_nested(&mut self.skater_fade, &b.skater_fade);
        merge_nested(&mut self.skater_line_chain, &b.skater_line_chain);
        merge_nested(&mut self.ped_fade, &b.ped_fade);
        merge_nested(&mut self.ped_obstacles, &b.ped_obstacles);
        merge_nested(&mut self.npc_skater_props, &b.npc_skater_props);
        merge_nested(&mut self.ped_body, &b.ped_body);
        merge_nested(&mut self.ped_nav, &b.ped_nav);
        merge_nested(&mut self.ped_vehicle_contact, &b.ped_vehicle_contact);
        merge_nested(&mut self.npc_tricks, &b.npc_tricks);
        merge_nested(&mut self.npc_simulated, &b.npc_simulated);
        merge_nested(&mut self.npc_avoid, &b.npc_avoid);
        merge_nested(&mut self.npc_walk_back, &b.npc_walk_back);
        merge_nested(&mut self.npc_line_controller, &b.npc_line_controller);
        merge_nested(&mut self.skater_path, &b.skater_path);
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
            penetration_push_speed, stuck_release_ticks, collision_box, water_density);
    }
}
impl Merge for PropSolverPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; row_solver, iterations, sleep_energy, sleep_frames, max_sleeps_per_step, rest_snap, bail_clamp,
            bail_linear_factor, bail_angular_factor, bail_heavy_mass, bail_blend, walkable_mass, skater_push_box, skater_rows, board_row_mass,
            ground_reduction, reduction_normal_dot, reduction_plane_distance, reduction_chunk);
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
            hold_box_extents, record_272_speed_scale, grab_end_exclusion, hand_ik_enter, hand_ik_curve, hand_ik_rate, hand_ik_reach, root_yaw_gain, commanded_material, upright_cos,
            apply_at_com, yaw_replaces_torque, ignore_vertical, wake_on_command);
        for (k, v) in &b.by_template {
            self.by_template.entry(k.clone()).and_modify(|a| { merge_opts!(a, v; material_held, material_free, material_free_upright, upright_pair, restitution, record_272, linear_drag, angular_drag, mass, maximum_linear_velocity, maximum_angular_velocity, inertia_scale, inertia_offset, plugin_rotation_tolerance, plugin_max_distance); }).or_insert_with(|| v.clone());
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
        merge_opts!(self, b; enabled, radius, max, respawn_seconds, respawn_min, respawn_max, placement_hold_seconds, respawn_hold_seconds, anticipation_distance, anticipation_frames, max_crossed_nodes, walk_back, walk_arrive_distance, walk_stuck_ticks, board_yaw, board_yaw_dead_zone, board_yaw_mag_scale, air_steer, air_steer_dead_zone_deg, air_steer_full_deg, avoid_anchor_ticks, manual_value, bump, reactions, reaction_radius, push, push_factor, push_reduce, push_fast_speed, push_downhill_slope, push_max_heading_deg, push_right_foot, auto_pump, brake, brake_factor, brake_ramp);
    }
}

impl Merge for PedBrainPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; enabled, mood, wander_speed, warn_seconds, know_about_seconds, mood_want_memory_seconds, conversation_turn_seconds, conversation_gather_seconds, unreachable_radius, reach_check_seconds, reach_snap_window, scatter_seconds, scatter_arrival_radius, scatter_channel_fade, scatter_hold, run_from_honker_distance, run_from_honker_speed, warn_speech, water_disables_seats, water_max_depth, water_above, attack_throw_speed, attack_throw_lead_seconds, attack_throw_jitter, attack_throw_lift, light_throw_speed, hand_prop_skater_contact, starting_hand_props, zombie_follow_distance, zombie_sprint_distance, zombie_ring_min, zombie_ring_max, zombie_sprint_speed, zombie_walk_speed, zombie_arrive_distance, trip_kind, trip_direction, skater_avoid_sense, skater_avoid_min_speed, skater_avoid_look_ahead, skater_avoid_radius, skater_avoid_jump_radius, skater_avoid_jump_speed, skater_avoid_sector, skater_avoid_period, skater_avoid_standing, object_flee, object_flee_radius, object_flee_min_speed, object_flee_lane_margin, vehicle_flee, vehicle_flee_radius, vehicle_flee_min_dot, vehicle_flee_away, vehicle_flee_toward, vehicle_flee_require_on_road, vehicle_flee_require_flagged, flee_leg, flee_arrival_radius, flee_threat_speed_gate, flee_side_blend, flee_releg_on_nav_done);
    }
}

impl Merge for NpcAvoidPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; enabled, max_entries, skitch_cooldown_ticks, radius_skater, radius_pedestrian, radius_vehicle, radius_prop, cone, wide_cone, wide_cone_distance, skater_radius, speed_margin, stop_gap, stop_gap_far, stop_cone, stop_gap_prop, side_on_angle, floor_headroom, skitch_cos, low_prop_height, low_prop_time, step_off_cap, step_off_time);
    }
}

/// A patch of a plain settings struct: every listed field optional (same name and unit as the Rust field), floats
/// finite and at least 0, counts at most `$max`. `floats()` / `counts()` list them for validation and application.
macro_rules! settings_patch {
    ($(#[$doc:meta])* $name:ident { floats: [$($f:ident),*], counts: [$($c:ident),*], $(extra: [$($e:ident: $t:ty),*],)? max_count: $max:expr }) => {
        $(#[$doc])*
        #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            $(pub $f: Option<f32>,)*
            $(pub $c: Option<u32>,)*
            $($(pub $e: Option<$t>,)*)?
        }
        impl $name {
            pub fn floats(&self) -> Vec<(&'static str, Option<f32>)> {
                vec![$((stringify!($f), self.$f)),*]
            }
            pub fn counts(&self) -> Vec<(&'static str, Option<u32>)> {
                vec![$((stringify!($c), self.$c)),*]
            }
            fn fields_valid(&self) -> bool {
                self.floats().into_iter().all(|(_, v)| v.is_none_or(|v| v.is_finite() && v >= 0.0))
                    && self.counts().into_iter().all(|(_, v)| v.is_none_or(|v| v <= $max))
            }
        }
        impl Merge for $name {
            fn merge(&mut self, b: &Self) {
                $( if self.$f.is_none() { self.$f = b.$f; } )*
                $( if self.$c.is_none() { self.$c = b.$c; } )*
                $($( if self.$e.is_none() { self.$e = b.$e.clone(); } )*)?
            }
        }
    };
}

settings_patch! {
    /// `npc_walk_back`: the NPC skaters' walk-back controller (controller B,
    /// `skate_core::living_world::controller_b::ControllerBSettings`); retail values by default. Distances m, angles rad,
    /// `*_sq` squared m, counts in ticks. `start_locate_radius` / `goal_locate_radius` default to unbounded (retail).
    NpcWalkBackPatch {
        floats: [plan_path_h, plan_path_v, goal_locate_radius, start_locate_radius, plan_direct_h, plan_direct_v, arrive_h, arrive_v, arrive_heading, line_up_h, line_up_v, leg_h, leg_v, leg_epsilon_sq, steer_full_lock, steer_gain, throttle_gain, slow_along, slow_gain, throttle_floor, floor_along, stall_steer, magnitude_cap, move_step_sq, move_anchor_sq, turn_step, turn_anchor],
        counts: [stall_ticks, push_period, stuck_waypoint_ticks, stuck_stall_ticks, stall_recovery_ticks],
        max_count: 36000
    }
}

impl NpcWalkBackPatch {
    pub fn validate(&self) -> bool {
        // A press period of 0 would press every tick forever (ours: kept at least 1).
        self.fields_valid() && self.push_period.is_none_or(|n| n >= 1)
    }
}

settings_patch! {
    /// `npc_line_controller`: the NPC skaters' line controller (stuck tracker, re-snap, reposition, safe node;
    /// `skate_core::living_world::ai_controller::ControllerSettings`); retail values by default. Counts in ticks, nodes
    /// or recorded frames; `stuck_limits` the six stage limits (ticks).
    NpcLineControllerPatch {
        floats: [wipe_out, safe_slope, stuck_clear_distance, stuck_start_fraction, stuck_start_approach],
        counts: [reposition_ahead, reposition_past, remembered_window, reposition_interval, safe_jump_frames, safe_flag_frames, safe_run, stuck_clear_ticks, stuck_hold_ticks, stuck_gate_ticks, resnap_frames],
        extra: [stuck_limits: [u32; 6]],
        max_count: 36000
    }
}

settings_patch! {
    /// `skater_path`: the simulated NPC skaters' path follower (`skate_core::living_world::path_follow::PathTuning`):
    /// `line_end_t` (0.8, the on-path line end trigger's segment fraction), `lost_radius` (6 m), `line_end_lost_radius`
    /// (4.5 m), `lost_max_speed` (14 m/s); retail values by default: docs/hails-additions/26e-living-world-skater-ai.md#line-end-trigger
    SkaterPathPatch {
        floats: [line_end_t, lost_radius, line_end_lost_radius, lost_max_speed],
        counts: [],
        max_count: 0
    }
}

impl SkaterPathPatch {
    pub fn validate(&self) -> bool {
        self.fields_valid() && self.line_end_t.is_none_or(|t| t <= 1.0)
    }
}

impl NpcLineControllerPatch {
    pub fn validate(&self) -> bool {
        // The safe node picker needs at least one good node in a row (ours).
        self.fields_valid() && self.safe_run.is_none_or(|n| n >= 1) && self.stuck_limits.is_none_or(|l| l.iter().all(|&n| n <= 36000))
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

impl Merge for PropSoundPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; material, secondary, use_contact_material, impact_min, impact_max, cooldown, speed_attenuation, spin_attenuation, patch, loop_max_kmh);
    }
}

impl Merge for PropSoundsPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; enabled);
        for (key, t) in &b.types {
            self.types.entry(key.clone()).or_default().merge(t);
        }
    }
}

impl Merge for VideoScreensPatch {
    fn merge(&mut self, b: &Self) {
        merge_opts!(self, b; enabled, width, height, rate_hz, max_distance);
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
            && self.npc_grind_disttocog.is_none_or(|v| v.is_finite() && (0.0..=1.0).contains(&v))
            && self.skater_fade.as_ref().is_none_or(|f| finite(f.fade_in_seconds) && finite(f.fade_seconds) && finite(f.despawn_alpha) && finite(f.bail_fade_lead_seconds))
            && self.skater_line_chain.as_ref().is_none_or(|c| finite(c.radius) && c.max_candidates.is_none_or(|n| n <= 256) && c.blend_seconds.is_none_or(|v| v.is_finite() && (0.0..=10.0).contains(&v))
                && c.facing_rule.as_deref().is_none_or(|r| NPC_SKATER_FACING_RULES.contains(&r))
                && [c.steer_dead_zone_deg, c.steer_full_deg].into_iter().all(|v| v.is_none_or(|v| v.is_finite() && (0.0..=180.0).contains(&v)))
                && [c.fakie_high_speed, c.fakie_low_speed, c.fakie_slow_seconds, c.fakie_spawn_seconds, c.fallback_valid_m].into_iter().all(|v| v.is_none_or(|v| v.is_finite() && (0.0..=MAX_NUMBER).contains(&v))))
            && self.ped_fade.as_ref().is_none_or(|f| finite(f.fade_in_seconds) && f.distance.is_none_or(|d| d.iter().all(|v| finite(Some(*v)))))
            && self.ped_obstacles.as_ref().is_none_or(|o| {
                [o.min_half_extent, o.moving_speed, o.recut_fraction, o.detour_margin, o.target_snap, o.step_height].into_iter().all(finite)
                    && [o.path_turn_accel, o.path_turn_full_deg, o.path_turn_boost_deg, o.path_turn_boost_rate, o.path_turn_boost_max, o.path_turn_radius, o.controller_step, o.contact_slow_fraction, o.contact_max_speed, o.brake_decel, o.brake_goal_radius, o.brake_body_radius, o.brake_away_fraction].into_iter().all(finite)
                    && o.path_turn_max_deg.is_none_or(|v| v.is_finite() && (0.0..=180.0).contains(&v))
            })
            && self.ped_nav.as_ref().is_none_or(|n| {
                n.steer_rate.is_none_or(|v| v.is_finite() && (0.0..=1000.0).contains(&v))
                    && n.turn_edges.is_none_or(|e| e.iter().all(|v| v.is_finite() && (0.0..=180.0).contains(v)) && e.windows(2).all(|w| w[0] <= w[1]))
                    && [n.stall_seconds, n.stuck_distance, n.stuck_seconds, n.give_up_seconds, n.yield_patience].into_iter().all(|v| v.is_none_or(|v| v.is_finite() && (0.0..=60.0).contains(&v)))
                    && n.rewrite_range.is_none_or(|v| v.is_finite() && v >= 0.0)
                    && n.crosswalk.as_deref().is_none_or(|c| matches!(c, "off" | "walk_signal"))
            })
            && self.ped_body.as_ref().is_none_or(|b| {
                let name = |n: &str| !n.is_empty() && n.len() <= 64 && n.bytes().all(|c| c.is_ascii_graphic());
                let proxy_ok = b.proxy_radius.is_none_or(|v| v.is_finite() && (0.01..=2.0).contains(&v))
                    && b.proxy_half_length.is_none_or(|v| v.is_finite() && (0.0..=2.0).contains(&v))
                    && b.proxy_mass.is_none_or(|v| v.is_finite() && (0.1..=MAX_NUMBER).contains(&v));
                proxy_ok &&
                b.parts.as_ref().is_none_or(|parts| parts.len() <= 8 && parts.iter().all(|p| {
                    name(&p.from) && name(&p.to) && p.centre.is_finite() && (0.0..=1.0).contains(&p.centre)
                        && p.radius.is_finite() && (0.01..=2.0).contains(&p.radius)
                        && p.half_length.is_finite() && (0.0..=2.0).contains(&p.half_length)
                        && p.mass.is_finite() && (0.1..=MAX_NUMBER).contains(&p.mass)
                        && p.friction.is_none_or(|v| v.is_finite() && (0.0..=2.0).contains(&v))
                }))
                    && b.look_ahead.is_none_or(|v| v.is_finite() && (0.0..=1.0).contains(&v))
                    && b.range.is_none_or(|v| v.is_finite() && (0.0..=200.0).contains(&v))
                    && b.reaction_part.is_none_or(|v| v < 8)
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
                    && n.placement_hold_seconds.is_none_or(|v| v.is_finite() && (0.0..=10.0).contains(&v))
                    && n.respawn_hold_seconds.is_none_or(|v| v.is_finite() && (0.0..=10.0).contains(&v))
                    && n.anticipation_distance.is_none_or(|v| v.is_finite() && (0.0..=50.0).contains(&v))
                    && n.anticipation_frames.is_none_or(|v| v <= 600)
                    && [n.push_factor, n.push_reduce].into_iter().all(|v| v.is_none_or(|v| v.is_finite() && (0.0..=1.0).contains(&v)))
                    && n.push_fast_speed.is_none_or(|v| v.is_finite() && (0.0..=100.0).contains(&v))
                    && n.push_downhill_slope.is_none_or(|v| v.is_finite() && (-10.0..=10.0).contains(&v))
                    && n.push_max_heading_deg.is_none_or(|v| v.is_finite() && (0.0..=180.0).contains(&v))
                    && n.brake_factor.is_none_or(|v| v.is_finite() && (0.0..=1.0).contains(&v))
                    && n.brake_ramp.is_none_or(|v| v.is_finite() && (0.0..=100.0).contains(&v))
                    && n.max_crossed_nodes.is_none_or(|v| v <= 60)
                    && n.walk_arrive_distance.is_none_or(|v| v.is_finite() && (0.0..=20.0).contains(&v))
                    && n.reaction_radius.is_none_or(|v| v.is_finite() && (0.0..=100.0).contains(&v))
                    && n.walk_stuck_ticks.is_none_or(|v| v <= 36000)
                    && n.board_yaw_dead_zone.is_none_or(|v| v.is_finite() && (0.0..=core::f32::consts::FRAC_PI_2).contains(&v))
                    && n.board_yaw_mag_scale.is_none_or(|v| v.is_finite() && (0.0..=100.0).contains(&v))
                    && [n.air_steer_dead_zone_deg, n.air_steer_full_deg].into_iter().all(|v| v.is_none_or(|v| v.is_finite() && (0.0..=180.0).contains(&v)))
                    && n.manual_value.is_none_or(|v| v.is_finite() && (0.0..=1.0).contains(&v))
                    && n.avoid_anchor_ticks.is_none_or(|v| v.is_finite() && (0.0..=600.0).contains(&v))
            })
            && self.npc_avoid.as_ref().is_none_or(NpcAvoidPatch::validate)
            && self.npc_walk_back.as_ref().is_none_or(NpcWalkBackPatch::validate)
            && self.npc_line_controller.as_ref().is_none_or(NpcLineControllerPatch::validate)
            && self.skater_path.as_ref().is_none_or(SkaterPathPatch::validate)
            && self.ped_brain.as_ref().is_none_or(|p| {
                [p.wander_speed, p.warn_seconds, p.know_about_seconds, p.mood_want_memory_seconds, p.conversation_turn_seconds, p.conversation_gather_seconds, p.unreachable_radius, p.reach_check_seconds, p.reach_snap_window, p.run_from_honker_distance, p.run_from_honker_speed, p.attack_throw_speed, p.attack_throw_lead_seconds, p.attack_throw_jitter, p.attack_throw_lift, p.light_throw_speed, p.zombie_follow_distance, p.zombie_sprint_distance, p.zombie_ring_min, p.zombie_ring_max, p.zombie_sprint_speed, p.zombie_walk_speed, p.zombie_arrive_distance, p.skater_avoid_sense, p.skater_avoid_min_speed, p.skater_avoid_look_ahead, p.skater_avoid_radius, p.skater_avoid_jump_radius, p.skater_avoid_jump_speed, p.skater_avoid_sector, p.skater_avoid_period, p.object_flee_radius, p.object_flee_min_speed, p.object_flee_lane_margin, p.vehicle_flee_radius, p.vehicle_flee_min_dot].into_iter().all(|v| v.is_none_or(|v| v.is_finite() && (0.0..=100.0).contains(&v)))
                    && p.scatter_seconds.is_none_or(|v| v.is_finite() && (0.0..=3600.0).contains(&v))
                    && p.scatter_arrival_radius.is_none_or(|v| v.is_finite() && (0.0..=100.0).contains(&v))
                    && p.scatter_channel_fade.is_none_or(|v| v.is_finite() && (0.0..=10.0).contains(&v))
                    && p.warn_speech.is_none_or(|v| (0..=127).contains(&v))
                    && [p.flee_leg, p.flee_arrival_radius, p.flee_threat_speed_gate, p.flee_side_blend].into_iter().all(|v| v.is_none_or(|v| v.is_finite() && (0.0..=100.0).contains(&v)))
                    && [p.water_max_depth, p.water_above].into_iter().all(|v| v.is_none_or(|v| v.is_finite() && (0.0..=100.0).contains(&v)))
                    && p.trip_kind.as_deref().is_none_or(|k| TRIP_KINDS.contains(&k))
                    && p.trip_direction.as_deref().is_none_or(|d| TRIP_DIRECTIONS.contains(&d))
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
            && self.water_density.is_none_or(|v| v.is_finite() && (10.0..=20_000.0).contains(&v))
    }
}
impl PropSolverPatch {
    pub fn validate(&self) -> bool {
        self.iterations.is_none_or(|n| (1..=256).contains(&n))
            && self.sleep_energy.is_none_or(|e| e.is_finite() && (0.0..=MAX_NUMBER).contains(&e))
            && self.sleep_frames.is_none_or(|n| (1..=10_000).contains(&n))
            && self.max_sleeps_per_step.is_none_or(|n| n >= 1)
            && self.board_row_mass.is_none_or(|v| v.is_finite() && v > 0.0 && v <= 1000.0)
            && [self.bail_linear_factor, self.bail_angular_factor, self.bail_heavy_mass, self.walkable_mass]
                .iter()
                .all(|v| v.is_none_or(|v| v.is_finite() && (0.0..=MAX_NUMBER).contains(&v)))
            && self.bail_blend.is_none_or(|v| v.is_finite() && (0.0..=1.0).contains(&v))
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
            && [self.lever_rotation, self.lever_yaw, self.mass_speed, self.inertia_yaw_gain, self.hand_ik_curve, self.root_yaw_gain]
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
                    && [v.inertia_scale, v.inertia_offset, v.plugin_rotation_tolerance].into_iter().flatten().flatten().all(|x| x.is_finite() && x.abs() <= MAX_NUMBER)
                    && v.plugin_max_distance.is_none_or(|x| x.is_finite() && (0.0..=MAX_NUMBER).contains(&x))
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
    VideoScreens(VideoScreensPatch),
    PropSounds(PropSoundsPatch),
    Vehicles(VehiclesPatch),
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
        "video_screens" => Patch::VideoScreens(serde_json::from_value(patch.clone()).ok()?),
        "prop_sounds" => Patch::PropSounds(serde_json::from_value(patch.clone()).ok()?),
        "vehicles" => Patch::Vehicles(serde_json::from_value(patch.clone()).ok()?),
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
        Patch::VideoScreens(p) => p.validate(),
        Patch::PropSounds(p) => p.validate(),
        Patch::Vehicles(p) => p.validate(),
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
        assert!(valid_patch("living_world", &json!({"npc_grind_disttocog": 0.6})));
        assert!(!valid_patch("living_world", &json!({"npc_grind_disttocog": 1.5})));
        assert!(valid_patch("props", &json!({"default": {"push_transfer": 0.5}, "by_template": {"bench01": {"collision_box": {"center": [0, 0.4, 0], "half_extents": [1, 0.4, 0.3]}}}})));
        assert!(valid_patch("carry", &json!({"grab_bit": 21, "grab_range": 3.5})));
        assert!(valid_patch("vehicles", &json!({"entities": {"my_cab": "taxi01"}, "spawn_tables": {"taxis": ["my_cab"], "sports": []}, "models": {"my_cab": "vehicle_taxi01"}, "glbs": {"vehicle_taxi01": "mods/x/cab.glb"}, "paint_gain": 1.5})));
        assert!(!valid_patch("vehicles", &json!({"entities": {"a": "a"}})));
        assert!(!valid_patch("vehicles", &json!({"paint_gain": -1.0})));
        assert!(!valid_patch("vehicles", &json!({"routes": {}})));
        // The NPC skaters' obstacle avoider.
        assert!(valid_patch("living_world", &json!({"npc_avoid": {"enabled": false, "speed_margin": 2.0, "radius_pedestrian": 12.0, "cone": 1.0, "max_entries": 8}})));
        assert!(!valid_patch("living_world", &json!({"npc_avoid": {"cone": 4.0}})));
        assert!(valid_patch("living_world", &json!({"ped_brain": {"warn_speech": 54}})));
        assert!(valid_patch("living_world", &json!({"traffic_horn": {"driver_taxi": {"enabled_chance": 0.0, "blocked_time": 2.0}}})));
        assert!(!valid_patch("living_world", &json!({"traffic_horn": {"all": {"enabled_chance": 2.0}}})));
        assert!(!valid_patch("living_world", &json!({"traffic_horn": {"all": {"honk": 1}}})));
        assert!(!valid_patch("living_world", &json!({"ped_brain": {"warn_speech": 128}})));
        assert!(valid_patch("living_world", &json!({"ped_brain": {"water_disables_seats": false, "water_max_depth": 2.5}})));
        assert!(valid_patch("living_world", &json!({"ped_brain": {"object_flee": false, "object_flee_radius": 12.0, "object_flee_min_speed": 0.5}})));
        assert!(!valid_patch("living_world", &json!({"ped_brain": {"object_flee_radius": -1.0}})));
        assert!(valid_patch("living_world", &json!({"ped_brain": {"vehicle_flee": true, "vehicle_flee_radius": 12.0, "vehicle_flee_toward": false}})));
        assert!(!valid_patch("living_world", &json!({"ped_brain": {"vehicle_flee_min_dot": -1.0}})));
        assert!(valid_patch("living_world", &json!({"ped_brain": {"flee_leg": 20.0, "flee_arrival_radius": 1.5, "flee_threat_speed_gate": 2.0, "flee_side_blend": 0.25}})));
        assert!(!valid_patch("living_world", &json!({"ped_brain": {"flee_leg": 101.0}})));
        assert!(!valid_patch("living_world", &json!({"ped_brain": {"flee_side_blend": -0.5}})));
        assert!(!valid_patch("living_world", &json!({"ped_brain": {"water_max_depth": 200.0}})));
        assert!(!valid_patch("living_world", &json!({"npc_avoid": {"radius_vehicle": -1.0}})));
        assert!(!valid_patch("living_world", &json!({"npc_avoid": {"max_entries": 100}})));
        assert!(!valid_patch("living_world", &json!({"npc_avoid": {"swerve": 1.0}})));
        assert!(valid_patch("living_world", &json!({"ped_brain": {"enabled": false, "warn_seconds": 5.0}})));
        assert!(!valid_patch("living_world", &json!({"ped_brain": {"wander_speed": -1.0}})));
        assert!(valid_patch("living_world", &json!({"zombie": true, "ped_brain": {"zombie_sprint_speed": 6.0, "attack_throw_jitter": 0.0, "starting_hand_props": false}})));
        assert!(!valid_patch("living_world", &json!({"ped_brain": {"zombie_ring_max": -1.0}})));
        assert!(valid_patch("living_world", &json!({"ped_brain": {"trip_kind": "Standing", "trip_direction": "FromLeft"}})));
        assert!(!valid_patch("living_world", &json!({"ped_brain": {"trip_kind": "Faceplant"}})));
        assert!(!valid_patch("living_world", &json!({"ped_brain": {"trip_direction": "frombehind"}})));
        assert!(valid_patch("living_world", &json!({"npc_simulated": {"push": false, "push_factor": 0.9, "push_downhill_slope": -0.5, "push_right_foot": false}})));
        assert!(!valid_patch("living_world", &json!({"npc_simulated": {"push_factor": 1.5}})));
        assert!(!valid_patch("living_world", &json!({"npc_simulated": {"push_max_heading_deg": 200.0}})));
        assert!(valid_patch("living_world", &json!({"ped_body": {"proxy": false, "proxy_radius": 0.4, "proxy_mass": 20.0}})));
        assert!(!valid_patch("living_world", &json!({"ped_body": {"proxy_radius": 0.0}})));
        assert!(!valid_patch("living_world", &json!({"ped_body": {"proxy_mass": -1.0}})));
        assert!(valid_patch("living_world", &json!({"npc_simulated": {"respawn_seconds": 3.0, "respawn_min": 0.0}})));
        assert!(!valid_patch("living_world", &json!({"npc_simulated": {"respawn_seconds": 90.0}})));
        assert!(valid_patch("living_world", &json!({"npc_simulated": {"anticipation_distance": 5.0, "anticipation_frames": 90, "max_crossed_nodes": 4}})));
        assert!(!valid_patch("living_world", &json!({"npc_simulated": {"anticipation_frames": 9000}})));
        assert!(valid_patch("living_world", &json!({"npc_simulated": {"manual_value": 0.8}})));
        assert!(!valid_patch("living_world", &json!({"npc_simulated": {"manual_value": -0.5}})));
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
        assert!(valid_patch("video_screens", &json!({"enabled": false, "width": 512, "height": 288, "rate_hz": 30.0, "max_distance": 300.0})));
        assert!(!valid_patch("video_screens", &json!({"width": 8})));
        assert!(!valid_patch("video_screens", &json!({"rate_hz": 0.0})));
        assert!(!valid_patch("video_screens", &json!({"source": "camera"})));
        assert!(valid_patch("prop_sounds", &json!({"enabled": true, "types": {"dumpster": {"material": 52, "impact_min": 0.2, "cooldown": 5}}})));
        assert!(!valid_patch("prop_sounds", &json!({"types": {"dumpster": {"material": 144}}})));
        assert!(!valid_patch("prop_sounds", &json!({"types": {"dumpster": {"speed_attenuation": 0.0}}})));
        assert!(!valid_patch("prop_sounds", &json!({"types": {"dumpster": {"volume": 1.0}}})));
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
