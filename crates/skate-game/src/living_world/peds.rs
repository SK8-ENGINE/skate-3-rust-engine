//! Pedestrians, body tier: spawn records -> visible, animated, navigating peds with footstep audio.
//! Details: docs/hails-additions/26f-living-world-peds.md#ped-body-tier

use super::{LivingWorldDespawn, LivingWorldSettings, LivingWorldSpawn, PopulationState};
use crate::world_audio::PedAudio;
use bevy::prelude::*;
use skate_core::living_world::peds::anim::{PedClip, PedClips, TestPath};
use skate_core::living_world::peds::wander::{NoSignals, constrain_move, crosswalk_ok, separation_blocker};
use skate_core::living_world::peds::{CrosswalkRule, Locomotion, NavMesh, NavObstacles, NavRules, Neighbour, ObstacleInput, PedAnimPlayer, PedCatalog, PedEvaluator, PedNav, PedOverrides, PedRig, WalkSignals, WanderParams};
use skate_core::living_world::{Decision, DespawnReason, Kind, LivingWorldId, SpawnChoice};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Seconds per population tick: the ped player steps once per world tick (retail's 60 Hz world
/// step, `clock::RETAIL_TICK_HZ`), so a ped's state is a function of its spawn record and the tick.
pub(crate) fn tick_seconds(hz: f64) -> f32 {
    (1.0 / hz.max(1.0)) as f32
}

/// Loaded ped data for the current world.
#[derive(Resource, Default, Clone)]
pub(crate) struct PedData {
    pub catalog: Arc<PedCatalog>,
    pub anim_sets: Arc<BTreeMap<String, skate_core::living_world::peds::PedAnimSet>>,
    pub rig: Arc<PedRig>,
    pub clips: Arc<BTreeMap<String, PedClip>>,
    pub status: String,
    /// The district's navmesh (M3); `None` on maps without one (peds use [`TestPath`]).
    pub nav: Option<Arc<NavMesh>>,
    /// The navmesh records as loaded (rebuilt with new [`NavRules`] when a mod changes them).
    pub nav_input: Option<Arc<skate_core::living_world::peds::NavMeshInput>>,
    /// Signaled junction arms of the loaded roads (mod crosswalk rule).
    pub arms: Arc<Vec<(usize, u8, [f32; 3])>>,
    /// The stock ped AI graph on the shared graph runtime (behavior runtime; `None` when the
    /// install has none).
    pub graph: Option<Arc<super::ped_graph::PedGraph>>,
    /// The stock mood tables and each entity type's reaction set (`ped_mood`).
    pub mood: Option<Arc<skate_core::living_world::peds::mood::MoodTables>>,
    pub reaction_sets: Arc<BTreeMap<String, String>>,
    /// Each entity type's chase record and the chase manager's `global` record (`ped_mood::chase_records`).
    pub chase: Arc<BTreeMap<String, skate_core::living_world::peds::chase::ChaseRecord>>,
    pub chase_global: Option<Arc<skate_core::living_world::peds::chase::ChaseRecord>>,
    /// The `zombie` chase record (zombie cheat: every chase group and every ped's own record, `ped_mood::ZOMBIE_KEY`).
    pub chase_zombie: Option<Arc<skate_core::living_world::peds::chase::ChaseRecord>>,
    /// The conversation plugin graph (`plugin/conversation.stategraph`) and the conversation tables.
    pub conversation_graph: Option<Arc<super::ped_graph::PedGraph>>,
    /// The world-prop plugin graphs by prop class (each descriptor's `file`, compiled).
    pub plugin_graphs: BTreeMap<String, Arc<super::ped_graph::PedGraph>>,
    pub conversations: Arc<super::ped_mood::ConversationTables>,
    /// Ped plugins on world props: classes, descriptors, `plugin_odds`, placed props (`ped_plugins`).
    pub plugins: Arc<super::ped_plugins::PluginData>,
    /// Hand props: records and models (`ped_hand_props`).
    pub hand_props: Arc<super::ped_hand_props::HandPropData>,
    /// Each entity type's starting hand prop chance and list (`ped_mood::starting_hand_props`).
    pub starting_props: Arc<BTreeMap<String, (f32, Vec<(String, f32)>)>>,
    /// The sit plugin's values by entity type (`ped_mood::sit_values`).
    pub sit: Arc<BTreeMap<String, skate_core::living_world::peds::brain::SitValues>>,
    /// Each entity type's vision test ranges (`ped_mood::sight`).
    pub sight: Arc<BTreeMap<String, skate_core::living_world::peds::perception::Sight>>,
    /// Each entity type's perception scales (near, far, fov) for the mood post's vision test.
    pub sight_scales: Arc<BTreeMap<String, [f32; 3]>>,
    /// Each entity type's takedown table (`ped_mood::takedown_tables`).
    pub takedowns: Arc<BTreeMap<String, skate_core::living_world::peds::takedown::TakedownTable>>,
    /// The `zombie` takedown table (zombie cheat, every ped).
    pub takedown_zombie: Option<Arc<skate_core::living_world::peds::takedown::TakedownTable>>,
    pub(super) loaded_for: Option<(String, u64)>,
    /// Loaded behind the loading screen: the first fixed tick leaves `arms` empty, as a late load did.
    pub(super) skip_arms_once: bool,
}

impl PedData {
    /// The animation set to play: the entity's, or `default` when the set's idle clips are not
    /// in the bank (the `granny` set names `GRAN_WNDR_*` clips that no shipped bank holds [data]).
    pub(crate) fn playable_set(&self, name: &str) -> Option<(&String, &skate_core::living_world::peds::PedAnimSet)> {
        let playable = |set: &skate_core::living_world::peds::PedAnimSet| {
            set.entries.get(skate_core::living_world::peds::anim::names::IDLE).is_some_and(|l| !l.is_empty() && l.iter().all(|c| self.clips.contains_key(&c.clip)))
        };
        self.anim_sets.get_key_value(name).filter(|(_, s)| playable(s)).or_else(|| self.anim_sets.get_key_value("default").filter(|(_, s)| playable(s)))
    }

    pub(crate) fn ready(&self) -> bool {
        !self.rig.names.is_empty() && !self.catalog.categories.is_empty()
    }

    /// Read the district's navmesh (M3) from `navmesh.bin` (none: peds use the test path).
    pub(crate) fn load_nav(&mut self, asset_root: &std::path::Path, district: &str, rules: &NavRules) {
        let input = std::fs::read(asset_root.join(skate_data::ped_nav::NAVMESH))
            .map_err(|e| e.to_string())
            .and_then(|b| skate_data::ped_nav::district(&b, district));
        match input {
            Ok(Some(input)) => {
                let mesh = NavMesh::build(&input, rules.clone());
                self.status.push_str(&format!(", navmesh {} polygons", mesh.polys.len()));
                self.nav = Some(Arc::new(mesh));
                self.nav_input = Some(Arc::new(input));
            }
            Ok(None) => self.status.push_str(", no navmesh for this map"),
            Err(e) => self.status.push_str(&format!(", navmesh: {e}")),
        }
    }

    /// Read the tables and the bank, decode every clip an animation set names.
    pub(crate) fn load(asset_root: &std::path::Path) -> Self {
        Self::load_with(asset_root, None)
    }

    /// [`Self::load`] with `tables.json` already parsed (`None`: read and parse it here).
    pub(crate) fn load_with(asset_root: &std::path::Path, tables: Option<&serde_json::Value>) -> Self {
        let tables = match tables {
            Some(doc) => skate_data::ped_anim::PedTables::from_value(doc),
            None => std::fs::read(asset_root.join("private/living_world/tables.json")).map_err(|e| e.to_string()).and_then(|b| skate_data::ped_anim::PedTables::parse(&b)),
        };
        let bank = skate_data::ped_anim::PedBank::load(asset_root);
        match (tables, bank) {
            (Ok(t), Ok(bank)) => {
                let mut clips = BTreeMap::new();
                let mut failed = 0;
                for set in t.anim_sets.values() {
                    for c in set.entries.values().flatten() {
                        if !clips.contains_key(&c.clip) {
                            match bank.clip(&c.clip) {
                                Ok(clip) => {
                                    clips.insert(c.clip.clone(), clip);
                                }
                                Err(_) => failed += 1,
                            }
                        }
                    }
                }
                let status = format!("peds: {} categories, {} sets, {} clips ({} missing, {} unresolved remaps)", t.catalog.categories.len(), t.anim_sets.len(), clips.len(), failed, t.unresolved);
                Self { catalog: Arc::new(t.catalog), anim_sets: Arc::new(t.anim_sets), rig: Arc::new(bank.rig), clips: Arc::new(clips), status, loaded_for: None, ..Self::default() }
            }
            (t, b) => Self { status: format!("peds: no body data ({})", [t.err(), b.err()].into_iter().flatten().collect::<Vec<_>>().join("; ")), ..Self::default() },
        }
    }
}

/// Mod / engine look overrides. Default = retail.
#[derive(Resource, Default, Clone, PartialEq)]
pub(crate) struct PedLooks {
    pub overrides: PedOverrides,
    /// recipe -> GLB asset path (with the ped bone names).
    pub glb: BTreeMap<String, String>,
    /// Material name (`character.livingworld_pedestrians_stamp`, ...) -> `m_params` rows over the export's.
    pub material_params: BTreeMap<String, Vec<[f32; 4]>>,
}

/// One ped's identity (from its spawn record).
#[derive(Component, Clone, Debug, PartialEq)]
pub(crate) struct Pedestrian {
    pub id: LivingWorldId,
    pub census: String,
    pub category: String,
    pub entity: String,
    pub model: String,
    pub recipe: String,
    pub anim_set: String,
    pub seed: u64,
    pub spawn_tick: u64,
    pub tint_a: [f32; 4],
    pub tint_b: [f32; 4],
}

/// Navigation settings (mod-facing; default = retail): wander parameters, which navmesh areas
/// peds may use and what they cost, and the crosswalk rule (retail `Off`).
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(crate) struct PedNavSettings {
    pub wander: WanderParams,
    pub rules: NavRules,
    pub crosswalk: CrosswalkRule,
}

/// The ped ground probe's retry reach after a miss (`82EA5F48`: 4.0 at `0x82257308`) [code, b107].
pub(crate) const GROUND_RETRY_FACTOR: f32 = 4.0;

/// The ped ground probe's length L (`82EA5F48`: the probe settings' +16), 1.6 m on every ped call in the recomp
/// [trace, 2026-10-19, b359 section 9].
pub(crate) const GROUND_PROBE_LENGTH: f32 = 1.6;

/// Dynamic objects as ped navigation obstacles (fix 11): props and mod bodies by stable id.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(crate) struct PedObstacles(pub NavObstacles);

/// Obstacle id of a mod body (props keep their prop id; mod bodies live above 2^40).
pub(crate) const MOD_BODY_OBSTACLE_BASE: u64 = 1 << 40;

/// The obstacle list of this tick: prop boxes (current pose, held flag) and mod bodies (world
/// AABBs, the attached one held). Held is not the retail obstacle-off gate (that word is never set by
/// Move Object), so a held prop stays an obstacle (`ObstacleParams::held_is_obstacle`, retail on).
pub(crate) fn obstacle_inputs(physics: Option<&crate::physics::GamePhysics>, mod_solids: &[(u64, [f32; 3], [f32; 3], [f32; 3], bool)]) -> Vec<ObstacleInput> {
    let mut out = physics.and_then(|p| p.prop_dynamics()).map(prop_obstacle_inputs).unwrap_or_default();
    for (id, min, max, v, attached) in mod_solids {
        let center = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5, (min[2] + max[2]) * 0.5];
        let half = [(max[0] - min[0]) * 0.5, (max[1] - min[1]) * 0.5, (max[2] - min[2]) * 0.5];
        if !half.iter().all(|h| h.is_finite()) {
            continue;
        }
        out.push(ObstacleInput { id: MOD_BODY_OBSTACLE_BASE | id, center, axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], half_extents: half, velocity: *v, inactive: false, held: *attached });
    }
    out
}

/// The prop boxes of this tick as obstacle inputs (current pose and velocity, held flag).
pub(crate) fn prop_obstacle_inputs(d: &crate::physics::prop_dynamics::PropDynamics) -> Vec<ObstacleInput> {
    d.obstacle_boxes()
        .into_iter()
        .map(|(id, c, basis, h, v, held)| ObstacleInput { id: id as u64, center: [c.x, c.y, c.z], axes: basis.columns, half_extents: [h.x, h.y, h.z], velocity: [v.x, v.y, v.z], inactive: false, held })
        .collect()
}

/// A prop as the object flee sees it: box center, velocity and the footprint corners of its x / z faces.
fn moving_object(o: &ObstacleInput) -> skate_core::living_world::peds::object_flee::MovingObject {
    let (c, a, b) = (o.center, o.axes[0], o.axes[2]);
    let (ha, hb) = (o.half_extents[0], o.half_extents[2]);
    let corner = |sa: f32, sb: f32| [c[0] + a[0] * ha * sa + b[0] * hb * sb, c[2] + a[2] * ha * sa + b[2] * hb * sb];
    skate_core::living_world::peds::object_flee::MovingObject { centre: c, velocity: o.velocity, corners: [corner(-1.0, -1.0), corner(1.0, -1.0), corner(1.0, 1.0), corner(-1.0, 1.0)] }
}

/// Diagnostics for moved props as ped obstacles (2026-10-07: "Moving objects does not update the
/// collision for peds", and the sessions had no obstacle lines). Spawn = where an id was first seen.
#[derive(Resource, Default)]
pub(crate) struct PedObstacleTrace {
    spawn: BTreeMap<u64, [f32; 3]>,
    /// Ids now more than 0.1 m from their spawn, with the spawn center.
    pub moved: Vec<(u64, [f32; 3])>,
    /// Last logged (cut, cut center, off, held, moving, tick) of a moved id.
    last: BTreeMap<u64, (bool, [f32; 3], bool, bool, bool, u64)>,
    next_summary: u64,
}

fn dist3(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Retail's per-object obstacle update, once per population tick before the peds step. Only a
/// changed cut bumps the version (resting props cost no rebuild).
pub(crate) fn update_ped_obstacles(
    settings: Res<LivingWorldSettings>,
    data: Res<PedData>,
    physics: Option<Res<crate::physics::GamePhysics>>,
    mods: Option<Res<crate::modding::Mods>>,
    mut obstacles: ResMut<PedObstacles>,
    state: Res<PopulationState>,
    mut trace: ResMut<PedObstacleTrace>,
    observers: Res<super::LivingWorldObservers>,
) {
    let _span = super::timing::span(super::timing::PED_OBSTACLES);
    obstacles.0.set_params(settings.ped_obstacles.clone());
    if data.nav.is_none() {
        if !obstacles.0.states.is_empty() {
            obstacles.0.update(&[]);
        }
        return;
    }
    let solids = mods.as_deref().map(crate::modding::bridge::obstacle_solids).unwrap_or_default();
    let mut inputs = obstacle_inputs(physics.as_deref(), &solids);
    // Every player skater (b135: always NavPower's moving avoider for peds), by player index.
    if settings.ped_obstacles.skaters {
        let h = settings.ped_obstacles.skater_half_extents;
        for (n, o) in observers.observers.iter().enumerate() {
            let center = [o.position[0], o.position[1] + h[1], o.position[2]];
            inputs.push(ObstacleInput { id: skate_core::living_world::peds::obstacles::SKATER_OBSTACLE_BASE + n as u64, center, axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], half_extents: h, velocity: o.velocity, inactive: false, held: false });
        }
    }
    obstacles.0.update(&inputs);
    let tick = state.world.tick();
    let hz = state.world.clock().hz as u64;
    let trace = &mut *trace;
    trace.moved.clear();
    for input in &inputs {
        let spawn = *trace.spawn.entry(input.id).or_insert(input.center);
        if dist3(spawn, input.center) <= 0.1 && !trace.last.contains_key(&input.id) {
            continue;
        }
        trace.moved.push((input.id, spawn));
        let Some(s) = obstacles.0.states.get(&input.id) else { continue };
        let now = (s.cut.is_some(), s.cut_at, s.inactive, s.held, s.moving, tick);
        // A state change, a re-cut, or once a second while held (the body pose of a dragged prop
        // next to its cut pose: "peds walk through the prop I am holding", 2026-10-08).
        let changed = trace.last.get(&input.id).is_none_or(|l| {
            l.0 != now.0 || l.2 != now.2 || l.3 != now.3 || l.4 != now.4 || (now.0 && dist3(l.1, now.1) > 0.05) || (now.3 && tick >= l.5 + hz)
        });
        if changed {
            trace.last.insert(input.id, now);
            let speed = dist3(input.velocity, [0.0; 3]);
            let role = if s.inactive { "off" } else if s.cut.is_some() { "cut" } else if obstacles.0.params.moving_solid { "solid" } else { "none" };
            info!(
                "PED_OBSTACLE id={} spawn=[{:.2}, {:.2}, {:.2}] now=[{:.2}, {:.2}, {:.2}] moved={:.2} cut={} cut_at=[{:.2}, {:.2}, {:.2}] cut_off={:.2} speed={speed:.2} moving={} held={} off={} role={role} half=[{:.2}, {:.2}] y=[{:.2}, {:.2}] version={} tick={tick}",
                input.id, spawn[0], spawn[1], spawn[2], input.center[0], input.center[1], input.center[2], dist3(spawn, input.center),
                now.0, now.1[0], now.1[1], now.1[2], if now.0 { dist3(now.1, input.center) } else { -1.0 }, s.moving, s.held, s.inactive, s.now.half[0], s.now.half[1], s.now.y_min, s.now.y_max, obstacles.0.version,
            );
        }
    }
    if tick >= trace.next_summary {
        trace.next_summary = tick + hz * if trace.moved.is_empty() { 10 } else { 2 };
        let o = &obstacles.0;
        info!(
            "PED_OBSTACLES inputs={} props={} states={} cut={} moving={} held={} off={} moved={} version={} tick={tick}",
            inputs.len(), inputs.iter().filter(|i| i.id < MOD_BODY_OBSTACLE_BASE).count(), o.states.len(), o.cut_count(),
            o.states.values().filter(|s| s.moving).count(), o.states.values().filter(|s| s.held).count(), o.states.values().filter(|s| s.inactive).count(), trace.moved.len(), o.version,
        );
    }
}

/// The simulated body: animation player, navigation, position and heading.
#[derive(Component, Clone, Debug)]
pub(crate) struct PedBody {
    pub player: PedAnimPlayer,
    /// Placeholder intent source on maps without a navmesh.
    pub path: TestPath,
    /// Navigation state (M3); a mod route goes in `nav.route`.
    pub nav: PedNav,
    /// Seconds the body's steps have been refused (walls, other peds, the crosswalk rule).
    pub blocked: f32,
    /// A stored contact reaction whose Colliding state began this tick: `advance_peds` plays it (b326).
    pub reaction_due: Option<skate_core::living_world::peds::skater_contact::StoredReaction>,
    pub position: Vec3,
    /// The last console tick's movement over its length, m/s (the attack throw's look-ahead, `82E3E960`).
    pub velocity: Vec3,
    pub heading: f32,
    /// Console ticks stepped since the spawn.
    pub ticks: u64,
    pub feet_down: [bool; 2],
    pub body_fall: f32,
    /// The taunt clip (motiongraph_taunt): requested by TakedownTauntVictim, playing, finished (the brain then
    /// drops its "SGIntent").
    pub taunt: TauntClip,
    pub plugin_motion: PluginMotionRun,
    /// The head / spine turn towards the attention stack's point (b1019), added on top of the pose.
    pub head_look: skate_core::living_world::peds::head_look::HeadLook,
}

/// Where a ped's taunt clip is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TauntClip {
    #[default]
    None,
    Requested,
    Playing,
    Done,
}

/// A plugin state's run on the motion side (`skate_core::living_world::peds::plugin_motion`): the packet it plays,
/// its progress (the taunt's handshake) and a pending release of its held cycle.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PluginMotionRun {
    pub packet: Option<String>,
    pub state: TauntClip,
    pub release: bool,
    /// The packet ended while its clips still play (the AI left the sit because the seat moved away). Retail's sit
    /// state is non-interruptable (`motiongraph_sit.xml`): Stand2Sit plays out, then SittingCyc leaves on
    /// "not HasIntent Sit" to Sit2Stand, so the held cycle is released as soon as it is reached.
    pub orphaned: bool,
    /// The step that spawns the hand prop reached its branch window (`SpawnInteractionBasedHandProp`); the brain side
    /// takes it once (`think_peds`) and `hand_prop_taken` keeps it from firing again in the same run.
    pub hand_prop_due: bool,
    pub hand_prop_taken: bool,
    /// A hand prop throw clip the brain asked for (`82E3E648` plays it on the ped, blend 0.2) and its blend.
    pub throw_clip: Option<(&'static str, f32)>,
    /// A tazer clip the brain asked for (`ChaseRequest::TazerClip`), and the last one the channel took with whether
    /// the ped's set had it (read by `ped_hand_props::sync_hand_props` for `PedBrain::update_tazer_clip`).
    pub tazer_clip: Option<&'static str>,
    /// A greet / warn gesture the brain asked for this tick (b677), played on the channel.
    pub gesture_clip: Option<&'static str>,
    pub tazer_played: Option<(&'static str, bool)>,
}

/// A ped counts as still while it stays within this distance of where it stopped, m.
const STILL_RADIUS: f32 = 0.1;
/// Ticks still before the first report and between reports (5 s on the 60 Hz world clock).
const STILL_REPORT_TICKS: u64 = 300;
/// Full still-ped reports per tick; due peds beyond this wait for a later tick, so many stuck
/// peds do not all format their report in the same frame.
const STILL_REPORTS_PER_TICK: u32 = 1;

/// Diagnostics: a ped that has not moved for 5 s logs `PED_STILL` with everything about it (brain,
/// mood, graph and plugin state, animation, navigation, conversation), then every 5 s while it
/// stays; a report identical to the previous one logs a short line.
/// A ped with a nav target that stays within this distance logs `PED_NAV_STALL` (walking in place: b511), m.
const STALL_RADIUS: f32 = 0.5;

/// Moves the watch's anchor when the ped left `radius`; else the tick it has been within it since, once a report is due.
fn watch_due(watch: &mut StillWatch, at: [f32; 3], radius: f32, tick: u64) -> Option<u64> {
    let moved = watch.anchor.is_none_or(|(p, _)| (p[0] - at[0]).hypot(p[2] - at[2]) > radius || (p[1] - at[1]).abs() > radius);
    if moved {
        watch.anchor = Some((at, tick));
        watch.due = tick + STILL_REPORT_TICKS;
        watch.last.clear();
        return None;
    }
    (tick >= watch.due).then(|| watch.anchor.map_or(tick, |a| a.1))
}

fn log_still_ped(ped: &Pedestrian, body: &PedBody, mind: &mut PedMind, state: &str, conversations: &BTreeMap<u64, skate_core::living_world::peds::conversation::Conversation>, tick: u64, budget: &mut u32) {
    let at = body.position.to_array();
    let still_since = watch_due(&mut mind.still, at, STILL_RADIUS, tick);
    let stall_since = if body.nav.target.is_some() {
        watch_due(&mut mind.stall, at, STALL_RADIUS, tick)
    } else {
        mind.stall = StillWatch::default();
        None
    };
    // A ped still for a full report period is PED_STILL's, not a stall.
    let still_long = mind.still.anchor.is_some_and(|a| tick >= a.1 + STILL_REPORT_TICKS);
    let stall_since = stall_since.filter(|_| !still_long);
    if (still_since.is_none() && stall_since.is_none()) || *budget == 0 {
        return;
    }
    *budget -= 1;
    let me = ped.id.to_u64();
    let member = conversations.values().find(|c| c.members.iter().any(|m| m.ped == me));
    let near = conversations.values().filter(|c| (c.center[0] - at[0]).hypot(c.center[2] - at[2]) < 5.0).map(|c| (c.id, c.state, c.members.len())).collect::<Vec<_>>();
    let conversation = member.map(|c| format!("member of {} state={} speaker={} members={:?} row={:?}", c.id, c.state, c.speaker, c.members, c.row));
    let mut brain = mind.brain.clone();
    brain.rng = None;
    let nav = &body.nav;
    let report = format!(
        "type={} census={} model={} state={state} plugin_graph={:?} plugin_state={:?} prop={:?} conversation={conversation:?} conversations_within_5m={near:?} \
         anim={{locomotion={} clip={} clip_time={:.2} channel={:?} taunt={:?} plugin_motion={:?}}} \
         nav={{target={:?} corner={}/{} waiting={:?} waited={:.1} route={} poly={:?} blocked={:.1}}} heading={:.2} velocity={:?} \
         flee_goal={:?} chase_goal={:?} mood={:?} brain={brain:?}",
        ped.entity,
        ped.census,
        ped.model,
        mind.plugin_graph,
        mind.plugin_state,
        mind.prop,
        body.player.state.name(),
        body.player.current_clip(),
        body.player.current_time(),
        body.player.channel_clip(),
        body.taunt,
        body.plugin_motion,
        nav.target,
        nav.next,
        nav.corners.len(),
        nav.waiting,
        nav.waited,
        nav.route.is_some(),
        nav.poly,
        body.blocked,
        body.heading,
        body.velocity.to_array(),
        mind.flee_goal,
        mind.chase_goal,
        mind.mood,
    );
    for (kind, since, watch) in [("PED_STILL", still_since, &mut mind.still), ("PED_NAV_STALL", stall_since, &mut mind.stall)] {
        let Some(since) = since else { continue };
        watch.due = tick + STILL_REPORT_TICKS;
        let seconds = (tick - since) as f32 / 60.0;
        if report == watch.last {
            info!("{kind} ped=#{} at=[{:.2}, {:.2}, {:.2}] still={seconds:.0}s unchanged tick={tick}", ped.id.serial, at[0], at[1], at[2]);
        } else {
            info!("{kind} ped=#{} at=[{:.2}, {:.2}, {:.2}] still={seconds:.0}s {report} tick={tick}", ped.id.serial, at[0], at[1], at[2]);
            watch.last = report.clone();
        }
    }
}

/// LivingWorldId -> entity.
#[derive(Resource, Default)]
pub(crate) struct PedIndex(pub BTreeMap<LivingWorldId, Entity>);

/// What happened to a ped (engine systems, mods).
#[derive(Message, Clone, Debug, PartialEq)]
pub(crate) enum PedEvent {
    Spawned { id: LivingWorldId, entity: String, recipe: String },
    /// The look could not be resolved (no entities / model): the population slot is released.
    Rejected { id: LivingWorldId, category: String },
    Despawned { id: LivingWorldId, reason: DespawnReason },
    /// A chase group ended by aggressivecapture of the local player through the ChaserGroupEndChase op (b680).
    ChaseCaught { target: u64 },
    State { id: LivingWorldId, state: Locomotion },
    /// The mood system picked a reaction (`skate_core::living_world::peds::mood`); `wants` were
    /// raised when `passed`.
    Mood { id: LivingWorldId, result: String, passed: bool, wants: Vec<String> },
    /// A skater knocked the ped down or made it stumble (`skate_core::living_world::peds::skater_contact`).
    Hit {
        id: LivingWorldId,
        kind: skate_core::living_world::peds::skater_contact::ReactionKind,
        direction: skate_core::living_world::peds::skater_contact::ReactionDirection,
        /// The skater's speed into the ped, m/s (our stand-in for the ped body's speed).
        closing: f32,
        /// Who touched the ped (the brain's contact record `+3184`; players: `PLAYER_TARGET_BASE + n`).
        toucher: u64,
    },
    /// A contact during the ped's takedown attempt (not by its target): stored, not played (`82E38FB8` stores the
    /// reaction; the attempt has no Colliding transition); it plays at the ped's next Colliding (b326).
    ContactStored {
        id: LivingWorldId,
        kind: skate_core::living_world::peds::skater_contact::ReactionKind,
        direction: skate_core::living_world::peds::skater_contact::ReactionDirection,
        closing: f32,
        toucher: u64,
    },
    /// A stored reaction played when the ped's Colliding state began (a contact during a takedown attempt, or a
    /// failed takedown's trip; `toucher` is `None` for a trip). Not a `Hit`: it does not arm Colliding again.
    DeferredHit {
        id: LivingWorldId,
        kind: skate_core::living_world::peds::skater_contact::ReactionKind,
        direction: skate_core::living_world::peds::skater_contact::ReactionDirection,
        closing: f32,
        toucher: Option<u64>,
    },
    /// The ped's Colliding state began (PedestrianColliding Begin `8269DA00`): the collision mood broadcast
    /// (`sub_8269D8A8`), read by every ped's think on the next step.
    Colliding { id: LivingWorldId },
    /// A chase group changed (`kind`: join, join_refused, leave, primary, group_end); `chasee` is
    /// the chased id (players: `PLAYER_TARGET_BASE + n`).
    Chase { id: LivingWorldId, chasee: u64, kind: &'static str, reason: Option<i32> },
    /// A ped decided to dodge a skater coming at it (SkaterAvoidance: `kind` jump / step, `dir` forward / left / right /
    /// back); `mover` as the chase ids (players: `PLAYER_TARGET_BASE + n`).
    Avoid { id: LivingWorldId, mover: u64, kind: &'static str, dir: &'static str, seated: bool },
    /// A ped's tazer hit `target` (the takedown's knock-down).
    Taze { id: LivingWorldId, target: u64 },
    /// A ped's takedown attempt ended (`success`: the target is knocked down).
    Takedown { id: LivingWorldId, target: u64, success: bool },
    /// The ped's AI graph changed its speech value (`SendSpeechEvent` / the warn); `state` is the
    /// graph state that sent it.
    /// `topic`: a conversation turn's variant and row value (`ped+2472` / `+2476`).
    Speech { id: LivingWorldId, value: i32, topic: Option<(u8, i32)>, state: String },
    /// The ped took a hand prop (`livingworld_handprops` key) at a plugin prop (SpawnInteractionBasedHandProp).
    HandProp { id: LivingWorldId, key: String },
}

/// Skater against ped contact (doc 26, "Skater hits peds"). NOT RETAIL YET: the skater is a
/// vertical cylinder at the observer (the board) of this radius and 2 m tall, and the ped body's
/// speed after the contact (retail: the Havok solve) is the skater's speed into the ped.
pub(crate) const SKATER_CONTACT_RADIUS: f32 = 0.35;
/// `82E3DBE0`'s gate on the owned object's step vector, |d|^2 > 2.5e-5 m^2 (`0x822F8BE0`) [code, b219: a step, not a
/// speed; smaller steps carry the ped].
pub(crate) const SEAT_PUSH_STEP_SQ: f32 = 2.5e-5;
/// The period that step is taken over, s: the DMO post-update `82C562D0` sends its pose change since the last
/// committed pose to its listeners once per 1/60 s sim tick (b220 [code]; main re-read the pose difference and the
/// slot 0x68 call), so the gate is 0.3 m/s (the speed-squared reading before 2026-10-17 fired at 5 mm/s).
pub(crate) const SEAT_PUSH_STEP_SECONDS: f32 = 1.0 / 60.0;
/// `82C539A8`'s seat probe tolerance, m (the navmesh point within this of the seat in x and z).
pub(crate) const SEAT_PROBE_TOLERANCE: f32 = 0.1;
/// The sit plugin's held cycle (`plugin_motion` Sit); the Avoid graph's clips out of it are
/// `skate_core::living_world::peds::avoid::seated` [data].
pub(crate) const SIT_CYCLE: &str = "SitIdleCyc";

/// The animation set's collision rules for one contact: DisableHeavyCollision (`brain+3277` bit 0x20) turns the
/// knock-down off and leaves the standing stumble (`sub_82E38FB8` clears its knock-down test when the bit is set).
pub(crate) fn contact_rules(
    set: &skate_core::living_world::peds::skater_contact::CollisionRules,
    heavy_collision_disabled: bool,
) -> skate_core::living_world::peds::skater_contact::CollisionRules {
    skate_core::living_world::peds::skater_contact::CollisionRules { can_knock_down: set.can_knock_down && !heavy_collision_disabled, ..*set }
}

/// The retail reaction to a skater at `skater` moving at `velocity` touching the ped at `ped`
/// facing `heading`: `None` when not touching, not moving into it, or no reaction.
pub(crate) fn skater_hit(
    ped: Vec3,
    heading: f32,
    ped_radius: f32,
    skater: [f32; 3],
    velocity: [f32; 3],
    rules: &skate_core::living_world::peds::skater_contact::CollisionRules,
    ignore_standing: bool,
) -> Option<(skate_core::living_world::peds::skater_contact::ReactionKind, skate_core::living_world::peds::skater_contact::ReactionDirection, f32)> {
    use skate_core::living_world::peds::skater_contact::{reaction_direction, skater_reaction};
    let d = [ped.x - skater[0], ped.z - skater[2]];
    let dist = (d[0] * d[0] + d[1] * d[1]).sqrt();
    if dist >= ped_radius + SKATER_CONTACT_RADIUS || (ped.y - skater[1]).abs() > 2.0 || dist <= 1e-4 {
        return None;
    }
    let n = [d[0] / dist, d[1] / dist];
    let closing = velocity[0] * n[0] + velocity[2] * n[1];
    if closing <= 0.0 {
        return None;
    }
    // IgnoreStandingCollisions (`brain+3277` bit 0x10, e.g. a seated ped): only a knock-down reacts.
    let kind = skater_reaction([closing, 0.0], [0.0, 0.0], rules, ignore_standing)?;
    Some((kind, reaction_direction(n, [heading.sin(), heading.cos()]), closing))
}

/// A ped touching the ped at `ped` (retail `sub_82E38FB8` kind 5 with a ped toucher): `toucher` stood there and moved
/// at `velocity` relative to the ped. NOT RETAIL YET: the ped body speed after the contact (retail: the Havok solve) is
/// the closing speed along the line between the two.
#[allow(clippy::too_many_arguments)]
pub(crate) fn ped_hit(
    ped: Vec3,
    heading: f32,
    toucher: [f32; 3],
    velocity: [f32; 3],
    rules: &skate_core::living_world::peds::skater_contact::CollisionRules,
    ignore_standing: bool,
    zombie: bool,
) -> Option<(skate_core::living_world::peds::skater_contact::ReactionKind, skate_core::living_world::peds::skater_contact::ReactionDirection, f32)> {
    use skate_core::living_world::peds::skater_contact::{ped_reaction, reaction_direction};
    let d = [ped.x - toucher[0], ped.z - toucher[2]];
    let dist = (d[0] * d[0] + d[1] * d[1]).sqrt();
    if dist <= 1e-4 {
        return None;
    }
    let n = [d[0] / dist, d[1] / dist];
    let closing = velocity[0] * n[0] + velocity[2] * n[1];
    if closing <= 0.0 {
        return None;
    }
    let kind = ped_reaction([closing, 0.0], [0.0, 0.0], rules, ignore_standing, zombie, false)?;
    Some((kind, reaction_direction(n, [heading.sin(), heading.cos()]), closing))
}

/// Two peds' bodies touched: a step refused by another ped (`separation_blocker`). Each ped takes the other as its
/// toucher on its next step.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PedContact {
    touched: LivingWorldId,
    toucher: LivingWorldId,
    toucher_at: [f32; 3],
    velocity: [f32; 3],
}

impl PedClips for PedData {
    fn clip(&self, name: &str) -> Option<&PedClip> {
        self.clips.get(name)
    }
}

/// Ground height near a point: the first hit of a line from `up` above to `down` below, `None`
/// when nothing is hit. The first hit from the top wins, so `up` must not reach overhead geometry.
pub(crate) fn ground(physics: Option<&crate::physics::GamePhysics>, at: Vec3, up: f32, down: f32) -> Option<f32> {
    use skate_core::math::Vector3;
    let p = physics?;
    match p.world().query_thin_line(Vector3::new(at.x, at.y + up, at.z), Vector3::new(at.x, at.y - down, at.z)) {
        Ok(Some(hit)) => Some(hit.geometry.position.y),
        _ => None,
    }
}

/// The first hit of a line from `at` straight up to `up` above it (a ceiling), `None` when nothing is hit.
pub(crate) fn ceiling(physics: Option<&crate::physics::GamePhysics>, at: Vec3, up: f32) -> Option<f32> {
    use skate_core::math::Vector3;
    let p = physics?;
    match p.world().query_thin_line(Vector3::new(at.x, at.y, at.z), Vector3::new(at.x, at.y + up, at.z)) {
        Ok(Some(hit)) => Some(hit.geometry.position.y),
        _ => None,
    }
}

/// Fixed tick: keep the loaded data current, then load the map's ped data unless the loading screen already did.
pub(super) fn load_ped_data(
    config: Option<Res<crate::config::Config>>,
    map: Option<Res<crate::map_transition::CurrentMap>>,
    mut data: ResMut<PedData>,
    audio: Option<ResMut<crate::world_audio::LivingWorldAudio>>,
    settings: Res<LivingWorldSettings>,
    nav: Res<PedNavSettings>,
    state: Res<PopulationState>,
) {
    // A mod changed the nav rules: rebuild the mesh from the loaded records.
    if data.nav.as_ref().is_some_and(|m| m.rules != nav.rules) {
        if let Some(input) = data.nav_input.clone() {
            data.nav = Some(Arc::new(NavMesh::build(&input, nav.rules.clone())));
        }
    }
    // A late load ran this upkeep on the old data and then replaced it, so a map's first tick had no arms; an early
    // load skips it once to keep that tick the same.
    let skip_arms = std::mem::take(&mut data.skip_arms_once);
    if !skip_arms && data.arms.is_empty() {
        if let Some(roads) = state.roads.as_ref() {
            data.arms = Arc::new(skate_core::living_world::peds::crosswalk::signalled_arms(roads));
        }
    }
    let (Some(config), Some(map)) = (config, map) else { return };
    load_ped_data_for_map(&config, &map, &mut data, audio, &settings, &nav, None);
}

/// Behind the loading screen (`super::behind_loading_gate`): the same load, so the first fixed tick finds it done.
#[allow(clippy::too_many_arguments)]
pub(super) fn early_load_ped_data(
    config: Res<crate::config::Config>,
    map: Res<crate::map_transition::CurrentMap>,
    mut data: ResMut<PedData>,
    audio: Option<ResMut<crate::world_audio::LivingWorldAudio>>,
    settings: Res<LivingWorldSettings>,
    nav: Res<PedNavSettings>,
    prepared: Option<ResMut<crate::map_read_ahead::PreparedMapData>>,
) {
    let ready = prepared.and_then(|mut p| p.take_peds(&(map.name.clone(), map.generation), &nav.rules));
    if load_ped_data_for_map(&config, &map, &mut data, audio, &settings, &nav, ready) {
        data.skip_arms_once = true;
    }
}

/// Load the map's ped data unless it is loaded already (`ready`: read ahead for this map); true when it loaded.
fn load_ped_data_for_map(
    config: &crate::config::Config,
    map: &crate::map_transition::CurrentMap,
    data: &mut PedData,
    audio: Option<ResMut<crate::world_audio::LivingWorldAudio>>,
    settings: &LivingWorldSettings,
    nav: &PedNavSettings,
    ready: Option<PedData>,
) -> bool {
    let key = (map.name.clone(), map.generation);
    if data.loaded_for.as_ref() == Some(&key) {
        return false;
    }
    let mut loaded = ready.unwrap_or_else(|| prepare(&config.asset_root, &map.name, &nav.rules));
    loaded.loaded_for = Some(key);
    if let Some(mut audio) = audio {
        audio.expected |= settings.enabled && settings.pedestrians.enabled && loaded.ready();
    }
    *data = loaded;
    true
}

/// A map's ped data from the files (no game state; also run on the map loader thread, `map_read_ahead`).
pub(crate) fn prepare(asset_root: &std::path::Path, map_name: &str, rules: &NavRules) -> PedData {
    // Where the first fixed tick after a map switch goes (low-end PCs: 141 ms here on the desktop, 2026-10-17).
    let t0 = std::time::Instant::now();
    // Parsed once for every reader below (1.8 MB; the eleven readers parsed it again each before, about 85 ms on the
    // desktop). A reader given none reads it itself, so a missing or broken file reports the same errors as before.
    let tables = std::fs::read(asset_root.join("private/living_world/tables.json")).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).map_err(|e| e.to_string()));
    let mut loaded = PedData::load_with(asset_root, tables.as_ref().ok());
    let t_data = t0.elapsed();
    loaded.load_nav(asset_root, map_name, rules);
    let t_nav = t0.elapsed();
    match super::ped_graph::PedGraph::load(asset_root, super::ped_graph::AI_GRAPH) {
        Ok(graph) => {
            let pending = graph.pending();
            info!(
                "PED_GRAPH loaded: {} behaviours, {} conditions; {} operation names not ported yet: {}",
                graph.behaviors.len(),
                graph.conditions.len(),
                pending.len(),
                pending.keys().cloned().collect::<Vec<_>>().join(", ")
            );
            loaded.graph = Some(Arc::new(graph));
            match super::ped_graph::PedGraph::load(asset_root, super::ped_graph::CONVERSATION_GRAPH) {
                Ok(g) => {
                    info!("PED_GRAPH conversation plugin: {} behaviours, {} conditions; not ported yet: {:?}", g.behaviors.len(), g.conditions.len(), g.pending().keys().collect::<Vec<_>>());
                    loaded.conversation_graph = Some(Arc::new(g));
                }
                Err(error) => warn!("PED_GRAPH conversation plugin not loaded (no conversations): {error}"),
            }
        }
        Err(error) => warn!("PED_GRAPH not loaded (peds keep wandering without the behaviour graph): {error}"),
    }
    let t_graphs = t0.elapsed();
    match tables.as_ref().map_err(String::clone).and_then(|b| { use super::ped_mood::*; Ok((parse_in(&b)?, reaction_sets_in(&b), chase_records_in(&b), takedown_tables_in(&b), sight_in(&b), conversation_tables_in(&b), sit_values_in(&b), starting_hand_props_in(&b))) }) {
        Ok((tables, sets, (chase, global, zombie), (takedowns, takedown_zombie), (sight, sight_scales), conversations, sit, starting_props)) => {
            info!(
                "PED_MOOD tables: {} categories, {} results, {} reaction sets, {} entity types, {} chase records (global {}, zombie {}, zombie takedowns {})",
                tables.categories.len(),
                tables.results.len(),
                tables.reactions.len(),
                sets.len(),
                chase.len(),
                global.is_some(),
                zombie.is_some(),
                takedown_zombie.is_some()
            );
            loaded.mood = Some(Arc::new(tables));
            loaded.reaction_sets = Arc::new(sets);
            loaded.chase = Arc::new(chase);
            loaded.chase_global = global.map(Arc::new);
            loaded.chase_zombie = zombie.map(Arc::new);
            loaded.takedown_zombie = takedown_zombie.map(Arc::new);
            loaded.takedowns = Arc::new(takedowns);
            loaded.sight = Arc::new(sight);
            loaded.sight_scales = Arc::new(sight_scales);
            loaded.conversations = Arc::new(conversations);
            loaded.sit = Arc::new(sit);
            loaded.starting_props = Arc::new(starting_props);
        }
        Err(error) => warn!("PED_MOOD tables not loaded (no mood reactions): {error}"),
    }
    let t_tables = t0.elapsed();
    match super::ped_plugins::PluginData::load_with(asset_root, tables.as_ref().ok()) {
        Ok(mut p) => {
            // The map's DMO hotpoint props (benches, bins, newspaper boxes) beside the placed waypoint groups.
            let seats = super::ped_plugins::map_props(asset_root, map_name);
            info!("PED_PLUGINS {}: {} hotpoint props ({} seats)", map_name, seats.len(), seats.iter().filter(|s| s.class == "waypoint_sit").count());
            p.placed.entry(map_name.to_owned()).or_default().extend(seats);
            info!(
                "PED_PLUGINS {} prop classes ({} with descriptors), {} ped types with plugin_odds, placed props {:?}",
                p.classes.len(),
                p.classes.values().filter(|c| c.descriptor.is_some()).count(),
                p.odds.len(),
                p.placed.iter().map(|(k, v)| (k.clone(), v.len())).collect::<Vec<_>>()
            );
            // Each class's plugin graph (`plugin/<name>.xml` compiled to `.stategraph`).
            for (class, c) in &p.classes {
                let Some(d) = c.descriptor.as_ref().filter(|d| !d.graph.is_empty() && class != "waypoint_conversation") else { continue };
                let relative = format!("private/stock/data/{}", d.graph.replace('\\', "/").trim_end_matches(".xml").to_string() + ".stategraph");
                match super::ped_graph::PedGraph::load(asset_root, &relative) {
                    Ok(g) => {
                        info!("PED_GRAPH plugin {class}: {} behaviours, {} conditions; not ported yet: {:?}", g.behaviors.len(), g.conditions.len(), g.pending().keys().collect::<Vec<_>>());
                        loaded.plugin_graphs.insert(class.clone(), Arc::new(g));
                    }
                    Err(error) => warn!("PED_GRAPH plugin {class} not loaded: {error}"),
                }
            }
            loaded.plugins = Arc::new(p);
        }
        Err(error) => warn!("PED_PLUGINS not loaded (no plugin props): {error}"),
    }
    let t_plugins = t0.elapsed();
    match super::ped_hand_props::HandPropData::load_with(asset_root, tables.as_ref().ok()) {
        Ok(h) => {
            info!("PED_HAND_PROPS {} records, {} with models", h.props.len(), h.props.values().filter(|p| p.glb.is_some()).count());
            loaded.hand_props = Arc::new(h);
        }
        Err(error) => warn!("PED_HAND_PROPS not loaded (no hand props): {error}"),
    }
    let ms = |a: std::time::Duration, b: std::time::Duration| (b - a).as_secs_f64() * 1000.0;
    let t_end = t0.elapsed();
    info!(
        "PED_DATA_LOAD {}: {:.1} ms (bodies {:.1}, nav {:.1}, graphs {:.1}, tables {:.1}, plugins {:.1}, hand props {:.1})",
        map_name, t_end.as_secs_f64() * 1000.0, t_data.as_secs_f64() * 1000.0, ms(t_data, t_nav), ms(t_nav, t_graphs), ms(t_graphs, t_tables), ms(t_tables, t_plugins), ms(t_plugins, t_end)
    );
    info!("LIVING_WORLD {}", loaded.status);
    loaded
}

/// Spawn and despawn ped entities from the population's records.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_ped_records(
    mut commands: Commands,
    mut spawns: MessageReader<LivingWorldSpawn>,
    mut despawns: MessageReader<LivingWorldDespawn>,
    data: Res<PedData>,
    looks: Res<PedLooks>,
    mut index: ResMut<PedIndex>,
    physics: Option<Res<crate::physics::GamePhysics>>,
    mut rejected: ResMut<PedRejected>,
    mut events: MessageWriter<PedEvent>,
    bodies: Query<&PedBody>,
    observers: Option<Res<super::LivingWorldObservers>>,
) {
    for LivingWorldDespawn(r) in despawns.read() {
        if r.id.kind != Kind::Pedestrian {
            continue;
        }
        if let Some(e) = index.0.remove(&r.id) {
            // Where the ped was and how far from the census focus, so a "vanished up close" report can be checked.
            let at = bodies.get(e).ok().map(|b| b.position);
            let focus = observers.as_deref().and_then(|o| o.observers.first()).map(|o| Vec3::from_array(o.position));
            let distance = at.zip(focus).map(|(a, f)| a.distance(f));
            info!(
                "PED_DESPAWN ped=#{} reason={:?} tick={} at={:?} focus={:?} distance={}",
                r.id.serial, r.reason, r.tick, at.map(|p| [p.x, p.y, p.z]), focus.map(|p| [p.x, p.y, p.z]),
                distance.map_or("none".to_string(), |d| format!("{d:.1}"))
            );
            crate::replay::retire(&mut commands, e);
            events.write(PedEvent::Despawned { id: r.id, reason: r.reason });
        }
    }
    for LivingWorldSpawn(s) in spawns.read() {
        if s.id.kind != Kind::Pedestrian || index.0.contains_key(&s.id) {
            continue;
        }
        let SpawnChoice::Census { record, category } = &s.choice else { continue };
        if !data.ready() {
            continue; // no body data: the population still runs (audio-less, invisible)
        }
        let look = data.catalog.choose(category, s.seed, &looks.overrides);
        let (Some(mut look), Some(set)) = (look.clone(), look.as_ref().and_then(|l| data.playable_set(&l.anim_set))) else {
            events.write(PedEvent::Rejected { id: s.id, category: category.clone() });
            rejected.0.push(s.id);
            continue;
        };
        look.anim_set = set.0.clone();
        let Some(player) = PedAnimPlayer::new(set.1, s.seed) else { continue };
        let mut at = Vec3::from_array(s.position);
        let mut nav = PedNav::default();
        // On the navmesh at the record's own height (a ground probe first could land on a wall
        // top above the record and put the ped on that layer, fix 17); maps without a navmesh
        // take the ground below.
        match data.nav.as_ref().and_then(|m| m.locate(at.to_array())) {
            Some(p) => {
                at = Vec3::from_array(p.position);
                nav.poly = Some(p.poly);
            }
            // The census ring point carries the observer height; with no navmesh floor near it, release the spawn: docs/hails-additions/26f-living-world-peds.md#ped-spawn-without-floor
            None if data.nav.is_some() => {
                events.write(PedEvent::Rejected { id: s.id, category: category.clone() });
                rejected.0.push(s.id);
                continue;
            }
            None => {
                if let Some(y) = ground(physics.as_deref(), at, 3.0, 3.0) {
                    at.y = y;
                }
            }
        }
        let ped = Pedestrian {
            id: s.id,
            census: record.clone(),
            category: category.clone(),
            entity: look.entity.clone(),
            model: look.model.clone(),
            recipe: look.recipe.clone(),
            anim_set: look.anim_set.clone(),
            seed: s.seed,
            spawn_tick: s.tick,
            tint_a: look.tint_a,
            tint_b: look.tint_b,
        };
        let body = PedBody { player, path: TestPath::new(s.seed), nav, blocked: 0.0, reaction_due: None, position: at, velocity: Vec3::ZERO, heading: s.heading, ticks: 0, feet_down: [false; 2], body_fall: 0.0, taunt: TauntClip::None, plugin_motion: PluginMotionRun::default(), head_look: Default::default() };
        let e = commands
            .spawn((
                Name::new(format!("Pedestrian {} ({})", s.id.serial, look.recipe)),
                Transform::from_translation(at).with_rotation(Quat::from_rotation_y(s.heading)),
                Visibility::Inherited,
                PedAudio { voice: look.voice, ..Default::default() },
                // Spawn fade in starts at 0 (retail `+576`); `present_ped_pose` raises it.
                super::npc_skaters::NpcFade { alpha: 0.0, ..Default::default() },
                body,
                PedMind::default(),
                ped,
            ))
            .id();
        index.0.insert(s.id, e);
        events.write(PedEvent::Spawned { id: s.id, entity: look.entity, recipe: look.recipe });
    }
}

/// Spawn records whose look did not resolve (retail: no spawn): released next.
#[derive(Resource, Default)]
pub(crate) struct PedRejected(pub Vec<LivingWorldId>);

/// Give the population slot of a rejected spawn back (a host decision; a client waits for it).
pub(crate) fn release_rejected(mut rejected: ResMut<PedRejected>, mut state: ResMut<PopulationState>, settings: Res<LivingWorldSettings>, mut out: MessageWriter<LivingWorldDespawn>) {
    for id in rejected.0.drain(..) {
        if settings.net_role == super::NetRole::Client {
            continue;
        }
        if let Some(Decision::Despawn(r)) = state.world.despawn(id, DespawnReason::External) {
            state.despawned += 1;
            out.write(LivingWorldDespawn(r));
        }
    }
}

/// Step every ped to the population tick: navigation (M3), animation, root motion kept on the
/// navmesh and apart from other peds, ground, audio, position. Peds step in id order against a
/// shared position list, so the result does not depend on query order (host-deterministic).
#[allow(clippy::too_many_arguments)]
pub(crate) fn advance_peds(
    mut state: ResMut<PopulationState>,
    data: Res<PedData>,
    nav_settings: Res<PedNavSettings>,
    traffic: Option<Res<super::vehicles::TrafficState>>,
    physics: Option<Res<crate::physics::GamePhysics>>,
    obstacles: Res<PedObstacles>,
    mut peds: Query<(&Pedestrian, &mut PedBody, &mut Transform, &mut PedAudio, Option<&PedMind>)>,
    mut events: MessageWriter<PedEvent>,
    (mut floating_logged, mut blocked_logged): (Local<std::collections::HashMap<LivingWorldId, u64>>, Local<std::collections::HashMap<LivingWorldId, u64>>),
    trace: Res<PedObstacleTrace>,
    skater_runtime: Option<Res<crate::physics::SkaterRuntime>>,
    observers: Res<super::LivingWorldObservers>,
    npcs: Query<(&super::npc_skaters::NpcSkater, &super::npc_skaters::NpcReplay)>,
    settings: Res<LivingWorldSettings>,
    mut ped_contacts: Local<Vec<PedContact>>,
    // The hand prop cues played and the unlinks stopped, per ped.
    mut hand_prop_seen: Local<(BTreeMap<LivingWorldId, u32>, BTreeMap<LivingWorldId, u32>)>,
) {
    let _span = super::timing::span(super::timing::ADVANCE_PEDS);
    let (played_cues, stopped_unlinks) = &mut *hand_prop_seen;
    let obstacles = &obstacles.0;
    let hz = state.world.clock().hz as u64;
    let tick = state.world.tick();
    let dt = tick_seconds(state.world.clock().hz);
    let mut list: Vec<_> = peds.iter_mut().collect();
    list.sort_by_key(|(p, ..)| p.id);
    let mut neighbours: Vec<Neighbour> = list.iter().map(|(p, b, ..)| Neighbour { order: id_order(p.id), position: b.position.to_array() }).collect();
    let ids: Vec<LivingWorldId> = list.iter().map(|(p, ..)| p.id).collect();
    let pending = std::mem::take(&mut *ped_contacts);
    // Skater touchers (`IActor` owners, contact kind 5 with the skater thresholds): the players, then the NPC skaters
    // in id order (the user, 2026-10-16: NPC skaters knock peds over in retail).
    let mut actors: Vec<(u64, [f32; 3], [f32; 3])> = observers.observers.iter().enumerate().map(|(i, o)| (PLAYER_TARGET_BASE + i as u64, o.position, o.velocity)).collect();
    let mut npc_actors: Vec<(u64, [f32; 3], [f32; 3])> = npcs.iter().filter_map(|(n, r)| r.last.as_ref().map(|s| (n.id.to_u64(), s.position, s.velocity))).collect();
    npc_actors.sort_by_key(|a| a.0);
    actors.extend(npc_actors);
    let clock = traffic.as_ref().and_then(|t| t.clock.as_ref());
    let road_signals = clock.map(|clock| skate_core::living_world::peds::crosswalk::RoadWalkSignals { arms: &data.arms, clock, radius: 20.0 });
    let signals: &dyn WalkSignals = match &road_signals {
        Some(s) => s,
        None => &NoSignals,
    };
    for (k, (ped, mut body, mut transform, mut audio, mind)) in list.into_iter().enumerate() {
        // OverrideAnimData (zombie mode: the `zombie` set) wins over the ped's own set while its state runs.
        let anim_set = mind.and_then(|m| m.brain.anim_override.as_deref()).filter(|n| data.anim_sets.contains_key(*n)).unwrap_or(&ped.anim_set);
        let Some(set) = data.anim_sets.get(anim_set).or_else(|| data.anim_sets.get("default")) else { continue };
        let target = tick.saturating_sub(ped.spawn_tick);
        let body = &mut *body;
        let me = id_order(ped.id);
        while body.ticks < target {
            let mut turn = 0.0;
            let step_from = body.position;
            // The taunt clip (motiongraph_taunt `PlayTaunt`: the remapped "Taunt" once, blend 0.1, then
            // `MajorIntentComplete`); a set without the clip completes at once.
            match body.taunt {
                TauntClip::Requested if body.player.state != Locomotion::Reaction => {
                    let step = skate_core::living_world::peds::skater_contact::ReactionStep { anim: "Taunt", mirror: false, blend: 0.1, cycle: false };
                    body.taunt = if body.player.react(set, vec![step], 0.0) { TauntClip::Playing } else { TauntClip::Done };
                }
                TauntClip::Playing if body.player.state != Locomotion::Reaction => body.taunt = TauntClip::Done,
                _ => {}
            }
            // A hand prop throw clip (`82E3E648`, ped vfunc +240 = the channel request `82E3A418`): a channel clip (its
            // weights pick the upper body, b99), played once over locomotion; it replaces the carry pose, which comes back
            // once the slot is free and the prop is still held.
            if let Some((clip, blend)) = body.plugin_motion.throw_clip.take() {
                if !body.player.channel_request(set, clip, blend, blend) {
                    info!("PED_HAND_PROP ped=#{} no clip {clip} tick={tick}", ped.id.serial);
                }
            }
            // The tazer's draw / cycle / holster clip (ped vfn248, blends 0.2, b248).
            if let Some(clip) = body.plugin_motion.tazer_clip.take() {
                let blend = settings.ped_brain.values.tazer_clip_blend;
                let ok = body.player.channel_request(set, clip, blend, blend);
                body.plugin_motion.tazer_played = Some((clip, ok));
                info!("PED_TAZER_CLIP ped=#{} {clip}{} tick={tick}", ped.id.serial, if ok { "" } else { " (not in the anim set)" });
            }
            // The greet / warn gesture (ChannelGreet / ChannelWarnWantTarget Begin, ped vfn+0xF8, blends 0.2; b677).
            if let Some(clip) = body.plugin_motion.gesture_clip.take() {
                let blend = settings.ped_brain.values.gesture_blend;
                let ok = body.player.channel_request(set, clip, blend, blend);
                info!("PED_GESTURE ped=#{} {clip}{} tick={tick}", ped.id.serial, if ok { "" } else { " (not in the anim set)" });
            }
            // A plugin state's clips (sit, ATM, vending machine, water fountain, newspaper box).
            let run = &mut body.plugin_motion;
            // The Avoid graph's SitJump out of the sitting cycle (`think_peds` raises `sit_avoid`).
            // Leaving the seat ends the sit sequence; the others play the clip and go on with it (ReturnToSit / SitSmall).
            if let Some(avoid) = mind.and_then(|m| m.sit_avoid).filter(|_| body.player.reaction_anim() == Some(SIT_CYCLE)) {
                let mut steps = vec![skate_core::living_world::peds::skater_contact::ReactionStep { anim: avoid.clip, mirror: avoid.mirror, blend: 0.1, cycle: false }];
                if !avoid.leaves_seat {
                    steps.extend(body.player.remaining_steps());
                }
                // The sit plugin's cycle holds until its StandUp intent (the sequence started with no ground time limit).
                let hold = if avoid.leaves_seat { 0.0 } else { f32::INFINITY };
                if body.player.interrupt(set, steps, hold) && avoid.leaves_seat {
                    run.state = TauntClip::Done;
                }
            }
            // The Avoid graph's Jump (at once) / Step (at the locomotion clip's branch window), blend 0.1, back to
            // locomotion when it ends (`think_peds` raises `stand_avoid`).
            if let Some(avoid) = mind.and_then(|m| m.stand_avoid) {
                if !body.player.reacting() && (!avoid.waits_for_branch() || body.player.in_branch_window()) {
                    let step = skate_core::living_world::peds::skater_contact::ReactionStep { anim: avoid.clip, mirror: avoid.mirror, blend: 0.1, cycle: false };
                    if body.player.react(set, vec![step], 0.0) {
                        // Avoid's StopChannel.
                        body.player.channel_stop_own();
                    } else {
                        warn!("PED_AVOID ped=#{} {} not in the anim set", ped.id.serial, avoid.clip);
                    }
                }
            }
            match (run.state, run.packet.as_deref().and_then(skate_core::living_world::peds::plugin_motion::motion_for)) {
                (TauntClip::Requested, Some(m)) if !body.player.reacting() => {
                    use skate_core::living_world::peds::plugin_motion::BLEND;
                    let steps = m.steps.iter().map(|st| skate_core::living_world::peds::skater_contact::ReactionStep { anim: st.anim, mirror: false, blend: BLEND, cycle: st.hold_until.is_some() }).collect();
                    run.state = if body.player.react(set, steps, f32::INFINITY) { TauntClip::Playing } else { TauntClip::Done };
                }
                (TauntClip::Playing, m) => {
                    if std::mem::take(&mut run.release) || run.orphaned {
                        body.player.release_hold();
                    }
                    // VendHandProp / CollectHandProp: the collect clip's branch window (`InTurnBranchWindow`).
                    let collect = m.and_then(|m| m.steps.iter().find(|st| st.hand_prop)).map(|st| st.anim);
                    if !run.hand_prop_taken && collect.is_some() && body.player.reaction_anim() == collect && body.player.in_branch_window() {
                        run.hand_prop_due = true;
                        run.hand_prop_taken = true;
                    }
                    if !body.player.reacting() {
                        run.state = TauntClip::Done;
                    }
                }
                _ => {}
            }
            // A stored reaction whose Colliding began this tick (think_peds handed it over, b326): it plays now, before
            // this tick's contacts (the ped is colliding from here on, so the contact test below is skipped).
            if let Some(r) = body.reaction_due.take() {
                let steps = skate_core::living_world::peds::skater_contact::reaction_steps(r.kind, r.direction);
                if body.player.react(set, steps, set.collision.ground_seconds) {
                    // Entering Collision stops the upper-body channel (`StopChannel`, motiongraph_collision.xml).
                    body.player.channel_stop_own();
                    if r.kind == skate_core::living_world::peds::skater_contact::ReactionKind::Knockdown {
                        body.player.begin_knockdown(r.direction);
                    }
                    info!("PED_SKATER_CONTACT_DEFERRED ped=#{} by={:?} kind={} direction={} closing={:.2} at=[{:.2}, {:.2}, {:.2}] tick={tick}", ped.id.serial, r.toucher, r.kind.name(), r.direction.name(), r.closing, body.position.x, body.position.y, body.position.z);
                    events.write(PedEvent::DeferredHit { id: ped.id, kind: r.kind, direction: r.direction, closing: r.closing, toucher: r.toucher });
                }
            }
            // A skater running into the ped (retail `sub_82E38FB8` kind 5). Retail skips the contact only while the ped is
            // already in a collision reaction (controller byte +9), not during any clip: a ped sitting on a bench or
            // using a prop (its plugin clip plays as a reaction) can be knocked over.
            // A dodge clip too: the Avoid state goes to Collision on a hit (`motiongraph_avoid.xml`, b683 2.3).
            let clip_reaction = body.taunt == TauntClip::Playing || body.plugin_motion.state == TauntClip::Playing
                || body.player.reaction_anim().is_some_and(|a| skate_core::living_world::peds::avoid::STANDING_CLIPS.contains(&a));
            let in_collision = mind.is_some_and(|m| m.brain.colliding) || (body.player.state == Locomotion::Reaction && !clip_reaction);
            // A knocked-down ped still takes contacts (retail drops them only during a stumble, `sub_8269DEE0` state 0, or
            // once blocked): they re-arm its ground timer or re-hit it while it gets up (b372).
            let knocked_down = body.player.knocked_down();
            if !in_collision || knocked_down {
                let radius = data.nav.as_deref().map_or(super::vehicle_contacts::FALLBACK_PED_RADIUS, |m| m.agent[1]);
                // Contacts from the entity the ped ignores (`ped+5680`, its takedown target) are dropped (`sub_82E38400`).
                let ignored = mind.and_then(|m| m.brain.ignore_collisions_with);
                let rules = contact_rules(&set.collision, mind.is_some_and(|m| m.brain.heavy_collision_disabled));
                let ignore_standing = mind.is_some_and(|m| m.brain.ignore_standing_collisions);
                // The local skater meets a near ped in its own solve (group 11, b185); others keep the distance test: docs/hails-additions/26f-living-world-peds.md#skater-vs-ped-contact
                let local = PLAYER_TARGET_BASE;
                let body_in_solve = skater_runtime.is_some() && settings.ped_body.enabled && !settings.ped_body.parts.is_empty()
                    && observers.observers.first().is_some_and(|o| Vec3::from_array(o.position).distance(body.position) <= settings.ped_body.range);
                let solve_hit = || {
                    let observer = observers.observers.first()?;
                    if ignored == Some(local) {
                        return None;
                    }
                    // Retail reads the ped part's linear and angular speed after the solve (|v| / |w| > 3.0).
                    let runtime = skater_runtime.as_ref()?;
                    let mine = |h: &&(u64, [f32; 3], [f32; 2])| super::ped_body::proxy_serial(h.0) == Some(ped.id.serial);
                    // Any touched part counts; the reaction part's speed decides (retail: torso, b283), else the fastest part (ours).
                    let touched_max = runtime.ped_hits.iter().filter(mine).map(|h| {
                        info!("PED_BODY_CONTACT ped=#{} part={} part_speed={:.2} part_spin={:.2} tick={tick}", ped.id.serial, h.0 & 3, h.2[0], h.2[1]);
                        h.2
                    }).fold(None, |m: Option<[f32; 2]>, s| Some(m.map_or(s, |m| [m[0].max(s[0]), m[1].max(s[1])])))?;
                    let reaction = super::ped_body::reaction_part_speed(&runtime.ped_after, ped.id.serial, &settings.ped_body);
                    let speed = reaction.unwrap_or(touched_max);
                    info!("PED_REACTION_BODY ped=#{} part={} speed={:.2} spin={:.2} touched_max_speed={:.2} touched_max_spin={:.2} tick={tick}",
                        ped.id.serial, if reaction.is_some() { settings.ped_body.reaction_part.to_string() } else { "touched_max".into() },
                        speed[0], speed[1], touched_max[0], touched_max[1]);
                    let closing = speed[0];
                    let d = [body.position.x - observer.position[0], body.position.z - observer.position[2]];
                    let len = d[0].hypot(d[1]).max(1e-4);
                    let n = [d[0] / len, d[1] / len];
                    use skate_core::living_world::peds::skater_contact::{reaction_direction, skater_reaction};
                    let Some(kind) = skater_reaction(speed, [0.0, 0.0], &rules, ignore_standing) else {
                        // The contact decided nothing (below the reaction rules, or a standing ped ignored).
                        info!("PED_BODY_CONTACT_NO_REACTION ped=#{} closing={closing:.2} ignore_standing={ignore_standing} tick={tick}", ped.id.serial);
                        return None;
                    };
                    Some(((kind, reaction_direction(n, [body.heading.sin(), body.heading.cos()]), closing), local))
                };
                let skater = actors.iter()
                    .filter(|(id, ..)| ignored != Some(*id) && !(body_in_solve && *id == local))
                    .find_map(|&(id, at, v)| skater_hit(body.position, body.heading, radius, at, v, &rules, ignore_standing).map(|h| (h, id)));
                let skater = if body_in_solve { solve_hit().or(skater) } else { skater };
                let touched = || pending.iter()
                    .filter(|c| c.touched == ped.id && ignored != Some(c.toucher.to_u64()))
                    .find_map(|c| ped_hit(body.position, body.heading, c.toucher_at, c.velocity, &rules, ignore_standing, settings.zombie).map(|h| (h, c.toucher.to_u64())));
                // A takedown under way stores contacts instead of reacting (82E38FB8, 826A4BE8, 826A4E50, b317 / b326): docs/hails-additions/26f-living-world-peds.md#takedown-contact-handling
                let attempting = mind.is_some_and(|m| m.brain.takedown_active);
                let attacking = |toucher: u64| attempting && mind.is_some_and(|m| m.brain.takedown_target == Some(toucher));
                let contact = skater.or_else(touched).filter(|(_, t)| !attacking(*t));
                if knocked_down {
                    if let Some(((kind, direction, closing), toucher)) = contact {
                        use skate_core::living_world::peds::anim::KnockdownContact;
                        let result = body.player.knockdown_contact(set, direction);
                        if result != KnockdownContact::Ignored {
                            let what = match result {
                                KnockdownContact::ReHit(posture, dir) => format!("rehit posture={} direction={}", posture.name(), dir.name()),
                                _ => "pending".to_string(),
                            };
                            info!("PED_COLLIDING ped=#{} by={toucher} {what} kind={} closing={closing:.2} tick={tick}", ped.id.serial, kind.name());
                            if matches!(result, KnockdownContact::ReHit(..)) {
                                events.write(PedEvent::Hit { id: ped.id, kind, direction, closing, toucher });
                            }
                        }
                    }
                } else if let Some(((kind, direction, closing), toucher)) = contact.filter(|_| attempting) {
                    // Stored, not played (the attempt states have no Colliding transition); the latest contact wins.
                    if mind.is_some_and(|m| m.brain.pending_reaction.is_none()) {
                        info!("PED_SKATER_CONTACT_STORED ped=#{} by={toucher} kind={} direction={} closing={closing:.2} tick={tick}", ped.id.serial, kind.name(), direction.name());
                    }
                    events.write(PedEvent::ContactStored { id: ped.id, kind, direction, closing, toucher });
                } else if let Some(((kind, direction, closing), toucher)) = contact {
                    let steps =skate_core::living_world::peds::skater_contact::reaction_steps(kind, direction);
                    // A plugin or taunt clip runs as a reaction, which `react` refuses; retail knocks such a ped over
                    // (only its collision reaction gates the contact, see above), so the hit replaces that clip (b371).
                    let started = if clip_reaction {
                        let hit = body.player.interrupt(set, steps, set.collision.ground_seconds);
                        if hit {
                            if body.plugin_motion.state == TauntClip::Playing {
                                body.plugin_motion.state = TauntClip::Done;
                            }
                            if body.taunt == TauntClip::Playing {
                                body.taunt = TauntClip::Done;
                            }
                        }
                        hit
                    } else {
                        body.player.react(set, steps, set.collision.ground_seconds)
                    };
                    if started {
                        body.player.channel_stop_own();
                        if kind == skate_core::living_world::peds::skater_contact::ReactionKind::Knockdown {
                            body.player.begin_knockdown(direction);
                        }
                        info!("PED_SKATER_CONTACT ped=#{} by={toucher} kind={} direction={} closing={closing:.2} at=[{:.2}, {:.2}, {:.2}] tick={tick}", ped.id.serial, kind.name(), direction.name(), body.position.x, body.position.y, body.position.z);
                        events.write(PedEvent::Hit { id: ped.id, kind, direction, closing, toucher });
                    }
                }
            }
            // StopAndFaceWantTarget etc. (speed suggestion 0, face point): stand and turn at the nav turn rate (ours).
            let face = mind.filter(|m| m.brain.speed_suggestion == Some(0.0)).and_then(|m| m.brain.face);
            // LockToCurrentPosition: stand where the ped is (turning only for a face point).
            // DisableNavigation (ped `+5936` bit 0x80 clear): no navigation, the ped stands the same way.
            let locked = mind.is_some_and(|m| m.brain.position_locked || m.brain.navigation_disabled);
            // TargetWaypoint's slide: within the slide distance the think step moves the ped onto the point; the body
            // stands (no wander target while the route is off).
            let sliding = mind.is_some_and(|m| {
                m.brain.approach.zip(m.brain.approach_slide).is_some_and(|((p, _), (slide, _))| (p[0] - body.position.x).hypot(p[2] - body.position.z) <= slide)
            });
            body.player.tier = skate_core::living_world::peds::anim::Tier::Default;
            // RestFromChase's Tired packet; its End posts StopTired (`8269F608`, b684).
            body.player.resting = mind.is_some_and(|m| m.brain.chase_resting);
            match data.nav.as_deref() {
                _ if body.player.state == Locomotion::Reaction => body.player.intent = skate_core::living_world::peds::anim::Intent::Idle,
                _ if (locked || sliding) && face.is_none() => {
                    body.player.intent = skate_core::living_world::peds::anim::Intent::Idle;
                    // The explicit turn direction (`+2100` bit 0x20): a ped standing on its waypoint keeps turning to
                    // the waypoint's orientation.
                    if let Some(dir) = mind.and_then(|m| m.brain.explicit_turn).filter(|d| d[0] * d[0] + d[2] * d[2] > 1e-6) {
                        let mut error = dir[0].atan2(dir[2]) - body.heading;
                        while error > std::f32::consts::PI {
                            error -= std::f32::consts::TAU;
                        }
                        while error < -std::f32::consts::PI {
                            error += std::f32::consts::TAU;
                        }
                        let step = nav_settings.wander.turn_rate * dt;
                        turn = error.clamp(-step, step);
                    }
                }
                _ if face.is_some() => {
                    let p = face.unwrap_or_default();
                    let d = [p[0] - body.position.x, p[2] - body.position.z];
                    body.player.intent = skate_core::living_world::peds::anim::Intent::Idle;
                    if d[0] * d[0] + d[1] * d[1] > 1e-6 {
                        let desired = d[0].atan2(d[1]);
                        let mut error = desired - body.heading;
                        while error > std::f32::consts::PI {
                            error -= std::f32::consts::TAU;
                        }
                        while error < -std::f32::consts::PI {
                            error += std::f32::consts::TAU;
                        }
                        let step = nav_settings.wander.turn_rate * dt;
                        turn = error.clamp(-step, step);
                    }
                }
                Some(mesh) => {
                    // The graph's Pedestrian nav modifier off (a chase): no avoiding other peds.
                    let avoid_peds = mind.is_none_or(|m| m.brain.nav_modifier(skate_core::living_world::peds::brain::nav_modifier::PEDESTRIAN));
                    let neighbours_now: &[_] = if avoid_peds { &neighbours } else { &[] };
                    body.nav.ignore_skaters = mind.is_some_and(|m| !m.brain.nav_modifier(skate_core::living_world::peds::brain::nav_modifier::SKATER_AVOIDANCE));
                    let out = body.nav.step_avoiding(mesh, &nav_settings.wander, nav_settings.crosswalk, signals, me, body.position.to_array(), body.heading, body.player.state, neighbours_now, Some(obstacles), dt);
                    // Fleeing: run (the chase run cycle) where the nav walks.
                    let fleeing = mind.is_some_and(|m| m.flee_goal.is_some() || m.chase_goal.is_some());
                    let (intent, tier) = skate_core::living_world::peds::anim::moving_intent(out.intent, fleeing, mind.and_then(|m| m.brain.speed_suggestion), &set.speed_rows, settings.ped_brain.speed_bump);
                    body.player.intent = intent;
                    body.player.tier = tier;
                    turn = out.turn;
                    if out.turn_degrees != 0 {
                        use skate_core::living_world::peds::anim::{names, Intent};
                        let walking = body.player.state == Locomotion::Walk;
                        let clip = if walking { names::walk_turn(out.turn_degrees) } else { names::stand_turn(out.turn_degrees) };
                        if set.entries.contains_key(clip) {
                            body.player.turn_degrees = out.turn_degrees;
                        } else {
                            // Ours: a set without the bucket's clip (a mod set, a test set) pivots / steers instead.
                            body.player.intent = if walking { Intent::Walk } else { Intent::Idle };
                            turn = out.steer;
                        }
                    }
                }
                None => body.player.intent = body.path.intent(dt, body.player.state),
            }
            // BlockChasee's strafe flag (b686): facing the chasee, a strafe turn (45 / 90) or a step toward the block
            // point; the clips' root motion turns and moves the ped (no steering, no face pivot).
            body.player.strafe = mind.and_then(|m| m.brain.strafe_target.map(|t| (m, t))).filter(|_| body.player.state != Locomotion::Reaction && set.entries.contains_key(skate_core::living_world::peds::anim::names::STRAFE_INTO)).map(|(m, t)| {
                let p = body.position;
                let rules = &settings.ped_brain.strafe;
                // `sub_82E2A6B0`: toward the first path node farther than `node_reach`, else the block point (b695).
                let reach2 = rules.node_reach * rules.node_reach;
                let step = m.brain.block_point.filter(|_| m.brain.speed_suggestion != Some(0.0)).map(|b| {
                    let node = body.nav.corners.iter().skip(body.nav.next).find(|c| (c[0] - p.x).powi(2) + (c[1] - p.y).powi(2) + (c[2] - p.z).powi(2) > reach2).copied().unwrap_or(b);
                    [node[0] - p.x, node[2] - p.z]
                });
                skate_core::living_world::peds::anim::strafe_input(body.heading, [t[0] - p.x, t[1] - p.y, t[2] - p.z], step, m.brain.speed_suggestion.unwrap_or(0.0), rules)
            });
            if body.player.strafe.is_some() {
                body.player.intent = skate_core::living_world::peds::anim::Intent::Idle;
                turn = 0.0;
            }
            // The carry pose (`82E3F048`): while the hand prop is held and the channel is free, its record's carry
            // animation is requested again (blend 0.2); the unlink (`82E3FAE0`) stops any channel clip once with its
            // fade (b649). Retail requests it only while the prop is linked; ours: while held.
            if mind.is_some_and(|m| unlink_due(stopped_unlinks, ped.id, m.unlink_seq)) && !body.player.channel_idle() {
                body.player.channel_stop(settings.ped_brain.values.hand_prop.unlink_fade);
            }
            if let Some(h) = mind.map(|m| &m.brain.hand_prop) {
                let carry = h.key.as_ref().and_then(|k| data.hand_props.props.get(k)).and_then(|r| r.carry_channel.as_deref());
                // The idle action's cue (`82E3F470`, only while held): a new cue plays once; the talk / read cycles are
                // asked for every frame in retail, so ours asks again when their clip ends early.
                if let Some((cue, seq)) = mind.and_then(|m| m.action_cue.map(|c| (c, m.action_seq))) {
                    let blend = settings.ped_brain.values.hand_prop.actions.clip_blend;
                    let new = played_cues.get(&ped.id) != Some(&seq);
                    if let Some(name) = cue_clip(cue, h.holding, new, body.player.channel_idle(), carry) {
                        body.player.channel_request(set, name, blend, blend);
                    }
                    played_cues.insert(ped.id, seq);
                }
                if let (true, Some(name)) = (h.holding, carry) {
                    if body.player.channel_idle() {
                        body.player.channel_request(set, name, 0.2, 0.2);
                    }
                }
            }
            let out = body.player.step(dt, set, &*data);
            body.heading += turn;
            let rotation = Quat::from_rotation_y(body.heading);
            let to = body.position + rotation * Vec3::from_array(out.root.translation);
            match data.nav.as_deref() {
                Some(mesh) => {
                    // Over linked polygons only: across tile seams, never onto an unconnected
                    // layer such as a wall top (fix 17).
                    let from = body.position.to_array();
                    let moving = (to - body.position).length_squared() > 1e-10;
                    // Inside a prop footprint: walk out toward its nearest free edge, then re-plan (ours; b349).
                    let inside = moving && mind.is_none_or(|m| !m.brain.ignore_source_collision) && obstacles.blocked(from, mesh.agent[1]);
                    let mut to = to;
                    if inside {
                        if let Some(out) = obstacles.snap_out(from, mesh.agent[1] + 0.02) {
                            let dir = (Vec3::from_array(out) - body.position).with_y(0.0);
                            let (d, step) = (dir.length(), (to - body.position).with_y(0.0).length());
                            if d > 1e-4 {
                                to = body.position + dir / d * step.min(d);
                                body.nav.repath_due = true;
                            }
                        }
                    }
                    let (next, mut poly, on_mesh) = constrain_move(mesh, body.nav.poly, from, to.to_array());
                    // Never into a prop or mod body: slide along its face or stay (fix 11). A ped in a plugin whose
                    // `DisableCollisionsWithBehaviourSource` is active ignores the plugin's own prop (its body id is the
                    // plugin prop id's low 32 bits), so it can reach a seat inside its bench.
                    let ignore = skate_core::living_world::peds::obstacles::Ignore {
                        id: mind.filter(|m| m.brain.ignore_source_collision).and_then(|m| m.prop).map(|(_, id, _)| u64::from(id as u32)),
                        // SkaterAvoidance off (ChangeNavModifierSetting index 2): the skaters are not obstacles.
                        skaters: mind.is_some_and(|m| !m.brain.nav_modifier(skate_core::living_world::peds::brain::nav_modifier::SKATER_AVOIDANCE)),
                    };
                    // The contact slide's tests read the ped's velocity and steer target (b717 / b718).
                    let contact = skate_core::living_world::peds::obstacles::Contact {
                        velocity: Some([body.velocity.x, body.velocity.z]),
                        target: body.nav.corners.get(body.nav.next.min(body.nav.corners.len().saturating_sub(1))).map(|c| [c[0], c[2]]),
                    };
                    let (next, clear) = match obstacles.resolve_step_contact(from, next, mesh.agent[1], ignore, contact) {
                        Some(n) if n == next => (n, true),
                        Some(n) => {
                            let (m, k, ok) = constrain_move(mesh, body.nav.poly, from, n);
                            poly = k;
                            (m, ok)
                        }
                        None => (next, false),
                    };
                    // A sliding step is applied however short, like retail's mover 82EA5C40 / 82EA4C20 (b311, b331, b325): docs/hails-additions/26f-living-world-peds.md#edge-slide-step
                    let blocker = separation_blocker(body.position.to_array(), next, me, &neighbours, mesh.agent[1]);
                    let ok = on_mesh && clear && blocker.is_none() && crosswalk_ok(mesh, nav_settings.crosswalk, signals, body.position.to_array(), next);
                    // Refused by another ped: the two bodies touch; both react to the other on their next step.
                    if let Some(j) = blocker.filter(|_| moving) {
                        let v = ((to - body.position) / dt).with_y(0.0).to_array();
                        ped_contacts.push(PedContact { touched: ids[j], toucher: ped.id, toucher_at: from, velocity: v });
                        ped_contacts.push(PedContact { touched: ped.id, toucher: ids[j], toucher_at: neighbours[j].position, velocity: [-v[0], 0.0, -v[2]] });
                    }
                    // Moved-prop diagnostics: a refused step into a prop, or anywhere near the spot a
                    // moved prop left (once per ped per second).
                    if !ok && moving {
                        let by = obstacles.blocker_at(to.to_array(), mesh.agent[1]);
                        let near = trace.moved.iter().find(|(_, c)| (c[0] - from[0]).hypot(c[2] - from[2]) < 3.0).map(|(id, _)| *id);
                        let now = state.world.tick();
                        if (by.is_some() || near.is_some()) && blocked_logged.get(&ped.id).is_none_or(|t| now >= t + hz) {
                            blocked_logged.insert(ped.id, now);
                            // The blocker as the log reader sees it: the prop body's model and where it is (its id is a
                            // prop body id, not a waypoint group id; 2026-10-14 a tree planter read as a newspaper box).
                            let blocker = by.filter(|id| *id < MOD_BODY_OBSTACLE_BASE).and_then(|id| {
                                let d = physics.as_deref()?.prop_dynamics()?;
                                let (c, ..) = d.body_state(id as u32)?;
                                // "<id>/DMO_DT_BenchFlat_1015_0x..." -> "DMO_DT_BenchFlat_1015".
                                let t = d.template_of(id as u32)?;
                                let model = t.split_once('/').map_or(t, |x| x.1).split("_0x").next().unwrap_or(t);
                                Some(format!("{model} at [{:.2}, {:.2}, {:.2}]", c.x, c.y, c.z))
                            });
                            let blocker = blocker.unwrap_or_else(|| match by {
                                Some(id) if id >= skate_core::living_world::peds::obstacles::SKATER_OBSTACLE_BASE => {
                                    format!("skater {}", id - skate_core::living_world::peds::obstacles::SKATER_OBSTACLE_BASE)
                                }
                                Some(id) if id >= MOD_BODY_OBSTACLE_BASE => "mod body".into(),
                                _ => "-".into(),
                            });
                            info!(
                                "PED_BLOCKED ped=#{} at=[{:.2}, {:.2}, {:.2}] to=[{:.2}, {:.2}, {:.2}] by={by:?} ({blocker}) inside={inside} near_moved_spawn={near:?} on_mesh={on_mesh} clear={clear} tick={now}",
                                ped.id.serial, from[0], from[1], from[2], to.x, to.y, to.z,
                            );
                        }
                    }
                    if ok {
                        body.position = Vec3::from_array(next);
                        body.nav.poly = poly;
                        body.blocked = 0.0;
                    } else if moving {
                        body.blocked += dt;
                        if body.blocked > nav_settings.wander.yield_patience {
                            // Refused (off the mesh, into a prop corner with no free slide, a ped, a red light): re-path to the
                            // same target (ours; retail never gives the target up, b332 / b336); no path there picks a
                            // new target. A ped standing head-on at a wall is left to the still re-path.
                            body.blocked = 0.0;
                            body.nav.repath_due = true;
                        }
                    }
                    neighbours[k].position = body.position.to_array();
                }
                None => body.position = to,
            }
            body.heading += out.root.yaw;
            body.feet_down = out.feet_down;
            body.body_fall = out.body_fall;
            body.velocity = (body.position - step_from) / dt;
            body.ticks += 1;
            if let Some(s) = out.entered {
                events.write(PedEvent::State { id: ped.id, state: s });
            }
        }
        // Render ground: a line query round the navmesh height, never fed back into navigation (fix 17): docs/hails-additions/26f-living-world-peds.md#ped-render-ground
        let mut shown = body.position;
        // Retail NavPower mover probe 82EA5F48 (b359, b358): 1.6 m casts, then up / down 4L after a miss: docs/hails-additions/26f-living-world-peds.md#ped-ground-probe-82ea5f48
        let reach = GROUND_PROBE_LENGTH;
        if let Some(y) = ground(physics.as_deref(), body.position, reach, reach).or_else(|| {
            let wide = reach * GROUND_RETRY_FACTOR;
            let top = ceiling(physics.as_deref(), body.position, wide).map_or(wide, |c| (c - body.position.y - 0.01).max(0.0));
            ground(physics.as_deref(), body.position, top, wide)
        }) {
            shown.y = y;
        }
        transform.translation = shown;
        // PED_FLOATING check every 2 s per ped (staggered): drawn over 0.3 m above the floor or over none; once per 10 s.
        if (body.ticks + ped.id.serial as u64) % 120 == 0 {
            let floor = ground(physics.as_deref(), shown, 0.2, 30.0);
            // No floor below but one just above: the ped is drawn sunk, not floating (b351); logged with a
            // negative gap.
            let floor = floor.or_else(|| ground(physics.as_deref(), shown, 2.0, 0.2));
            if floor.map_or(true, |y| (shown.y - y).abs() > 0.3)
                && floating_logged.get(&ped.id).map_or(true, |t| tick >= t + 600)
            {
                floating_logged.insert(ped.id, tick);
                let area = body.nav.poly.and_then(|k| data.nav.as_deref().and_then(|m| m.polys.get(k as usize)).map(|p| p.area));
                warn!(
                    "PED_FLOATING ped=#{} model={} drawn=[{:.2}, {:.2}, {:.2}] nav_y={:.2} floor_y={} gap={} poly={:?} area={:?} tick={tick}",
                    ped.id.serial, ped.model, shown.x, shown.y, shown.z, body.position.y,
                    floor.map_or("none".to_string(), |y| format!("{y:.2}")),
                    floor.map_or("none".to_string(), |y| format!("{:.2}", shown.y - y)),
                    body.nav.poly, area,
                );
            }
        }
        transform.rotation = Quat::from_rotation_y(body.heading);
        audio.feet_down = body.feet_down;
        audio.body_fall = body.body_fall;
        state.world.update_position(ped.id, body.position.to_array());
    }
}

/// A stable ordering key for a ped (avoidance priority: lower first).
pub(crate) fn id_order(id: LivingWorldId) -> u64 {
    id.serial as u64
}

/// The look of one ped (render side).
#[derive(Component, Default)]
pub(crate) struct PedPuppet {
    pub(crate) scene: Option<Entity>,
    pub(crate) bindings: Option<crate::animation::AnimationStatus>,
    /// Rig bones without clip data: (bone, rig parent, bind offset from the parent, native space).
    pub(crate) followers: Vec<(usize, usize, Mat4)>,
    lods: Vec<(Entity, u8)>,
    lod: u8,
    /// Bone-local basis of this GLB's joint frames relative to the rig's ([`ped_bone_basis`]).
    basis: Mat4,
}

fn render_basis() -> Mat4 {
    Mat4::from_cols(Vec4::X, -Vec4::Z, Vec4::Y, Vec4::W)
}

/// GLB path of a ped look (mod override first).
pub(crate) fn glb_path(looks: &PedLooks, recipe: &str) -> String {
    looks.glb.get(recipe).cloned().unwrap_or_else(|| format!("private/living_world/models/{recipe}.glb"))
}

/// Load the GLB and bind its joints to the ped rig by name.
#[allow(clippy::too_many_arguments)]
pub(crate) fn present_ped_looks(
    mut commands: Commands,
    server: Res<AssetServer>,
    looks: Res<PedLooks>,
    data: Res<PedData>,
    mut peds: Query<(Entity, &Pedestrian, Option<&mut PedPuppet>)>,
    skins: Query<(Entity, &bevy::mesh::skinning::SkinnedMesh)>,
    nodes: Query<(&Name, &Transform)>,
    named: Query<(Entity, &Name)>,
    parents: Query<&ChildOf>,
    instances: Query<&bevy::scene::SceneInstance>,
    spawner: Res<SceneSpawner>,
    bindposes: Res<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
) {
    for (e, ped, puppet) in &mut peds {
        let Some(mut puppet) = puppet else {
            let path = glb_path(&looks, &ped.recipe);
            let scene = commands.spawn((SceneRoot(server.load(GltfAssetLabel::Scene(0).from_asset(path))), Transform::default(), Visibility::Hidden, ChildOf(e))).id();
            commands.entity(e).insert(PedPuppet { scene: Some(scene), ..Default::default() });
            continue;
        };
        let Some(scene) = puppet.scene else { continue };
        if puppet.bindings.is_some() || !instances.get(scene).is_ok_and(|i| spawner.instance_is_ready(**i)) {
            continue;
        }
        let _event = super::timing::event(super::timing::EV_PED_BIND);
        match crate::animation::AnimationStatus::for_scene(scene, &data.rig.names, &skins, &nodes, &parents) {
            Ok(b) => {
                // GLB bind globals per rig bone from the skin's inverse bind matrices.
                let mut glb_bind: BTreeMap<usize, Mat4> = BTreeMap::new();
                for (mesh, skin) in &skins {
                    if !parents.iter_ancestors(mesh).any(|p| p == scene) {
                        continue;
                    }
                    commands.entity(mesh).insert(bevy::camera::visibility::NoFrustumCulling);
                    let Some(ibms) = bindposes.get(&skin.inverse_bindposes) else { continue };
                    for (joint, ibm) in skin.joints.iter().zip(ibms.iter()) {
                        let Ok((name, _)) = nodes.get(*joint) else { continue };
                        if let Some(i) = data.rig.names.iter().position(|n| n.eq_ignore_ascii_case(name.as_str())) {
                            glb_bind.insert(i, ibm.inverse());
                        }
                    }
                }
                let basis = ped_bone_basis(&reference_globals(&data.rig), &glb_bind);
                // Bind in the rig's bone frames (native space).
                let bind: BTreeMap<usize, Mat4> = glb_bind.iter().map(|(&i, m)| (i, *m * basis.inverse())).collect();
                puppet.basis = basis;
                puppet.followers = follower_offsets(&data.rig, &bind);
                puppet.lods = named
                    .iter()
                    .filter(|(n, name)| (name.as_str() == "LOD0" || name.as_str() == "LOD1") && parents.iter_ancestors(*n).any(|p| p == scene))
                    .map(|(n, name)| (n, if name.as_str() == "LOD0" { 0 } else { 1 }))
                    .collect();
                commands.entity(scene).insert(Visibility::Inherited);
                puppet.bindings = Some(b);
            }
            Err(err) => {
                warn!("Ped look rejected ({}): {err}", ped.recipe);
                commands.entity(scene).despawn();
                puppet.scene = None;
            }
        }
    }
}

/// A ped whose body materials carry its tint pair (look side, set once per ped).
#[derive(Component)]
pub(crate) struct PedTinted;

/// Recoloured ped materials per (source material, tint pair): peds sharing a model and a palette
/// entry share one texture copy. Weak ids: a copy (and its texture) is freed with the last ped
/// mesh using it, and its entry is dropped.
#[derive(Resource, Default)]
pub(crate) struct PedMaterials(BTreeMap<(AssetId<StandardMaterial>, [u32; 8]), AssetId<StandardMaterial>>);

/// The most recently used recoloured ped materials, held strongly so a ped spawning with a recent (source material,
/// tint pair) reuses the finished texture instead of recolouring and uploading it again on the main thread (doc 34
/// "Ped tint cache"). `SKATE_PED_TINT_CACHE=<n>` entries, default 32 (0 = off: copies live only while a ped uses them).
/// Entries whose source material is gone (a map change) are dropped.
#[derive(Resource)]
pub(crate) struct PedTintKeep {
    cap: usize,
    keep: std::collections::VecDeque<((AssetId<StandardMaterial>, [u32; 8]), UntypedHandle)>,
}

impl Default for PedTintKeep {
    fn default() -> Self {
        let cap = std::env::var("SKATE_PED_TINT_CACHE").ok().and_then(|v| v.parse().ok()).unwrap_or(32);
        Self { cap, keep: Default::default() }
    }
}

impl PedTintKeep {
    #[cfg(test)]
    pub(crate) fn with_cap(cap: usize) -> Self {
        Self { cap, keep: Default::default() }
    }
    #[cfg(test)]
    pub(crate) fn keys(&self) -> Vec<(AssetId<StandardMaterial>, [u32; 8])> {
        self.keep.iter().map(|(k, _)| *k).collect()
    }
    pub(crate) fn touch(&mut self, key: (AssetId<StandardMaterial>, [u32; 8]), handle: impl FnOnce() -> UntypedHandle) {
        if self.cap == 0 {
            return;
        }
        match self.keep.iter().position(|(k, _)| *k == key) {
            Some(i) => {
                let entry = self.keep.remove(i).expect("index from position");
                self.keep.push_back(entry);
            }
            None => {
                self.keep.push_back((key, handle()));
                while self.keep.len() > self.cap {
                    self.keep.pop_front();
                }
            }
        }
    }
}

/// Whether a ped GLB material gets the tint (shader extras, else body slot `Rostral_*`): docs/hails-additions/26f-living-world-peds.md#ped-material-tint-rule
pub(crate) fn ped_material_colorized(name: Option<&str>, extras: Option<&str>) -> bool {
    match extras {
        Some(json) => serde_json::from_str::<serde_json::Value>(json)
            .ok()
            .and_then(|v| v.get("shader").and_then(|s| s.as_str()).map(skate_core::living_world::peds::colorize::colorized_shader))
            .unwrap_or(false),
        None => name.is_some_and(|n| n.starts_with("Rostral_")),
    }
}

/// Retail-lit ped materials by (glTF source material, tint pair); held weakly like [`PedMaterials`].
#[derive(Resource, Default)]
pub(crate) struct PedLitMaterials(BTreeMap<(AssetId<StandardMaterial>, [u32; 8]), AssetId<super::ped_material::PedMaterial>>);

/// Paint each bound ped's body materials with its tint pair (retail's ped shader rule,
/// `skate_core::living_world::peds::colorize`): a copy of the diffuse texture per tint pair. Opaque parts of the stamp
/// program get the retail ped material (`ped_material`, research b614) when the retail lighting runs and the part has
/// its `m_params` rows; the rest keep their glTF material.
#[allow(clippy::too_many_arguments)]
pub(crate) fn present_ped_tints(
    mut commands: Commands,
    peds: Query<(Entity, &Pedestrian, &PedPuppet), Without<PedTinted>>,
    children: Query<&Children>,
    meshes: Query<(&MeshMaterial3d<StandardMaterial>, Option<&bevy::gltf::GltfMaterialName>, Option<&bevy::gltf::GltfMaterialExtras>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<PedMaterials>,
    mut lit_cache: ResMut<PedLitMaterials>,
    mut keep: ResMut<PedTintKeep>,
    mut lit_materials: Option<ResMut<Assets<super::ped_material::PedMaterial>>>,
    lighting: Option<Res<crate::retail_character::Lighting>>,
    looks: Res<PedLooks>,
) {
    cache.0.retain(|_, id| materials.contains(*id));
    keep.keep.retain(|(key, _)| materials.contains(key.0));
    if let Some(lit) = lit_materials.as_ref() {
        lit_cache.0.retain(|_, id| lit.contains(*id));
    }
    for (e, ped, puppet) in &peds {
        let (Some(scene), true) = (puppet.scene, puppet.bindings.is_some()) else { continue };
        let bits = |c: [f32; 4]| c.map(f32::to_bits);
        let (a, b) = (bits(ped.tint_a), bits(ped.tint_b));
        let pair = [a[0], a[1], a[2], a[3], b[0], b[1], b[2], b[3]];
        let mut pending = false;
        for ent in children.iter_descendants(scene) {
            let Ok((handle, name, extras)) = meshes.get(ent) else { continue };
            let colorized = ped_material_colorized(name.map(|n| n.0.as_str()), extras.map(|x| x.value.as_str()));
            let extras: Option<serde_json::Value> = extras.and_then(|x| serde_json::from_str(&x.value).ok());
            let Some(source) = materials.get(&handle.0).cloned() else {
                pending = true;
                continue;
            };
            let rows = (lighting.is_some() && lit_materials.is_some() && super::ped_material::runs_program(extras.as_ref(), source.alpha_mode))
                .then(|| super::ped_material::material_rows(extras.as_ref(), &looks.material_params))
                .flatten();
            if !colorized && rows.is_none() {
                continue;
            }
            let key = (handle.0.id(), if colorized { pair } else { [0; 8] });
            if rows.is_some()
                && let Some(m) = lit_cache.0.get(&key).and_then(|id| lit_materials.as_mut()?.get_strong_handle(*id))
            {
                let _hit = super::timing::event(super::timing::EV_PED_TINT_HIT);
                keep.touch(key, || m.clone().untyped());
                commands.entity(ent).remove::<MeshMaterial3d<StandardMaterial>>().insert(MeshMaterial3d(m));
                continue;
            }
            if rows.is_none()
                && let Some(m) = cache.0.get(&key).and_then(|id| materials.get_strong_handle(*id))
            {
                let _hit = super::timing::event(super::timing::EV_PED_TINT_HIT);
                keep.touch(key, || m.clone().untyped());
                commands.entity(ent).insert(MeshMaterial3d(m));
                continue;
            }
            let Some(base) = source.base_color_texture.clone() else { continue };
            let _event = super::timing::event(super::timing::EV_PED_TINT_MISS);
            let texture = if colorized {
                let Some(image) = images.get(&base) else {
                    pending = true;
                    continue;
                };
                let mut tinted = image.clone();
                match tinted.data.as_mut() {
                    Some(px) if tinted.texture_descriptor.format.block_copy_size(None) == Some(4) => {
                        skate_core::living_world::peds::colorize::colorize_rgba8(px, ped.tint_a, ped.tint_b)
                    }
                    _ => warn!("LIVING_WORLD peds: {} base texture is not RGBA8; drawn untinted", ped.recipe),
                }
                images.add(tinted)
            } else {
                base
            };
            match (rows, lighting.as_ref(), lit_materials.as_mut()) {
                (Some(rows), Some(lighting), Some(lit)) => {
                    let h = lit.add(super::ped_material::PedMaterial {
                        params: super::ped_material::params(&rows, lighting.light, source.normal_map_texture.is_some()),
                        diffuse: Some(texture),
                        normal: source.normal_map_texture.clone(),
                        alpha: AlphaMode::Opaque,
                    });
                    lit_cache.0.insert(key, h.id());
                    keep.touch(key, || h.clone().untyped());
                    commands.entity(ent).remove::<MeshMaterial3d<StandardMaterial>>().insert(MeshMaterial3d(h));
                }
                _ => {
                    let mut m = source;
                    m.base_color_texture = Some(texture);
                    let h = materials.add(m);
                    cache.0.insert(key, h.id());
                    keep.touch(key, || h.clone().untyped());
                    commands.entity(ent).insert(MeshMaterial3d(h));
                }
            }
        }
        if !pending {
            commands.entity(e).insert(PedTinted);
        }
    }
}

/// Follow the character lighting's light on every ped material (as [`super::vehicles::update_car_light`]: only a
/// material whose light differs is written).
pub(crate) fn update_ped_light(lighting: Option<Res<crate::retail_character::Lighting>>, materials: Option<ResMut<Assets<super::ped_material::PedMaterial>>>) {
    let (Some(lighting), Some(mut materials)) = (lighting, materials) else { return };
    if !lighting.is_changed() {
        return;
    }
    let stale: Vec<_> = materials.iter().filter(|(_, m)| m.params.light != lighting.light).map(|(id, _)| id).collect();
    for id in stale {
        if let Some(m) = materials.get_mut(id) {
            m.params.light = lighting.light;
        }
    }
}

/// Model-space globals of the rig's reference pose (`PEDESTRIAN_RIG_TPOSE`, root at the origin
/// like [`PedAnimPlayer::pose`](skate_core::living_world::peds::PedAnimPlayer::pose)).
pub(crate) fn reference_globals(rig: &PedRig) -> Vec<Mat4> {
    let mut locals = rig.reference.clone();
    if let Some(root) = locals.first_mut() {
        *root = skate_core::living_world::peds::anim::IDENTITY;
    }
    PedEvaluator::globals(rig, &locals).into_iter().map(crate::animation::native_matrix).collect()
}

/// Bone-local basis between a ped GLB's joint frames and the rig (identity for shipped peds): docs/hails-additions/26f-living-world-peds.md#ped-bone-basis
pub(crate) fn ped_bone_basis(reference: &[Mat4], glb_bind: &BTreeMap<usize, Mat4>) -> Mat4 {
    let angle = |a: Mat4, b: Mat4| {
        let (qa, qb) = (Quat::from_mat4(&a).normalize(), Quat::from_mat4(&b).normalize());
        2.0 * qa.dot(qb).abs().min(1.0).acos()
    };
    let score = |basis: Mat4| -> f32 { glb_bind.iter().filter_map(|(&i, b)| Some(angle(*reference.get(i)? * basis, *b))).sum() };
    [Mat4::IDENTITY, render_basis()].into_iter().map(|b| (score(b), b)).fold((f32::INFINITY, Mat4::IDENTITY), |best, x| if x.0 < best.0 { x } else { best }).1
}

/// Joint globals for [`AnimationStatus::pose_transforms`](crate::animation::AnimationStatus)
/// (which right-multiplies the skater's `render_basis`): the rig globals in the GLB's joint
/// frames, so the skin matrix is `global x basis x inverse(GLB bind)`.
pub(crate) fn ped_joint_globals(globals: &[Mat4], basis: Mat4) -> Vec<Mat4> {
    let cancel = render_basis().inverse() * basis;
    globals.iter().map(|g| *g * cancel).collect()
}

/// Bones the clips do not carry follow their rig parent with the bind offset between them.
pub(crate) fn follower_offsets(rig: &PedRig, bind: &BTreeMap<usize, Mat4>) -> Vec<(usize, usize, Mat4)> {
    (0..rig.names.len())
        .filter(|&i| !rig.animated.get(i).copied().unwrap_or(false))
        .filter_map(|i| {
            let p = usize::try_from(*rig.parents.get(i)?).ok()?;
            let (bp, bi) = (bind.get(&p)?, bind.get(&i)?);
            Some((i, p, bp.inverse() * *bi))
        })
        .collect()
}

/// The head look-at chain's rig indices (`head_look::BONES`).
pub(crate) fn head_bones(rig: &PedRig) -> Vec<Option<usize>> {
    skate_core::living_world::peds::head_look::BONES.iter().map(|n| rig.names.iter().position(|r| r.eq_ignore_ascii_case(n))).collect()
}

/// Model-space bone matrices for a ped pose (column convention, like `puppet_pose`).
pub(crate) fn ped_globals(rig: &PedRig, body: &PedBody, clips: &dyn PedClips, ahead: f32, followers: &[(usize, usize, Mat4)]) -> Option<Vec<Mat4>> {
    let mut locals = body.player.pose(rig, clips, ahead)?;
    if !body.head_look.is_centered() {
        let found;
        let bones = if body.head_look.bones.is_empty() {
            found = head_bones(rig);
            &found
        } else {
            &body.head_look.bones
        };
        let mut rotations: Vec<[f32; 4]> = locals.iter().map(|s| s.rotation).collect();
        body.head_look.apply(bones, &mut rotations);
        for (s, r) in locals.iter_mut().zip(rotations) {
            s.rotation = r;
        }
    }
    let mut g: Vec<Mat4> = PedEvaluator::globals(rig, &locals).into_iter().map(crate::animation::native_matrix).collect();
    for &(i, p, offset) in followers {
        if p < g.len() && i < g.len() {
            g[i] = g[p] * offset;
        }
    }
    Some(g)
}

/// Drawn ped opacity with the NPC draw distance multiplier (QoL, not retail); LOD distances stay retail.
pub(crate) fn ped_draw_alpha(settings: &LivingWorldSettings, pair: Option<[f32; 2]>, distance: f32, since_spawn: f32) -> f32 {
    let dd = settings.draw_distance();
    if dd.is_retail() {
        return skate_core::living_world::peds::draw_alpha(&settings.ped_fade, pair, distance, since_spawn);
    }
    let scale = |p: [f32; 2]| [dd.distance(p[0]), dd.distance(p[1])];
    let cfg = skate_core::living_world::peds::PedFadeConfig { distance: scale(settings.ped_fade.distance), ..settings.ped_fade };
    skate_core::living_world::peds::draw_alpha(&cfg, pair.map(scale), distance, since_spawn)
}

/// LOD placeholder: LOD0 within the model's first distance (45 m), LOD1 beyond its second
/// (55 m), hysteresis between [data pair `Hash_73B6874C7B46C7C6`, meaning unconfirmed].
pub(crate) fn lod_for(distance: f32, current: u8, near: [f32; 2]) -> u8 {
    if distance < near[0] {
        0
    } else if distance > near[1] {
        1
    } else {
        current
    }
}

/// Place the root and pose the skeleton between world ticks.
#[allow(clippy::too_many_arguments)]
pub(crate) fn present_ped_pose(
    data: Res<PedData>,
    state: Res<PopulationState>,
    settings: Res<LivingWorldSettings>,
    fixed: Res<Time<Fixed>>,
    cameras: Query<&GlobalTransform, (With<Camera3d>, Without<crate::video_screens::FeedCamera>)>,
    mut peds: Query<(&Pedestrian, &PedBody, &mut PedPuppet, Option<&mut super::npc_skaters::NpcFade>, Option<&super::ped_hand_props::HeldHandProp>)>,
    mut joints: Query<&mut Transform, Without<PedBody>>,
    mut vis: Query<&mut Visibility>,
    replay: Option<Res<crate::replay::Replay>>,
) {
    // During a replay the ped is drawn where it was recorded (`replay::world::play_back`), not at its frozen live body,
    // so the draw distance fade and LOD do not apply: every recorded ped draws at full detail (laptop session
    // 2026-10-20: replayed peds only showed once the camera came near their live spot).
    let replaying = replay.is_some_and(|r| r.active);
    let distance = |cam: Vec3, at: Vec3| if replaying { 0.0 } else { cam.distance(at) };
    let hz = state.world.clock().hz;
    let ahead = ((state.world.clock().overstep() + fixed.overstep_fraction() as f64 * fixed.timestep().as_secs_f64() * hz).clamp(0.0, 1.0) as f32) * tick_seconds(hz);
    let camera = cameras.iter().next().map(|c| c.translation());
    let hand_bone = data.rig.names.iter().position(|n| n.eq_ignore_ascii_case(super::ped_hand_props::HAND_PROP_BONE));
    for (ped, body, mut puppet, fade, held) in &mut peds {
        let Some(bindings) = puppet.bindings.as_ref() else { continue };
        let Some(globals) = ped_globals(&data.rig, body, &*data, ahead, &puppet.followers) else { continue };
        // The held hand prop at the hand bone's native frame and the record's offset (`82E3E4F0`, every frame).
        if let (Some(h), Some(bone)) = (held, hand_bone.and_then(|i| globals.get(i)))
            && let Ok(mut t) = joints.get_mut(h.entity)
        {
            *t = Transform::from_matrix(*bone * h.local);
        }
        for (joint, local) in bindings.pose_transforms(&ped_joint_globals(&globals, puppet.basis)) {
            if let Ok(mut t) = joints.get_mut(joint) {
                *t = local;
            }
        }
        if let (Some(cam), Some(mut fade)) = (camera, fade) {
            let pair = data.catalog.models.get(&ped.model).and_then(|m| m.lod_near);
            let since = state.world.tick().saturating_sub(ped.spawn_tick) as f32 / hz.max(1.0) as f32;
            let alpha = ped_draw_alpha(&settings, pair, distance(cam, body.position), since);
            if fade.alpha != alpha {
                fade.alpha = alpha;
            }
            if let Some(scene) = puppet.scene
                && let Ok(mut v) = vis.get_mut(scene)
            {
                let want = if alpha > 0.0 { Visibility::Inherited } else { Visibility::Hidden };
                if *v != want {
                    *v = want;
                }
            }
        }
        if let Some(cam) = camera {
            let near = data.catalog.models.get(&ped.model).and_then(|m| m.lod_near).unwrap_or([45.0, 55.0]);
            let lod = lod_for(distance(cam, body.position), puppet.lod, near);
            if lod != puppet.lod || puppet.lod == 0 {
                puppet.lod = lod;
                for &(node, level) in &puppet.lods {
                    if let Ok(mut v) = vis.get_mut(node) {
                        *v = if level == lod { Visibility::Inherited } else { Visibility::Hidden };
                    }
                }
            }
        }
    }
}

/// One-line ped summary for the debug readout.
pub(crate) fn ped_readout(peds: &[(LivingWorldId, String, Locomotion, Vec3)], player: Option<Vec3>) -> String {
    let nearest = player.and_then(|p| peds.iter().map(|x| (x, x.3.distance(p))).min_by(|a, b| a.1.total_cmp(&b.1)));
    match nearest {
        Some(((id, recipe, s, _), d)) => format!("peds {} nearest #{} {recipe} {d:.0} m {}", peds.len(), id.serial, s.name()),
        None => format!("peds {}", peds.len()),
    }
}

fn log_ped_readout(state: Res<PopulationState>, observers: Res<super::LivingWorldObservers>, peds: Query<(&Pedestrian, &PedBody, Option<&PedMind>)>, mut last: Local<u64>) {
    // Always on (was debug only, so sessions never had ped positions): every 2.5 s the count, the
    // nearest ped and the player position, so a reported spot can be found in the log.
    if !super::report_due(state.world.tick(), &mut last, 150) {
        return;
    }
    let list: Vec<_> = peds.iter().map(|(p, b, _)| (p.id, p.recipe.clone(), b.player.state, b.position)).collect();
    // Zombie mode: how many peds are on ZombieFollow (0 lines out of the mode).
    let zombies = peds.iter().filter(|(_, _, m)| m.is_some_and(|m| m.brain.motion_intent == Some(skate_core::living_world::peds::brain::motion::ZOMBIE_FOLLOW))).count();
    // What the peds are doing (session checks of "too much sitting / gathering / no chasing"): per plugin class
    // (conversation included), chasing, ZombieFollow, and the rest (free: wander, greet, react).
    let mut activity: BTreeMap<String, u32> = BTreeMap::new();
    for (_, _, m) in &peds {
        let key = match m {
            Some(m) if m.brain.chasee.is_some() => "chasing".to_owned(),
            Some(m) if m.brain.in_plugin => m.plugin_graph.clone().unwrap_or_else(|| "plugin".to_owned()),
            Some(m) if m.brain.motion_intent == Some(skate_core::living_world::peds::brain::motion::ZOMBIE_FOLLOW) => "zombie_follow".to_owned(),
            _ => "free".to_owned(),
        };
        *activity.entry(key).or_default() += 1;
    }
    let activity = activity.iter().map(|(k, n)| format!("{k}={n}")).collect::<Vec<_>>().join(" ");
    let player = observers.observers.first().map(|o| Vec3::from_array(o.position));
    let nearest = player.and_then(|at| list.iter().min_by(|a, b| a.3.distance(at).total_cmp(&b.3.distance(at))).map(|n| n.3));
    info!(
        "LIVING_WORLD {} player={:?} nearest_ped_at={:?}{} activity: {activity}",
        ped_readout(&list, player),
        player.map(|p| [p.x, p.y, p.z]),
        nearest.map(|p| [p.x, p.y, p.z]),
        if zombies > 0 { format!(" zombie_followers={zombies}") } else { String::new() },
    );
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<PedData>()
        .init_resource::<PedLooks>()
        .init_resource::<PedIndex>()
        .init_resource::<super::ped_hand_props::ReleasedHandProps>()
        .init_resource::<PedRejected>()
        .init_resource::<PedNavSettings>()
        .add_systems(PreUpdate, sync_nav_settings)
        .init_resource::<PedObstacles>()
        .init_resource::<PedObstacleTrace>()
        .add_message::<PedEvent>()
        .add_message::<crate::world_audio::PedSpeechEvent>()
        .add_message::<SkaterTakedownRequest>()
        .init_resource::<PedChaseGroups>()
        .init_resource::<PedConversations>()
        .init_resource::<PedPluginProps>()
        .add_message::<crate::hud_central::CentralMessage>()
        .init_resource::<super::ped_alerts::PedAlerts>()
        .add_systems(PostStartup, super::ped_alerts::load_alert_textures)
        .add_systems(Update, super::ped_alerts::draw_ped_alerts.after(crate::app::FrameSet::Animation))
        .add_systems(FixedUpdate, (load_ped_data, apply_ped_records, release_rejected, update_ped_obstacles, think_peds, super::ped_hand_props::sync_hand_props, apply_skater_takedowns, post_chase_messages, super::ped_alerts::update_ped_alerts, advance_peds, log_ped_readout).chain().after(super::step_population).distributive_run_if(crate::replay::world_live))
        .init_resource::<PedMaterials>()
            .init_resource::<PedLitMaterials>()
            .init_resource::<PedTintKeep>()
        .add_systems(Update, (present_ped_looks, present_ped_tints, update_ped_light, present_ped_pose).chain().after(crate::app::FrameSet::Animation).before(super::npc_skaters::present_fade::<StandardMaterial>));
}

/// `PedMind::plugin_graph` of the conversation graph (prop graphs use their class name).
const CONVERSATION_GRAPH: &str = "conversation";

/// The plugin controller for graph `key` with `states` states: kept while the graph is the same, made fresh when there
/// is none or the ped switched graphs (a controller only fits the graph it was sized for). Returns the old graph's key
/// on a switch.
pub(crate) fn plugin_controller_for(
    slot: &mut Option<skate_core::graph::controller::Controller>,
    graph: &mut Option<String>,
    key: &str,
    states: usize,
) -> Option<String> {
    let switched = graph.as_deref().is_some_and(|g| g != key);
    if switched || slot.is_none() {
        *slot = Some(skate_core::graph::controller::Controller::new(states));
    }
    let old = graph.replace(key.to_owned());
    if switched { old } else { None }
}

/// No brain move this tick: whether the route a chase or an approach set must go now (both markers are cleared).
pub(crate) fn ended_move_drops_route(chase_goal: &mut Option<[f32; 3]>, approach_route: &mut bool) -> bool {
    chase_goal.take().is_some() | std::mem::take(approach_route)
}

/// The clip a hand prop action cue asks for this frame (`82E3F470`, run only while held, b649): a new cue once; the
/// talk / read cycles again whenever the channel idles.
fn cue_clip<'a>(cue: skate_core::living_world::peds::hand_prop::ActionCue, holding: bool, new: bool, idle: bool, carry: Option<&'a str>) -> Option<&'a str> {
    use skate_core::living_world::peds::hand_prop::ActionCue;
    match cue {
        _ if !holding => None,
        ActionCue::Clip(name) if new || (idle && matches!(name, "HandPropPhoneTalk" | "HandPropReadCyc")) => Some(name),
        ActionCue::Carry if new => carry,
        _ => None,
    }
}

/// True once per new unlink count of a ped (the unlink's channel stop, `82E3FAE0`).
fn unlink_due(stopped: &mut BTreeMap<LivingWorldId, u32>, id: LivingWorldId, seq: u32) -> bool {
    seq != 0 && stopped.insert(id, seq) != Some(seq)
}

/// A ped's behavior brain and its controller on the stock ped AI graph (behavior runtime,
/// doc 26). Host-owned plain data.
#[derive(Component, Default)]
pub(crate) struct PedMind {
    pub brain: skate_core::living_world::peds::brain::PedBrain,
    /// The idle hand prop action's channel cue and its change count (`ped_hand_props::sync_hand_props` sets them,
    /// `advance_peds` plays a new cue once).
    pub action_cue: Option<skate_core::living_world::peds::hand_prop::ActionCue>,
    pub action_seq: u32,
    pub mood: skate_core::living_world::peds::mood::MoodStore,
    /// The current flee leg's goal (`skate_core::living_world::peds::flee`).
    pub flee_goal: Option<[f32; 3]>,
    /// The current intercept goal (`skate_core::living_world::peds::chase::intercept`).
    pub chase_goal: Option<[f32; 3]>,
    /// The nav route is ApproachWantTarget's one-point route: it goes when the approach ends, as retail's mover drops
    /// a move's path with the move (research b177; before, the point stayed and every give-up re-planned it, a ped stood
    /// 20 s at a wall, laptop session 2026-10-17 #111).
    pub approach_route: bool,
    /// Seconds since the intercept's last reachability check (retail every 10 mover updates, `82E2CB08`).
    pub reach_clock: f32,
    /// Where the body was at the last think (the ped's speed for the takedown window).
    last_at: Option<[f32; 3]>,
    presence_timer: f32,
    /// Time to the next zombie broadcast (`mood::BROADCAST_PERIOD`).
    zombie_timer: f32,
    rng: Option<skate_core::living_world::Rng>,
    controller: Option<skate_core::graph::controller::Controller>,
    /// The plugin graph's controller while the main graph runs the Plugin state.
    plugin_controller: Option<skate_core::graph::controller::Controller>,
    plugin_state: Option<usize>,
    /// Which plugin graph `plugin_controller` was made for (the prop class, or the conversation graph).
    plugin_graph: Option<String>,
    /// A seated avoid to play: the seat's prop moved under the owner (`82E3DBE0`, the forward jump) or a skater comes
    /// at it (SkaterAvoidance, `skate_core::living_world::peds::avoid`). Held until the clip starts or the ped is no
    /// longer sitting (a one-tick pulse lost the race against the plugin's stand-up).
    sit_avoid: Option<skate_core::living_world::peds::avoid::SeatedAvoid>,
    /// Where the jump off started (`PED_SIT_AVOID_DONE` reports how far the ped got).
    sit_avoid_from: Option<[f32; 3]>,
    /// A standing / walking dodge to play (the Avoid graph's Jump / Step), held until the clip starts or the ped can
    /// no longer dodge; where it started (`PED_AVOID_DONE`).
    stand_avoid: Option<skate_core::living_world::peds::avoid::StandingAvoid>,
    stand_avoid_from: Option<[f32; 3]>,
    /// A Jump's `skateravoid` post into the ped's own store (`sub_82E2E890`, b685): the skater and where; the mood
    /// step posts it on the next think.
    avoid_post: Option<(u64, [f32; 3])>,
    /// The speech value last sent to the audio side (`PedestrianSpeech` requests a line only when
    /// the value changes; a fresh ped holds the constructor's 68).
    spoken: Option<i32>,
    /// Population ticks run so far.
    ticks: u64,
    state: Option<usize>,
    /// The world prop the ped took (index in `PedPluginProps`, prop id, waypoint) and the props it refused (`82E40BD0`).
    prop: Option<(usize, u64, usize)>,
    refusals: skate_core::living_world::peds::plugins::RefusalMemory,
    /// A release the brain posted (DropHandProp) for `sync_hand_props`: the velocity, m/s.
    pub hand_prop_release: Option<[f32; 3]>,
    /// Hand prop unlinks so far: `advance_peds` stops the channel once per new count (`82E3FAE0`, b649).
    pub unlink_seq: u32,
    /// Log repeat limiter for PED_BRAIN / PED_MOOD lines (a refused startconversation re-raises its want every few
    /// ticks, retail by b28 / b29, and filled the log with 27k lines in 4 minutes).
    log_repeats: LogRepeats,
    /// Where the ped stopped and since when, for the still-ped report (`log_still_ped`).
    still: StillWatch,
    stall: StillWatch,
}

/// A ped that has not moved: where, since which tick, the next tick a report is due, and the
/// last full report (a repeat that matches it logs a short line instead).
#[derive(Default)]
struct StillWatch {
    anchor: Option<([f32; 3], u64)>,
    due: u64,
    last: String,
}

/// Per-ped log repeat limiter: the same line key logs at most once per `LOG_REPEAT_TICKS`; the next line of that
/// key carries how many were held back. `.1`: the key of the ped's last PED_MOOD result.
#[derive(Default)]
struct LogRepeats(BTreeMap<u64, (u64, u32)>, u64);

/// 5 s of the 60 Hz world tick.
const LOG_REPEAT_TICKS: u64 = 300;

impl LogRepeats {
    /// Whether the ped's mood result differs from its previous one (always logged).
    fn mood_changed(&mut self, key: u64) -> bool {
        std::mem::replace(&mut self.1, key) != key
    }
    /// `Some(held back count)` when the line should be logged now, `None` when it is held back.
    fn allow(&mut self, key: u64, tick: u64) -> Option<u32> {
        match self.0.get_mut(&key) {
            Some((last, held)) if tick < *last + LOG_REPEAT_TICKS => {
                *held += 1;
                None
            }
            Some((last, held)) => {
                *last = tick;
                Some(std::mem::take(held))
            }
            None => {
                self.0.insert(key, (tick, 0));
                Some(0)
            }
        }
    }
}

/// The world props offering ped plugins on the current map (host-owned; `skate_core::living_world::peds::plugins`).
#[derive(Resource, Default)]
pub(crate) struct PedPluginProps {
    pub key: Option<(String, u64)>,
    pub props: Vec<skate_core::living_world::peds::plugins::PluginProp>,
    pub settings: skate_core::living_world::peds::plugins::PluginSettings,
    /// World ticks since the last offer scan (`826C0058` every 11th tick) and the last world tick seen.
    pub scan_ticks: u32,
    pub last_tick: Option<u64>,
    pub rng: Option<skate_core::living_world::Rng>,
    /// Moved props by id and whether a seat is in water (the water rule, `PluginSettings::water_disables_seats`):
    /// re-checked only when a seat moved, so a resting prop costs no water query.
    pub in_water: BTreeMap<u64, bool>,
    /// Moved seat props whose seats moved on the last think: when one stops, `PED_PLUGIN_WATER_REST` logs its water
    /// state where it came to rest (laptop 2026-10-19: a bench pushed into a canal never logged wet=true).
    pub settling: std::collections::BTreeSet<u64>,
    /// The last `PED_PLUGIN_WATER_REST` per prop (wet, lowest seat in 10 cm steps): a prop that jitters at rest logs once.
    pub rest_logged: BTreeMap<u64, (bool, [i32; 3])>,
}

/// Members that left the world leave their conversation; an area their leaving emptied is removed, an area nobody
/// joined yet is kept (retail, b348). Returns (area, ped, members left) per leave.
pub(crate) fn leave_despawned(map: &mut BTreeMap<u64, skate_core::living_world::peds::conversation::Conversation>, alive: &dyn Fn(u64) -> bool) -> Vec<(u64, u64, usize)> {
    let mut out = Vec::new();
    let mut emptied = Vec::new();
    for c in map.values_mut() {
        let gone: Vec<u64> = c.members.iter().map(|m| m.ped).filter(|&p| !alive(p)).collect();
        for p in gone {
            let left = c.leave(p);
            out.push((c.id, p, c.members.len()));
            if matches!(left, skate_core::living_world::peds::conversation::Left::Empty) {
                emptied.push(c.id);
            }
        }
    }
    map.retain(|id, _| !emptied.contains(id));
    out
}

/// The live conversations by id (host-owned plain data, `skate_core::living_world::peds::conversation`).
#[derive(Resource, Default, Debug)]
pub(crate) struct PedConversations {
    pub map: BTreeMap<u64, skate_core::living_world::peds::conversation::Conversation>,
    pub next_id: u64,
    rng: Option<skate_core::living_world::Rng>,
    /// Refusals a founder's offer test stored on the partner (ped, area key), stored by that ped's next think.
    pending_refusals: Vec<(u64, u64)>,
}

/// The chase groups by chasee id (host-owned; retail keeps one on every chasee: the player's
/// actor and each ped).
#[derive(Resource, Default, Debug)]
pub(crate) struct PedChaseGroups(pub BTreeMap<u64, skate_core::living_world::peds::chase::ChaseGroup>);

/// A ped took player `player` down from `chaser` (retail `T.vfn20(chaser position)`, `82592390`):
/// the skater's takedown latch. Host-owned; a mod can write it too.
#[derive(Message, Clone, Copy, Debug)]
pub(crate) struct SkaterTakedownRequest {
    pub player: usize,
    pub chaser: [f32; 3],
}

/// Set the skater's takedown latch (direction from the chaser to the deck).
pub(crate) fn apply_skater_takedowns(
    mut requests: MessageReader<SkaterTakedownRequest>,
    skater: Option<ResMut<crate::physics::SkaterRuntime>>,
    physics: Option<Res<crate::physics::GamePhysics>>,
) {
    let (Some(mut skater), Some(physics)) = (skater, physics) else {
        requests.clear();
        return;
    };
    for r in requests.read() {
        // Local play: player 0 is the simulated skater.
        if r.player != 0 {
            continue;
        }
        let deck = physics.board.bodies()[skate_core::physics::board::BodyId::Deck.index()].rates.position;
        skater.takedown = Some(crate::physics::Takedown::from_positions([deck.x, deck.y, deck.z], r.chaser));
        info!("SKATER_TAKEDOWN from=[{:.1}, {:.1}, {:.1}]", r.chaser[0], r.chaser[1], r.chaser[2]);
    }
}

/// A taze or a successful takedown of the local player shows the chase line in the HUD's central message
/// (`826B4FC0`, chase manager slot +64 with (1, 1) for a taze, (1, 0) for a takedown): "ZAP!" / "Tazed!" or one of
/// the four "caught" lines, by retail's `rand() % 101` roll. The roll stream is ours (presentation only).
pub(crate) fn post_chase_messages(
    mut events: MessageReader<PedEvent>,
    mut messages: MessageWriter<crate::hud_central::CentralMessage>,
    mut random: Local<u32>,
) {
    use skate_core::living_world::peds::chase::{chase_message, chase_message_roll};
    for event in events.read() {
        let (target, tazed) = match *event {
            PedEvent::Taze { target, .. } => (target, true),
            PedEvent::Takedown { target, success: true, .. } | PedEvent::ChaseCaught { target } => (target, false),
            _ => continue,
        };
        // Local play: player 0.
        if target != PLAYER_TARGET_BASE {
            continue;
        }
        // Seeded once per run (presentation only): a fixed seed showed the same caught line on every first takedown.
        if *random == 0 {
            *random = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0x2545_f491, |d| d.subsec_nanos() | 1);
        }
        *random ^= *random << 13;
        *random ^= *random >> 17;
        *random ^= *random << 5;
        let id = chase_message(true, tazed, chase_message_roll(*random));
        info!("PED_CHASE_MESSAGE {id} tazed={tazed}");
        messages.write(crate::hud_central::CentralMessage::new(id));
    }
}

/// Obstacle / want target ids of the players (above every living-world id).
pub(crate) const PLAYER_TARGET_BASE: u64 = u64::MAX - 64;
/// Want target ids of the props (top byte 0xFE: no living-world kind, below the players): base | DMO body id.
pub(crate) const PROP_TARGET_BASE: u64 = 0xFE << 56;

/// Run every ped's AI graph once per population tick (before the bodies step), in id order.
/// Wants come from the mood system (not ported yet) or a mod; without them a ped wanders as
/// before. Logs `PED_BRAIN` on every state change.
pub(crate) fn think_peds(
    state: Res<PopulationState>,
    data: Res<PedData>,
    settings: Res<super::LivingWorldSettings>,
    observers: Res<super::LivingWorldObservers>,
    mut peds: Query<(Entity, &Pedestrian, &mut PedBody, &mut PedMind)>,
    mut speech: MessageWriter<crate::world_audio::PedSpeechEvent>,
    mut chase_groups: ResMut<PedChaseGroups>,
    mut conversations: ResMut<PedConversations>,
    mut skater_takedowns: MessageWriter<SkaterTakedownRequest>,
    mut tazers: MessageWriter<crate::world_audio::PedTazerEvent>,
    // `greeted` events for other peds' mood stores (posted when the target thinks next).
    mut greeted: Local<Vec<(u64, u64, [f32; 3])>>,
    // Conversation listeners that get the speech value 41 when they think next (`82E3DAD8`).
    mut listening: Local<Vec<u64>>,
    // One store for reading the hits and writing mood events (a reader and a writer of the
    // same message in one system conflict).
    mut events: ResMut<bevy::ecs::message::Messages<PedEvent>>,
    mut cursor: Local<Option<bevy::ecs::message::MessageCursor<PedEvent>>>,
    mut touchers: Local<BTreeMap<u64, u64>>,
    (mut traffic_events, cars, mut plugin_props, physics, linked_props, skater, mut scatter, traffic, mut recognizers): (MessageReader<super::vehicles::TrafficEvent>, Query<(&super::vehicles::TrafficCar, &super::vehicles::CarMotion)>, ResMut<PedPluginProps>, Option<Res<crate::physics::GamePhysics>>, Query<&super::ped_hand_props::ReleasedHandProp>, Option<Res<crate::physics::SkaterRuntime>>, Local<(f32, Option<u64>, bool)>, Option<Res<super::vehicles::TrafficState>>, Local<Vec<u64>>),
) {
    let _span = super::timing::span(super::timing::THINK_PEDS);
    // The world's recognizer list (b88 §5): peds that left go (End `826A2E40` erases them).
    if !recognizers.is_empty() {
        let alive: std::collections::BTreeSet<u64> = peds.iter().map(|(_, p, _, _)| p.id.to_u64()).collect();
        recognizers.retain(|r| alive.contains(r));
    }
    // A map change (b216; not the first map, whose key goes from none to some): the conversations, chase groups,
    // pending refusals and greetings belong to the old world's peds, and the RNG streams restart from the new
    // world's seed as on a fresh start there. The prop list itself reloads below.
    if plugin_props.key.is_some() && plugin_props.key != data.loaded_for {
        conversations.map.clear();
        conversations.pending_refusals.clear();
        conversations.rng = None;
        chase_groups.0.clear();
        plugin_props.rng = None;
        plugin_props.last_tick = None;
        greeted.clear();
        listening.clear();
        touchers.clear();
        info!("PED_WORLD_RESET map changed: conversations, chase groups and ped RNG cleared");
    }
    // Streamed-out props (DMO streaming): their plugins are not offered and a ped using one lets go. A world prop's
    // plugin id carries its DMO body id in the low 32 bits (`ped_plugins::map_props`).
    let dormant = physics.as_ref().and_then(|p| p.prop_dynamics()).map(|d| d.dormant_ids()).unwrap_or_default();
    let streamed_out = |plugin_id: u64| dormant.contains(&(plugin_id as u32));
    // Peds whose seat's prop moved this tick (grabbed, pushed, knocked): `82E3DBE0` fires the seated owner's avoid.
    let mut seat_pushed: Vec<u64> = Vec::new();
    // A seat's sub-threshold step, per seated occupant: retail sums it (brain `+3168`) into the ped's root (`82E36D70`).
    let mut seat_carry: Vec<(u64, [f32; 3])> = Vec::new();
    // Horn kind 2 at a ped (`sub_82E3C3D0`: the honker id into brain `+3232`; the last car wins).
    let honked: BTreeMap<u64, u64> = traffic_events.read().filter_map(|e| match e { super::vehicles::TrafficEvent::HonkedAt { id, ped } => Some((*ped, id.to_u64())), _ => None }).collect();
    let Some(graph) = data.graph.as_deref() else { return };
    // The honking cars' pose for RunFromHonker (`826A1358` looks the car up every frame): position and forward.
    let car_pose: BTreeMap<u64, ([f32; 3], [f32; 3])> = cars.iter().map(|(c, m)| (c.id.to_u64(), (m.curr.translation.to_array(), (m.curr.rotation * Vec3::Z).to_array()))).collect();
    // Skater contact collisions (`PedEvent::Hit`) broadcast; non-hit peds record `nearbycollision`: docs/hails-additions/26f-living-world-peds.md#ped-collision-broadcast
    let cursor = cursor.get_or_insert_with(|| events.get_cursor());
    // A reacting contact sets the hit ped's collision component (`sub_82E38FB8`: needs-to-begin); the mood broadcast
    // waits for its Colliding state to begin (`PedEvent::Colliding`), so a ped whose state does not allow Colliding
    // (a non-interruptible chase, a plugin with the collision override) posts nothing yet.
    let (mut hits, mut collisions) = (Vec::<u64>::new(), Vec::<(u64, Option<u64>)>::new());
    let mut stored = Vec::<(u64, skate_core::living_world::peds::skater_contact::StoredReaction)>::new();
    for e in cursor.read(&events) {
        match e {
            PedEvent::Hit { id, toucher, .. } => {
                hits.push(id.to_u64());
                touchers.insert(id.to_u64(), *toucher);
            }
            PedEvent::ContactStored { id, kind, direction, closing, toucher } => {
                stored.push((id.to_u64(), skate_core::living_world::peds::skater_contact::StoredReaction { kind: *kind, direction: *direction, closing: *closing, toucher: Some(*toucher) }));
                touchers.insert(id.to_u64(), *toucher);
            }
            PedEvent::Colliding { id } => collisions.push((id.to_u64(), touchers.remove(&id.to_u64()))),
            _ => {}
        }
    }
    if settings.net_role == super::NetRole::Client || !settings.ped_brain.enabled {
        return;
    }
    let program = &graph.graph.runtime.program;
    let dt = tick_seconds(state.world.clock().hz);
    let tick = state.world.tick();
    // The scatter alarm (`sub_826C1348` once per world step, copied to every ped by `sub_826BDB50`; b654): raised while
    // the local player's gesture runs on D-pad Down (MotionGraph +5989, `sub_8258FB30`).
    {
        let raised = skater.as_ref().and_then(|s| s.animation.motion.gesture_publication.as_ref()).is_some_and(|g| g.down)
            || settings.ped_brain.scatter_hold;
        let steps = scatter.1.map_or(1, |last| tick.saturating_sub(last));
        if steps > 0 {
            let length = settings.ped_brain.values.scatter_seconds;
            let was = scatter.2;
            scatter.2 = skate_core::living_world::peds::scatter::step(&mut scatter.0, raised && length > 0.0, settings.zombie, dt * steps as f32, length);
            scatter.1 = Some(tick);
            if scatter.2 != was {
                info!("PED_SCATTER {} timer {:.1} s tick={tick}", if scatter.2 { "on" } else { "off" }, scatter.0);
            }
        }
    }
    let scattering = scatter.2;
    let players: Vec<[f32; 3]> = observers.observers.iter().map(|o| o.position).collect();
    let mut list: Vec<_> = peds.iter_mut().collect();
    list.sort_by_key(|(_, p, ..)| p.id);
    // The streamed-in props as want targets (the object flee), and the moving ones the flee rule tests.
    let props_now = physics.as_deref().and_then(|p| p.prop_dynamics()).map(prop_obstacle_inputs).unwrap_or_default();
    let object_flee = settings.ped_brain.object_flee;
    let moving_props: Vec<(u64, skate_core::living_world::peds::object_flee::MovingObject)> = if object_flee.enabled {
        props_now.iter().map(|o| (PROP_TARGET_BASE | o.id, moving_object(o))).collect()
    } else {
        Vec::new()
    };
    let vehicle_flee = settings.ped_brain.vehicle_flee;
    // Each car with its gate B flag (held / skitched this tick, `Car::held`; b821).
    let held_car = |serial: u32| traffic.as_deref().and_then(|t| t.cars.binary_search_by_key(&serial, |c| c.key).ok().map(|i| t.cars[i].held)).unwrap_or(false);
    let moving_cars: Vec<(u64, ([f32; 3], [f32; 3], bool))> = if vehicle_flee.enabled {
        cars.iter().map(|(c, m)| (c.id.to_u64(), (m.curr.translation.to_array(), m.velocity.to_array(), held_car(c.id.serial)))).collect()
    } else {
        Vec::new()
    };
    // Entity type and position of every id this tick (players are the `skater` type, props `prop`, cars their entity).
    let entities: BTreeMap<u64, (String, [f32; 3])> = list
        .iter()
        .map(|(_, p, b, _)| (p.id.to_u64(), (p.entity.clone(), b.position.to_array())))
        .chain(players.iter().enumerate().map(|(i, p)| (PLAYER_TARGET_BASE + i as u64, ("skater".to_string(), *p))))
        .chain(props_now.iter().map(|o| (PROP_TARGET_BASE | o.id, ("prop".to_string(), o.center))))
        .chain(cars.iter().map(|(c, m)| (c.id.to_u64(), (c.entity.clone(), m.curr.translation.to_array()))))
        .collect();
    // A hit ped that is gone no longer waits for its Colliding state.
    touchers.retain(|id, _| entities.contains_key(id));
    // Velocity of every id this tick (the attack throw's look-ahead).
    let velocities: BTreeMap<u64, [f32; 3]> = list
        .iter()
        .map(|(_, p, b, _)| (p.id.to_u64(), b.velocity.to_array()))
        .chain(observers.observers.iter().enumerate().map(|(i, o)| (PLAYER_TARGET_BASE + i as u64, o.velocity)))
        .chain(props_now.iter().map(|o| (PROP_TARGET_BASE | o.id, o.velocity)))
        .chain(cars.iter().map(|(c, m)| (c.id.to_u64(), m.velocity.to_array())))
        .collect();
    let velocity_of = |id: u64| velocities.get(&id).copied();
    // World-prop plugins: the map's props, then the offer scan every 11th world tick (`826C0058` -> `826BFE18`).
    {
        let pp = &mut *plugin_props;
        let key = data.loaded_for.clone();
        if pp.key != key {
            pp.props = key.as_ref().and_then(|k| data.plugins.placed.get(&k.0)).cloned().unwrap_or_default();
            pp.key = key;
            pp.scan_ticks = 0;
            pp.in_water.clear();
            info!("PED_PLUGINS props on this map: {}", pp.props.len());
        }
        pp.settings = settings.ped_brain.plugins;
        // Hotpoint seats ride on their object (a prop's plugin id carries its DMO body id in the low 32 bits): every
        // think they follow the body from its authored pose, so a moved bench takes its seats along.
        if let (Some(authored), Some(dynamics)) = (pp.key.as_ref().and_then(|k| data.plugins.placed.get(&k.0)), physics.as_ref().and_then(|p| p.prop_dynamics())) {
            let pose = |(o, b): (skate_core::math::Vector3, skate_core::math::Basis3)| ([o.x, o.y, o.z], b.columns);
            for (prop, original) in pp.props.iter_mut().zip(authored) {
                let body = prop.id as u32;
                let (Some(spawn), Some(now)) = (dynamics.spawn_pose(body), dynamics.pose(body)) else { continue };
                let (spawn, now) = (pose(spawn), pose(now));
                let mut seats_moved = false;
                for (w, a) in prop.waypoints.iter_mut().zip(&original.waypoints) {
                    let (position, facing) = super::ped_plugins::follow_prop(a.position, a.facing, spawn, now);
                    // A seat whose prop moves: the seated ped jumps off and re-sits (b102, 82E3DBE0, 826AB9C0): docs/hails-additions/26f-living-world-peds.md#moving-seat
                    let step = (0..3).map(|k| (position[k] - w.position[k]).powi(2)).sum::<f32>().sqrt();
                    seats_moved |= step > 1e-3;
                    let speed = step / dt.max(1e-4);
                    let per_step = speed * SEAT_PUSH_STEP_SECONDS;
                    if let Some(occupant) = w.occupant.filter(|_| per_step * per_step > SEAT_PUSH_STEP_SQ) {
                        info!("PED_PLUGIN_SEAT_MOVED prop={:016X} {} occupant={occupant} speed={speed:.2} from=[{:.2}, {:.2}, {:.2}] to=[{:.2}, {:.2}, {:.2}] tick={tick}", prop.id, prop.class, w.position[0], w.position[1], w.position[2], position[0], position[1], position[2]);
                        seat_pushed.push(occupant);
                    } else if let Some(occupant) = w.occupant.filter(|_| step > 0.0) {
                        // b219: steps at or under the gate are accumulated and carry the seated ped with the seat (a
                        // slowly pushed bench). Without it the seat crept away from the locked body past OnWaypoint's
                        // 0.05 m and the graph re-sat the ped every few ticks (laptop session 2026-10-20, #20, b396).
                        seat_carry.push((occupant, [position[0] - w.position[0], position[1] - w.position[1], position[2] - w.position[2]]));
                    }
                    (w.position, w.facing) = (position, facing);
                }
                // DMO plugin check 82C56DB0 (b136 / b137); when retail runs it is open (NOT RETAIL YET): docs/hails-additions/26f-living-world-peds.md#dmo-plugin-check
                use skate_core::living_world::peds::plugins::{check_prop, seats_in_water, PropCheck, ROTATION_EPSILON};
                let moved = (0..3).any(|k| (now.0[k] - spawn.0[k]).abs() > 1e-3)
                    || (0..3).any(|c| (0..3).any(|r| (now.1[c][r] - spawn.1[c][r]).abs() > ROTATION_EPSILON));
                // The per-seat pavement probe (`82C539A8`) is not applied yet: our probe fails 10-23% of untouched
                // authored seats (test `living_world_authored_plugin_seats_pass_the_pavement_probe`), so it is not
                // retail's yet; a moved, upright prop keeps its seats as before.
                let tipped = moved && dynamics.plugin_check_limits(body)
                    .is_some_and(|(tolerance, max_distance)| check_prop(spawn, now, tolerance, max_distance) == PropCheck::DisableAll);
                // NOT RETAIL (ours, user 2026-10-18: a seat stops being offered once its bench is in the water): a moved
                // prop with a seat in water is off too; asked again only when a seat moved (or first seen moved).
                let wet = if !moved {
                    pp.in_water.remove(&prop.id);
                    pp.rest_logged.remove(&prop.id);
                    false
                } else if let (false, Some(&cached)) = (seats_moved, pp.in_water.get(&prop.id)) {
                    cached
                } else {
                    let world = physics.as_ref().map(|p| p.world());
                    let at = |p: [f32; 3]| skate_core::math::Vector3::new(p[0], p[1], p[2]);
                    let wet = world.is_some_and(|w| seats_in_water(prop, &pp.settings, |p, above, depth| w.water_surface_at(at(p), above, depth)));
                    // PED_PLUGIN_WATER (laptop 2026-10-19: benches pushed into the water left no line): each change of
                    // a moved prop's water state, with its lowest seat and any water surface up to 2 m above it, so a
                    // bench standing in shallow water with dry seats shows too.
                    if pp.in_water.insert(prop.id, wet) != Some(wet) && pp.settings.water_disables_seats {
                        let low = prop.waypoints.iter().map(|w| w.position).min_by(|a, b| a[1].total_cmp(&b[1]));
                        let surface = low.zip(world).and_then(|(p, w)| w.water_surface_at(at(p), 2.0, pp.settings.water_max_depth));
                        let occupants = prop.waypoints.iter().filter(|w| w.occupant.is_some()).count();
                        info!("PED_PLUGIN_WATER {} {:016X} wet={wet} lowest_seat={:?} surface={surface:?} occupants={occupants} tick={tick}",
                            prop.class, prop.id, low.map(|p| [(p[0] * 100.0).round() / 100.0, (p[1] * 100.0).round() / 100.0, (p[2] * 100.0).round() / 100.0]));
                    }
                    wet
                };
                // PED_PLUGIN_WATER_REST: once per stop, the seats' water state at rest and the nearest water surface
                // within 5 m above or below the lowest seat (so a canal whose surface the rule never meets shows).
                if moved && seats_moved {
                    pp.settling.insert(prop.id);
                } else if pp.settling.remove(&prop.id) && pp.settings.water_disables_seats {
                    let world = physics.as_ref().map(|p| p.world());
                    let low = prop.waypoints.iter().map(|w| w.position).min_by(|a, b| a[1].total_cmp(&b[1]));
                    let key = (wet, low.map_or([0; 3], |p| p.map(|v| (v * 10.0).round() as i32)));
                    if pp.rest_logged.insert(prop.id, key) != Some(key) {
                        let near = low.zip(world).and_then(|(p, w)| w.water_surface_at(skate_core::math::Vector3::new(p[0], p[1], p[2]), 5.0, 5.0));
                        info!("PED_PLUGIN_WATER_REST {} {:016X} wet={wet} lowest_seat={:?} surface_within_5m={near:?} rule_above={} rule_depth={} tick={tick}",
                            prop.class, prop.id, low.map(|p| [(p[0] * 100.0).round() / 100.0, (p[1] * 100.0).round() / 100.0, (p[2] * 100.0).round() / 100.0]),
                            pp.settings.water_above, pp.settings.water_max_depth);
                    }
                }
                let off = tipped || wet;
                if off != prop.disabled {
                    prop.disabled = off;
                    let why = if off && wet { "disabled (a seat in water), occupants evicted" } else if off { "disabled (moved / tipped), occupants evicted" } else { "enabled again" };
                    info!("PED_PLUGIN_PROP {} {:016X} {why} tick={tick}", prop.class, prop.id);
                }
            }
        }
        let seed = state.world.seed();
        let rng = pp.rng.get_or_insert_with(|| skate_core::living_world::Rng::new(skate_core::living_world::rng::derive(seed, &[0x504c_5547])));
        let elapsed = pp.last_tick.map_or(1, |t| tick.saturating_sub(t)) as u32;
        pp.last_tick = Some(tick);
        pp.scan_ticks += elapsed;
        // Waypoints held by peds that are gone or no longer on the prop are released.
        let holding: Vec<(u64, u64)> = list.iter().filter_map(|(_, p, _, m)| m.prop.map(|(_, id, _)| (p.id.to_u64(), id))).collect();
        for prop in &mut pp.props {
            for w in &mut prop.waypoints {
                if let Some(o) = w.occupant.filter(|o| !holding.contains(&(*o, prop.id))) {
                    info!("PED_PLUGIN_PROP {} {:016X} released: ped {o} gone tick={tick}", prop.class, prop.id);
                    w.occupant = None;
                }
            }
        }
        while pp.scan_ticks >= pp.settings.scan_period.max(1) {
            pp.scan_ticks -= pp.settings.scan_period.max(1);
            let mut attaches = Vec::new();
            {
                // The descriptors' transfer expressions (`UseWorldProp`: e.g. usetrashbin needs a disposable hand prop).
                let views: std::collections::HashMap<u64, skate_core::living_world::peds::plugins::TransferView> =
                    list.iter().map(|(_, p, _, m)| (p.id.to_u64(), skate_core::living_world::peds::plugins::TransferView::of(&m.brain))).collect();
                let offer_settings = pp.settings;
                let mut qualifies = |prop: &skate_core::living_world::peds::plugins::PluginProp, ped: &skate_core::living_world::peds::plugins::OfferPed| {
                    let view = views.get(&ped.id).copied().unwrap_or_default();
                    let transfer = data.plugins.classes.get(&prop.class).and_then(|c| c.descriptor.as_ref()).and_then(|d| d.transfer.as_ref());
                    transfer.is_none_or(|t| t.evaluate(&mut |name, _| view.condition(name, prop, ped.position, &offer_settings)))
                };
                let mut offer_peds: Vec<skate_core::living_world::peds::plugins::OfferPed> = list
                    .iter_mut()
                    .map(|(_, p, b, m)| {
                        let m = &mut **m;
                        skate_core::living_world::peds::plugins::OfferPed {
                            id: p.id.to_u64(),
                            entity: &p.entity,
                            position: b.position.to_array(),
                            has_plugin: m.brain.has_plugin || m.prop.is_some(),
                            odds: data.plugins.odds.get(&p.entity).map(Vec::as_slice).unwrap_or(&[]),
                            memory: &mut m.refusals,
                        }
                    })
                    .collect();
                for (index, prop) in pp.props.iter_mut().enumerate() {
                    if !data.plugin_graphs.contains_key(&prop.class) || streamed_out(prop.id) || prop.disabled {
                        continue;
                    }
                    let class = data.plugins.classes.get(&prop.class);
                    // [inference] the offer radius: the class field E2101F2B17A0E6B5 (10 sit / 5 bin / 15 ATM).
                    let radius = class.and_then(|c| c.numbers.get("Hash_E2101F2B17A0E6B5").copied()).unwrap_or(10.0);
                    let max = class.and_then(|c| c.descriptor.as_ref()).map_or(1, |d| d.max_participants);
                    for a in skate_core::living_world::peds::plugins::offer(prop, radius, max, &mut offer_peds, &mut qualifies, &pp.settings, rng) {
                        attaches.push((index, a));
                    }
                }
                // Spawned conversation areas are offers too (b212; after the placed props [inference]): passers-by join
                // while they gather, through the descriptor's transfer expression.
                use skate_core::living_world::peds::plugins::{conversation_qualifies, offer_conversation, CONVERSATION_CLASS};
                let class = data.plugins.classes.get(CONVERSATION_CLASS);
                let radius = class.and_then(|c| c.numbers.get("Hash_E2101F2B17A0E6B5").copied()).unwrap_or(20.0);
                let transfer = class.and_then(|c| c.descriptor.as_ref()).and_then(|d| d.transfer.as_ref());
                let rows = &data.conversations.rows;
                let mut qualifies = |c: &skate_core::living_world::peds::conversation::Conversation, ped: &skate_core::living_world::peds::plugins::OfferPed| {
                    conversation_qualifies(transfer, c, ped.entity, views.get(&ped.id).copied().unwrap_or_default(), rows)
                };
                for c in conversations.map.values_mut() {
                    for ped in offer_peds.iter_mut() {
                        if offer_conversation(c, radius, ped, &mut qualifies, rows, &offer_settings, rng) {
                            info!("PED_CONVERSATION id={} join ped={} members={} state={} tick={tick}", c.id, ped.id, c.members.len(), c.state);
                        }
                    }
                }
            }
            for (index, a) in attaches {
                if let Some((_, p, _, m)) = list.iter_mut().find(|(_, p, ..)| p.id.to_u64() == a.ped) {
                    m.prop = Some((index, a.prop, a.waypoint));
                    info!("PED_PLUGIN_PROP ped=#{} takes {} {:016X} waypoint {} tick={tick}", p.id.serial, pp.props[index].class, a.prop, a.waypoint);
                }
            }
        }
    }
    let entity = |id: u64| entities.get(&id).cloned();
    let target = |id: u64| entities.get(&id).map(|e| e.1);
    let player_list: Vec<(u64, [f32; 3])> = players.iter().enumerate().map(|(i, p)| (PLAYER_TARGET_BASE + i as u64, *p)).collect();
    let world_seed = state.world.seed();
    // Live peds for the presence scan (`sub_82E3CA20`: every spawned, not torn-down ped counts).
    let ped_list: Vec<(u64, [f32; 3])> = list.iter().map(|(_, p, b, _)| (p.id.to_u64(), b.position.to_array())).collect();
    let greeted_now = std::mem::take(&mut *greeted);
    // Who is busy this tick (check 8 of the mood results; players: not busy, unverified), and what a conversation
    // founder's offer test reads of the partner (in a plugin, its transfer view).
    let busy_now: BTreeMap<u64, (bool, bool, skate_core::living_world::peds::plugins::TransferView)> = list
        .iter()
        .map(|(_, p, _, m)| (p.id.to_u64(), (m.brain.busy(), m.brain.has_plugin || m.prop.is_some(), skate_core::living_world::peds::plugins::TransferView::of(&m.brain))))
        .collect();
    let busy = |id: u64| busy_now.get(&id).is_some_and(|b| b.0);
    // Chasers that left the world leave their groups (the ped's destruction; retail removes it
    // with the chaser component).
    for g in chase_groups.0.values_mut() {
        g.chasers.retain(|c| entities.contains_key(c));
    }
    chase_groups.0.retain(|chasee, g| !g.chasers.is_empty() && entities.contains_key(chasee));
    // Conversations: members leave, an emptied area goes (82E1D470 -> 82E1BD08); an unjoined area stays (b348).
    for (id, p, members) in leave_despawned(&mut conversations.map, &|p| entities.contains_key(&p)) {
        info!("PED_CONVERSATION id={id} leave ped={p} result=despawned members={members} tick={tick}");
    }
    listening.retain(|p| entities.contains_key(p));
    let conversations = &mut *conversations;
    let conv_rng_seed = world_seed;
    let conv_rng = conversations.rng.get_or_insert_with(|| skate_core::living_world::Rng::new(skate_core::living_world::rng::derive(conv_rng_seed, &[0x434f_4e56])));
    let mut conv_rand = || conv_rng.unit();
    let conv_params = skate_core::living_world::peds::conversation::ConversationParams {
        turn_seconds: settings.ped_brain.values.conversation_turn_seconds,
        gather_seconds: settings.ped_brain.values.conversation_gather_seconds,
        // The descriptor's maxNumberOfParticipants (3).
        max_members: data
            .plugins
            .classes
            .get(skate_core::living_world::peds::plugins::CONVERSATION_CLASS)
            .and_then(|c| c.descriptor.as_ref())
            .map_or(skate_core::living_world::peds::conversation::MAX_MEMBERS, |d| d.max_participants as usize),
        ..Default::default()
    };
    // The gather timer (`82E1EB10`): a conversation starts with whoever is there when it runs out.
    for c in conversations.map.values_mut() {
        if c.tick(dt, &data.conversations.rows, &mut conv_rand) {
            info!("PED_CONVERSATION id={} gather_timeout state={} members={} tick={tick}", c.id, c.state, c.members.len());
        }
    }
    // How many chasers a chasee takes: every chase group (skater and ped alike) reads `global`, or `zombie` under the
    // zombie cheat (group init `82D967B0`, b105).
    let group_record = if settings.zombie { data.chase_zombie.as_deref() } else { data.chase_global.as_deref() };
    let max_chasers = |_chasee: u64| skate_core::living_world::peds::chase::ChaseRecord::max_chasers(group_record);
    // Still-ped reports queue: at most this many full reports per tick, the rest wait their turn.
    let mut still_budget = STILL_REPORTS_PER_TICK;
    for (entity_id, ped, mut body, mut mind) in list {
        let mind = &mut *mind;
        let body = &mut *body;
        let me = ped.id.to_u64();
        let at = body.position.to_array();
        // Perception (`82E418E8`): memory, age and suppression run, the vision test from the eye
        // (ours: 1.6 m above the feet; retail's eye point is set elsewhere) along the heading; the
        // line of sight is our navmesh line (retail: a physics ray).
        {
            let steps = tick.saturating_sub(ped.spawn_tick).saturating_sub(mind.ticks) as f32;
            if steps > 0.0 && !mind.brain.perceptions.entries.is_empty() {
                use skate_core::living_world::peds::perception;
                let sight = data.sight.get(&ped.entity).copied();
                let eye = [at[0], at[1] + 1.6, at[2]];
                let forward = [body.heading.sin(), 0.0, body.heading.cos()];
                let here = data.nav.as_deref().and_then(|m| m.locate(at).map(|h| (m, h)));
                let los = |_: [f32; 3], t: [f32; 3]| here.is_none_or(|(m, h)| m.clear_line(h, t));
                let look = |t: u64| {
                    let p = target(t)?;
                    let v = observers.observers.get(t.wrapping_sub(PLAYER_TARGET_BASE) as usize).map_or([0.0; 3], |o| o.velocity);
                    Some((p, v, sight.is_some_and(|s| perception::sees(at, eye, forward, p, &s, &los))))
                };
                mind.brain.perceptions.tick(dt * steps, &look);
            }
        }
        // Mood: tick the records, post presence and collisions, produce wants.
        if let (Some(tables), true) = (data.mood.as_deref(), settings.ped_brain.mood) {
            use skate_core::living_world::peds::mood::{self, MoodContext, MoodEvent};
            let set = data.reaction_sets.get(&ped.entity).cloned().unwrap_or_else(|| "default".into());
            let magnitude = |c: &str| tables.categories.get(c).map_or(0.0, |c| c.magnitude);
            let lifetime = |c: &str| tables.categories.get(c).map_or(30.0, |c| c.lifetime);
            // `82BFF308`: the category's eviction Priority (absent = the attribute default, 0).
            let priority = |c: &str| tables.categories.get(c).map_or(0, |c| c.priority);
            // The post's vision test (`sub_82E41060` -> `82E27240`, b5 / b18): the event position against the
            // category's near / far ranges times the type's scales, the same eye, cone and line as the perception
            // pass. A type without a perception record keeps every post (retail's record default is unread).
            let sight_scales = data.sight_scales.get(&ped.entity).copied();
            let post_eye = [at[0], at[1] + 1.6, at[2]];
            let post_forward = [body.heading.sin(), 0.0, body.heading.cos()];
            let post_here = data.nav.as_deref().and_then(|m| m.locate(at).map(|h| (m, h)));
            let post_los = |_: [f32; 3], t: [f32; 3]| post_here.is_none_or(|(m, h)| m.clear_line(h, t));
            let seen = |e: &MoodEvent| match (sight_scales, tables.categories.get(&e.category)) {
                (Some(s), Some(c)) => skate_core::living_world::peds::perception::sees(
                    at,
                    post_eye,
                    post_forward,
                    e.position,
                    &skate_core::living_world::peds::perception::Sight::new(c.near, c.far, s[0], s[1], s[2]),
                    &post_los,
                ),
                _ => true,
            };
            let steps = tick.saturating_sub(ped.spawn_tick).saturating_sub(mind.ticks) as f32;
            if steps > 0.0 {
                mind.mood.tick(dt * steps, &lifetime);
                mind.presence_timer -= dt * steps;
                if mind.presence_timer <= 0.0 {
                    mind.presence_timer = magnitude(mood::category::PRESENCE).max(dt);
                    // `sub_82E41060` drops a post whose prerequisites fail (presence: within 12 m in
                    // the stock sets).
                    let post_ctx = MoodContext { ped: me, ped_type: &set, position: at, entity: &entity, zombie: settings.zombie, busy: &busy, own_plugin_object: mind.brain.own_plugin_object, chasing: mind.brain.chasee.is_some(), flee_pending: mind.brain.wants.get("flee").is_some_and(|w| w.needs_addressing) };
                    for e in mood::presence(me, at, &ped_list, &player_list) {
                        // A suppressed entity raises no mood (perception `+64`).
                        if !e.instigator.is_some_and(|i| mind.brain.perceptions.suppressed(i)) && tables.accepts(&set, &e, &post_ctx) && seen(&e) {
                            mind.mood.post_ranked(e, magnitude(mood::category::PRESENCE), &priority);
                        }
                    }
                }
                // The zombie broadcast (`sub_82E24B60`): every console frame while the cheat is on.
                if settings.zombie {
                    mind.zombie_timer -= dt * steps;
                    let mut posts = 0;
                    while mind.zombie_timer <= 0.0 && posts < 8 {
                        mind.zombie_timer += mood::BROADCAST_PERIOD;
                        posts += 1;
                    }
                    if posts > 0 {
                        let post_ctx = MoodContext { ped: me, ped_type: &set, position: at, entity: &entity, zombie: true, busy: &busy, own_plugin_object: mind.brain.own_plugin_object, chasing: mind.brain.chasee.is_some(), flee_pending: mind.brain.wants.get("flee").is_some_and(|w| w.needs_addressing) };
                        for e in mood::zombie_mode(&player_list) {
                            let suppressed = e.instigator.is_some_and(|i| mind.brain.perceptions.suppressed(i));
                            let in_sight = seen(&e);
                                let accepted = !suppressed && in_sight && tables.accepts(&set, &e, &post_ctx);
                            if let Some(held) = mind.log_repeats.allow(0x5a4f_4d42 ^ u64::from(accepted), tick) {
                                info!(
                                    "PED_ZOMBIE_POST ped=#{} {set} accepted={accepted} seen={in_sight} suppressed={suppressed} posts={posts} busy={} tick={tick}{}",
                                    ped.id.serial,
                                    mind.brain.busy(),
                                    if held > 0 { format!(" (+{held} held back)") } else { String::new() }
                                );
                            }
                            if accepted {
                                for _ in 0..posts {
                                    mind.mood.post_ranked(e.clone(), magnitude(mood::category::ZOMBIE_MODE), &priority);
                                }
                            }
                        }
                    }
                } else {
                    mind.zombie_timer = 0.0;
                }
            }
            // ShouldScatter (brain +3281 bit 0x80): the world alarm, the same for every live ped.
            mind.brain.scatter = scattering;
            if let Some(&car) = honked.get(&me) {
                mind.brain.honker = Some(car);
            }
            if let Some(i) = listening.iter().position(|&p| p == me) {
                listening.swap_remove(i);
                mind.brain.speech = Some(skate_core::living_world::peds::conversation::LISTENER_SPEECH);
            }
            // Every post runs the ped's prerequisites (`sub_82E41060`).
            let post_ctx = MoodContext { ped: me, ped_type: &set, position: at, entity: &entity, zombie: settings.zombie, busy: &busy, own_plugin_object: mind.brain.own_plugin_object, chasing: mind.brain.chasee.is_some(), flee_pending: mind.brain.wants.get("flee").is_some_and(|w| w.needs_addressing) };
            // ChannelGreetWantTarget's `greeted` (magnitude 1.0, `8269FAC0`).
            for &(_, instigator, position) in greeted_now.iter().filter(|g| g.0 == me) {
                let e = MoodEvent { category: mood::category::GREETED.into(), instigator: Some(instigator), second: Some(me), position };
                if !mind.brain.perceptions.suppressed(instigator) && tables.accepts(&set, &e, &post_ctx) && seen(&e) {
                    mind.mood.post_ranked(e, 1.0, &priority);
                }
            }
            // SkaterAvoidance's Jump post (`sub_82E2E890` -> `sub_82E3DA40`): category skateravoid, the skater as instigator.
            if let Some((instigator, position)) = mind.avoid_post.take() {
                let e = MoodEvent { category: mood::category::SKATER_AVOID.into(), instigator: Some(instigator), second: Some(me), position };
                if !mind.brain.perceptions.suppressed(instigator) && tables.accepts(&set, &e, &post_ctx) && seen(&e) {
                    mind.mood.post_ranked(e, 1.0, &priority);
                }
            }
            // A contact stored during a takedown attempt arms Colliding without playing anything yet; an Attempt Begin
            // in this update still drops it (`826A49D8`), as retail.
            for &(_, r) in stored.iter().filter(|s| s.0 == me) {
                mind.brain.pending_reaction = Some(r);
                mind.brain.begin_colliding = true;
            }
            if hits.contains(&me) {
                mind.brain.begin_colliding = true;
                mind.brain.colliding = true;
            }
            // IsPedestrianColliding holds while the contact reaction plays.
            if mind.brain.colliding && !body.player.reacting() {
                mind.brain.colliding = false;
            }
            for &(hit, toucher) in &collisions {
                let instigator = toucher.or_else(|| player_list.first().map(|p| p.0));
                if instigator.is_some_and(|i| mind.brain.perceptions.suppressed(i) || mind.brain.ignore_collisions_with == Some(i)) {
                    continue;
                }
                // Event origin (`sub_8269D8A8`: the contact's entity position): the hit ped's, so bystanders run the
                // vision test against it [inferred: which entity +188 is was not read].
                let origin = entity(hit).map_or(at, |(_, p)| p);
                // `sub_82E41060`: the contact is posted as `collision`; when the prerequisites refuse it and the hit ped
                // is another ped, it is posted again as `nearbycollision` (stock sets refuse a bystander's `collision`
                // with kind 6; the security sets have no such item, so their guards react to `collision`).
                let mut e = MoodEvent { category: mood::category::COLLISION.into(), instigator, second: Some(hit), position: origin };
                if !tables.accepts(&set, &e, &post_ctx) {
                    if hit == me {
                        continue;
                    }
                    e.category = mood::category::NEARBY_COLLISION.into();
                    if !tables.accepts(&set, &e, &post_ctx) {
                        continue;
                    }
                }
                if seen(&e) {
                    let m = magnitude(&e.category);
                    mind.mood.post_ranked(e, m, &priority);
                }
            }
            // The brain's own rolls (sit time, stand-up) and its type's sit values, once per ped.
            if mind.brain.rng.is_none() {
                mind.brain.rng = Some(skate_core::living_world::Rng::new(skate_core::living_world::rng::derive(world_seed, &[0x4252_4e52, me])));
                mind.brain.sit = entity(me).and_then(|(name, _)| data.sit.get(name.as_str()).copied()).unwrap_or_default();
                // The starting hand prop (ped constructor `82E33198`): the type's chance, then its weighted list.
                let start = entity(me).and_then(|(name, _)| data.starting_props.get(name.as_str()));
                if let (Some((chance, list)), true) = (start, settings.ped_brain.values.hand_prop.starting_props) {
                    let r = mind.brain.rng.as_mut().expect("seeded above");
                    let (a, b) = (r.modulo(100) + 1, r.modulo(100) + 1);
                    if let Some(key) = skate_core::living_world::peds::brain::HandProp::starting_pick(*chance, list, a, b).map(str::to_string) {
                        mind.brain.hand_prop.request(&key);
                        if let Some(r) = data.hand_props.props.get(&key) {
                            let h = &mut mind.brain.hand_prop;
                            (h.disposable, h.can_sit, h.can_attack_throw) = (r.disposable, r.can_sit, r.can_attack_throw);
                        }
                        info!("PED_HAND_PROP ped=#{} requested {key} at spawn tick={tick}", ped.id.serial);
                    }
                }
            }
            let rng = mind.rng.get_or_insert_with(|| skate_core::living_world::Rng::new(skate_core::living_world::rng::derive(world_seed, &[0x4d4f_4f44, me])));
            let ctx = MoodContext { ped: me, ped_type: &set, position: at, entity: &entity, zombie: settings.zombie, busy: &busy, own_plugin_object: mind.brain.own_plugin_object, chasing: mind.brain.chasee.is_some(), flee_pending: mind.brain.wants.get("flee").is_some_and(|w| w.needs_addressing) };
            let brain = &mind.brain;
            // `7FF7`: the reactions about the instigator the ped has finished (its perception entry `+68`, `82E42A58`).
            let outstanding = |id: u64| brain.perceptions.get(id).map_or(0, |e| e.reacted);
            let pending = |w: &str| brain.wants.get(w).is_some_and(|x| x.needs_addressing);
            // `sub_82E41B98` does nothing for a busy ped (`sub_82E3BF70`) or while DisableAllMoods holds (`brain+0xCCE` bit
            // 0x08, set / restored by `826A5A20` / `826A5A50`, research b176); a pass sets WaitingToReact.
            // `072D`: the hand-prop bit (`brain+3278` 0x02, set while a hand prop is attached, `82E3EC60` / `82E3EBE0`).
            let produced = if brain.busy() || brain.moods_disabled { None } else { tables.produce(&mut mind.mood, &ctx, brain.hand_prop.holding, &outstanding, &pending, rng) };
            if let Some(reaction) = produced {
                let key = reaction.result.bytes().chain([0]).chain(reaction.category.bytes()).fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3)) & !(1 << 63);
                // Collision results and any change of result are always logged: a held-back converse_everyone line
                // hid why a bumped ped skipped the warn (laptop session 2026-10-20, b393).
                let always = mind.log_repeats.mood_changed(key) || reaction.category == "collision";
                if let Some(held) = if always { Some(0) } else { mind.log_repeats.allow(key, tick) } {
                    info!(
                        "PED_MOOD ped=#{} {} result={} category={} roll={:?} passed={} wants={:?} tick={tick}{}",
                        ped.id.serial,
                        set,
                        reaction.result,
                        reaction.category,
                        reaction.rolled,
                        reaction.passed,
                        reaction.wants.iter().map(|w| w.want.as_str()).collect::<Vec<_>>(),
                        if held > 0 { format!(" (+{held} held back)") } else { String::new() }
                    );
                }
                if reaction.passed {
                    mind.brain.waiting_to_react = true;
                }
                for w in &reaction.wants {
                    mind.brain.raise_mood_want(&w.want, w.target, w.flag_40, settings.ped_brain.values.mood_want_memory_seconds);
                }
                events.write(PedEvent::Mood { id: ped.id, result: reaction.result.clone(), passed: reaction.passed, wants: reaction.wants.iter().map(|w| w.want.clone()).collect() });
            }
        }
        // A prop moving at the ped (`82E2EDC0`): flee it, outside the mood system (ask 374). Ours: never the ped's own
        // dropped hand prop (b649; retail's class cast 0x8269B3D0 not read).
        let own = linked_props.get(entity_id).ok().map(|r| PROP_TARGET_BASE | u64::from(r.body));
        if let Some((prop, _)) = moving_props.iter().find(|(id, o)| Some(*id) != own && skate_core::living_world::peds::object_flee::in_path(at, o, &object_flee)) {
            let fresh = mind.brain.wants.get("flee").is_none_or(|w| w.target != *prop || !w.needs_addressing);
            mind.brain.flee_object(*prop);
            if fresh {
                info!("PED_FLEE_OBJECT ped=#{} prop={} at [{:.2}, {:.2}, {:.2}] tick={tick}", ped.id.serial, prop & 0xFFFF_FFFF, at[0], at[1], at[2]);
            }
        } else if let Some((car, (cp, cv, _))) = (!moving_cars.is_empty())
            // Gate A: the ped stands on a road triangle (ped vf200 -> road network vf0x3C, b821).
            .then(|| state.roads.as_ref().is_some_and(|r| skate_core::living_world::traffic::spawn::road_under(r, at).is_some()))
            .and_then(|on_road| moving_cars.iter().find(|(_, (p, v, held))| skate_core::living_world::peds::object_flee::flees_vehicle_gated(at, *p, *v, on_road, *held, &vehicle_flee)))
        {
            // A car near the ped moving along the line between them (`82E2C0C8`, opt-in, W8).
            let fresh = mind.brain.wants.get("flee").is_none_or(|w| w.target != *car || !w.needs_addressing);
            mind.brain.flee_object(*car);
            if fresh {
                let d = [cp[0] - at[0], cp[1] - at[1], cp[2] - at[2]];
                let dot = d[0] * cv[0] + d[2] * cv[2];
                info!(
                    "PED_FLEE_VEHICLE ped=#{} car={car} at [{:.2}, {:.2}, {:.2}] car_at [{:.2}, {:.2}, {:.2}] dist={:.1} speed={:.1} dot={dot:.1} branch={} tick={tick}",
                    ped.id.serial, at[0], at[1], at[2], cp[0], cp[1], cp[2],
                    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt(), (cv[0] * cv[0] + cv[2] * cv[2]).sqrt(),
                    if dot > 0.0 { "away" } else { "toward" }
                );
            }
        }
        let due = tick.saturating_sub(ped.spawn_tick);
        let controller = mind.controller.get_or_insert_with(|| skate_core::graph::controller::Controller::new(program.topology.states.len()));
        // The ped's own chase record: its type's, or `zombie` under the zombie cheat (`82E33FAC`, b105).
        let record = if settings.zombie { data.chase_zombie.as_deref() } else { data.chase.get(&ped.entity) };
        let ped_speed = mind.last_at.map_or(0.0, |l| ((at[0] - l[0]).powi(2) + (at[2] - l[2]).powi(2)).sqrt() / (dt * tick.saturating_sub(ped.spawn_tick).saturating_sub(mind.ticks).max(1) as f32));
        mind.last_at = Some(at);
        // The takedown contact (`82E38FB8` sets the controller's begin-colliding byte +8 and records the touched entity
        // at brain +0xC70; `826A4BE8` succeeds when that entity is the target, b317): our skater cylinder against the
        // ped while the takedown plays. `takedowns` is our own count (brain+3248 counts ped-on-ped chase touches).
        if mind.brain.takedown_active && !mind.brain.takedown_contact {
            let ped_radius = data.nav.as_deref().map_or(super::vehicle_contacts::FALLBACK_PED_RADIUS, |m| m.agent[1]);
            if let Some(p) = mind.brain.takedown_target.filter(|t| *t >= PLAYER_TARGET_BASE).and_then(|t| target(t)) {
                let d = ((p[0] - at[0]).powi(2) + (p[2] - at[2]).powi(2)).sqrt();
                if d < ped_radius + SKATER_CONTACT_RADIUS && (p[1] - at[1]).abs() <= 2.0 {
                    mind.brain.takedown_contact = true;
                    mind.brain.takedowns += 1;
                }
            }
        }
        // HasLineOfSightToTazeTarget (`+3280` bit 0x10, retail's batched ray pass `82E23D68`):
        // ours is the navmesh line to the tazer want's target while the tazer is out.
        if mind.brain.tazer_drawn_requested {
            let goal = mind.brain.tazer_want.as_ref().and_then(|w| mind.brain.wants.get(w)).and_then(|w| target(w.target));
            mind.brain.tazer_line_of_sight = match (goal, data.nav.as_deref()) {
                (Some(g), Some(mesh)) => mesh.locate(at).is_some_and(|h| mesh.clear_line(h, g)),
                (Some(_), None) => true,
                _ => false,
            };
        }
        let takedown_table = if settings.zombie { data.takedown_zombie.as_deref() } else { data.takedowns.get(&ped.entity) };
        let forward = [body.heading.sin(), body.heading.cos()];
        let choose = |t: u64| {
            let (table, p) = (takedown_table?, target(t)?);
            let v = observers.observers.get(t.wrapping_sub(PLAYER_TARGET_BASE) as usize).map_or([0.0; 3], |o| o.velocity);
            skate_core::living_world::peds::takedown::choose(table, at, forward, ped_speed, p, v)
        };
        // The seat's carried steps move the seated ped's root and its lock with it (`82E36D70`, b219).
        if mind.brain.own_plugin_object && mind.sit_avoid.is_none() {
            for (_, d) in seat_carry.iter().filter(|(o, _)| *o == me) {
                if let Some(p) = mind.brain.locked_at.as_mut() {
                    (p[0], p[2]) = (p[0] + d[0], p[2] + d[2]);
                }
                body.position.x += d[0];
                body.position.z += d[2];
            }
        }
        // LockToCurrentPosition: the body stays on the locked point (root motion does not move it).
        if let Some(p) = mind.brain.locked_at {
            body.position.x = p[0];
            body.position.z = p[2];
        }
        while mind.ticks < due {
            mind.ticks += 1;
            mind.brain.tick_timers(dt);
            // The taunt clip ended (`MajorIntentComplete`): the intent goes, the graph leaves DoTaunt.
            if body.taunt == TauntClip::Done {
                body.taunt = TauntClip::None;
                mind.brain.monitored.remove("SGIntent");
            }
            // ApproachWantTarget sets its goal each tick it runs.
            mind.brain.approach = None;
            mind.brain.approach_slide = None;
            // HasPlugin (`826A99C0`): the ped is a participant of a live conversation.
            if mind.brain.plugin.is_none_or(|id| !conversations.map.get(&id).is_some_and(|c| c.members.iter().any(|m| m.ped == me))) {
                mind.brain.plugin = conversations.map.values().find(|c| c.members.iter().any(|m| m.ped == me)).map(|c| c.id);
            }
            conversations.pending_refusals.retain(|&(p, key)| {
                if p == me {
                    mind.refusals.remember(key, &plugin_props.settings);
                }
                p != me
            });
            mind.refusals.tick(dt, &plugin_props.settings);
            // The Plugin behavior's Update (`8269F248` -> `82E40BD0`, b104) re-stores the 30 s refusal for its prop (or
            // conversation area) every tick it runs, so a ped that leaves it is not offered it again for 30 s.
            if mind.brain.in_plugin {
                if let Some(prop) = mind.prop.and_then(|(i, ..)| plugin_props.props.get(i)) {
                    mind.refusals.remember(prop.id, &plugin_props.settings);
                } else if let Some(id) = mind.brain.plugin {
                    mind.refusals.remember(skate_core::living_world::peds::plugins::conversation_key(id), &plugin_props.settings);
                }
            }
            // 82E3DBE0: the seated owner of a moving object jumps off forward (SitAvoidFwd): docs/hails-additions/26f-living-world-peds.md#seated-owner-jump-off
            let anim = body.player.reaction_anim();
            let seated = mind.brain.own_plugin_object && anim == Some(SIT_CYCLE);
            use skate_core::living_world::peds::avoid;
            // A skater coming at the seated ped (SkaterAvoidance, every `period` on the ped's own phase): the first
            // skater whose closest approach qualifies decides (docs/hails-additions/26f-living-world-peds.md#seated-peds-avoid-an-approaching-skater).
            let rules = settings.ped_brain.values.avoid;
            let every = ((rules.period / dt).round() as u64).max(1);
            let on_phase = (tick + u64::from(ped.id.serial)) % every == 0
                && mind.brain.nav_modifier(skate_core::living_world::peds::brain::nav_modifier::SKATER_AVOIDANCE);
            // The first skater whose decision passes `accept` and the escape point's 0.1 m ground probe (82C46460).
            let decide = |accept: &dyn Fn(&avoid::AvoidDecision) -> bool| {
                observers.observers.iter().enumerate().find_map(|(i, o)| {
                    let d = avoid::decide(at, forward, o.position, o.velocity, &rules).filter(|d| accept(d))?;
                    let ground = data.nav.as_ref().is_none_or(|nav| {
                        nav.locate_any(d.escape_point).is_some_and(|q| (q.position[0] - d.escape_point[0]).abs() <= SEAT_PROBE_TOLERANCE && (q.position[2] - d.escape_point[2]).abs() <= SEAT_PROBE_TOLERANCE)
                    });
                    if !ground {
                        info!("PED_AVOID_SKIP ped=#{} skater={i} why=off_ground {} {} t={:.2} miss={:.2} tick={tick}", ped.id.serial, d.kind.name(), d.dir.name(), d.t, d.miss);
                    }
                    ground.then_some((i, d))
                })
            };
            let skater_avoid = if seated && mind.sit_avoid.is_none() && on_phase { decide(&|_| true) } else { None };
            // Standing / walking peds: the same decision into the Avoid graph's Jump / Step (b683):
            // docs/hails-additions/26f-living-world-peds.md#standing-peds-dodge-an-approaching-skater
            let can_dodge = !seated && rules.standing && avoid::may_dodge(body.player.state) && !body.player.reacting();
            let pending = mind.stand_avoid;
            if pending.is_some_and(|p| anim == Some(p.clip)) {
                // `advance_peds` started the clip.
                mind.stand_avoid = None;
            } else if can_dodge && on_phase {
                if let Some((skater, d)) = decide(&|d| avoid::replaces(pending, d.kind)) {
                    let play = avoid::standing(d.kind, d.dir);
                    if play.speech.is_some() {
                        mind.brain.speech = play.speech;
                    }
                    mind.stand_avoid = Some(play);
                    mind.stand_avoid_from = Some(at);
                    events.write(PedEvent::Avoid { id: ped.id, mover: PLAYER_TARGET_BASE + skater as u64, kind: d.kind.name(), dir: d.dir.name(), seated: false });
                    if d.kind == avoid::AvoidKind::Jump {
                        mind.avoid_post = Some((PLAYER_TARGET_BASE + skater as u64, at));
                    }
                    info!(
                        "PED_AVOID ped=#{} skater={skater} {} {} standing state={} clip={}{} t={:.2} miss={:.2} speed={:.2} angle={:.2} tick={tick}",
                        ped.id.serial, d.kind.name(), d.dir.name(), body.player.state.name(), play.clip, if play.mirror { " mirrored" } else { "" }, d.t, d.miss, d.speed, d.angle
                    );
                }
            } else if !can_dodge && mind.stand_avoid.take().is_some() {
                debug!("PED_AVOID ped=#{} dropped (state {}) tick={tick}", ped.id.serial, body.player.state.name());
            }
            if mind.stand_avoid.is_none() && !anim.is_some_and(|a| avoid::STANDING_CLIPS.contains(&a)) {
                if let Some(from) = mind.stand_avoid_from.take() {
                    let moved = ((at[0] - from[0]).powi(2) + (at[2] - from[2]).powi(2)).sqrt();
                    info!("PED_AVOID_DONE ped=#{} moved={moved:.2} tick={tick}", ped.id.serial);
                    // AvoidComplete: PedestrianWandering 1.
                    mind.brain.speech = Some(avoid::AVOID_COMPLETE_SPEECH);
                }
            }
            if seat_pushed.contains(&me) && seated && mind.sit_avoid.is_none() {
                let jump = avoid::seated(avoid::AvoidKind::Jump, avoid::AvoidDir::Forward);
                mind.brain.speech = jump.speech;
                mind.sit_avoid = Some(jump);
                mind.sit_avoid_from = Some(at);
                info!("PED_SIT_AVOID ped=#{} jumps off forward (its seat moved) locked={} tick={tick}", ped.id.serial, mind.brain.locked_at.is_some());
            } else if let Some((skater, d)) = skater_avoid {
                let play = avoid::seated(d.kind, d.dir);
                if play.speech.is_some() {
                    mind.brain.speech = play.speech;
                }
                mind.sit_avoid = Some(play);
                mind.sit_avoid_from = Some(at);
                events.write(PedEvent::Avoid { id: ped.id, mover: PLAYER_TARGET_BASE + skater as u64, kind: d.kind.name(), dir: d.dir.name(), seated: true });
                if d.kind == avoid::AvoidKind::Jump {
                    mind.avoid_post = Some((PLAYER_TARGET_BASE + skater as u64, at));
                }
                info!(
                    "PED_AVOID ped=#{} skater={skater} {} {} seated clip={}{} leaves_seat={} t={:.2} miss={:.2} speed={:.2} angle={:.2} tick={tick}",
                    ped.id.serial, d.kind.name(), d.dir.name(), play.clip, if play.mirror { " mirrored" } else { "" }, play.leaves_seat, d.t, d.miss, d.speed, d.angle
                );
            } else if let Some(played) = mind.sit_avoid.filter(|_| !seated) {
                // `advance_peds` started the clip (or the ped left the seat another way first).
                mind.sit_avoid = None;
                info!("PED_SIT_AVOID ped=#{} played={} anim={anim:?} locked={} tick={tick}", ped.id.serial, anim == Some(played.clip), mind.brain.locked_at.is_some());
            } else if mind.sit_avoid.is_none() && !anim.is_some_and(|a| avoid::SEATED_CLIPS.contains(&a)) {
                if let Some(from) = mind.sit_avoid_from.take() {
                    let moved = ((at[0] - from[0]).powi(2) + (at[2] - from[2]).powi(2)).sqrt();
                    info!("PED_SIT_AVOID_DONE ped=#{} moved={moved:.2} anim={anim:?} tick={tick}", ped.id.serial);
                }
            }
            // HasPlugin: a conversation member, or a ped that took a world prop.
            let prop_wp = mind.prop.and_then(|(i, id, w)| plugin_props.props.get(i).filter(|p| p.id == id && !streamed_out(id) && !p.disabled).and_then(|p| p.waypoints.get(w)).copied());
            // A prop lost without ExitPlugin (streamed out, disabled, moved away) releases its waypoint like ExitPlugin
            // does: retail's waypoint slot `ped+0x1718` belongs to the plugin, and LockFirstWaypoint (`826A0580`) only
            // locks an empty slot, so a stale prop waypoint would steer a later conversation (b186).
            if mind.prop.is_some() && prop_wp.is_none() {
                if let Some((i, id, w)) = mind.prop.take() {
                    // Which condition dropped it (b300 §4: an occupied bench dropped amid stream evictions).
                    let reason = match plugin_props.props.get(i).filter(|p| p.id == id) {
                        None => "prop gone",
                        Some(_) if streamed_out(id) => "streamed out",
                        Some(p) if p.disabled => "disabled",
                        Some(p) if p.waypoints.get(w).is_none() => "waypoint missing",
                        Some(_) => "?",
                    };
                    let class = plugin_props.props.get_mut(i).filter(|p| p.id == id).map(|p| {
                        p.release(me);
                        p.class.clone()
                    });
                    info!("PED_PLUGIN_PROP ped=#{} leaves {} {:016X} (dropped: {reason}) tick={tick}", ped.id.serial, class.as_deref().unwrap_or("?"), id);
                    // A seated ped stands up (Sit2Stand) before its next behavior, the way a packet that ends mid-clip
                    // does: the sit cycle was held until a StandUp stage that never comes once the seat is dropped
                    // (laptop 2026-10-19: #105 left its seat for a greet and floated seated in the sit idle).
                    if body.player.reaction_anim() == Some(SIT_CYCLE) && body.plugin_motion.state == TauntClip::Playing && !body.plugin_motion.orphaned {
                        body.plugin_motion.orphaned = true;
                        info!("PED_PLUGIN_MOTION ped=#{} seat dropped while seated, standing up tick={tick}", ped.id.serial);
                    }
                }
                mind.brain.release_plugin_waypoint();
            }
            // The locked waypoint is read live (`826AB9C0` re-reads its position on every evaluation): a seat that moved
            // with its prop is where the ped walks and sits.
            if let (Some(w), true) = (prop_wp, mind.brain.waypoint.is_some()) {
                mind.brain.waypoint = Some(w.position);
            }
            mind.brain.has_plugin = mind.brain.plugin.is_some() || mind.prop.is_some();
            // ThrowHandPropAtTrashBin's target, the plugin object's hotpoint 0 (`826A7998`, b87 §3): our hotpoint props
            // are one prop per hotpoint and every stock bin has only hotpoint 0, so the taken prop's own point.
            mind.brain.plugin_target = mind.prop.and_then(|(i, id, _)| plugin_props.props.get(i).filter(|p| p.id == id)).and_then(|p| p.waypoints.first()).map(|w| w.position);
            let conv = mind.brain.plugin.and_then(|id| conversations.map.get(&id));
            let free: Vec<[f32; 3]> = match (conv, prop_wp) {
                (Some(c), _) => c.waypoints.iter().filter(|w| w.1.is_none()).map(|w| w.0).collect(),
                (None, Some(w)) => vec![w.position],
                _ => Vec::new(),
            };
            // A world prop's view: its waypoint (reserved at the offer) and a facing point along its orientation.
            let conversation = match (conv, prop_wp) {
                (Some(c), _) => Some(skate_core::living_world::peds::brain::ConversationInfo { complete: c.is_complete(), speaker: c.speaker(), center: c.center, free_waypoints: &free, speech: c.turn_speech() }),
                (None, Some(w)) => {
                    mind.brain.waypoint_facing = Some(w.facing);
                    let center = [w.position[0] + w.facing[0] * 10.0, w.position[1], w.position[2] + w.facing[2] * 10.0];
                    Some(skate_core::living_world::peds::brain::ConversationInfo { complete: false, speaker: None, center, free_waypoints: &free, speech: None })
                }
                _ => None,
            };
            let groups_now = &chase_groups.0;
            let groups = |chasee: u64| groups_now.get(&chasee).map(|g| g.info(max_chasers(chasee)));
            // UpdateBlockPrediction (`82D99540`): the chasee radius `G+1648` has no known writer: 0.0.
            let block = |chasee: u64| {
                let (r, q, g) = (record?, target(chasee)?, groups_now.get(&chasee)?);
                let v = observers.observers.get(chasee.wrapping_sub(PLAYER_TARGET_BASE) as usize).map_or([0.0; 3], |o| o.velocity);
                Some((skate_core::living_world::peds::chase::should_block(at, q, v, 0.0, r), g.formation_point(me, q)))
            };
            mind.brain.zombie = settings.zombie;
            // The ped manager pass posts the skater's score / bail to every ped while there is one (b157).
            mind.brain.skater_feed = skater.as_deref().map(|s| s.scoring.spectator_feed()).filter(|&(score, bail)| score > 0 || bail)
                .map(|(score, bail)| skate_core::living_world::peds::brain::SkaterFeed { score, bail });
            mind.brain.gesture_busy = !body.player.channel_idle();
            // ChaseeIsWipedOut / the takedown gate: a player chasee's wiping-out bit; a ped chasee's flag (brain `+3277`
            // bit 0x04) is not modeled yet (false).
            let wiping_of = |id: u64| id >= PLAYER_TARGET_BASE && observers.wiping.get((id - PLAYER_TARGET_BASE) as usize).copied().unwrap_or(false);
            let mut host = skate_core::living_world::peds::brain::BrainHost {
                behaviors: &graph.behaviors,
                conditions: &graph.conditions,
                brain: &mut mind.brain,
                settings: &settings.ped_brain.values,
                position: body.position.to_array(),
                heading: body.heading,
                skater: observers.observers.first().map(|o| (o.position, o.velocity)),
                target_position: &target,
                chase: skate_core::living_world::peds::brain::ChaseView { me, record, groups: Some(&groups), takedowns: Some(&choose), block: Some(&block), conversation, velocity: Some(&velocity_of), own_velocity: body.velocity.to_array(), wiping: Some(&wiping_of), recognizers: recognizers.len(), can_recognize: settings.ped_brain.values.can_recognize_player },
            };
            controller.update(program, dt, &mut host);
            // RegisterRecognizer (`826A2E00`): the world list, capped at 16 (`sub_82E1C538`).
            if mind.brain.recognizer {
                if !recognizers.contains(&me) && recognizers.len() < settings.ped_brain.values.max_recognizers {
                    recognizers.push(me);
                }
            } else {
                recognizers.retain(|&r| r != me);
            }
            // The head look-at (`82E243D0` -> `82E27F90`, b1019): the top of the attention stack, stepped at 1/60 s.
            // The ped type's records: `livingworld_entity_headtracking` (on for every shipped type) and its post-adjust
            // limits (marquee pros `ip_with_skateboard`, the rest `default`).
            let ped_type = data.catalog.entities.get(&ped.entity);
            let tracking = ped_type.and_then(|e| e.head_tracking.as_ref()).and_then(|r| data.catalog.head_tracking.get(r)).copied().unwrap_or(true);
            let look_settings = ped_type.and_then(|e| e.postadjust.as_ref()).and_then(|r| data.catalog.head_look.get(r)).unwrap_or(&settings.ped_brain.head_look);
            if settings.ped_brain.head_tracking && tracking {
                use skate_core::living_world::peds::{brain::LookTarget, head_look};
                let point = match mind.brain.look_at.last() {
                    Some(LookTarget::Player) => observers.observers.first().map(|o| o.position),
                    Some(LookTarget::Entity(id)) => target(*id),
                    Some(LookTarget::Prop(p)) => Some(*p),
                    Some(LookTarget::Ahead) => None,
                    // Kind 4, the empty stack (`82E274B8`, b1018): the next path point at eye height.
                    None => body.nav.corners.get(body.nav.next).map(|c| [c[0], body.position.y + settings.ped_brain.head_look_eye_height, c[2]]),
                };
                let eye = settings.ped_brain.head_look_eye_height;
                let aim = point.map(|p| head_look::target_angles(body.position.to_array(), body.heading, eye, p))
                    // Straight ahead instead when the path point is 80 deg or more off the facing (`0x820E5748`).
                    .filter(|a| !(mind.brain.look_at.is_empty() && a.0.abs() >= settings.ped_brain.head_look_path_limit));
                if body.head_look.bones.is_empty() {
                    body.head_look.bones = head_bones(&data.rig);
                }
                for _ in 0..((dt / head_look::STEP).round() as usize).max(1) {
                    body.head_look.step(look_settings, aim.map(|a| (a.0, a.1)), aim.map_or(1.0, |a| a.2));
                }
            }
            // A stored reaction moved to `reaction_due` at Colliding Begin: the body plays it in `advance_peds` later
            // this tick (same fixed tick as the Begin, `8269DA00`); IsPedestrianColliding holds while it plays.
            if let Some(r) = mind.brain.reaction_due.take() {
                body.reaction_due = Some(r);
                mind.brain.colliding = true;
            }
            if std::mem::take(&mut mind.brain.collision_began) {
                info!("PED_COLLIDING ped=#{} begins: collision broadcast tick={tick}", ped.id.serial);
                events.write(PedEvent::Colliding { id: ped.id });
            }
            // The Plugin state runs the plugin's own graph on the same brain (`8269F248`).
            let plugin_graph = match mind.prop {
                Some((i, ..)) => plugin_props.props.get(i).and_then(|p| Some((p.class.clone(), data.plugin_graphs.get(&p.class)?))).map(|(k, g)| (k, &**g)),
                None => data.conversation_graph.as_deref().map(|g| (CONVERSATION_GRAPH.to_owned(), g)),
            };
            // Leaving Plugin ends the old graph's behaviors (8269F310 -> 82E0CFA0 -> 82C13498, b300): docs/hails-additions/26f-living-world-peds.md#plugin-behavior-end
            let next_key = plugin_graph.as_ref().filter(|_| mind.brain.in_plugin).map(|(k, _)| k.as_str());
            if let (Some(old_key), Some(pc)) = (mind.plugin_graph.clone(), mind.plugin_controller.as_mut()) {
                let old = if old_key == CONVERSATION_GRAPH { data.conversation_graph.as_deref() } else { data.plugin_graphs.get(&old_key).map(|g| &**g) };
                if let (true, Some(og)) = (next_key != Some(old_key.as_str()), old) {
                    let mut host = skate_core::living_world::peds::brain::BrainHost {
                        behaviors: &og.behaviors,
                        conditions: &og.conditions,
                        brain: &mut mind.brain,
                        settings: &settings.ped_brain.values,
                        position: body.position.to_array(),
                        heading: body.heading,
                        skater: observers.observers.first().map(|o| (o.position, o.velocity)),
                        target_position: &target,
                        chase: skate_core::living_world::peds::brain::ChaseView { me, record, groups: Some(&groups), takedowns: Some(&choose), block: Some(&block), conversation, velocity: Some(&velocity_of), own_velocity: body.velocity.to_array(), wiping: Some(&wiping_of), recognizers: recognizers.len(), can_recognize: settings.ped_brain.values.can_recognize_player },
                    };
                    pc.end_all_behaviors(&mut host);
                    info!("PED_PLUGIN_END ped=#{} graph {old_key} ended (in_plugin={}) tick={tick}", ped.id.serial, mind.brain.in_plugin);
                }
            }
            match (mind.brain.in_plugin, plugin_graph) {
                (true, Some((key, pg))) => {
                    let states = pg.graph.runtime.program.topology.states.len();
                    if let Some(old) = plugin_controller_for(&mut mind.plugin_controller, &mut mind.plugin_graph, &key, states) {
                        // The prop the ped used streamed out or was dropped while it stayed in the Plugin state: the
                        // other graph starts fresh (its controller was sized for the old one; a session crashed, 2026-10-12).
                        info!("PED_PLUGIN ped=#{} graph {old} -> {key} tick={tick}", ped.id.serial);
                        mind.plugin_state = None;
                    }
                    let pc = mind.plugin_controller.as_mut().expect("plugin controller just made");
                    let mut host = skate_core::living_world::peds::brain::BrainHost {
                        behaviors: &pg.behaviors,
                        conditions: &pg.conditions,
                        brain: &mut mind.brain,
                        settings: &settings.ped_brain.values,
                        position: body.position.to_array(),
                        heading: body.heading,
                        skater: observers.observers.first().map(|o| (o.position, o.velocity)),
                        target_position: &target,
                        chase: skate_core::living_world::peds::brain::ChaseView { me, record, groups: Some(&groups), takedowns: Some(&choose), block: Some(&block), conversation, velocity: Some(&velocity_of), own_velocity: body.velocity.to_array(), wiping: Some(&wiping_of), recognizers: recognizers.len(), can_recognize: settings.ped_brain.values.can_recognize_player },
                    };
                    pc.update(&pg.graph.runtime.program, dt, &mut host);
                    if mind.prop.is_some() && pc.frame.current == mind.plugin_state && tick % 150 == 0 {
                        // Progress of a ped on a world prop (logs must diagnose a stall).
                        let name = |s: Option<usize>| s.and_then(|s| pg.graph.binding.states.get(s)).map_or("none", |s| s.name.as_str());
                        let dist = mind.brain.waypoint.map(|w| ((w[0] - body.position.x).powi(2) + (w[2] - body.position.z).powi(2)).sqrt());
                        // The plugin's give-up timer (`PluginTimeout`, 60 s from EnterInteraction) and how many brain timers
                        // run: a ped stuck past 60 s must show why the timer did not expire (2026-10-14).
                        let timeout = mind.brain.timers.get(&skate_core::living_world::peds::brain::timers::index("PluginTimeout")).copied();
                        info!("PED_PLUGIN_WAIT ped=#{} in {} dist={:?} at=[{:.2}, {:.2}, {:.2}] approach={:?} slide={:?} route={} plugin_timeout={timeout:?} timers={} tick={tick}", ped.id.serial, name(pc.frame.current), dist.map(|d| (d * 1000.0).round() / 1000.0), body.position.x, body.position.y, body.position.z, mind.brain.approach.map(|a| a.1), mind.brain.approach_slide, body.nav.route.is_some(), mind.brain.timers.len());
                    }
                    if pc.frame.current != mind.plugin_state {
                        let name = |s: Option<usize>| s.and_then(|s| pg.graph.binding.states.get(s)).map_or("none", |s| s.name.as_str());
                        let dist = mind.brain.waypoint.map(|w| ((w[0] - body.position.x).powi(2) + (w[2] - body.position.z).powi(2)).sqrt());
                        info!("PED_PLUGIN ped=#{} {} -> {} waypoint={:?} dist={:?} at=[{:.2}, {:.2}, {:.2}] tick={tick}", ped.id.serial, name(mind.plugin_state), name(pc.frame.current), mind.brain.waypoint.map(|w| [w[0].round(), w[2].round()]), dist.map(|d| (d * 1000.0).round() / 1000.0), body.position.x, body.position.y, body.position.z);
                        mind.plugin_state = pc.frame.current;
                    }
                }
                _ => {
                    mind.plugin_controller = None;
                    mind.plugin_state = None;
                    mind.plugin_graph = None;
                }
            }
            // The motion side of the monitored packets (`82E29540`): a plugin state plays the front packet, releases its
            // held cycle on the next stage intent and ends the packet when its clips are done (`IntentStageComplete`
            // decrement, `MajorIntentComplete`); packets whose motion state is not ported end at their last stage.
            {
                use skate_core::living_world::peds::plugin_motion::motion_for;
                let run = &mut body.plugin_motion;
                match run.packet.clone() {
                    None => {
                        if let Some(name) = mind.brain.monitored.iter().find(|(n, p)| p.active && motion_for(n).is_some()).map(|(n, _)| n.clone()) {
                            info!("PED_PLUGIN_MOTION ped=#{} {name} start tick={tick}", ped.id.serial);
                            *run = PluginMotionRun { packet: Some(name), state: TauntClip::Requested, ..Default::default() };
                        }
                    }
                    Some(name) => match mind.brain.monitored.get_mut(&name).filter(|_| !run.orphaned) {
                        // The packet ended mid-clip: its clips play out (the cycle released) before the run ends.
                        None if run.state == TauntClip::Playing => {
                            if !run.orphaned {
                                info!("PED_PLUGIN_MOTION ped=#{} {name} ended mid-clip, standing up tick={tick}", ped.id.serial);
                            }
                            run.orphaned = true;
                        }
                        None => *run = PluginMotionRun::default(),
                        Some(p) => {
                            let m = motion_for(&name).expect("only plugin states start a run");
                            if run.state == TauntClip::Done {
                                if m.decrement && p.stage > 0 {
                                    p.stage -= 1;
                                }
                                p.active = false;
                                info!("PED_PLUGIN_MOTION ped=#{} {name} complete tick={tick}", ped.id.serial);
                                *run = PluginMotionRun::default();
                            } else if m.steps.iter().any(|st| st.hold_until.is_some() && st.hold_until == p.current()) {
                                run.release = true;
                            }
                        }
                    },
                }
            }
            // SpawnInteractionBasedHandProp (`826AE950` -> `82E3DDA0`): the plugin prop's hand prop is requested
            // (`brain+3279` 0x01) with its record's bools; `ped_hand_props::sync_hand_props` creates the object.
            if std::mem::take(&mut body.plugin_motion.hand_prop_due) {
                let class = mind.prop.and_then(|(i, _, _)| plugin_props.props.get(i)).map(|p| p.class.clone());
                let list = class.as_ref().and_then(|c| data.plugins.classes.get(c)).map(|c| c.hand_props.clone()).unwrap_or_default();
                let roll = mind.brain.rng.as_mut().map_or(0.0, |r| r.unit());
                match skate_core::living_world::peds::brain::HandProp::pick(&list, roll) {
                    Some(key) => {
                        mind.brain.hand_prop.request(key);
                        if let Some(r) = data.hand_props.props.get(key) {
                            let h = &mut mind.brain.hand_prop;
                            (h.disposable, h.can_sit, h.can_attack_throw) = (r.disposable, r.can_sit, r.can_attack_throw);
                        }
                        info!("PED_HAND_PROP ped=#{} requested {key} from {} tick={tick}", ped.id.serial, class.as_deref().unwrap_or("?"));
                        events.write(PedEvent::HandProp { id: ped.id, key: key.to_string() });
                    }
                    None => info!("PED_HAND_PROP ped=#{} none offered by {} tick={tick}", ped.id.serial, class.as_deref().unwrap_or("?")),
                }
            }
            mind.brain.settle_packets(&|name| name == "SGIntent" || skate_core::living_world::peds::plugin_motion::motion_for(name).is_some());
            // Group changes, in the order the graph asked for them.
            for request in std::mem::take(&mut mind.brain.chase_requests) {
                use skate_core::living_world::peds::brain::ChaseRequest;
                let (chasee, kind, reason) = match request {
                    ChaseRequest::Join { chasee } => {
                        // The formation offset: chaser - chasee at the join (`82D96C38`).
                        let offset = target(chasee).map_or([0.0; 3], |q| [at[0] - q[0], at[1] - q[1], at[2] - q[2]]);
                        let joined = chase_groups.0.entry(chasee).or_default().add(me, offset, max_chasers(chasee));
                        if !joined && mind.brain.chasee == Some(chasee) {
                            mind.brain.chasee = None;
                        }
                        (chasee, if joined { "join" } else { "join_refused" }, None)
                    }
                    ChaseRequest::Leave { chasee } => {
                        if let Some(g) = chase_groups.0.get_mut(&chasee) {
                            g.remove(me);
                        }
                        (chasee, "leave", None)
                    }
                    ChaseRequest::GiveUpPrimary { chasee } => {
                        chase_groups.0.get_mut(&chasee).map(|g| g.give_up_primary(me));
                        (chasee, "primary", None)
                    }
                    // Only a player gets the marker message (`vfn92` "is a player").
                    ChaseRequest::StateMessage { target, state } => {
                        if target >= PLAYER_TARGET_BASE {
                            let kind = ["state_warn", "state_chase", "state_tired", "state_giveup", "state_other"][usize::from(state.min(4))];
                            info!("PED_CHASE ped=#{} {kind} chasee={target} tick={tick}", ped.id.serial);
                            events.write(PedEvent::Chase { id: ped.id, chasee: target, kind, reason: None });
                        }
                        continue;
                    }
                    ChaseRequest::Takedown { target } | ChaseRequest::TakedownFailed { target } => {
                        let success = matches!(request, ChaseRequest::Takedown { .. });
                        let player = target >= PLAYER_TARGET_BASE;
                        if success {
                            // `826A4E50`: speech 65 when the player was taken down, else 19.
                            mind.brain.speech = Some(if player { 65 } else { 19 });
                            if player {
                                skater_takedowns.write(SkaterTakedownRequest { player: (target - PLAYER_TARGET_BASE) as usize, chaser: at });
                            }
                        }
                        info!("PED_TAKEDOWN ped=#{} target={target} success={success} entry={:?} takedowns={} tick={tick}", ped.id.serial, mind.brain.takedown_choice.map(|c| c.entry), mind.brain.takedowns);
                        events.write(PedEvent::Takedown { id: ped.id, target, success });
                        continue;
                    }
                    // `826A8420`: `T.vfn20(ped position)`, the takedown's latch on the skater.
                    ChaseRequest::Taze { target } => {
                        if target >= PLAYER_TARGET_BASE {
                            skater_takedowns.write(SkaterTakedownRequest { player: (target - PLAYER_TARGET_BASE) as usize, chaser: at });
                        }
                        info!("PED_TAZE ped=#{} target={target} tick={tick}", ped.id.serial);
                        events.write(PedEvent::Taze { id: ped.id, target });
                        continue;
                    }
                    // The tazer's burst sound for the state graph's TazerCycTime (2.0 s).
                    ChaseRequest::TazerOn => {
                        tazers.write(crate::world_audio::PedTazerEvent { ped: entity_id, seconds: None });
                        continue;
                    }
                    ChaseRequest::Greeted { target } => {
                        greeted.push((target, me, at));
                        info!("PED_GREET ped=#{} target={target} tick={tick}", ped.id.serial);
                        continue;
                    }
                    // SpawnConversationArea 826A6CD8 and its waypoint generator 82E1D620 (b134): docs/hails-additions/26f-living-world-peds.md#spawn-conversation-area
                    ChaseRequest::SpawnConversation { target: partner } => {
                        let near = conversations.map.values().any(|c| (c.center[0] - at[0]).powi(2) + (c.center[2] - at[2]).powi(2) < conv_params.exclusion * conv_params.exclusion);
                        let has_target = target(partner).is_some();
                        // Refused starts are logged (5 s per ped and reason): a ped retrying every 2 ticks was invisible (b511).
                        if near || !has_target {
                            if let Some(held) = mind.log_repeats.allow(0x434f_4e56 ^ u64::from(near), tick) {
                                info!("PED_CONVERSATION start ped=#{} with={partner} refused: {} held={held} tick={tick}", ped.id.serial, if near { "another conversation within the exclusion radius" } else { "no target" });
                            }
                        }
                        if !near && has_target {
                            let ahead = [at[0] + body.heading.sin() * conv_params.ahead, at[1], at[2] + body.heading.cos() * conv_params.ahead];
                            // Without a physics world (tests) the center is used as it is.
                            let center = match physics.as_deref() {
                                Some(p) => ground(Some(p), Vec3::from_array(ahead), conv_params.drop_height, conv_params.drop_height).map(|y| [ahead[0], y, ahead[2]]),
                                None => Some(ahead),
                            };
                            let probe = |w: [f32; 3]| data.nav.as_deref().and_then(|n| n.locate_any(w).map(|p| (p.position, n.area(p.poly))));
                            // The group's category by probability, its rows the candidates.
                            let cats = data.conversations.by_entity.get(&ped.entity).cloned().unwrap_or_default();
                            let total: f32 = cats.iter().map(|c| c.0).sum();
                            let mut pick = conv_rand() * total;
                            let candidates = cats.iter().find(|c| {
                                pick -= c.0;
                                pick < 0.0
                            }).or(cats.last()).map(|c| c.1.clone()).unwrap_or_default();
                            conversations.next_id += 1;
                            let id = conversations.next_id;
                            let mut c = skate_core::living_world::peds::conversation::Conversation::new(id, center, candidates, &conv_params, data.nav.is_some().then_some(&probe as skate_core::living_world::peds::conversation::Probe), &mut conv_rand);
                            // No waypoints (the ground ray missed, or a waypoint off cheap navmesh): retail destroys the area
                            // at once and offers nobody (`82E1D620`, area vfunc 6; b348).
                            if c.waypoints.is_empty() && data.nav.is_some() {
                                if let Some(held) = mind.log_repeats.allow(0x434f_4e57, tick) {
                                    info!("PED_CONVERSATION id={id} start ped=#{} with={partner} no waypoints (grounded={}): not spawned held={held} tick={tick}", ped.id.serial, center.is_some());
                                }
                                continue;
                            }
                            // The founders go through the area's offer test (`826A6CD8` -> manager slot 13 `826BFE18`, its
                            // result ignored): the starter, then the target (a ped only; a player has no participant).
                            use skate_core::living_world::peds::plugins::{conversation_qualifies, offer_conversation, OfferPed, RefusalMemory, TransferView, CONVERSATION_CLASS};
                            let rows = &data.conversations.rows;
                            let class = data.plugins.classes.get(CONVERSATION_CLASS);
                            let radius = class.and_then(|c| c.numbers.get("Hash_E2101F2B17A0E6B5").copied()).unwrap_or(20.0);
                            let transfer = class.and_then(|c| c.descriptor.as_ref()).and_then(|d| d.transfer.as_ref());
                            let odds = |e: &str| data.plugins.odds.get(e).map(Vec::as_slice).unwrap_or(&[]);
                            let pp = &mut *plugin_props;
                            let offer_rng = pp.rng.get_or_insert_with(|| skate_core::living_world::Rng::new(skate_core::living_world::rng::derive(world_seed, &[0x504c_5547])));
                            let my_view = TransferView::of(&mind.brain);
                            let mut starter = OfferPed { id: me, entity: &ped.entity, position: at, has_plugin: mind.brain.has_plugin || mind.prop.is_some(), odds: odds(&ped.entity), memory: &mut mind.refusals };
                            let mut q = |c: &skate_core::living_world::peds::conversation::Conversation, p: &OfferPed| conversation_qualifies(transfer, c, p.entity, my_view, rows);
                            let starter_joined = offer_conversation(&mut c, radius, &mut starter, &mut q, rows, &pp.settings, offer_rng);
                            let mut partner_joined = false;
                            if let (Some((kind, pos)), Some(&(_, in_plugin, view))) = (entities.get(&partner), busy_now.get(&partner)) {
                                // The partner's refusal memory is stored by its own think (`pending_refusals`); a new area is in
                                // nobody's memory yet.
                                let mut memory = RefusalMemory::default();
                                let mut other = OfferPed { id: partner, entity: kind, position: *pos, has_plugin: in_plugin, odds: odds(kind), memory: &mut memory };
                                let mut q = |c: &skate_core::living_world::peds::conversation::Conversation, p: &OfferPed| conversation_qualifies(transfer, c, p.entity, view, rows);
                                partner_joined = offer_conversation(&mut c, radius, &mut other, &mut q, rows, &pp.settings, offer_rng);
                                conversations.pending_refusals.extend(memory.entries.iter().map(|e| (partner, e.0)));
                            }
                            info!(
                                "PED_CONVERSATION id={id} start ped=#{} with={partner} joined={starter_joined}/{partner_joined} center=[{:.1}, {:.1}, {:.1}] grounded={} waypoints={} tick={tick}",
                                ped.id.serial, c.center[0], c.center[1], c.center[2], center.is_some(), c.waypoints.len()
                            );
                            conversations.map.insert(id, c);
                            if starter_joined {
                                mind.brain.plugin = Some(id);
                            }
                        }
                        continue;
                    }
                    ChaseRequest::LockWaypoint { at: w } => {
                        if let Some(c) = mind.brain.plugin.and_then(|id| conversations.map.get_mut(&id)) {
                            c.lock_closest_waypoint(me, w);
                        }
                        continue;
                    }
                    ChaseRequest::UnlockWaypoint => {
                        if let Some(c) = mind.brain.plugin.and_then(|id| conversations.map.get_mut(&id)) {
                            c.unlock_waypoint(me);
                        }
                        continue;
                    }
                    ChaseRequest::ExitPlugin if mind.prop.is_some() => {
                        if let Some((i, id, _)) = mind.prop.take() {
                            if let Some(p) = plugin_props.props.get_mut(i).filter(|p| p.id == id) {
                                p.release(me);
                                info!("PED_PLUGIN_PROP ped=#{} leaves {} {:016X} tick={tick}", ped.id.serial, p.class, id);
                            }
                        }
                        mind.brain.has_plugin = false;
                        mind.brain.release_plugin_waypoint();
                        mind.plugin_controller = None;
                        mind.plugin_state = None;
                        mind.plugin_graph = None;
                        continue;
                    }
                    ChaseRequest::ExitPlugin => {
                        if let Some(id) = mind.brain.plugin.take() {
                            use skate_core::living_world::peds::conversation::Left;
                            let left = conversations.map.get_mut(&id).map(|c| (c.is_complete(), c.leave(me)));
                            let how = match left {
                                Some((false, Left::Remaining)) => "abort",
                                Some((_, Left::Empty)) => "empty",
                                _ => "leave",
                            };
                            if matches!(left, Some((_, Left::Empty))) {
                                conversations.map.remove(&id);
                            }
                            info!("PED_CONVERSATION id={id} leave ped=#{} result={how} tick={tick}", ped.id.serial);
                        }
                        mind.brain.has_plugin = false;
                        mind.brain.release_plugin_waypoint();
                        continue;
                    }
                    ChaseRequest::SignalInPosition => {
                        if let Some(c) = mind.brain.plugin.and_then(|id| conversations.map.get_mut(&id)) {
                            let rows = &data.conversations.rows;
                            let before = c.state;
                            c.signal_in_position(me, rows, &mut conv_rand);
                            if before < 2 && c.state == 2 {
                                info!("PED_CONVERSATION id={} begins row={:?} value={:?} tick={tick}", c.id, c.row.and_then(|r| rows.get(r)).map(|r| r.name.as_str()), c.value);
                            }
                        }
                        continue;
                    }
                    ChaseRequest::PassTurn => {
                        if let Some(c) = mind.brain.plugin.and_then(|id| conversations.map.get_mut(&id)) {
                            c.pass_turn(&mut conv_rand);
                            info!("PED_CONVERSATION id={} turn state={} speaker={:?} tick={tick}", c.id, c.state, c.speaker());
                        }
                        continue;
                    }
                    ChaseRequest::Spoke => {
                        if let Some(c) = mind.brain.plugin.and_then(|id| conversations.map.get(&id)) {
                            listening.extend(c.listeners());
                        }
                        continue;
                    }
                    // TakedownTauntVictim (`826A70E0`): speech 65 when the victim is the player, else 19.
                    ChaseRequest::Taunt { target } => {
                        mind.brain.speech = Some(if target >= PLAYER_TARGET_BASE { 65 } else { 19 });
                        body.taunt = TauntClip::Requested;
                        info!("PED_TAUNT ped=#{} target={target} tick={tick}", ped.id.serial);
                        continue;
                    }
                    ChaseRequest::HandPropClip { clip } => {
                        body.plugin_motion.throw_clip = Some((clip, settings.ped_brain.values.hand_prop.clip_blend));
                        info!("PED_HAND_PROP ped=#{} throw clip {clip} target={:?} tick={tick}", ped.id.serial, mind.brain.hand_prop.throw.map(|t| t.target));
                        continue;
                    }
                    ChaseRequest::TazerClip { clip } => {
                        body.plugin_motion.tazer_clip = Some(clip);
                        continue;
                    }
                    ChaseRequest::GestureClip { clip } => {
                        body.plugin_motion.gesture_clip = Some(clip);
                        continue;
                    }
                    ChaseRequest::HandPropReleased { velocity } => {
                        mind.hand_prop_release = Some(velocity);
                        continue;
                    }
                    ChaseRequest::MoodReset { target } => {
                        mind.mood.forget(target);
                        continue;
                    }
                    ChaseRequest::GroupEnd { chasee, reason, notify } => {
                        let in_group = chase_groups.0.get_mut(&chasee).map(|g| g.end_reason = Some(reason)).is_some();
                        // `826A5318`: a chaser in a group ending it by aggressivecapture of the local player shows a
                        // CAUGHT line (chase manager +64 with (1, 0), b680).
                        if notify && in_group && reason == skate_core::living_world::peds::chase::reason::AGGRESSIVE_CAPTURE
                            && chasee == PLAYER_TARGET_BASE {
                            events.write(PedEvent::ChaseCaught { target: chasee });
                        }
                        (chasee, "group_end", Some(reason))
                    }
                };
                info!("PED_CHASE ped=#{} {kind} chasee={chasee} reason={reason:?} group={:?} tick={tick}", ped.id.serial, chase_groups.0.get(&chasee).map(|g| &g.chasers));
                events.write(PedEvent::Chase { id: ped.id, chasee, kind, reason });
            }
        }
        // Intercept: each tick a new goal from the solver (`826A3BF0`): the chasee must be
        // reachable (`sub_82C465A8`, our navmesh line) and the goal on the mesh (`sub_82C460E0`).
        // A locked ped drops its route (the plugin's waypoint walk is over).
        if mind.brain.position_locked && body.nav.route.is_some() {
            body.nav.set_route(None);
        }
        use skate_core::living_world::peds::brain::ChaseSteer;
        let one_point = |p: [f32; 3]| Some(skate_core::living_world::peds::PedRoute { points: vec![p], looped: false });
        match (mind.brain.motion_intent, mind.brain.chasee, data.nav.as_deref()) {
            // LostChasee: walk to the last known position (a one-point route).
            _ if mind.brain.search_point.is_some() => {
                let p = mind.brain.search_point.unwrap_or_default();
                if mind.chase_goal.take().is_some() || body.nav.route.as_ref().is_none_or(|r| r.points.first() != Some(&p)) {
                    body.nav.set_route(one_point(p));
                }
            }
            // ZombieFollow (`826A9250`): walk / run to the brain's goal (the player or a point of the ring round them).
            (Some(skate_core::living_world::peds::brain::motion::ZOMBIE_FOLLOW), ..) if mind.brain.zombie_goal.is_some() => {
                let p = mind.brain.zombie_goal.unwrap_or_default();
                mind.chase_goal = None;
                if body.nav.route.as_ref().is_none_or(|r| r.points.first() != Some(&p)) {
                    body.nav.set_route(one_point(p));
                }
            }
            // ApproachWantTarget (`8269FDF0`): walk to the want's target.
            _ if mind.brain.approach.is_some() => {
                let p = mind.brain.approach.map(|a| a.0).unwrap_or_default();
                mind.chase_goal = None;
                let d = [p[0] - at[0], p[2] - at[2]];
                let dist = (d[0] * d[0] + d[1] * d[1]).sqrt();
                match mind.brain.approach_slide {
                    // TargetWaypoint's slide (`slideDistance` / `slideSpeed`): the last stretch
                    // moves the body straight onto the point (ours: no slide clip).
                    Some((slide, speed)) if dist <= slide => {
                        body.nav.set_route(None);
                        mind.approach_route = false;
                        if dist > 1e-4 {
                            let step = (speed * dt).min(dist);
                            body.position.x += d[0] / dist * step;
                            body.position.z += d[1] / dist * step;
                        }
                    }
                    _ => {
                        body.nav.set_approach_route(p);
                        mind.approach_route = true;
                    }
                }
            }
            // PursueChasee (`826A43B0`): run to the formation point.
            (Some(skate_core::living_world::peds::brain::motion::INTERCEPT), Some(chasee), Some(_)) if mind.brain.chase_steer == Some(ChaseSteer::Pursue) => {
                if let (Some(q), Some(g)) = (target(chasee), chase_groups.0.get(&chasee)) {
                    let p = g.formation_point(me, q);
                    body.nav.set_route(one_point(p));
                    mind.chase_goal = Some(p);
                }
            }
            // BlockChasee (`826A40E8`): walk to the block point; at it the brain stands and faces.
            (Some(skate_core::living_world::peds::brain::motion::INTERCEPT), Some(_), Some(_)) if matches!(mind.brain.chase_steer, Some(ChaseSteer::Block { .. })) => {
                mind.chase_goal = None;
                match mind.brain.block_point.filter(|_| mind.brain.speed_suggestion != Some(0.0)) {
                    Some(p) => body.nav.set_route(one_point(p)),
                    None => body.nav.set_route(None),
                }
            }
            (Some(skate_core::living_world::peds::brain::motion::INTERCEPT), Some(chasee), Some(mesh)) => {
                use skate_core::living_world::peds::chase;
                // InterceptChasee Update 826A3BF0 (b306): destination each think: docs/hails-additions/26f-living-world-peds.md#intercept-chasee
                if let Some(q) = target(chasee) {
                    let reachable = !chase::reach_failed_at(mesh, at, q, settings.ped_brain.values.reach_snap_window);
                    let velocity = observers.observers.get(chasee.wrapping_sub(PLAYER_TARGET_BASE) as usize).map_or([0.0; 3], |o| o.velocity);
                    let speed = record.and_then(|r| r.run_speed()).unwrap_or(0.0);
                    // No record: the max lead time reads 0.0, so no intercept (the chasee itself is the destination).
                    let max_lead = record.map_or(0.0, |r| r.max_lead_time());
                    let intercept = reachable.then(|| chase::intercept(at, q, velocity, speed, max_lead, chase::predict_angle(data.chase_global.as_deref()))).flatten();
                    let destination = intercept.map_or(q, |i| mesh.locate(i.point).map_or(i.point, |p| p.position));
                    if let Some(i) = intercept.filter(|_| mind.chase_goal.is_none()) {
                        info!("PED_INTERCEPT ped=#{} chasee={chasee} goal=[{:.1}, {:.1}, {:.1}] leads={} t={:.2} tick={tick}", ped.id.serial, i.point[0], i.point[1], i.point[2], i.leads, i.time);
                    }
                    if mesh.locate(destination).is_some() {
                        body.nav.set_route(Some(skate_core::living_world::peds::PedRoute { points: vec![destination], looped: false }));
                    }
                    mind.chase_goal = Some(destination);
                }
                // CannotReachChaseTarget 82E31D58 (b108 / b302): reachability check every 10 mover updates: docs/hails-additions/26f-living-world-peds.md#cannot-reach-chase-target
                mind.reach_clock += dt;
                if let Some(q) = target(chasee).filter(|_| mind.reach_clock >= settings.ped_brain.values.reach_check_seconds) {
                    mind.reach_clock = 0.0;
                    let destination = mind.chase_goal.unwrap_or(q);
                    let failed = chase::reach_failed_at(mesh, at, destination, settings.ped_brain.values.reach_snap_window);
                    let near = (q[0] - at[0]).hypot(q[2] - at[2]) <= settings.ped_brain.values.unreachable_radius;
                    let was = mind.brain.target_unreachable;
                    chase::apply_reach_report(&mut mind.brain.target_unreachable, failed, near);
                    if mind.brain.target_unreachable != was {
                        info!(
                            "PED_UNREACHABLE ped=#{} chasee={chasee} {} near={near} dist={:.2} at=[{:.2}, {:.2}, {:.2}] chasee_at=[{:.2}, {:.2}, {:.2}] tick={tick}",
                            ped.id.serial, if was { "reachable again" } else { "cannot reach" }, (q[0] - at[0]).hypot(q[2] - at[2]), at[0], at[1], at[2], q[0], q[1], q[2],
                        );
                    }
                }
            }
            _ => {
                if ended_move_drops_route(&mut mind.chase_goal, &mut mind.approach_route) {
                    body.nav.set_route(None);
                }
            }
        }
        // Flee movement: 15 m legs away from the threat, a new one within 2 m of the goal; the
        // leg goes to the nav as a one-point route (cleared when the flee ends).
        // A channel stop the brain asked for (Scatter's Begin, b824).
        if let Some(fade) = mind.brain.channel_stop.take() {
            body.player.channel_stop(fade);
        }
        // Scatter (b645): the threat is the local player, whatever the flee want says, and legs end within 3 m.
        let scatter = mind.brain.scatter_threat;
        let mut flee = settings.ped_brain.flee;
        if scatter {
            flee.arrival_radius = settings.ped_brain.values.scatter_arrival_radius;
        }
        let threat_id = if scatter { (!players.is_empty()).then_some(PLAYER_TARGET_BASE) } else { mind.brain.flee_from };
        match (mind.brain.motion_intent, threat_id, data.nav.as_deref()) {
            (Some(skate_core::living_world::peds::brain::motion::FLEE), Some(threat), Some(mesh)) => {
                // A new leg on arrival (82E2BC70) or when the bot is done short of the goal (82E2CB08, b697).
                let bot_done = flee.releg_on_nav_done && body.nav.route_done;
                if mind.flee_goal.is_none_or(|g| bot_done || skate_core::living_world::peds::flee::arrived(at, g, &flee)) {
                    if let Some(threat_at) = target(threat) {
                        let velocity = velocity_of(threat).unwrap_or([0.0; 3]);
                        let mut goal = skate_core::living_world::peds::flee::goal(at, threat_at, velocity, &flee);
                        if let Some(here) = mesh.locate(at) {
                            goal = skate_core::living_world::peds::flee::goal_on_mesh(mesh, here.poly, goal);
                        }
                        body.nav.set_route(Some(skate_core::living_world::peds::PedRoute { points: vec![goal], looped: false }));
                        mind.flee_goal = Some(goal);
                        info!("PED_FLEE ped=#{} from={} scatter={scatter} goal=[{:.1}, {:.1}, {:.1}] tick={tick}", ped.id.serial, threat, goal[0], goal[1], goal[2]);
                    }
                }
            }
            // RunFromHonker (`826A1358`): every frame while the car exists, a goal 10 m sideways of its line; a
            // car that is gone leaves the last goal (retail keeps it).
            (Some(skate_core::living_world::peds::brain::motion::RUN_FROM_HONKER), _, _) => {
                if let Some((car_at, dir)) = mind.brain.honker.and_then(|c| car_pose.get(&c)) {
                    if let Some((goal, speed)) = skate_core::living_world::peds::honk::run_goal(at, *car_at, *dir, &settings.ped_brain.run_from_honker) {
                        if mind.flee_goal.is_none() {
                            info!("PED_HONKED ped=#{} car={:?} goal=[{:.1}, {:.1}, {:.1}] tick={tick}", ped.id.serial, mind.brain.honker, goal[0], goal[1], goal[2]);
                        }
                        body.nav.set_route(Some(skate_core::living_world::peds::PedRoute { points: vec![goal], looped: false }));
                        mind.flee_goal = Some(goal);
                        mind.brain.speed_suggestion = Some(speed);
                    }
                }
            }
            _ => {
                if mind.flee_goal.take().is_some() {
                    body.nav.set_route(None);
                }
            }
        }
        // Speech: the brain stores the value on the ped (`ped+2468`); PedestrianSpeech speaks when
        // it changes, so only a change goes to the audio side (a state re-sending its value stays
        // silent, as in retail).
        if mind.brain.speech != mind.spoken {
            mind.spoken = mind.brain.speech;
            // A conversation turn's variant and row value go with its line only.
            let topic = mind.brain.speech_topic.take();
            if let Some(value) = mind.brain.speech {
                let state = controller.frame.current.and_then(|s| graph.graph.binding.states.get(s)).map_or("none", |s| s.name.as_str());
                info!("PED_SPEECH ped=#{} value={value} topic={topic:?} state={state} tick={tick}", ped.id.serial);
                speech.write(crate::world_audio::PedSpeechEvent { ped: entity_id, value: crate::world_audio::SpeechValue(value), topic });
                events.write(PedEvent::Speech { id: ped.id, value, topic, state: state.to_string() });
            }
        }
        let state_now = controller.frame.current.and_then(|s| graph.graph.binding.states.get(s)).map_or("none", |s| s.name.as_str()).to_string();
        if controller.frame.current != mind.state {
            let name = |s: Option<usize>| s.and_then(|s| graph.graph.binding.states.get(s)).map_or("none", |s| s.name.as_str());
            let key = (1 << 63) | (mind.state.map_or(0, |s| s as u64 + 1) << 24) | controller.frame.current.map_or(0, |s| s as u64 + 1);
            if let Some(held) = mind.log_repeats.allow(key, tick) {
                let held = if held > 0 { format!(" (+{held} held back)") } else { String::new() };
                info!("PED_BRAIN ped=#{} {} -> {} intent={:?} wants={:?} tick={tick}{held}", ped.id.serial, name(mind.state), name(controller.frame.current), mind.brain.motion_intent, mind.brain.wants.keys().collect::<Vec<_>>());
            }
            // Why a new chase was refused (NewChasee/CancelChase: not CanNewChaseStart or not CanChaseeAddNewChaser).
            if name(controller.frame.current) == "CancelChase" {
                let chasee = mind.brain.wants.get("angrychase").map(|w| w.target);
                let group = chasee.and_then(|c| chase_groups.0.get(&c).map(|g| g.info(max_chasers(c))));
                if let Some(held) = mind.log_repeats.allow(0x4341_4e43, tick) {
                    info!(
                        "PED_CHASE_CANCEL ped=#{} chasee={chasee:?} chases_enabled={} group={:?} max_chasers={:?} tick={tick}{}",
                        ped.id.serial,
                        settings.ped_brain.values.chases_enabled,
                        group,
                        chasee.map(&max_chasers),
                        if held > 0 { format!(" (+{held} held back)") } else { String::new() }
                    );
                }
            }
            mind.state = controller.frame.current;
        }
        log_still_ped(ped, body, mind, &state_now, &conversations.map, tick, &mut still_budget);
    }
}

/// The mod-facing nav settings (`LivingWorldSettings::ped_nav`, mod `ped_nav`; reset when the mod is disabled) into the
/// resource the ped step reads.
pub(super) fn sync_nav_settings(settings: Res<super::LivingWorldSettings>, mut nav: ResMut<PedNavSettings>) {
    if settings.is_changed() && *nav != settings.ped_nav {
        *nav = settings.ped_nav.clone();
    }
}


#[cfg(test)]
mod stall_tests {
    use super::*;

    /// b649 (ask 391): a phone ped that drops its phone for a chase stops asking for the talk, and the unlink stops
    /// the channel once.
    #[test]
    fn a_dropped_phone_stops_the_talk_and_the_unlink_stops_the_channel_once() {
        use skate_core::living_world::peds::hand_prop::ActionCue;
        let talk = ActionCue::Clip("HandPropPhoneTalk");
        assert_eq!(cue_clip(talk, true, false, true, None), Some("HandPropPhoneTalk"), "held: the talk loops");
        assert_eq!(cue_clip(talk, false, false, true, None), None, "dropped: no talk");
        assert_eq!(cue_clip(talk, false, true, true, None), None, "dropped: not even a new cue");
        assert_eq!(cue_clip(ActionCue::Carry, true, true, false, Some("NPC_CARRY_x")), Some("NPC_CARRY_x"));
        let id = LivingWorldId { kind: Kind::Pedestrian, serial: 189 };
        let mut stopped = BTreeMap::new();
        assert!(!unlink_due(&mut stopped, id, 0), "never unlinked");
        assert!(stopped.is_empty());
        assert!(unlink_due(&mut stopped, id, 1), "the unlink stops the channel");
        assert!(!unlink_due(&mut stopped, id, 1), "once");
        assert!(unlink_due(&mut stopped, id, 2), "the next unlink stops it again");
        assert_eq!(skate_core::living_world::peds::hand_prop::HandPropSettings::default().unlink_fade, 0.2);
    }

    /// b511: a ped creeping 0.3 m over minutes never counted as still (0.1 m); the 0.5 m stall watch reports it after
    /// the report period, and a real step resets it.
    #[test]
    fn a_ped_walking_in_place_is_due_for_a_stall_report() {
        let (mut still, mut stall) = (StillWatch::default(), StillWatch::default());
        let mut due = (None, None);
        for tick in 0..=STILL_REPORT_TICKS {
            let at = [101.0 + 0.3 * (tick as f32 / STILL_REPORT_TICKS as f32), 64.4, -295.7];
            due = (watch_due(&mut still, at, STILL_RADIUS, tick), watch_due(&mut stall, at, STALL_RADIUS, tick));
        }
        assert_eq!(due, (None, Some(0)), "not still (0.3 m > 0.1 m), stalled since tick 0");
        assert_eq!(watch_due(&mut stall, [102.0, 64.4, -295.7], STALL_RADIUS, STILL_REPORT_TICKS + 1), None, "a step resets it");
    }
}