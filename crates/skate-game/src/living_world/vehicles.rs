//! Traffic cars (doc 26, milestone V3): turns the population's vehicle spawn / despawn records
//! into visible cars that drive their lanes.
//!
//! - **Entity** per spawn record: [`TrafficCar`] (stable `LivingWorldId`, entity / model keys,
//!   palette ids and colours, engine record) + `Transform` + #32's
//!   [`TrafficAudio`](crate::world_audio::TrafficAudio). [`TrafficState`] keeps the follower cars
//!   (`skate_core::living_world::traffic::follow`) and the id -> entity map.
//! - **Motion** (`FixedUpdate`, after the population step): one follower step per 60 Hz world
//!   tick, the signal clock ticked once per world tick (retail ticks its 4 controllers by 1/60 s
//!   per world tick, `sub_826B2C18`); a car spawned at tick `s` steps from tick `s + 1`. Lane state
//!   goes back to the population (`LivingWorld::update_lane`, `update_position`) so the census
//!   cull and the placement test see the real cars. A car at a dead end leaves (despawn, reason
//!   External).
//! - **Look** (`Update`): the car's GLB (`private/living_world/vehicles/<recipe>.glb`, or a mod's
//!   path from [`VehicleOverrides`]) as a scene, drawn with the glTF materials on render layers 0
//!   and 28 like mod graphics; the `vehicle_chassis` material gets a tinted copy of its base
//!   texture ([`tint_rgba8`], rule below), the `vehicle_glass` material is made see-through
//!   ([`GLASS_ALPHA`], engine value); the six wheel bones spin with the distance driven over the
//!   model's wheel radius (`wheel_hint`, `Hash_FD7A66142F16B9CC`, equal to the wheel bone height
//!   [data, V0]). Drivers are part of the body mesh [data, V0]; no vehicle lights exist [data].
//! - **Collision**: one kinematic box per car from the GLB bounds (`mesh_bounds`), infinite mass,
//!   the car's velocity, joined to the skater solve through `physics::network::Proxies` like the
//!   NPC skaters. Solid contact only; bails and roof behaviour are V5.
//! - **Audio**: `TrafficAudio { engine: <spec engine_audio record>, speed: +3412, load: +3408 }`
//!   and `AudioVelocity`, so #32's traffic engine host (nearest 4 within 40 m, Doppler) plays it.
//! - **Events** ([`TrafficEvent`]): spawned, despawned, junction answer changed, entered a
//!   junction, entered a lane; the planned `sdk.living_world` events read these.
//!
//! Tint rule (`vehicle_chassis` shader not read; most likely rule, open): the body atlases paint
//! the car body pure blue `(0, 0, b)` with the shading in `b`, and the model records' base
//! palettes are chassis `(0, 0, 1)` and secondary `(1, 0, 0)` [data, V0]. So the chassis colour
//! replaces the blue channel and the secondary colour the red channel, weighted by how pure the
//! channel is: `out = rgb + m_b (b x chassis - (0, 0, b)) + m_r (r x secondary - (r, 0, 0))`, `m_b
//! = (b - max(r, g)) / b`, `m_r = (r - max(g, b)) / r`. With the base palette the texture is
//! unchanged at gain 1 (the identity the base records imply). [`PAINT_GAIN`] scales the painted
//! value (default 2x modulate: an estimate from the data, not retail; the shader's tint is not
//! decoded; overridable through [`VehicleOverrides::paint_gain`]).
//!
//! Multiplayer (no networking): a car's motion is a function of its spawn record, the world tick,
//! the signal clock's tick count (= world ticks since the world loaded) and the other cars (which
//! are themselves spawn records); the connector choice reads the occupancy, so a client must run
//! the whole set of cars, not one. See doc 26 V3.
//!
//! Moddability: [`VehicleOverrides`] (model per entity, GLB per model, colour per palette id,
//! follower numbers per entity, connector choice); restoring `VehicleOverrides::default()` undoes
//! a mod; the spec / palette defaults come from the export (`tables.json`, `vehicles.json`).

use super::{LivingWorldDespawn, LivingWorldSettings, LivingWorldSpawn, NetRole, PopulationState};
use crate::world_audio::{AudioVelocity, HornState, TrafficAudio};
use bevy::prelude::*;
use skate_core::living_world::rng::Rng;
use skate_core::living_world::traffic::follow::{self, Car, FollowEvent, FollowParams};
use skate_core::living_world::traffic::{ConnectorChoice, Entry, LaneCursor, Place, RoadNetwork, SegmentId, SignalClock, SignalTimings};
use skate_core::living_world::{DespawnReason, Kind, LivingWorldId, SpawnChoice};
use std::collections::BTreeMap;

/// The six wheel bones of the car rig [data, V0].
pub(crate) const WHEEL_BONES: [&str; 6] = ["LeftFront_wheel", "RightFront_wheel", "LeftRear_wheel_1", "RightRear_wheel_1", "LeftRear_wheel_2", "RightRear_wheel_2"];
/// Glass opacity (engine value; the `vehicle_glass` shader is not read, open).
pub(crate) const GLASS_ALPHA: f32 = 0.55;
/// Default gain on the painted value of the tint (a 2x modulate). ESTIMATE, NOT RETAIL: the
/// `vehicle_chassis` shader's tint maths is not decoded. Chosen from the data: the atlases' paint
/// blue sits around 0.5-0.56, and only with 2x does a palette "white" (0.90) or the taxi yellow
/// read as that colour (headless renders in `.local/research/npc/v3-tint`; gain 1 gives muddy
/// half-bright paint). A mod or setting overrides it ([`VehicleOverrides::paint_gain`]).
pub(crate) const PAINT_GAIN: f32 = 2.0;
/// Solid ids of car proxies: a tag in the top bits keeps them apart from NPCs and mod bodies.
pub(crate) const PROXY_ID_TAG: u64 = 0x5643_0000_0000_0000;

/// One car model (`vehicles.json` `models.<record>`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct VehicleModel {
    /// Asset path relative to the asset root.
    pub glb: String,
    pub chassis: Vec<[f32; 4]>,
    pub secondary: Vec<[f32; 4]>,
    pub chassis_ids: Vec<String>,
    pub secondary_ids: Vec<String>,
    /// m (`wheel_hint`).
    pub wheel_radius: f32,
    /// Mesh bounds, model space (min, max).
    pub bounds: [[f32; 3]; 2],
    /// The skitch grab splines (RW4 GRABDATA of the first part arena that has one, b35; one rear-edge spline per
    /// stock car, none on `reda_car`). Empty = the car cannot be skitched (retail).
    pub grab_splines: Vec<CarGrabSpline>,
}

/// The exported `grab_splines` list (cars and props): a spline that is not whole Bezier segments is dropped.
pub(crate) fn parse_grab_splines(list: &serde_json::Value) -> Vec<CarGrabSpline> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| {
            let points: Vec<[f32; 3]> = g["points"].as_array()?.iter().map(vec3).collect::<Option<_>>()?;
            (!points.is_empty() && points.len() % 4 == 0).then_some(CarGrabSpline { points, direction: vec3(&g["direction"])?, flags: g["flags"].as_u64().unwrap_or(0) as u32 })
        })
        .collect()
}

/// One authored grab spline (cars and props), model space: chains of cubic Bezier segments (4 control points each) and the grab
/// direction (stock cars: (0, 0, -1), backwards).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CarGrabSpline {
    pub points: Vec<[f32; 3]>,
    pub direction: [f32; 3],
    /// Entry +60 (0x3E4 on stock cars; meaning open), the record's word 60 [inferred].
    pub flags: u32,
}

/// What a car entity drives with (`livingworld_entities` -> spec record).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct VehicleSpec {
    /// `aud_traffic_engine` record (spec `engine_audio`).
    pub engine: String,
    pub params: FollowParams,
    /// The entity's driver record (`livingworld_vehicle_drivers`; the horn values are in `params.horn`).
    pub driver: String,
}

/// Car data of the loaded world.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct VehicleData {
    pub models: BTreeMap<String, VehicleModel>,
    /// Entity name -> spec.
    pub specs: BTreeMap<String, VehicleSpec>,
    pub timings: Option<SignalTimings>,
}

fn vec3(v: &serde_json::Value) -> Option<[f32; 3]> {
    Some([v.get(0)?.as_f64()? as f32, v.get(1)?.as_f64()? as f32, v.get(2)?.as_f64()? as f32])
}

/// Read `vehicles.json` and `tables.json` (the follower numbers and engine record from each
/// entity's spec record; missing fields keep [`FollowParams::default`]).
pub(crate) fn parse_vehicle_data(vehicles: &[u8], tables: Option<&[u8]>) -> Result<VehicleData, String> {
    let doc: serde_json::Value = serde_json::from_slice(vehicles).map_err(|e| format!("vehicles.json: {e}"))?;
    let tables: Option<serde_json::Value> = tables.and_then(|t| serde_json::from_slice(t).ok());
    let mut data = VehicleData { timings: tables.as_ref().and_then(skate_data::roads::signal_timings), ..Default::default() };
    for (key, m) in doc["models"].as_object().into_iter().flatten() {
        let Some(glb) = m["glb"].as_str() else { continue };
        let colours = |k: &str| -> Vec<[f32; 4]> {
            m[k].as_array().into_iter().flatten().filter_map(|c| Some([c.get(0)?.as_f64()? as f32, c.get(1)?.as_f64()? as f32, c.get(2)?.as_f64()? as f32, c.get(3).and_then(|x| x.as_f64()).unwrap_or(1.0) as f32])).collect()
        };
        let ids = |k: &str| -> Vec<String> { m["palette_ids"][k].as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_string)).collect() };
        let bounds = (|| Some([vec3(&m["mesh_bounds"][0])?, vec3(&m["mesh_bounds"][1])?]))().unwrap_or([[-0.9, 0.0, -2.2], [0.9, 1.5, 2.2]]);
        let wheel_radius = m["wheel_hint"].as_f64().map(|x| x as f32).filter(|r| *r > 0.05).unwrap_or(0.32);
        data.models.insert(
            key.clone(),
            VehicleModel {
                glb: format!("private/living_world/{glb}"),
                chassis: colours("chassis_colours"),
                secondary: colours("secondary_colours"),
                chassis_ids: ids("chassis"),
                secondary_ids: ids("secondary"),
                wheel_radius,
                bounds,
                grab_splines: parse_grab_splines(&m["grab_splines"]),
            },
        );
    }
    let class = |c: &str, r: &str| tables.as_ref().and_then(|t| t.pointer(&format!("/classes/{c}/{r}/fields")).cloned());
    for (name, e) in doc["entities"].as_object().into_iter().flatten() {
        let spec_name = e["spec"].as_str().unwrap_or("default");
        let mut params = FollowParams::default();
        let mut engine = "default".to_string();
        let driver = e["driver"].as_str().unwrap_or("default").to_string();
        if let Some(f) = class("livingworld_vehicle_drivers", &driver) {
            let num = |k: &str| f.get(k).and_then(|v| v.as_f64()).map(|v| v as f32);
            let h = &mut params.horn;
            h.blocked_time = num("honk_blocked_time").unwrap_or(h.blocked_time);
            h.obstacle_time = num("honk_obstacle_time").unwrap_or(h.obstacle_time);
            h.approach_speed = num("honk_approach_speed_kmh").map_or(h.approach_speed, |v| v / 3.6);
            // Driver block +36 / +20 (`82C42348`, b74).
            h.enabled_chance = num("Hash_50E084076390A573").unwrap_or(h.enabled_chance);
            h.blocked_long_chance = num("Hash_20E9C6487FDDBDE8").unwrap_or(h.blocked_long_chance);
            // The manoeuvre decider's driver values (b74: driver block +28 go, +24 least loaded; hashed fields).
            let m = &mut params.manoeuvre;
            m.lane_change_chance = num("Hash_52CF2CF346699CA1").unwrap_or(m.lane_change_chance);
            m.least_loaded_chance = num("Hash_7C6B48BD9ADF8E6E").unwrap_or(m.least_loaded_chance);
            m.overtake_chance = num("Hash_9366C67755A24D89").unwrap_or(m.overtake_chance);
            m.pull_over_chance = num("pull_over_chance").or_else(|| num("Hash_559BA807F95FF93E")).unwrap_or(m.pull_over_chance);
            m.held_pull_over_chance = num("Hash_99083122A1B7116A").unwrap_or(m.held_pull_over_chance);
            params.parked_time = num("parked_time").or_else(|| num("Hash_988BB0F6F043EB3D")).unwrap_or(params.parked_time);
        }
        if let Some(f) = class("livingworld_vehicle_characteristics", spec_name) {
            let num = |k: &str| f.get(k).and_then(|v| v.as_f64()).map(|v| v as f32);
            if let Some(v) = num("Hash_328B9F4685A14018") {
                params.accel_max = v;
            }
            if let Some(v) = num("Hash_758229215579C6D1") {
                params.plan_decel = v;
            }
            // Retail reads spec+40 (`328B9F46`) as the lane-change passage factor and spec+48 (`90AB56A5`) as the lane
            // timer (b72 / b74); the follower's accel / decel reads above are todo traffic-accel-fields-mislabelled.
            if let Some(v) = num("Hash_328B9F4685A14018") {
                params.passage_factor = v;
            }
            // spec+36: the pull-over approach factor (`+3672` = ext x it, b71 / b77).
            if let Some(v) = num("Hash_758229215579C6D1") {
                params.approach_factor = v;
            }
            if let Some(v) = num("Hash_90AB56A5DDCF2A3A") {
                params.manoeuvre.lane_timer = v;
            }
            // The skater-behind rule (b63). Older exports name 3AB7FC7C `follow_speed_margin_kmh` (it is the FAR
            // braking distance) and keep the others as raw hashes.
            let either = |a: &str, b: &str| num(a).or_else(|| num(b));
            let s = &mut params.skater;
            if let Some(v) = num("follow_min_speed_kmh") {
                s.min_speed = v / 3.6;
            }
            if let Some(v) = either("skater_follow_margin_kmh", "Hash_256A412E350A2659") {
                s.margin = v / 3.6;
            }
            if let Some(v) = either("skater_far_distance", "follow_speed_margin_kmh") {
                s.far_distance = v;
            }
            if let Some(v) = either("skater_scan_range", "Hash_33466832D8178EAF") {
                s.range = v;
            }
            if let Some(v) = either("skater_near_range", "Hash_D49FC49019181EE5") {
                s.near_range = v;
            }
            if let Some(v) = either("skater_near_distance", "Hash_F682D359CDBC4D12") {
                s.near_distance = v;
            }
            if let Some(v) = either("release_grace", "Hash_4727CF785EF735C8") {
                s.release_grace = v;
            }
            if let Some(k) = f.pointer("/engine_audio/key").and_then(|v| v.as_str()) {
                engine = k.to_string();
            }
        }
        data.specs.insert(name.clone(), VehicleSpec { engine, params, driver });
    }
    Ok(data)
}

/// Mod and engine overrides (all keyed by the export's stable names). Empty = retail.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(crate) struct VehicleOverrides {
    /// Entity name -> `livingworld_models` record.
    pub models: BTreeMap<String, String>,
    /// Model record -> GLB asset path.
    pub glbs: BTreeMap<String, String>,
    /// Palette id (`<model>/chassis/<i>`, `<model>/secondary/<i>`) -> RGBA.
    pub colours: BTreeMap<String, [f32; 4]>,
    /// Entity name -> follower numbers.
    pub params: BTreeMap<String, FollowParams>,
    /// Connector choice (retail: least loaded on every car).
    pub connector_choice: ConnectorChoice,
    /// Tint gain; `None` = [`PAINT_GAIN`] (estimate, not retail).
    pub paint_gain: Option<f32>,
}

/// One traffic car.
#[derive(Component, Clone, Debug, PartialEq)]
pub(crate) struct TrafficCar {
    pub id: LivingWorldId,
    pub entity: String,
    pub model: String,
    pub glb: String,
    pub chassis_id: String,
    pub secondary_id: String,
    pub chassis: [f32; 4],
    pub secondary: [f32; 4],
    pub engine: String,
    /// Tint gain this car is drawn with ([`PAINT_GAIN`] unless overridden).
    pub paint_gain: f32,
    pub wheel_radius: f32,
    pub bounds: [[f32; 3]; 2],
    pub spawn_tick: u64,
}

/// Fixed-step pose of a car (render interpolates between `prev` and `curr`).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct CarMotion {
    pub prev: Transform,
    pub curr: Transform,
    pub wheel_prev: f32,
    pub wheel: f32,
    pub velocity: Vec3,
}

/// What happened to a car (engine systems, the planned mod events).
#[derive(Message, Clone, Debug, PartialEq)]
pub(crate) enum TrafficEvent {
    Spawned { id: LivingWorldId, entity: String, model: String, chassis: String },
    Despawned { id: LivingWorldId, reason: DespawnReason },
    /// The junction answer for the chosen connector changed (retail junction state).
    Junction { id: LivingWorldId, junction: u64, connector: u32, entry: Entry },
    EnteredJunction { id: LivingWorldId, junction: u64, connector: u32 },
    EnteredLane { id: LivingWorldId, segment: u64, lane: u8 },
    /// A lane change started on `segment` from lane `from` to `to` (`82C3A9E0`; b69 / b74).
    LaneChange { id: LivingWorldId, segment: u64, from: u8, to: u8 },
    /// The car pulls over to its spot / pulls out again (b73).
    PullingOver { id: LivingWorldId, segment: u64, spot: f32 },
    PullingOut { id: LivingWorldId, segment: u64 },
    /// The horn state changed (`+3420`: 0 silent, 1..=5 the decider's kinds; horn.rs).
    Horn { id: LivingWorldId, kind: u8 },
    /// Horn kind 2 at a ped (`sub_82C40660` -> vt+100 `sub_82E3C3D0`): the ped's honker is this car. Sent every
    /// frame while it lasts, as in retail.
    HonkedAt { id: LivingWorldId, ped: u64 },
}

/// The traffic of the loaded world.
#[derive(Resource)]
pub(crate) struct TrafficState {
    /// Follower cars, sorted by key (= the id serial).
    pub cars: Vec<Car>,
    /// Serial -> (entity, spawn tick).
    pub index: BTreeMap<u32, (Entity, u64)>,
    pub clock: Option<SignalClock>,
    /// World tick the cars were stepped to.
    pub last_tick: Option<u64>,
    pub rng: Rng,
    pub data: VehicleData,
    pub(crate) loaded_for: Option<(String, u64)>,
}

impl Default for TrafficState {
    fn default() -> Self {
        Self { cars: Vec::new(), index: BTreeMap::new(), clock: None, last_tick: None, rng: Rng::new(0x5452_4146), data: VehicleData::default(), loaded_for: None }
    }
}

/// Car rotation from the lane direction (+Z forward, y up).
pub(crate) fn car_rotation(forward: [f32; 3]) -> Quat {
    let f = Vec3::from_array(forward).normalize_or(Vec3::Z);
    let x = Vec3::Y.cross(f).normalize_or(Vec3::X);
    let y = f.cross(x);
    Quat::from_mat3(&Mat3::from_cols(x, y, f))
}

/// The car's pose: on its lane-change curve while one runs (`Car::pose`), else its cursor frame.
fn car_transform(net: &RoadNetwork, car: &Car) -> Transform {
    let frame = car.pose(net);
    Transform::from_translation(Vec3::from_array(frame.position)).with_rotation(car_rotation(frame.forward))
}

/// Tint an RGBA8 (sRGB) pixel buffer with the chassis and secondary colours (rule in the module
/// doc). Alpha is kept.
pub(crate) fn tint_rgba8(pixels: &mut [u8], chassis: [f32; 4], secondary: [f32; 4], gain: f32) {
    for px in pixels.chunks_exact_mut(4) {
        let [r, g, b] = [px[0], px[1], px[2]].map(|c| c as f32 / 255.0);
        let mb = if b > 0.0 { ((b - r.max(g)) / b).clamp(0.0, 1.0) } else { 0.0 };
        let mr = if r > 0.0 { ((r - g.max(b)) / r).clamp(0.0, 1.0) } else { 0.0 };
        let (pb, pr) = ((b * gain).min(1.0), (r * gain).min(1.0));
        let out = [
            r + mb * (pb * chassis[0]) + mr * (pr * secondary[0] - r),
            g + mb * (pb * chassis[1]) + mr * (pr * secondary[1]),
            b + mb * (pb * chassis[2] - b) + mr * (pr * secondary[2]),
        ];
        for i in 0..3 {
            px[i] = (out[i].clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
}

/// Load the car data when the world changes.
pub(crate) fn load_traffic_data(mut commands: Commands, config: Res<crate::config::Config>, map: Res<crate::map_transition::CurrentMap>, mut traffic: ResMut<TrafficState>) {
    let key = (map.name.clone(), map.generation);
    if traffic.loaded_for.as_ref() == Some(&key) {
        return;
    }
    // A new world: cars of the old one go (the population's reset records normally did this).
    for (_, (e, _)) in std::mem::take(&mut traffic.index) {
        commands.entity(e).despawn();
    }
    traffic.cars.clear();
    let dir = config.asset_root.join("private/living_world");
    let tables = std::fs::read(dir.join("tables.json")).ok();
    traffic.data = std::fs::read(dir.join("vehicles.json"))
        .ok()
        .and_then(|b| parse_vehicle_data(&b, tables.as_deref()).map_err(|e| warn!("LIVING_WORLD traffic: {e}")).ok())
        .unwrap_or_default();
    traffic.clock = traffic.data.timings.map(SignalClock::new);
    traffic.last_tick = None;
    traffic.loaded_for = Some(key);
    info!("LIVING_WORLD traffic models {} specs {} lights {}", traffic.data.models.len(), traffic.data.specs.len(), traffic.clock.is_some());
}

/// A mod's horn values over the driver record's.
fn apply_horn_patch(h: &skate_mods::world_tuning::TrafficHornPatch, p: &mut skate_core::living_world::traffic::horn::HornParams) {
    p.blocked_time = h.blocked_time.unwrap_or(p.blocked_time);
    p.obstacle_time = h.obstacle_time.unwrap_or(p.obstacle_time);
    p.approach_speed = h.approach_speed_kmh.map_or(p.approach_speed, |v| v / 3.6);
    p.approach_ttc = h.approach_seconds.unwrap_or(p.approach_ttc);
    p.enabled_chance = h.enabled_chance.unwrap_or(p.enabled_chance);
    p.blocked_long_chance = h.blocked_long_chance.unwrap_or(p.blocked_long_chance);
}

/// Build a car from a spawn record (pure; the engine and the tests use it).
pub(crate) fn car_from_record(
    net: &RoadNetwork,
    data: &VehicleData,
    overrides: &VehicleOverrides,
    horn_patches: &BTreeMap<String, skate_mods::world_tuning::TrafficHornPatch>,
    record: &skate_core::living_world::SpawnRecord,
    length: f32,
) -> Option<(TrafficCar, Car, TrafficAudio)> {
    let SpawnChoice::Vehicle { entity, model, chassis, secondary, segment, lane, distance, .. } = &record.choice else { return None };
    let model_key = overrides.models.get(entity).unwrap_or(model);
    let m = data.models.get(model_key).cloned().unwrap_or_default();
    let pick = |ids: &[String], colours: &[[f32; 4]], i: u32, base: [f32; 4]| -> (String, [f32; 4]) {
        let id = ids.get(i as usize).cloned().unwrap_or_else(|| format!("{model_key}/{i}"));
        let c = overrides.colours.get(&id).copied().or_else(|| colours.get(i as usize).copied()).unwrap_or(base);
        (id, c)
    };
    let (chassis_id, chassis_c) = pick(&m.chassis_ids, &m.chassis, *chassis, [0.0, 0.0, 1.0, 1.0]);
    let (secondary_id, secondary_c) = pick(&m.secondary_ids, &m.secondary, *secondary, [1.0, 0.0, 0.0, 1.0]);
    let spec = data.specs.get(entity).cloned().unwrap_or(VehicleSpec { engine: "default".into(), params: FollowParams::default(), driver: "default".into() });
    let mut params = overrides.params.get(entity).copied().unwrap_or(spec.params);
    // Mod horn values per driver record (`traffic_horn`), then the driver bits rolled from the spawn seed.
    for key in ["all", spec.driver.as_str()] {
        if let Some(h) = horn_patches.get(key) {
            apply_horn_patch(h, &mut params.horn);
        }
    }
    let si = net.segment_index(SegmentId(*segment))?;
    // The connector is chosen on the first step from the live occupancy (cursor next = None here
    // is replaced at once by the engine with the loads of that tick).
    let cursor = LaneCursor { place: Place::Lane { segment: si, lane: (*lane).min(net.segments[si].lanes - 1) }, distance: distance.clamp(0.0, net.segments[si].length), next: None, lane_shift: 0.0 };
    let length = if length > 0.5 { length } else { (m.bounds[1][2] - m.bounds[0][2]).max(3.0) };
    let mut car = Car::new(record.id.serial, cursor, length, params);
    let mut horn_rng = Rng::new(skate_core::living_world::rng::derive(record.seed, &[0x484f_524e]));
    car.driver = skate_core::living_world::traffic::horn::DriverBits::roll(&params.horn, &mut || horn_rng.unit());
    // `+4402` bit 0x80 (`82C42348`, the last roll): pulls over even while a skater holds the car.
    car.params.manoeuvre.pulls_over_while_held = skate_core::living_world::traffic::horn::percent_roll(params.manoeuvre.held_pull_over_chance, horn_rng.unit());
    let audio = TrafficAudio { engine: spec.engine.clone(), speed: Some(0.0), load: Some(0.0), ..TrafficAudio::new(spec.engine.clone()) };
    let glb = overrides.glbs.get(model_key).cloned().unwrap_or(m.glb.clone());
    Some((
        TrafficCar {
            id: record.id,
            entity: entity.clone(),
            model: model_key.clone(),
            glb,
            chassis_id,
            secondary_id,
            chassis: chassis_c,
            secondary: secondary_c,
            engine: spec.engine,
            paint_gain: overrides.paint_gain.filter(|g| g.is_finite() && *g > 0.0).unwrap_or(PAINT_GAIN),
            wheel_radius: m.wheel_radius.max(0.05),
            bounds: m.bounds,
            spawn_tick: record.tick,
        },
        car,
        audio,
    ))
}

/// Pick the first connector of a freshly spawned car with the live loads (retail chooses on
/// entering the lane, `sub_82C376E8`).
fn choose_first(net: &RoadNetwork, cars: &[Car], car: &mut Car, choice: ConnectorChoice, rng: &mut Rng) {
    if let Place::Lane { segment, lane } = car.cursor.place {
        let occ = follow::occupancy(cars);
        car.cursor.next = skate_core::living_world::traffic::choose_connector(net, segment, lane, choice, &|s, l| occ.lane_load(s, l), rng);
    }
}

/// Spawn and despawn car entities from the population's records.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_vehicle_records(
    mut commands: Commands,
    mut spawns: MessageReader<LivingWorldSpawn>,
    mut despawns: MessageReader<LivingWorldDespawn>,
    state: Res<PopulationState>,
    overrides: Res<VehicleOverrides>,
    settings: Res<LivingWorldSettings>,
    mut traffic: ResMut<TrafficState>,
    mut events: MessageWriter<TrafficEvent>,
) {
    let traffic = &mut *traffic;
    for LivingWorldDespawn(r) in despawns.read() {
        if r.id.kind != Kind::Vehicle {
            continue;
        }
        if let Some((e, _)) = traffic.index.remove(&r.id.serial) {
            commands.entity(e).despawn();
            traffic.cars.retain(|c| c.key != r.id.serial);
            events.write(TrafficEvent::Despawned { id: r.id, reason: r.reason });
        }
    }
    let Some(net) = state.roads.as_ref() else {
        spawns.clear();
        return;
    };
    for LivingWorldSpawn(s) in spawns.read() {
        if s.id.kind != Kind::Vehicle || traffic.index.contains_key(&s.id.serial) {
            continue;
        }
        let length = state.world.live(Kind::Vehicle).find(|l| l.id == s.id).and_then(|l| l.lane).map_or(0.0, |l| l.length);
        let Some((meta, mut car, audio)) = car_from_record(net, &traffic.data, &overrides, &settings.traffic_horn, s, length) else {
            warn!("LIVING_WORLD traffic: car #{} has no lane on this road network", s.id.serial);
            continue;
        };
        choose_first(net, &traffic.cars, &mut car, overrides.connector_choice, &mut traffic.rng);
        let t = car_transform(net, &car);
        events.write(TrafficEvent::Spawned { id: s.id, entity: meta.entity.clone(), model: meta.model.clone(), chassis: meta.chassis_id.clone() });
        let e = commands
            .spawn((
                Name::new(format!("Traffic car {} ({})", s.id.serial, meta.entity)),
                t,
                Visibility::Inherited,
                CarMotion { prev: t, curr: t, wheel_prev: 0.0, wheel: 0.0, velocity: Vec3::ZERO },
                audio,
                AudioVelocity(Vec3::ZERO),
                meta,
            ))
            .id();
        traffic.index.insert(s.id.serial, (e, s.tick));
        let at = traffic.cars.partition_point(|c| c.key < car.key);
        traffic.cars.insert(at, car);
    }
}

/// Step the cars to the population's tick, publish pose, lane state and audio.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_traffic(
    mut commands: Commands,
    settings: Res<LivingWorldSettings>,
    overrides: Res<VehicleOverrides>,
    mut state: ResMut<PopulationState>,
    mut traffic: ResMut<TrafficState>,
    mut cars_q: Query<(&TrafficCar, &mut CarMotion, &mut TrafficAudio, &mut AudioVelocity)>,
    mut despawns: MessageWriter<LivingWorldDespawn>,
    mut events: MessageWriter<TrafficEvent>,
    (observers, peds, ped_obstacles): (Res<super::LivingWorldObservers>, Query<(&super::peds::Pedestrian, &super::peds::PedBody)>, Option<Res<super::peds::PedObstacles>>),
    skater: Option<Res<crate::physics::SkaterRuntime>>,
    npc_sims: Query<&super::npc_sim::NpcSim>,
    (alarms, parked_q): (Option<Res<crate::game_audio::world_bridge::Bridge>>, Query<Has<crate::world_audio::VehicleParked>>),
) {
    let st = &mut *state;
    let traffic = &mut *traffic;
    let Some(net) = st.roads.as_ref() else { return };
    // The car alarm (`+3424` bit 0x10) is the alarm rule's (game_audio::car_alarm): read back every tick.
    for car in &mut traffic.cars {
        let on = match (alarms.as_deref(), traffic.index.get(&car.key)) {
            (Some(b), Some((e, _))) => b.alarm_left(*e).is_some(),
            _ => false,
        };
        if on != car.alarming {
            info!("VEHICLE_ALARM car=#{} on={on} manoeuvre={:?}", car.key, car.manoeuvre);
        }
        car.alarming = on;
    }
    // The held bits are cleared and set again every tick (82C34CD0 / 82C361E8; b57): the local player's state 104.
    let held = skater.as_deref().filter(|s| s.player_state.current() == skate_core::player::state::PhysicalStateId::Skitching).and_then(|s| s.skitch_state.held_car());
    // Simulated NPC skaters hold cars too (82C361E8 sets +4402 0x02 for any holder; 0x80 only for the player).
    let npc_held: Vec<u32> = npc_sims.iter().filter_map(|s| s.held_car()).collect();
    for car in &mut traffic.cars {
        car.player_held = Some(car.key) == held;
        car.held = car.player_held || npc_held.contains(&car.key);
    }
    look_ahead(traffic, &cars_q, &observers, &peds, ped_obstacles.as_deref(), held.is_some());
    let now = st.world.tick();
    let from = traffic.last_tick.unwrap_or(now);
    traffic.last_tick = Some(now);
    let mut travelled: BTreeMap<u32, f32> = BTreeMap::new();
    let mut prev_pose: BTreeMap<u32, Transform> = BTreeMap::new();
    for c in &traffic.cars {
        prev_pose.insert(c.key, car_transform(net, c));
    }
    let mut dead = Vec::new();
    let dt = (1.0 / skate_core::living_world::clock::RETAIL_TICK_HZ) as f32;
    for tick in from + 1..=now {
        let changes = traffic.clock.as_mut().map(|c| {
            let mut v = Vec::new();
            c.tick(&mut v);
            v
        });
        drop(changes);
        let Some(clock) = traffic.clock.as_ref() else { break };
        // Cars decided before this tick drive in it.
        let active: Vec<usize> = (0..traffic.cars.len()).filter(|&i| traffic.index.get(&traffic.cars[i].key).is_some_and(|(_, s)| *s < tick)).collect();
        let mut batch: Vec<Car> = active.iter().map(|&i| traffic.cars[i]).collect();
        let before: Vec<f32> = batch.iter().map(|c| c.cursor.distance).collect();
        let before_place: Vec<Place> = batch.iter().map(|c| c.cursor.place).collect();
        let out = follow::step(net, clock, &mut batch, dt, overrides.connector_choice, &mut traffic.rng);
        for (k, c) in batch.iter().enumerate() {
            let ds = if c.cursor.place == before_place[k] { (c.cursor.distance - before[k]).max(0.0) } else { c.speed * dt };
            *travelled.entry(c.key).or_default() += ds;
        }
        for (k, &i) in active.iter().enumerate() {
            traffic.cars[i] = batch[k];
        }
        for e in out {
            let id = |key: u32| LivingWorldId { kind: Kind::Vehicle, serial: key };
            match e {
                FollowEvent::Junction { key, connector, entry } => {
                    let c = &net.connectors[connector];
                    events.write(TrafficEvent::Junction { id: id(key), junction: c.id.junction.0, connector: c.id.index, entry });
                }
                FollowEvent::EnteredJunction { key, connector } => {
                    let c = &net.connectors[connector];
                    events.write(TrafficEvent::EnteredJunction { id: id(key), junction: c.id.junction.0, connector: c.id.index });
                }
                FollowEvent::EnteredLane { key, segment, lane } => {
                    events.write(TrafficEvent::EnteredLane { id: id(key), segment: net.segments[segment].id.0, lane });
                }
                FollowEvent::LaneChange { key, segment, from, to } => {
                    events.write(TrafficEvent::LaneChange { id: id(key), segment: net.segments[segment].id.0, from, to });
                }
                FollowEvent::PullingOver { key, segment, spot } => {
                    events.write(TrafficEvent::PullingOver { id: id(key), segment: net.segments[segment].id.0, spot });
                }
                FollowEvent::PullingOut { key, segment } => {
                    events.write(TrafficEvent::PullingOut { id: id(key), segment: net.segments[segment].id.0 });
                }
                FollowEvent::DeadEnd { key } => dead.push(key),
            }
        }
    }
    // Lane state and position back to the population; pose, audio.
    for c in &traffic.cars {
        let id = LivingWorldId { kind: Kind::Vehicle, serial: c.key };
        let frame = c.cursor.frame(net);
        if frame.position.iter().chain(&frame.forward).chain([&c.speed, &c.cursor.distance]).any(|x| !x.is_finite()) {
            // Guard (engine): one broken car leaves on its own; it never reaches the census.
            warn!("LIVING_WORLD traffic: car #{} has a non-finite state (position {:?}, speed {}, distance {}); removed", c.key, frame.position, c.speed, c.cursor.distance);
            dead.push(c.key);
            continue;
        }
        st.world.update_position(id, frame.position);
        let (segment, lane, distance) = match c.cursor.place {
            Place::Lane { segment, lane } => (segment, lane, c.cursor.distance),
            Place::Connector { connector } => {
                let k = &net.connectors[connector];
                (net.connector_exit(connector).unwrap_or(0), k.to_lane, 0.0)
            }
        };
        st.world.update_lane(id, net.segments[segment].id, lane, distance, c.speed);
        let Some((e, _)) = traffic.index.get(&c.key) else { continue };
        // StayingParked (`82C39120` begin / `82C391F0` end, `+3424` bit 0x80): only a parked car's contacts alarm.
        let parked = matches!(c.manoeuvre, skate_core::living_world::traffic::manoeuvre::Manoeuvre::Parked { .. });
        if parked_q.get(*e).is_ok_and(|has| has != parked) {
            if parked {
                commands.entity(*e).insert(crate::world_audio::VehicleParked);
            } else {
                commands.entity(*e).remove::<crate::world_audio::VehicleParked>();
            }
        }
        let Ok((meta, mut motion, mut audio, mut velocity)) = cars_q.get_mut(*e) else { continue };
        let t = car_transform(net, c);
        motion.prev = prev_pose.get(&c.key).copied().unwrap_or(t);
        motion.curr = t;
        motion.wheel_prev = motion.wheel;
        motion.wheel = (motion.wheel + travelled.get(&c.key).copied().unwrap_or(0.0) / meta.wheel_radius) % std::f32::consts::TAU;
        motion.velocity = Vec3::from_array(frame.forward) * c.speed;
        velocity.0 = motion.velocity;
        audio.speed = Some(c.speed);
        audio.load = Some(c.accel);
        // The horn state every frame (the sound side keeps a mod's `VehicleHorn` on top); the alarm is not ours.
        if audio.horn != HornState::Alarm {
            let horn = if c.horn == 0 { HornState::None } else { HornState::Honk(c.horn) };
            if audio.horn != horn {
                events.write(TrafficEvent::Horn { id, kind: c.horn });
            }
            audio.horn = horn;
        }
        if let Some(ped) = c.honk_target {
            events.write(TrafficEvent::HonkedAt { id, ped });
        }
    }
    // Dead ends: the car leaves (hosts decide; a client waits for the host's record).
    if settings.net_role != NetRole::Client {
        for key in dead {
            let id = LivingWorldId { kind: Kind::Vehicle, serial: key };
            if let Some(skate_core::living_world::Decision::Despawn(r)) = st.world.despawn(id, DespawnReason::External) {
                st.despawned += 1;
                despawns.write(LivingWorldDespawn(r));
            }
            if let Some((e, _)) = traffic.index.remove(&key) {
                commands.entity(e).despawn();
                traffic.cars.retain(|c| c.key != key);
                events.write(TrafficEvent::Despawned { id, reason: DespawnReason::External });
            }
        }
    }
}

/// The kinematic box of one car, world space.
pub(crate) fn proxy(car: &TrafficCar, pose: &Transform, velocity: Vec3) -> skate_dynamics::SolidBody {
    use skate_dynamics::rapier3d::prelude::{Pose, Rotation, SharedShape, Vector};
    let [lo, hi] = car.bounds;
    let half = [(hi[0] - lo[0]) * 0.5, (hi[1] - lo[1]) * 0.5, (hi[2] - lo[2]) * 0.5];
    let centre = pose.translation + pose.rotation * Vec3::new((hi[0] + lo[0]) * 0.5, (hi[1] + lo[1]) * 0.5, (hi[2] + lo[2]) * 0.5);
    let q = pose.rotation;
    let rotation = Rotation::from_xyzw(q.x, q.y, q.z, q.w).normalize();
    let p = |v: Vec3| Vector::new(v.x, v.y, v.z);
    skate_dynamics::SolidBody {
        id: PROXY_ID_TAG | car.id.to_u64(),
        pose: Pose::from_parts(p(pose.translation), rotation),
        center_of_mass: p(centre),
        inertia_rotation: rotation,
        inverse_mass: 0.0,
        inverse_inertia: Vector::new(0.0, 0.0, 0.0),
        linvel: p(velocity),
        angvel: Vector::new(0.0, 0.0, 0.0),
        // Retail's vehicle contact group (8; the skeleton contact pass keeps the largest
        // relative normal speed against it for the car-hit bail, `sub_82BD4A30` / `sub_82D90C98`).
        contact_group: crate::physics::VEHICLE_GROUP,
        colliders: vec![skate_dynamics::SolidCollider {
            shape: SharedShape::cuboid(half[0].max(0.1), half[1].max(0.1), half[2].max(0.1)),
            pose: Pose::from_parts(p(centre), rotation),
            friction: 0.5,
        }],
    }
}

/// The skater's body hit a car (`sub_82C3C150`, actor branch): a contact ahead of the car (ours:
/// along its velocity; retail's axis is the car's vtable +24, inferred forward) latches the hit
/// brake on the follower car. Logs `VEHICLE_HIT`. Every contact also goes to the car alarm rule as a
/// [`VehicleImpact`](crate::world_audio::VehicleImpact) (relative speed at the contact; the rule sets a
/// parked car's alarm off).
pub(crate) fn apply_vehicle_hits(
    mut skater: Option<ResMut<crate::physics::SkaterRuntime>>,
    cars: Query<(Entity, &TrafficCar, &CarMotion)>,
    mut traffic: ResMut<TrafficState>,
    mut impacts: Option<MessageWriter<crate::world_audio::VehicleImpact>>,
) {
    use crate::world_audio::{ImpactSource, VehicleImpact};
    let Some(skater) = skater.as_deref_mut() else { return };
    for (id, point, speed) in std::mem::take(&mut skater.vehicle_hits) {
        if id & PROXY_ID_TAG != PROXY_ID_TAG {
            continue;
        }
        let Some((entity, car, motion)) = cars.iter().find(|(_, c, _)| (PROXY_ID_TAG | c.id.to_u64()) == id) else { continue };
        if let Some(w) = impacts.as_mut() {
            w.write(VehicleImpact::speed(entity, ImpactSource::Player, speed));
        }
        let at = motion.curr.translation;
        let v = motion.velocity;
        let ahead = (Vec3::from_array(point) - at).dot(v) > 0.0;
        let serial = car.id.serial;
        if let Some(f) = traffic.cars.iter_mut().find(|c| c.key == serial) {
            if ahead && !f.hit_brake && f.speed > 0.0 {
                f.hit_brake = true;
                info!("VEHICLE_HIT car=#{serial} speed={:.1} at=[{:.1}, {:.1}, {:.1}]", f.speed, point[0], point[1], point[2]);
            }
        }
    }
}

/// V4 look-ahead (`skate_core::living_world::traffic::obstacles`): the obstacle lists (`sub_826B2EE0`: skaters
/// radius 0 and peds, soft; movable props (DMOs) radius half their smallest extent, hard; traffic cars add nothing,
/// `b42`) against each car's look-ahead quad from last frame's pose; the nearest free distance goes to the follower
/// car. Ours: the ped radius is the fallback ped radius (retail 0.5 x the body extents' z, the per-ped value is
/// collision data), props carry no corner points, a bailing skater's four extra points are not added, and the
/// turn widening is off (the turn side `+3748` is open; threshold 0.025).
fn look_ahead(
    traffic: &mut TrafficState,
    cars_q: &Query<(&TrafficCar, &mut CarMotion, &mut TrafficAudio, &mut AudioVelocity)>,
    observers: &super::LivingWorldObservers,
    peds: &Query<(&super::peds::Pedestrian, &super::peds::PedBody)>,
    props: Option<&super::peds::PedObstacles>,
    local_skitching: bool,
) {
    use skate_core::living_world::traffic::horn::ObstacleHit;
    use skate_core::living_world::traffic::skater_scan::{rear_zone_quad, scan, ScanActor};
    use skate_core::living_world::traffic::obstacles::{look_ahead_quad, nearest, CarFrame, LookAheadParams, Obstacle};
    let params = LookAheadParams::default();
    let mut list: Vec<Obstacle> = observers.observers.iter().map(|o| Obstacle { position: o.position, radius: 0.0, soft: true, id: None }).collect();
    list.extend(peds.iter().map(|(p, b)| Obstacle { position: b.position.to_array(), radius: super::vehicle_contacts::FALLBACK_PED_RADIUS, soft: true, id: Some(p.id.to_u64()) }));
    // The skater scan's list (82C414A8 walks the skater manager's actors, b63): our observers (index 0 = the local
    // player, skipped while skitching). NOT RETAIL YET: the facing is the velocity heading (retail: the actor
    // matrix row +32), and the NEAR flag byte `[[state+52]+55]` is not identified (false: no NEAR rule).
    let actors: Vec<ScanActor> = observers
        .observers
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let l = (o.velocity[0] * o.velocity[0] + o.velocity[2] * o.velocity[2]).sqrt();
            let forward = if l > 1e-6 { [o.velocity[0] / l, o.velocity[2] / l] } else { [0.0, 0.0] };
            ScanActor { position: o.position, velocity: o.velocity, forward, skitching: i == 0 && local_skitching, near_flag: false }
        })
        .collect();
    let cars: Vec<(u32, CarFrame, [f32; 3])> = cars_q
        .iter()
        .map(|(car, motion, ..)| {
            let r = motion.curr.rotation;
            let f = r * Vec3::Z;
            let s = r * Vec3::X;
            let flat = |v: Vec3| {
                let l = (v.x * v.x + v.z * v.z).sqrt().max(1e-6);
                [v.x / l, v.z / l]
            };
            let [lo, hi] = car.bounds;
            let frame = CarFrame {
                position: motion.curr.translation.to_array(),
                forward: flat(f),
                side: flat(s),
                half_width: (hi[0] - lo[0]) * 0.5,
                half_length: (hi[2] - lo[2]) * 0.5,
            };
            (car.id.serial, frame, [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]])
        })
        .collect();
    // Props: active (not obstacle-off) DMOs; mod bodies live above `MOD_BODY_OBSTACLE_BASE` and are not DMOs.
    list.extend(props.into_iter().flat_map(|p| p.0.states.iter()).filter(|(id, st)| **id < super::peds::MOD_BODY_OBSTACLE_BASE && !st.inactive).map(|(_, st)| Obstacle {
        position: [st.now.center[0], st.now.y_min, st.now.center[1]],
        radius: st.now.half[0].min(st.now.half[1]),
        soft: false,
        id: None,
    }));
    for (serial, frame, size) in &cars {
        let Some(car) = traffic.cars.iter_mut().find(|c| c.key == *serial) else { continue };
        let quad = look_ahead_quad(frame, car.speed, car.params.min_gap, 0.0, None, &params);
        // 82C344D0: a car the player holds skips the slow-speed soft records (b57).
        car.obstacle = nearest(frame, &quad, &list, car.speed, !car.player_held, &params).map(|(i, d)| ObstacleHit { distance: d, soft: list[i].soft, id: list[i].id });
        // 82C414A8 after the look-ahead: quad B with the same widths as quad A (speed ratio 0, see above).
        let w = frame.half_width * (1.0 + params.base_widen);
        let zone = rear_zone_quad(frame, w, w, car.params.skater.range);
        car.skater = scan(frame, *size, car.held, &zone, &actors, &car.params.skater);
    }
}

/// Add the car proxies to the skater solve (after the network proxies were rebuilt).
pub(crate) fn push_vehicle_proxies(
    cars: Query<(&TrafficCar, &CarMotion)>,
    mut physics: ResMut<crate::physics::GamePhysics>,
    skater: Res<crate::physics::SkaterRuntime>,
    replay: Res<crate::replay::Replay>,
) {
    if replay.active || cars.is_empty() {
        return;
    }
    let mut proxies = std::mem::take(&mut physics.network_proxies);
    let mut list: Vec<_> = cars.iter().collect();
    list.sort_by_key(|(c, _)| c.id);
    for (car, motion) in list {
        proxies.append_solid(proxy(car, &motion.curr, motion.velocity), &physics, &skater, false);
    }
    physics.network_proxies = proxies;
}

/// Grab-scene ids of the cars: object `CAR_GRAB_TAG | serial`, spline / geometry `CAR_GRAB_TAG | serial << 3 | index`
/// (ours: stable per car; retail numbers splines from a global counter, b34).
pub(crate) const CAR_GRAB_TAG: u32 = 0x8000_0000;

/// The cars' authored grab splines into the grab scene each tick (retail: the vehicle provider `82C35B98`, scene
/// slot `+4088`, answering the riding skitch query's mode 255; b49). The car's world transform is the record frame,
/// its velocity the record vector. NOT RETAIL YET: the assembly is a stand-in with the car's id (retail reads it
/// from the car's `+172` interface, object open).
pub(crate) fn push_vehicle_grab_splines(
    cars: Query<(&TrafficCar, &CarMotion)>,
    traffic: Res<TrafficState>,
    mut physics: ResMut<crate::physics::GamePhysics>,
    replay: Res<crate::replay::Replay>,
) {
    if replay.active {
        return;
    }
    let mut list: Vec<_> = cars.iter().collect();
    list.sort_by_key(|(c, _)| c.id);
    let objects: Vec<_> = list.into_iter().filter_map(|(car, motion)| car_grab_object(car, motion, &traffic.data)).collect();
    if let Err(e) = physics.set_grab_cars(objects) {
        warn!("LIVING_WORLD traffic: car grab splines rejected: {e}");
    }
}

/// One car as a grab-scene object (pure; the engine and the tests use it).
pub(crate) fn car_grab_object(car: &TrafficCar, motion: &CarMotion, data: &VehicleData) -> Option<skate_core::player::offboard::grab_scene::Object> {
    use skate_core::player::offboard::grab_scene::{AssemblyData, Descriptor, Geometry, Object, Provider, Spline};
    let splines = &data.models.get(&car.model)?.grab_splines;
    let serial = car.id.serial & 0x0FFF_FFFF;
    let id = CAR_GRAB_TAG | serial;
    let t = &motion.curr;
    let axis = |v: Vec3| {
        let w = t.rotation * v;
        [w.x, w.y, w.z, 0.0]
    };
    Some(Object {
        id,
        provider: Provider::Vehicle,
        disabled: false,
        assembly_ready: true,
        assembly: Some(AssemblyData { identity: id, first_part: None }),
        frame: [axis(Vec3::X), axis(Vec3::Y), axis(Vec3::Z), [t.translation.x, t.translation.y, t.translation.z, 1.0]],
        object_vector_128: [motion.velocity.x, motion.velocity.y, motion.velocity.z, 0.0],
        splines: splines
            .iter()
            .take(8)
            .enumerate()
            .map(|(i, s)| {
                let key = CAR_GRAB_TAG | serial << 3 | i as u32;
                Spline {
                    descriptor: Descriptor { kind: 1, id: key },
                    geometry: std::sync::Arc::new(Geometry {
                        id: key,
                        points: s.points.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect(),
                        approach_vectors: vec![[s.direction[0], s.direction[1], s.direction[2], 0.0]],
                        word_60: s.flags,
                    }),
                    word_272: 0,
                }
            })
            .collect(),
    })
}

/// The render side of one car.
#[derive(Component, Default)]
pub(crate) struct CarLook {
    scene: Option<Entity>,
    /// Wheel joint entity and its bind transform.
    wheels: Vec<(Entity, Transform)>,
    ready: bool,
}

/// Tinted / glass materials per (source material, colours).
#[derive(Resource, Default)]
pub(crate) struct CarMaterials(BTreeMap<(AssetId<StandardMaterial>, [u32; 8]), Handle<StandardMaterial>>);

/// Load the GLB, then tint the chassis, clear the glass and find the wheel bones.
#[allow(clippy::too_many_arguments)]
pub(crate) fn present_car_looks(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut cars: Query<(Entity, &TrafficCar, Option<&mut CarLook>)>,
    instances: Query<&bevy::scene::SceneInstance>,
    spawner: Res<SceneSpawner>,
    named: Query<(&Name, &Transform)>,
    meshes: Query<(&MeshMaterial3d<StandardMaterial>, Option<&bevy::gltf::GltfMaterialName>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<CarMaterials>,
) {
    for (e, car, look) in &mut cars {
        let Some(mut look) = look else {
            let scene = commands.spawn((SceneRoot(server.load(GltfAssetLabel::Scene(0).from_asset(car.glb.clone()))), Transform::default(), Visibility::Hidden, ChildOf(e))).id();
            commands.entity(e).insert(CarLook { scene: Some(scene), wheels: Vec::new(), ready: false });
            continue;
        };
        let Some(scene) = look.scene else { continue };
        if look.ready || !instances.get(scene).is_ok_and(|i| spawner.instance_is_ready(**i)) {
            continue;
        }
        let Ok(instance) = instances.get(scene) else { continue };
        let entities: Vec<Entity> = spawner.iter_instance_entities(**instance).collect();
        let mut pending = false;
        let mut wheels = Vec::new();
        for ent in entities {
            if let Ok((name, t)) = named.get(ent) {
                if WHEEL_BONES.contains(&name.as_str()) {
                    wheels.push((ent, *t));
                }
            }
            let Ok((handle, name)) = meshes.get(ent) else { continue };
            let kind = name.map(|n| n.0.as_str()).unwrap_or("");
            commands.entity(ent).insert((bevy::camera::visibility::RenderLayers::from_layers(&[0, 28]), bevy::camera::visibility::NoFrustumCulling));
            let bits = |c: [f32; 4]| c.map(f32::to_bits);
            let ch = bits(car.chassis);
            let se = bits(car.secondary);
            let key = (handle.0.id(), [ch[0], ch[1], ch[2], ch[3], se[0], se[1], se[2], if kind.starts_with("vehicle_glass") { 1 } else { car.paint_gain.to_bits() | 2 }]);
            if let Some(m) = cache.0.get(&key) {
                commands.entity(ent).insert(MeshMaterial3d(m.clone()));
                continue;
            }
            let Some(source) = materials.get(&handle.0).cloned() else {
                pending = true;
                continue;
            };
            let replacement = if kind.starts_with("vehicle_chassis") {
                let Some(tex) = source.base_color_texture.clone() else { continue };
                let Some(image) = images.get(&tex) else {
                    pending = true;
                    continue;
                };
                let mut tinted = image.clone();
                match tinted.data.as_mut() {
                    Some(px) if tinted.texture_descriptor.format.block_copy_size(None) == Some(4) => tint_rgba8(px, car.chassis, car.secondary, car.paint_gain),
                    _ => {
                        warn!("LIVING_WORLD traffic: {} base texture is not RGBA8; drawn untinted", car.glb);
                    }
                }
                let mut m = source.clone();
                m.base_color_texture = Some(images.add(tinted));
                m
            } else if kind.starts_with("vehicle_glass") {
                let mut m = source.clone();
                m.base_color = m.base_color.with_alpha(GLASS_ALPHA);
                m.alpha_mode = AlphaMode::Blend;
                m.perceptual_roughness = 0.1;
                m.reflectance = 0.8;
                m
            } else {
                continue;
            };
            let h = materials.add(replacement);
            cache.0.insert(key, h.clone());
            commands.entity(ent).insert(MeshMaterial3d(h));
        }
        if pending {
            continue;
        }
        look.wheels = wheels;
        look.ready = true;
        commands.entity(scene).insert(Visibility::Inherited);
    }
}

/// Interpolate the car between fixed steps and spin the wheels.
pub(crate) fn present_car_pose(fixed: Res<Time<Fixed>>, mut cars: Query<(&CarMotion, &CarLook, &mut Transform)>, mut joints: Query<&mut Transform, Without<CarLook>>) {
    let a = fixed.overstep_fraction();
    for (motion, look, mut t) in &mut cars {
        t.translation = motion.prev.translation.lerp(motion.curr.translation, a);
        t.rotation = motion.prev.rotation.slerp(motion.curr.rotation, a);
        let mut d = motion.wheel - motion.wheel_prev;
        if d < 0.0 {
            d += std::f32::consts::TAU;
        }
        let angle = motion.wheel_prev + d * a;
        for (joint, bind) in &look.wheels {
            if let Ok(mut j) = joints.get_mut(*joint) {
                j.rotation = Quat::from_rotation_x(angle) * bind.rotation;
            }
        }
    }
}

/// One-line traffic summary for the debug readout (`SKATE_LIVING_WORLD_DEBUG=1`).
pub(crate) fn traffic_readout(traffic: &TrafficState) -> String {
    let moving = traffic.cars.iter().filter(|c| c.speed > 0.5).count();
    let waiting = traffic.cars.iter().filter(|c| matches!(c.entry, Some(Entry::Signal | Entry::Yield | Entry::Blocked)) && c.speed < 0.5).count();
    let mean = if traffic.cars.is_empty() { 0.0 } else { traffic.cars.iter().map(|c| c.speed).sum::<f32>() / traffic.cars.len() as f32 };
    format!("traffic cars {} moving {} waiting {} mean speed {:.1} m/s signal ticks {}", traffic.cars.len(), moving, waiting, mean, traffic.clock.as_ref().map_or(0, |c| c.ticks()))
}

fn log_traffic(settings: Res<LivingWorldSettings>, state: Res<PopulationState>, traffic: Res<TrafficState>, mut last: Local<u64>) {
    if !settings.debug || !super::report_due(state.world.tick(), &mut last, 300) {
        return;
    }
    info!("LIVING_WORLD {}", traffic_readout(&traffic));
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<TrafficState>()
        .init_resource::<VehicleOverrides>()
        .init_resource::<CarMaterials>()
        .add_message::<TrafficEvent>()
        .add_systems(FixedUpdate, (load_traffic_data, apply_vehicle_records, apply_vehicle_hits, drive_traffic, log_traffic).chain().after(super::step_population))
        .add_systems(
            FixedUpdate,
            (
                push_vehicle_proxies.after(crate::multiplayer::prepare),
                super::ped_hand_props::push_hand_prop_proxies.after(push_vehicle_proxies),
                push_vehicle_grab_splines,
            )
                .after(crate::app::SimulationSet::Controls)
                .before(crate::app::SimulationSet::Physics),
        )
        .add_systems(Update, (present_car_looks, present_car_pose).chain().after(crate::app::FrameSet::Animation));
}
