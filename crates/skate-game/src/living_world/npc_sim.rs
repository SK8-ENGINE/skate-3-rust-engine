//! Simulated NPC skaters: full physics skaters driven by their AI record, taken over from the replay tier.
//! Details: docs/hails-additions/26e-living-world-skater-ai.md#simulated-npc-skaters-model

use super::npc_skaters::{NpcReplay, NpcSkater};
use super::{LivingWorldObservers, LivingWorldSettings, NetRole, PopulationState};
use crate::physics::{GamePhysics, PlayerControls, SkaterPhysicsContext, SkaterRuntime};
use bevy::prelude::*;
use skate_core::living_world::ai_record;
use skate_core::living_world::replay::node_flags;
use skate_core::player::state::PhysicalStateId;

/// The simulated tier's rules (a mod may change them; `Default` = off, no distance limit, 3 skaters).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SimulatedTierSettings {
    pub enabled: bool,
    /// Simulated within this distance of an observer (m); handed back beyond 1.1 x. Retail: every distance (`f32::MAX`).
    pub radius: f32,
    /// At most this many simulated at once (retail keeps 3 ambient skaters).
    pub max: usize,
    /// Bail respawn delay of an ambient skater, seconds, and its clamp (retail
    /// `sub_8246EE30`: 5.0 `0x821F1790` for AI kind 0, clamped to 1.5 `0x822249B4` .. 7.9
    /// `0x822572EC`).
    pub respawn_seconds: f32,
    pub respawn_min: f32,
    pub respawn_max: f32,
    /// The PathController's ActionGraph signals (anticipation and trick dispatch, retail values).
    pub signals: skate_core::living_world::ai_signals::SignalSettings,
    /// Mode 7 (a prop blocks the line): step off and walk back to the line past it with controller B
    /// ("NavMeshController", retail on; a mod may turn it off) and its numbers, plus the sub-mode / reposition
    /// numbers of the line controller.
    pub walk_back: bool,
    /// Retail's path projection values (`skate_core::living_world::path_follow`, b110): the cursor follows the sim,
    /// on path drops beyond 1.0 m outside the lane (no recorded trick then) and rejoins within 0.5 m.
    pub path: skate_core::living_world::path_follow::PathTuning,
    pub navmesh: skate_core::living_world::controller_b::ControllerBSettings,
    pub controller: skate_core::living_world::ai_controller::ControllerSettings,
    /// The line controller's analog steer on the ground (`sub_8246AA00` / `sub_82471070`, b121; retail values).
    pub ground_steer: skate_core::living_world::ground_steer::GroundSteerTuning,
    /// ... and in the air (`sub_8246B358` / `sub_82471188`, b123; retail values).
    pub air_steer: skate_core::living_world::air_steer::AirSteerTuning,
    /// The air arm (retail on, every airborne tick; b616). Mod: `npc_simulated.air_steer*`; A/B knob
    /// `SKATE_NPC_AIR_STEER=0`.
    pub air_steer_on: bool,
    /// The takeoff board yaw (`sub_8246B1F8`, posted in the air arm; research b113): retail on, independent of the
    /// air steer switch (same arm, separate signals). Mod: `npc_simulated.board_yaw*`.
    pub board_yaw_on: bool,
    /// The player and simulated NPCs collide as full skaters (each one's own skeleton contact and wipeout check: a hard
    /// bump knocks a skater down; retail, research m7 / b125). Mod: `npc_simulated.bump`.
    pub bump: bool,
    pub board_yaw: skate_core::living_world::ai_signals::BoardYawTuning,
    /// The powerslide writer arm's values (`sub_824696B8` case 9, b676).
    pub powerslide: skate_core::living_world::ai_signals::PowerslideValues,
    /// After a reset placement (stage-6 stuck respawn) the line controller holds: no steer and no trick for this long,
    /// then the spawn push on its last tick (retail's hold counter `pc+888` = 30 from the reset `82468C10`, counted down
    /// in `824701F8`; b178). A switch from the replay tier holds one tick (engine handover, no visible stop).
    pub placement_hold_seconds: f32,
    /// The same hold after a bail respawn (retail 8 ticks, `8246EA90`).
    pub respawn_hold_seconds: f32,
    /// NPC skaters' reactions to wipeouts near them (slam / slam_b / crash speech; retail on, 15 m, research b166 /
    /// b168). Mod: `npc_simulated.reactions`, `npc_simulated.reaction_radius`.
    pub reactions: skate_core::living_world::npc_reactions::ReactionParams,
    /// The PathController's push below the line's speed (`sub_8246B4C8`, b341; retail values). Mod: `npc_simulated.push*`.
    pub push: skate_core::living_world::ai_push::PushSettings,
    /// ... and its brake far above it (`sub_8246B810`, b347; retail values). Mod: `npc_simulated.brake*`.
    pub brake: skate_core::living_world::ai_push::BrakeSettings,
}

impl Default for SimulatedTierSettings {
    fn default() -> Self {
        // `SKATE_NPC_SIM_RADIUS` (m): a test knob so an unattended run sees many switches to physics.
        let radius = std::env::var("SKATE_NPC_SIM_RADIUS").ok().and_then(|v| v.parse::<f32>().ok()).filter(|r| r.is_finite() && *r > 0.0).unwrap_or(f32::MAX);
        Self { enabled: std::env::var("SKATE_NPC_SIM").ok().as_deref() == Some("1"), radius, max: 3, respawn_seconds: 5.0, respawn_min: 1.5, respawn_max: 7.9, signals: Default::default(), walk_back: true, path: Default::default(), navmesh: Default::default(), controller: Default::default(), ground_steer: Default::default(), air_steer: Default::default(), air_steer_on: std::env::var("SKATE_NPC_AIR_STEER").ok().as_deref() != Some("0"), board_yaw_on: std::env::var("SKATE_NPC_BOARD_YAW").ok().as_deref() != Some("0"), board_yaw: Default::default(), powerslide: Default::default(), bump: std::env::var("SKATE_NPC_BUMP").ok().as_deref() != Some("0"), placement_hold_seconds: 0.5, respawn_hold_seconds: 8.0 / 60.0, reactions: Default::default(), push: Default::default(), brake: Default::default() }
    }
}

impl SimulatedTierSettings {
    /// The respawn delay in 60 Hz ticks.
    pub fn placement_hold_ticks(&self) -> u32 {
        (self.placement_hold_seconds.clamp(0.0, 10.0) * 60.0).round() as u32
    }

    pub fn respawn_hold_ticks(&self) -> u32 {
        (self.respawn_hold_seconds.clamp(0.0, 10.0) * 60.0).round() as u32
    }

    pub fn respawn_ticks(&self) -> u32 {
        let lo = self.respawn_min.min(self.respawn_max);
        (self.respawn_seconds.clamp(lo, self.respawn_max.max(lo)) * 60.0).round() as u32
    }
}

/// Wipeout bit of the flags word (flags2468 bit 18; 82DB6EC0 -> +59, 8246EF78 latch, 8246EE30 respawn).
pub(crate) const WIPEOUT_BIT: u32 = 0x0004_0000;

/// `NPC_SKATER_BACKWARDS` line for a simulated NPC, read from its body (b352): docs/hails-additions/26e-living-world-skater-ai.md#simulated-backwards-line
pub(crate) fn backwards_line(id: super::LivingWorldId, character: &str, sim: &NpcSim, line: Option<&skate_core::living_world::replay::ReplayLine>, cursor: &skate_core::living_world::replay::LineCursor) -> Option<String> {
    // Ground frames on the board only: air, off-board and wipeout frames are not a backwards ride (b616).
    let p = &sim.body.runtime.player_input.processed;
    let state = sim.body.runtime.player_state.current() as u32;
    let airborne = (200..300).contains(&state) && p.wheel_count_2556 == 0;
    if airborne || p.flags_2484 & 0x0800_0000 != 0 || p.flags_2480 & 0x4 != 0 || p.flags_2468 & WIPEOUT_BIT != 0 {
        return None;
    }
    let forward = GamePhysics::context_effective_forward(&sim.context);
    let velocity = GamePhysics::context_deck_velocity(&sim.context);
    let speed = (velocity[0] * velocity[0] + velocity[2] * velocity[2]).sqrt();
    let yaw = |v: [f32; 3]| v[0].atan2(v[2]);
    let off = (yaw(forward) - yaw(velocity) + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    if speed < 1. || off.abs() <= 135f32.to_radians() {
        return None;
    }
    let p = GamePhysics::context_deck(&sim.context).translation;
    let recorded_fakie = line.and_then(|l| l.nodes.get(cursor.node as usize)).map(|n| skate_core::living_world::replay::node_recorded_fakie(n, velocity));
    let line_id = cursor.line.iter().take(4).map(|b| format!("{b:02x}")).collect::<String>();
    Some(format!(
        "NPC_SKATER_BACKWARDS #{} {character} tier sim state {state} flip {} heading {:.0} velocity_yaw {:.0} off {:.0} deg {speed:.1} m/s riding_fakie {} line {line_id} node {} recorded_fakie {recorded_fakie:?} pos {:.1} {:.1} {:.1}",
        id.serial,
        sim.air.flip,
        yaw(forward).to_degrees(),
        yaw(velocity).to_degrees(),
        off.abs().to_degrees(),
        sim.body.runtime.animation.packet.riding_fakie,
        cursor.node,
        p.x,
        p.y,
        p.z,
    ))
}

/// A simulated NPC skater's own physics and runtime.
#[derive(Component)]
pub(crate) struct NpcSim {
    context: SkaterPhysicsContext,
    body: SimBody,
    /// Ticks left until the bail respawn (`pc+884` / `pc+892`), while bailing.
    respawn_in: Option<u32>,
    /// Ticks since the bail while `respawn_in` runs (the bail fade's clock).
    bail_ticks: u32,
    /// The bail fade's opacity (1 = solid); `advance` draws the NPC with it.
    pub(crate) bail_alpha: f32,
    /// The node the trick dispatcher handled last (`pc+820`).
    last_node: Option<u32>,
    /// The PathController posted its push last tick (`pc+926`; the log names a push's start once).
    pushing: bool,
    /// Controller B while it is the active controller (`brain+104`), with the line and node it walks back to.
    walker: Option<Walker>,
    /// Retail's path projection (cursor A / B) on the line it was started for.
    follow: Option<([u8; 16], skate_core::living_world::path_follow::PathFollow)>,
    /// Last tick's physical state and projected point (`pc+96`; NPC_SKATER_BAIL reports what the skater was doing).
    last_state: Option<PhysicalStateId>,
    last_deck: Option<[f32; 3]>,
    /// Placed again after a bail: the old wipeout bit is still set until the queued teleport resets the state, and
    /// counting it started a second 5 s bail right after every respawn (muted run 2026-10-13: 2 of 4 bails).
    placed: bool,
    /// The placing teleport is queued until the next physics step: until then the deck still reports where the body
    /// was before (a pooled body: anywhere), and projecting that moved the path cursor nodes ahead of the skater
    /// (muted run 2026-10-13: "left the path at node 1 (A 5+1.00, 79.25 m)" on the first tick).
    teleport_pending: bool,
    /// The skater's placement count when `place` queued the actor reply; the guard above lifts once it changes (the
    /// reply lands a tick or two later: lifting it after one tick projected the pooled body, 71 m off the line, and the
    /// skater was "lost" and walked off within 0.1 s, 2026-10-15). Ticks waited, for a logged limit.
    placements_at_place: u32,
    pending_ticks: u32,
    /// The line and node the bail places it at again (`pc+880`, chosen when the bail starts). The line is kept
    /// with the node: the cursor can chain to the next line during the respawn wait (a walk-back hand back at the
    /// line end), and the old line's node on the new line ran past its end (crash 2026-10-15, npc_avoid.rs:204).
    respawn_node: Option<([u8; 16], u32)>,
    /// Line controller A's off-board toggle (re-boards a skater left off its board, b117).
    toggle: skate_core::living_world::ai_controller::OffBoardToggle,
    /// Ticks left of the per-tick NPC_SKATER_PLACED trace after a placement (debug level; how the launch at line
    /// ends was found).
    trace_ticks: u32,
    /// Ground ticks left of NPC_SKATER_REJOIN after a line end chain (research b731).
    chain_trace: u32,
    /// Ticks left of NPC_TAKEOFF_TRACE after a trick dispatch (research b814: where the speed goes before a pop).
    takeoff_trace: u32,
    /// NPC_TAKEOFF already logged for this trick dispatch.
    takeoff_logged: bool,
    /// The last GroundAnimation launch speed since the dispatch (`82D93618`), for NPC_TAKEOFF.
    takeoff_launch: f32,
    /// `pc+944` / `pc+945` (centred, rejoining): written on ground ticks before the record (`sub_8246FA30` calls
    /// `sub_8246AA00` then `sub_8246DB50`), kept through the air.
    lane_flags: (bool, bool),
    /// The spawn push set by the replay -> sim handover (m/s), until the first measured speed after its teleport is
    /// logged (b174 4: does the handover keep its speed through the queued teleport?).
    handover_push: Option<f32>,
    /// The skater's placement count when the bail started: a higher count = the respawn teleport happened.
    placements_at_bail: u32,
    /// The last skater body its skeleton touched (the player or another NPC: `collision_feedback.other_skater`) and
    /// the steps since; NPC_SKATER_CONTACT logs each new contact, NPC_SKATER_BAIL names one within a second.
    skater_contact: Option<(u64, u32)>,
    /// The drawn pose and animation root before the last step, and the placement count then: the draw blends between
    /// them by the fixed-step overstep, as the player's presentation does (a simulated NPC jumped at 60 Hz before).
    pub(crate) previous_pose: Vec<skate_core::animation::output::NativeMatrix>,
    pub(crate) previous_root: skate_core::animation::output::NativeMatrix,
    pub(crate) previous_placements: u32,
    /// Ticks left of the hold after a placement (counted once the placement landed; the spawn push on the last one).
    hold_ticks: u32,
    /// Where the placed skater was when its hold started counting (the spawn push needs it still there).
    placed_at: Option<[f32; 3]>,
    /// The last walk-back goal with no navmesh plan (logged once per goal).
    no_plan: Option<u32>,
    /// NPC_SKATER_SPEED watch: the deck speed at the start of the window, the ticks in it and the ticks until the next
    /// line may print (laptop 2026-10-19: an NPC skater "gained a bunch of speed for no reason" uphill, b338).
    speed_watch: (f32, u32, u32),
    /// The takeoff flip latch (`pc+927` / `pc+928`, `sub_8246A700`), cleared with the controller.
    air: skate_core::living_world::air_steer::AirLatch,
    /// The PathController's trick slot (`pc+832` / `+844` / `+904`) and takeoff board yaw (`+856` / `+860` / `+930` /
    /// `+931`), research b113; cleared with the controller.
    slot: skate_core::living_world::ai_signals::TrickSlot,
    board_yaw: skate_core::living_world::ai_signals::BoardYaw,
    /// `pc+824`, the ground steer's last error; retail keeps it between steer ticks.
    ground_error: Option<f32>,
    /// Ticks since the last landing (`pc+900`, `sub_8246A700`).
    since_landing: u32,
    /// The body after the last step (board and skeleton), the proxy others collide with; `None` until the first step.
    last_body: Option<skate_net::packed::BodyState>,
    /// The full retail audio-state record of the last step (`skate_events::skater_audio_state`, what the player's
    /// own sound objects read; retail's bridge `sub_824B0DA8` fills an AI skater's record the same way) and its
    /// per-skater memory (air time, grind latch, rings, push plant).
    audio: Option<skate_audio::player::state::AudioState>,
    audio_memory: crate::game_audio::skate_events::SkaterAudioMemory,
}

/// Controller B's run: the line (`ctrl+592`) and node A re-attaches to when B hands back.
struct Walker {
    b: skate_core::living_world::controller_b::ControllerB,
    line: [u8; 16],
    node: u32,
}

/// Effective animation root (GetEffectiveRoot 82BE3650; what controller B reads, b80): docs/hails-additions/26e-living-world-skater-ai.md#effective-root-and-b-reads
fn effective_root(runtime: &SkaterRuntime) -> skate_core::animation::output::NativeMatrix {
    let mut root = runtime.animated_skeleton.roots.animation_to_world;
    if runtime.player_input.processed.flags_2476 & 4 != 0 {
        for axis in [0, 2] {
            root[axis] = root[axis].map(|v| -v);
        }
    }
    root
}

pub(crate) fn walker_input(runtime: &SkaterRuntime) -> skate_core::living_world::controller_b::BInput {
    // The skater, not the board (left lying once off): P = the animation root ([rec+20]+416), B+112 = its forward
    // row ([rec+20]+0 [inference: forward]) of the effective root.
    let root = effective_root(runtime);
    let position = [root[3][0], root[3][1], root[3][2]];
    let forward = [root[2][0], 0.0, root[2][2]];
    let p = &runtime.player_input.processed;
    let off_board = p.flags_2484 & 0x0800_0000 != 0;
    let toggling = p.flags_2480 & 0x4 != 0;
    skate_core::living_world::controller_b::BInput {
        position,
        body: forward,
        facing: forward[0].atan2(forward[2]),
        abort_flag: false,
        state_hand_back: p.flags_2468 & WIPEOUT_BIT != 0,
        online: false,
        can_move: off_board && !toggling,
        byte_161: off_board,
        byte_160: toggling,
    }
}

/// Per-line state the path reset 82468AC8 clears (also the 82467FC8 projection point): docs/hails-additions/26e-living-world-skater-ai.md#clear-line-state
fn clear_line_state(sim: &mut NpcSim) {
    sim.air = Default::default();
    sim.slot = Default::default();
    sim.board_yaw = Default::default();
    sim.ground_error = None;
}

/// The ground steer's far aim; while avoid-steering it moves into the gap (`8246D560` -> `82464E30`, b612).
fn avoid_far(line: &skate_core::living_world::replay::ReplayLine, a: (u32, f32), far: (u32, f32), av: &skate_core::living_world::avoid::AvoidOutput, t: &skate_core::living_world::ground_steer::GroundSteerTuning) -> [f32; 3] {
    use skate_core::living_world::{avoid, path_follow};
    let target = (av.mode == avoid::AvoidMode::Steer).then_some(av.steer_target).flatten().and_then(|id| av.entries.iter().find(|e| e.id == id)).and_then(|e| e.path);
    match target {
        Some(e) => avoid::avoid_aim(&path_follow::path_point(line, far), &path_follow::path_point(line, path_follow::advance(line, a, t.avoid_anchor_ticks)), &e, av.lateral).point,
        None => path_follow::point(line, far),
    }
}

fn projected_point(sim: &NpcSim, state: PhysicalStateId) -> ([f32; 3], [f32; 3]) {
    let wiped = sim.body.runtime.player_input.processed.flags_2468 & WIPEOUT_BIT != 0;
    if state == PhysicalStateId::BipedGround || wiped {
        let root = effective_root(&sim.body.runtime);
        ([root[3][0], root[3][1], root[3][2]], [root[2][0], root[2][1], root[2][2]])
    } else {
        // On the board retail projects `Reckoning+144`, the deck rigid body's position (its centre of mass), not the
        // deck part's origin (b674).
        (GamePhysics::context_deck_position(&sim.context), effective_deck_forward(sim))
    }
}

/// Effective deck forward (GetEffectiveTransform 82C01BF8, flipped by Processed+2468 bit 20; b116 / b292).
fn effective_deck_forward(sim: &NpcSim) -> [f32; 3] {
    let c = GamePhysics::context_deck(&sim.context).basis.columns[2];
    let sign = if sim.body.runtime.player_input.processed.flags_2468 & 0x0010_0000 != 0 { -1.0 } else { 1.0 };
    [c[0] * sign, c[1] * sign, c[2] * sign]
}

/// A loaded skater (runtime, controls, camera): about 220 ms to load from disk (session 2026-10-13: 35 of 39
/// switches to physics hitched over 150 ms), so loaded ones are kept in `NpcSimPool` and placed again with the
/// player's teleport (`SkaterRuntime::travel`), as retail places its AI skater object again on a respawn.
pub(crate) struct SimBody {
    runtime: Box<SkaterRuntime>,
    controls: Box<PlayerControls>,
    camera: Box<crate::camera::CameraRuntime>,
}

/// Loaded skaters waiting for a simulated NPC; filled up to the sim limit on the first enabled tick of each map.
#[derive(Resource, Default)]
pub(crate) struct NpcSimPool {
    /// With each its own physics context: its clock keeps running, so the camera's subject ticks stay monotonic.
    bodies: Vec<(SkaterPhysicsContext, SimBody)>,
    /// The `CurrentMap` generation the bodies were loaded for (b216): a body holds its map's grind world and skater
    /// settings, so a map change drops them and loads new ones, as the player is loaded again per world.
    prewarmed_for: Option<u64>,
    /// The animation banks and decoded poses the first loaded body read from disk, shared by every later body.
    source: Option<std::sync::Arc<crate::skater_animation::AnimationSource>>,
    /// The first enabled tick's load, on its own thread (b215: 0.5 to 0.8 s on the game thread in the first frame of
    /// play); joined before anything takes from or adds to `bodies`, so their order is the one a load here gives.
    loading: Option<std::thread::JoinHandle<PoolLoad>>,
    /// `simulate` calls so far, and per NPC entity the call before which it may not switch to physics again after a
    /// physics error (laptop 2026-11-08: two racers re-switched and failed every tick, 1900 errors, a 1.5 s frame).
    calls: u64,
    retry_after: std::collections::HashMap<Entity, u64>,
}

/// Fixed ticks an NPC stays in the replay tier after a physics error before it may switch again.
const RETRY_AFTER_ERROR_TICKS: u64 = 120;

/// What the pool loader thread hands back: the bodies in load order, the shared source, and its log lines.
struct PoolLoad {
    bodies: Vec<(SkaterPhysicsContext, SimBody)>,
    source: Option<std::sync::Arc<crate::skater_animation::AnimationSource>>,
    error: Option<String>,
    loads_ms: Vec<(f64, bool)>,
    started: std::time::Instant,
}

impl NpcSimPool {
    /// Take in the loader thread's bodies (blocking until it is done), logging what it loaded.
    fn finish_load(&mut self) {
        let Some(job) = self.loading.take() else { return };
        let load = match job.join() {
            Ok(load) => load,
            Err(_) => {
                warn!("NPC_SKATER_SIM pool: the loader thread panicked; skaters load on demand");
                return;
            }
        };
        for (ms, shared) in &load.loads_ms {
            info!("NPC_SKATER_SIM_LOAD loaded a skater in {ms:.1} ms (shared animation source: {shared})");
        }
        if let Some(error) = &load.error {
            warn!("NPC_SKATER_SIM pool: cannot load a skater: {error}");
        }
        self.bodies.extend(load.bodies);
        if self.source.is_none() {
            self.source = load.source;
        }
        info!("NPC_SKATER_SIM pool: {} loaded skaters ready ({:.0} ms, on the loader thread)", self.bodies.len(), load.started.elapsed().as_secs_f64() * 1000.0);
    }

    /// `finish_load` once the loader thread is done, without waiting for it.
    fn poll_load(&mut self) {
        if self.loading.as_ref().is_some_and(|job| job.is_finished()) {
            self.finish_load();
        }
    }
}

fn load_body(
    physics: &mut GamePhysics,
    config: &crate::config::Config,
    graphs: &crate::graph_runtime::StockGraphs,
    spawn: skate_core::physics::drive_frames::RetailAffineTransform,
    source: &mut Option<std::sync::Arc<crate::skater_animation::AnimationSource>>,
) -> Result<(SkaterPhysicsContext, SimBody), String> {
    let t0 = std::time::Instant::now();
    let shared = source.is_some();
    let pooled = load_body_from(physics, &config.asset_root, graphs, config.difficulty.profile_key(), spawn, source)?;
    info!("NPC_SKATER_SIM_LOAD loaded a skater in {:.1} ms (shared animation source: {shared})", t0.elapsed().as_secs_f64() * 1000.0);
    Ok(pooled)
}

/// `load_body` without the log line, from plain inputs (the pool loader thread has no `Config`).
fn load_body_from(
    physics: &mut GamePhysics,
    asset_root: &std::path::Path,
    graphs: &crate::graph_runtime::StockGraphs,
    difficulty: &str,
    spawn: skate_core::physics::drive_frames::RetailAffineTransform,
    source: &mut Option<std::sync::Arc<crate::skater_animation::AnimationSource>>,
) -> Result<(SkaterPhysicsContext, SimBody), String> {
    let mut context = physics.new_skater_context(spawn)?;
    let runtime = Box::new(physics.load_skater_in_context(&mut context, asset_root, graphs, difficulty, source.clone())?);
    source.get_or_insert_with(|| runtime.animation.source.clone());
    let controls = Box::new(PlayerControls::load(asset_root)?);
    let camera = Box::new(crate::camera::CameraRuntime::load(asset_root)?);
    Ok((context, SimBody { runtime, controls, camera }))
}

/// The line frame as the teleport matrix ([right, up, forward, translation], as `modding::session::spawn_matrix`).
fn spawn_matrix(spawn: &skate_core::physics::drive_frames::RetailAffineTransform) -> [[f32; 4]; 4] {
    let c = spawn.basis.columns;
    let t = spawn.translation;
    [[c[0][0], c[0][1], c[0][2], 0.], [c[1][0], c[1][1], c[1][2], 0.], [c[2][0], c[2][1], c[2][2], 0.], [t.x, t.y, t.z, 0.]]
}

/// Move a simulated skater's loaded body into the pool (its entity goes back to replay or is despawned).
pub(crate) fn pool_from(world: &mut World, e: Entity) {
    let body = world.get_entity_mut(e).ok().and_then(|mut entity| entity.take::<NpcSim>()).map(|sim| (sim.context, sim.body));
    if let (Some(body), Some(mut pool)) = (body, world.get_resource_mut::<NpcSimPool>()) {
        pool.finish_load();
        pool.bodies.push(body);
    }
}

impl NpcSim {
    /// The body after the last step (`None` before the first one).
    pub(crate) fn last_body(&self) -> Option<&skate_net::packed::BodyState> {
        self.last_body.as_ref()
    }

    /// The traffic car this simulated skater holds in state 104 (the car side's held bit, b57 / b65).
    pub(crate) fn held_car(&self) -> Option<u32> {
        (self.body.runtime.player_state.current() == skate_core::player::state::PhysicalStateId::Skitching).then(|| self.body.runtime.skitch_state.held_car()).flatten()
    }

    pub(crate) fn render_pose(&self) -> &[skate_core::animation::output::NativeMatrix] {
        &self.body.runtime.render_pose
    }

    /// The skater's animation root in the world (the frame its render pose is relative to).
    /// The deck's world position (the simulated board).
    pub(crate) fn deck_position(&self) -> [f32; 3] {
        let t = GamePhysics::context_deck(&self.context).translation;
        [t.x, t.y, t.z]
    }

    /// The full audio-state record of the last step, when it has stepped since its placement.
    pub(crate) fn audio_state(&self) -> Option<&skate_audio::player::state::AudioState> {
        self.audio.as_ref()
    }

    /// The NPC skater audio state from the simulated body: rolling on its wheels, grinding only in a grind state,
    /// airborne in an air state with no wheel down, silent wheels off the board and in a wipeout.
    pub(crate) fn lite_state(&self, material: u32) -> skate_audio::player::state::AudioState {
        let p = &self.body.runtime.player_input.processed;
        let state = self.body.runtime.player_state.current() as u32;
        let wheels_down = p.wheel_count_2556 > 0;
        let off_board = p.flags_2484 & 0x0800_0000 != 0 || (300..400).contains(&state) || (500..600).contains(&state);
        let grinding = (400..500).contains(&state);
        let airborne = (200..300).contains(&state) && !wheels_down;
        let deck = GamePhysics::context_deck(&self.context);
        let v = GamePhysics::context_deck_velocity(&self.context);
        let forward = deck.basis.columns[2];
        skate_audio::player::state::AudioState::rolling(&skate_audio::player::state::LiteSkater {
            position: [deck.translation.x, deck.translation.y, deck.translation.z],
            velocity: v,
            heading: forward[0].atan2(forward[2]),
            wheels: if off_board || airborne { [false; 4] } else { [wheels_down; 4] },
            material: if off_board { skate_audio::player::state::NO_MATERIAL } else { material },
            grinding,
            grind_material: if grinding { material } else { skate_audio::player::state::NO_MATERIAL },
            airborne,
            air_time: if airborne { self.body.runtime.player_state.selector.air_frames.max(0) as f32 / 60.0 } else { 0.0 },
            dt: (1.0 / skate_core::living_world::clock::RETAIL_TICK_HZ) as f32,
        })
    }

    /// The skater's placement count (a change means it was teleported).
    pub(crate) fn placements(&self) -> u32 {
        self.body.runtime.placements()
    }

    pub(crate) fn animation_root(&self) -> skate_core::animation::output::NativeMatrix {
        self.body.runtime.animated_skeleton.roots.animation_to_world
    }
}

fn nearest(observers: &LivingWorldObservers, p: [f32; 3]) -> f32 {
    observers.observers.iter().map(|o| ((o.position[0] - p[0]).powi(2) + (o.position[2] - p[2]).powi(2)).sqrt()).fold(f32::INFINITY, f32::min)
}

fn spawn_sim(
    physics: &mut GamePhysics,
    config: &crate::config::Config,
    graphs: &crate::graph_runtime::StockGraphs,
    target: &ai_record::LineTarget,
    pool: &mut NpcSimPool,
    hold_ticks: u32,
    fit_to_ground: bool,
) -> Result<NpcSim, String> {
    let spawn = line_spawn(target);
    pool.finish_load();
    let (context, body) = match pool.bodies.pop() {
        // A new actor on a pooled body: the per-actor parts a fresh body would have, and no record from the last owner
        // (b295 / b296 / b299; the placement's teleport reset and state Exit / Enter do the rest).
        Some((mut context, mut body)) => {
            GamePhysics::abandon_context_queries(&mut context);
            body.runtime.reset_for_new_actor(physics.collections())?;
            (context, body)
        }
        None => load_body(physics, config, graphs, spawn, &mut pool.source)?,
    };
    let mut sim = NpcSim { context, body, respawn_in: None, bail_ticks: 0, bail_alpha: 1.0, last_node: None, pushing: false, walker: None, follow: None, last_state: None, last_deck: None, placed: false, teleport_pending: false, placements_at_place: 0, pending_ticks: 0, respawn_node: None, toggle: Default::default(), trace_ticks: 0, chain_trace: 0, takeoff_trace: 0, takeoff_logged: false, takeoff_launch: f32::NAN, lane_flags: (false, false), handover_push: None, previous_pose: Vec::new(), previous_root: skate_core::physics::skeleton_animation_record::IDENTITY, previous_placements: 0, hold_ticks: 0, placed_at: None, placements_at_bail: 0, skater_contact: None, no_plan: None, air: Default::default(), slot: Default::default(), board_yaw: Default::default(), ground_error: None, since_landing: 7200, last_body: None, audio: None, audio_memory: Default::default(), speed_watch: (0.0, 0, 0) };
    let v = place(&mut sim, target, true, hold_ticks)?;
    if fit_to_ground {
        sim.body.runtime.fit_next_arrival();
    }
    sim.handover_push = Some((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt());
    Ok(sim)
}

/// Hold ticks at a switch from the replay tier (retail places with a 30-tick hold; one tick keeps its order).
const HANDOVER_HOLD_TICKS: u32 = 1;

fn line_spawn(target: &ai_record::LineTarget) -> skate_core::physics::drive_frames::RetailAffineTransform {
    let q = target.frame;
    let basis = skate_core::physics::rigid_body::basis_from_quaternion(skate_core::physics::rigid_body::RetailQuaternion { x: q[0], y: q[1], z: q[2], w: q[3] });
    skate_core::physics::drive_frames::RetailAffineTransform {
        basis,
        translation: skate_core::math::Vector3::new(target.position[0], target.position[1], target.position[2]),
    }
}

/// Place a simulated skater on its line with the teleport and the spawn push as velocity (engine handover, b109).
/// Retail's placement (`8246E558`) teleports on board unless the node is an off-board one (node bit 3).
fn on_board_at(lines: &dyn skate_core::living_world::replay::LineSource, cursor: &skate_core::living_world::replay::LineCursor) -> bool {
    lines.line(&cursor.line).and_then(|l| l.nodes.get(cursor.node as usize)).is_none_or(|n| n.flags & node_flags::OFF_BOARD == 0)
}

fn place(sim: &mut NpcSim, target: &ai_record::LineTarget, on_board: bool, hold_ticks: u32) -> Result<[f32; 3], String> {
    let spawn = line_spawn(target);
    let c = spawn.basis.columns;
    info!("NPC_SKATER_PLACE_FRAME up=[{:.2}, {:.2}, {:.2}] forward=[{:.2}, {:.2}, {:.2}] at [{:.2}, {:.2}, {:.2}]", c[1][0], c[1][1], c[1][2], c[2][0], c[2][1], c[2][2], spawn.translation.x, spawn.translation.y, spawn.translation.z);
    // Placed at rest: retail's hold branch (`824701F8`) sends a zero-velocity record and pushes on the hold's last tick.
    // The returned velocity is the push that tick will give (for the logs).
    let velocity = ai_record::spawn_push(target.position, target.position, target.step, false, false).unwrap_or([0.0; 3]);
    GamePhysics::set_context_velocity(&mut sim.context, [0.0; 3]);
    // One reset, the actor reply (retail `825926F8`), like the respawn. `travel` queued the host teleport too, a
    // second reset that snapped the rolling board back (2026-10-15); the host teleport alone moved the board but not
    // the animated skeleton's root, so the skater was drawn where its pooled body was and vanished on screen.
    sim.placements_at_place = sim.body.runtime.placements();
    sim.pending_ticks = 0;
    sim.body.runtime.place_by_reply(spawn_matrix(&spawn), on_board);
    sim.body.runtime.clear_respawn_destination();
    settle(sim, hold_ticks);
    sim.teleport_pending = true;
    Ok(velocity)
}

/// Ticks of NPC_SKATER_PLACED / NPC_SKATER_STEER debug trace after a placement: 60, or `SKATE_NPC_TRACE_TICKS` for a
/// longer look at a stall (with `RUST_LOG=info,skate3rust::living_world::npc_sim=debug`).
fn trace_ticks() -> u32 {
    static TICKS: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *TICKS.get_or_init(|| std::env::var("SKATE_NPC_TRACE_TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(60))
}

/// Lift the placement guard once the actor reply has landed (or after a logged 30-tick limit).
fn lift_pending(sim: &mut NpcSim, npc: &super::npc_skaters::NpcSkater) {
    if !sim.teleport_pending {
        return;
    }
    sim.pending_ticks += 1;
    if sim.body.runtime.placements() != sim.placements_at_place {
        sim.teleport_pending = false;
        sim.last_deck = None;
    } else if sim.pending_ticks >= 30 {
        warn!("NPC_SKATER_SIM #{} {}: placement did not land in 30 ticks", npc.id.serial, npc.character);
        sim.teleport_pending = false;
    }
}

/// The line-state reset after a placement (the teleport itself is done elsewhere).
fn settle(sim: &mut NpcSim, hold_ticks: u32) {
    sim.respawn_in = None;
    sim.last_node = None;
    sim.walker = None;
    sim.follow = None;
    sim.placed = true;
    sim.toggle = Default::default();
    sim.last_deck = None;
    sim.trace_ticks = trace_ticks();
    sim.slot = Default::default();
    sim.board_yaw = Default::default();
    sim.ground_error = None;
    sim.hold_ticks = hold_ticks;
    sim.placed_at = None;
    sim.air = Default::default();
    sim.audio = None;
    sim.audio_memory = Default::default();
}

/// A host restart of the cursor (re-place, re-snap, bail respawn, walk-back hand back): the frame clock keeps running
/// and the restart is recorded with the branches, so a client restarts its mirrored cursor the same way (b711 O1):
/// docs/hails-additions/26e-living-world-skater-ai.md#line-end-trigger
fn restart_cursor(replay: &mut super::npc_skaters::NpcReplay, lines: &dyn skate_core::living_world::replay::LineSource, line: [u8; 16], node: u32) {
    let mut out = Vec::new();
    replay.cursor.respawn(lines, line, node, &mut out);
    replay.branches.extend(out.into_iter().filter_map(|e| match e {
        skate_core::living_world::replay::CursorEvent::Branch(r) => Some(r),
        _ => None,
    }));
}

/// A racer's line end that chained beyond the search radius: the nearest line starts (id, distance, height step), to
/// see whether the race set lacks the line that carries the course on (docs/hails-additions/42-challenges.md).
fn log_far_chain(lines: &dyn skate_core::living_world::replay::LineSource, npc: &super::npc_skaters::NpcSkater, at: [f32; 3], to: &[u8; 16], radius: f32) {
    let d = |p: [f32; 3]| ((p[0] - at[0]).powi(2) + (p[1] - at[1]).powi(2) + (p[2] - at[2]).powi(2)).sqrt();
    let chosen = lines.line(to).and_then(|l| l.nodes.first()).map_or(f32::NAN, |n| d(n.position));
    if !(chosen > radius) {
        return;
    }
    let mut starts: Vec<(f32, f32, String)> = Vec::new();
    lines.for_each_line(&mut |l| {
        if let Some(n) = l.nodes.first() {
            starts.push((d(n.position), n.position[1] - at[1], l.id.iter().map(|b| format!("{b:02x}")).collect()));
        }
    });
    starts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let near: Vec<String> = starts.iter().take(4).map(|(dist, dy, id)| format!("{id} {dist:.1} m dy {dy:+.1}")).collect();
    info!("NPC_SKATER_CHAIN_FAR #{} {} chained {chosen:.1} m (radius {radius:.1}) from [{:.1}, {:.1}, {:.1}]; nearest starts: {}", npc.id.serial, npc.character, at[0], at[1], at[2], near.join(", "));
}

/// Buffers the simulated skaters' line end reuses across ticks ([`simulate`]).
#[derive(Default)]
pub(crate) struct ChainScratch {
    players: Vec<[f32; 3]>,
    order: Vec<(super::LivingWorldId, [u8; 16], u32)>,
    others: Vec<([u8; 16], u32)>,
    in_use: Vec<[u8; 16]>,
    out: Vec<skate_core::living_world::replay::CursorEvent>,
}

/// One simulated NPC's step this tick, run after the per-NPC preparation (`run_steps`).
struct Job {
    entity: Entity,
    serial: u32,
    character: String,
    intents: Vec<(String, f32)>,
}

/// The simulated NPCs' physics steps, one worker each, synced after the player's step; results in id order.
#[allow(clippy::too_many_arguments)]
/// NPC_SKATER_CONTACT: a simulated NPC's skeleton meets another skater's body (the player or an NPC) in its own solve;
/// logged once per contact, with the skater force its wipeout check reads (`maximum_skater_force`), the NPC's speed,
/// and `team` = hit by the local player (the bumped skater's 8290 team line, `collide_skater_local`).
fn log_skater_contact(job: &Job, s: &mut NpcSim) {
    let fb = &s.body.runtime.collision_feedback;
    s.skater_contact = match (fb.other_skater, s.skater_contact) {
        (Some(other), last) => {
            // A new contact: another skater, or the same one after half a second apart (one-step gaps are the same touch).
            if last.is_none_or(|(o, age)| o != other || age as f64 > 0.5 * skate_core::living_world::clock::RETAIL_TICK_HZ) {
                let p = &s.body.runtime.player_input.processed;
                let at = s.last_deck.unwrap_or_default();
                info!(
                    "NPC_SKATER_CONTACT #{} {} with={} force={:.2} speed={:.2} team={} wiped={} at=[{:.1}, {:.1}, {:.1}]",
                    job.serial, job.character, super::npc_skaters::peer_name(other), fb.maximum_skater_force, p.scalar_2652,
                    other == super::npc_skaters::PLAYER_PEER, p.flags_2468 & WIPEOUT_BIT != 0, at[0], at[1], at[2]
                );
            }
            Some((other, 0))
        }
        (None, Some((o, age))) => Some((o, age.saturating_add(1))),
        (None, None) => None,
    };
}

fn run_steps(
    physics: &mut GamePhysics,
    npcs: &mut Query<(Entity, &NpcSkater, &mut NpcReplay, Option<&mut NpcSim>, Option<&super::npc_skaters::NpcRaceClock>)>,
    bump: Option<(&skate_net::packed::BodyState, &crate::physics::network::Schema)>,
    graphs: &crate::graph_runtime::StockGraphs,
    workers: &mut Vec<Box<GamePhysics>>,
    jobs: Vec<Job>,
    commands: &mut Commands,
    count: &mut usize,
) {
    if jobs.is_empty() {
        return;
    }
    while workers.len() < jobs.len() {
        match physics.npc_worker() {
            Ok(mut worker) => {
                // Shares only while it steps (sync below, release after): the player's step changes the shared parts.
                GamePhysics::release_npc_worker(&mut worker);
                workers.push(Box::new(worker));
            }
            Err(error) => {
                warn!("NPC_SKATER_SIM worker: {error}");
                return;
            }
        }
    }
    let t_sync = std::time::Instant::now();
    // Every simulated NPC's body from its last step (the others' proxies in each solve), in id order.
    let mut bodies: Vec<(super::LivingWorldId, Entity, skate_net::packed::BodyState)> =
        if bump.is_some() { npcs.iter().filter_map(|(e, n, _, sim, _)| sim.and_then(|s| s.last_body.clone()).map(|b| (n.id, e, b))).collect() } else { Vec::new() };
    bodies.sort_by_key(|x| x.0);
    let mut work: Vec<(&Job, Mut<NpcSim>, &mut Box<GamePhysics>)> = Vec::with_capacity(jobs.len());
    {
        let mut by_entity: std::collections::BTreeMap<Entity, Mut<NpcSim>> = npcs
            .iter_mut()
            .filter(|n| jobs.iter().any(|j| j.entity == n.0))
            .filter_map(|(e, _, _, sim, _)| sim.map(|sim| (e, sim)))
            .collect();
        for (job, worker) in jobs.iter().zip(workers.iter_mut()) {
            if let Some(mut sim) = by_entity.remove(&job.entity) {
                let sim_ref = &mut *sim;
                sim_ref.previous_pose.clone_from(&sim_ref.body.runtime.render_pose);
                sim_ref.previous_root = sim_ref.body.runtime.animated_skeleton.roots.animation_to_world;
                sim_ref.previous_placements = sim_ref.body.runtime.placements();
                physics.sync_npc_worker(worker);
                // Its solve: no copy of itself; the player and the other simulated NPCs instead (each as a peer body).
                if let (Some(mark), Some((player, schema)), Some(at)) = (physics.npc_sim_proxy_mark, bump, skate_net::prediction::CollisionPrediction::at(0.0)) {
                    let mut proxies = std::mem::take(&mut worker.network_proxies);
                    proxies.truncate(mark);
                    proxies.append(super::npc_skaters::PLAYER_PEER, player, schema, worker, &sim_ref.body.runtime, at);
                    for (id, _, b) in bodies.iter().filter(|x| x.1 != job.entity) {
                        proxies.append(super::npc_skaters::sim_peer(*id), b, schema, worker, &sim_ref.body.runtime, at);
                    }
                    worker.network_proxies = proxies;
                }
                work.push((job, sim, worker));
            }
        }
    }
    let sync_us = t_sync.elapsed().as_micros();
    let t_steps = std::time::Instant::now();
    let threads: std::sync::Mutex<Vec<std::thread::ThreadId>> = std::sync::Mutex::new(Vec::new());
    let threads_ref = &threads;
    let results: Vec<Result<(), String>> = bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default).scope(|scope| {
        for (job, sim, worker) in work.iter_mut() {
            let sim = &mut **sim;
            let worker = &mut ***worker;
            let intents = &job.intents;
            scope.spawn(async move {
                if let Ok(mut t) = threads_ref.lock() {
                    t.push(std::thread::current().id());
                }
                let NpcSim { context, body, audio, audio_memory, .. } = sim;
                crate::physics::set_log_npc(job.serial);
                let stepped = worker.advance_npc_skater(context, &mut body.runtime, &mut body.controls, graphs, &mut body.camera, intents);
                if stepped.is_ok() {
                    // The step's audio record, read with this NPC's own board and riding state swapped in.
                    worker.swap_skater_context(context);
                    let dt = (1.0 / skate_core::living_world::clock::RETAIL_TICK_HZ) as f32;
                    *audio = Some(crate::game_audio::skate_events::skater_audio_state(worker, &body.runtime, audio_memory, dt));
                    worker.swap_skater_context(context);
                }
                stepped
            });
        }
    });
    let steps_us = t_steps.elapsed().as_micros();
    let t_release = std::time::Instant::now();
    // In NPC id order: the shared proxies end as the last NPC's solve left them; errors drop that NPC to replay.
    if let Some((_, _, last)) = work.last() {
        physics.network_proxies.clone_from(&last.network_proxies);
        physics.network_contacts = last.network_contacts;
        // Not the player's own body (added for that solve only).
        if let Some(mark) = physics.npc_sim_proxy_mark {
            physics.network_proxies.truncate(mark);
        }
    }
    for (job, sim, _) in work.iter_mut() {
        let s = &mut **sim;
        if bump.is_some() {
            s.last_body = Some(GamePhysics::capture_context_body(&s.context, &s.body.runtime));
        }
        log_skater_contact(job, s);
    }
    for ((job, _, worker), result) in work.iter_mut().zip(results) {
        GamePhysics::release_npc_worker(worker);
        if let Err(error) = result {
            warn!("NPC_SKATER_SIM #{} {}: physics error, back to replay: {error}", job.serial, job.character);
            // Back to the pool like a hand-back (the next placement resets it as a fresh body, b299). Dropping it made
            // the next switch load a new body on the game thread: ~200 ms hitches (laptop 2026-10-19, KnownAir errors).
            let entity = job.entity;
            commands.queue(move |world: &mut World| {
                pool_from(world, entity);
                if let Some(mut pool) = world.get_resource_mut::<NpcSimPool>() {
                    let until = pool.calls + RETRY_AFTER_ERROR_TICKS;
                    pool.retry_after.insert(entity, until);
                }
            });
            *count -= 1;
        }
    }
    // NPC_SIM_STEPS (diagnostics): jobs this tick, how many threads ran them, and the wall time of each part (Âµs).
    static LAST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    if LAST.swap(now, std::sync::atomic::Ordering::Relaxed) != now {
        let mut ids = threads.into_inner().unwrap_or_default();
        ids.sort_by_key(|t| format!("{t:?}"));
        ids.dedup();
        info!("NPC_SIM_STEPS jobs {} threads {} sync {sync_us} us steps {steps_us} us release {} us", work.len(), ids.len(), t_release.elapsed().as_micros());
    }
}

/// Switch NPC skaters between the tiers and step the simulated ones.
#[allow(clippy::too_many_arguments)]
pub(crate) fn simulate(
    mut commands: Commands,
    settings: Res<LivingWorldSettings>,
    observers: Res<LivingWorldObservers>,
    state: Res<PopulationState>,
    config: Option<Res<crate::config::Config>>,
    graphs: Option<Res<crate::graph_runtime::StockGraphs>>,
    physics: Option<ResMut<GamePhysics>>,
    peds: Option<Res<super::peds::PedData>>,
    (player, net, map): (Res<crate::physics::SkaterRuntime>, Option<Res<crate::multiplayer::Multiplayer>>, Option<Res<crate::map_transition::CurrentMap>>),
    mut npcs: Query<(Entity, &NpcSkater, &mut NpcReplay, Option<&mut NpcSim>, Option<&super::npc_skaters::NpcRaceClock>)>,
    mut events: MessageWriter<super::npc_skaters::NpcSkaterEvent>,
    mut calls: Local<u64>,
    mut pool: ResMut<NpcSimPool>,
    (mut workers, valid, mut chain_scratch): (Local<Vec<Box<GamePhysics>>>, Option<Res<super::npc_skaters::NpcValidLines>>, Local<ChainScratch>),
) {
    let _span = super::timing::span(super::timing::NPC_SIM);
    let (Some(config), Some(graphs), Some(mut physics)) = (config, graphs, physics) else { return };
    if physics.failed {
        return;
    }
    let rules = settings.npc_simulated;
    *calls += 1;
    pool.calls = *calls;
    let now = *calls;
    pool.retry_after.retain(|_, until| *until > now);
    // A new map: the pooled bodies (the reset despawn put the simulated ones back already, `apply_records` runs
    // first) and the workers belong to the old world. The animation source is map independent and stays.
    let generation = map.as_ref().map_or(0, |m| m.generation);
    if pool.prewarmed_for.is_some_and(|g| g != generation) {
        info!("NPC_SKATER_SIM pool: map changed (generation {generation}); dropping {} loaded skaters", pool.bodies.len());
        pool.loading = None;
        pool.bodies.clear();
        pool.retry_after.clear();
        pool.prewarmed_for = None;
        workers.clear();
    }
    // Load the skaters once per map, up front (the first enabled tick, i.e. the map start), so a switch to physics
    // later takes a loaded one instead of reading it from disk mid-ride. Challenge racers ride over the ambient cap:
    // placed racers still without a body top the pool up on the loader thread during the countdown (laptop
    // 2026-11-08: two on-demand loads at the release froze the game 410 ms).
    pool.poll_load();
    let racers_waiting = npcs.iter().filter(|n| n.4.is_some() && n.3.is_none()).count();
    let top_up = pool.prewarmed_for == Some(generation) && pool.loading.is_none() && racers_waiting > pool.bodies.len();
    if rules.enabled && (pool.prewarmed_for.is_none() || top_up) && settings.net_role != NetRole::Client {
        pool.prewarmed_for = Some(generation);
        let at = observers.observers.first().map_or([0.0; 3], |o| o.position);
        let spawn = skate_core::physics::drive_frames::RetailAffineTransform {
            basis: skate_core::physics::rigid_body::basis_from_quaternion(skate_core::physics::rigid_body::RetailQuaternion { x: 0.0, y: 0.0, z: 0.0, w: 1.0 }),
            translation: skate_core::math::Vector3::new(at[0], at[1] + 50.0, at[2]),
        };
        let t0 = std::time::Instant::now();
        let in_use = npcs.iter().filter(|n| n.3.is_some()).count();
        let want = if top_up { in_use + racers_waiting } else { rules.max as usize };
        let have = pool.bodies.len() + in_use;
        // On a loader thread with a worker copy of this tick's physics (the settings, collections and grind world a
        // load reads; `npc_worker`), the same spawn and the same shared source: the bodies are the ones a load here
        // gives, in the same order, and `finish_load` joins before any use.
        let loader = physics.npc_worker().map(|mut worker| {
            GamePhysics::release_npc_worker(&mut worker);
            worker
        });
        let job = loader.map_err(|e| e.to_string()).and_then(|mut worker| {
            let (asset_root, graphs, difficulty, mut source) = (config.asset_root.clone(), graphs.clone(), config.difficulty.profile_key().to_string(), pool.source.clone());
            let count = want.saturating_sub(have);
            std::thread::Builder::new()
                .name("npc-pool-loader".into())
                .spawn(move || {
                    let mut load = PoolLoad { bodies: Vec::new(), source: None, error: None, loads_ms: Vec::new(), started: t0 };
                    for _ in 0..count {
                        let (t, shared) = (std::time::Instant::now(), source.is_some());
                        match load_body_from(&mut worker, &asset_root, &graphs, &difficulty, spawn, &mut source) {
                            Ok(pooled) => {
                                load.bodies.push(pooled);
                                load.loads_ms.push((t.elapsed().as_secs_f64() * 1000.0, shared));
                            }
                            Err(error) => {
                                load.error = Some(error);
                                break;
                            }
                        }
                    }
                    load.source = source;
                    load
                })
                .map_err(|e| e.to_string())
        });
        match job {
            Ok(job) => pool.loading = Some(job),
            Err(error) => {
                // No loader thread: load here, as before.
                warn!("NPC_SKATER_SIM pool: no loader thread ({error}); loading on the game thread");
                for _ in have..want {
                    match load_body(&mut physics, &config, &graphs, spawn, &mut pool.source) {
                        Ok(pooled) => pool.bodies.push(pooled),
                        Err(error) => {
                            warn!("NPC_SKATER_SIM pool: cannot load a skater: {error}");
                            break;
                        }
                    }
                }
                info!("NPC_SKATER_SIM pool: {} loaded skaters ready ({:.0} ms)", pool.bodies.len(), t0.elapsed().as_secs_f64() * 1000.0);
            }
        }
    }
    pool.poll_load();
    // Every 2 s: why each replay-tier NPC is not simulated (logs must diagnose).
    let report = rules.enabled && *calls % 120 == 0;
    let lines = super::npc_skaters::npc_lines(&state);
    let mut count = npcs.iter().filter(|n| n.3.is_some()).count();
    // The line end's inputs, as `advance` builds them: the players and every NPC's (line, node) by serial order; a chain
    // moves its own entry, so a later NPC at the same line end this tick sees the line in use (b620).
    let ChainScratch { players, order, others, in_use, out } = &mut *chain_scratch;
    players.clear();
    players.extend(observers.observers.iter().map(|o| o.position));
    order.clear();
    order.extend(npcs.iter().map(|(_, n, r, _, _)| (n.id, r.cursor.line, r.cursor.node)));
    order.sort_by_key(|x| x.0);
    let line_chain = skate_core::living_world::replay::ChainConfig { fallback: true, ..state.world.config.skaters.line_chain };
    let mut sorted: Vec<_> = npcs.iter_mut().collect();
    sorted.sort_by_key(|n| n.1.id);
    let mut jobs: Vec<Job> = Vec::new();
    // The grab scene's props, once for the whole NPC phase (each NPC step refreshed it before, with the same props).
    if let Err(error) = physics.refresh_grab_props() {
        warn!("NPC_SKATER_SIM grab scene refresh failed: {error}");
    }
    for (e, npc, mut replay, sim, race) in sorted {
        let Some(target) = replay.cursor.line_target(&*lines) else { continue };
        let distance = nearest(&observers, target.position);
        match sim {
            Some(mut sim) => {
                let state_now = sim.body.runtime.player_state.current();
                let settled = matches!(state_now, PhysicalStateId::PhysicsGround | PhysicalStateId::SlideGround);
                // A skater simulated because a prop blocks it (mode 7, allowed at any distance below) stays simulated
                // until it is clear; handing it back while still blocked restarted the sim the next frame, every
                // frame (session game-20261013-183803: 12 restarts in one second at 79 m, ~75 ms each).
                let blocked = rules.walk_back && replay.avoid.last.mode == skate_core::living_world::avoid::AvoidMode::StepOff;
                // A released challenge racer is a full skater at any distance (retail races its AI as full skaters).
                let too_far = distance > rules.radius * 1.1 && !blocked && race.is_none();
                // Not at a line end: retail keeps the skater and looks for the next line every tick (the line end
                // below; b118).
                if (!rules.enabled || too_far) && settled {
                    // Replay resumes from where the sim left the cursor: the hold-back lag takes up the difference.
                    let elapsed = state.world.tick().saturating_sub(npc.spawn_tick) * super::npc_skaters::FRAMES_PER_TICK;
                    replay.avoid.lag = elapsed.saturating_sub(replay.cursor.frames) as f32;
                    replay.follow_target = None;
                    // Back to the pool (the loaded skater is kept for the next switch to physics).
                    commands.queue(move |world: &mut World| pool_from(world, e));
                    count -= 1;
                    info!("NPC_SKATER_SIM #{} {} -> replay (distance {distance:.1} m)", npc.id.serial, npc.character);
                    continue;
                }
                let sim = &mut *sim;
                let replay = &mut *replay;
                // Bail: the line waits (the cursor is held back) until the respawn places the
                // skater back on it (retail: placed at the chosen node of its current path, path
                // state 8, then the spawn push; faded out over the last second before, b715).
                let wiped = sim.body.runtime.player_input.processed.flags_2468 & WIPEOUT_BIT != 0;
                if !wiped {
                    sim.placed = false;
                }
                if wiped && sim.respawn_in.is_none() && !sim.placed {
                    let ticks = rules.respawn_ticks();
                    sim.respawn_in = Some(ticks + 600);
                    sim.bail_ticks = 0;
                    // Retail picks the node now (`sub_8246EE30` -> `sub_8246ED68`): two past the projection's search
                    // node, then the first safe node. Respawning where it fell put a skater back on a take-off
                    // (muted run 2026-10-13: 24 bails in a row at node 17, each right after the placement).
                    let search_start = sim.follow.map_or(replay.cursor.node, |(_, f)| f.search_start);
                    sim.respawn_node = lines.get(&replay.cursor.line).map(|line| {
                        let node = skate_core::living_world::ai_controller::respawn_node(&replay.avoid.controller, &rules.controller, search_start, line);
                        // NOT RETAIL: retail clamps to the last node and re-picks; ours stays off the last two nodes so it respawns on one.
                        let last = line.nodes.len().saturating_sub(3) as u32;
                        (replay.cursor.line, node.min(last))
                    });
                    // The respawn is the skater's own wipeout-end teleport, sent to the chosen node.
                    if let Some(target) = sim.respawn_node.and_then(|(line, node)| skate_core::living_world::replay::LineCursor::spawn(&*lines, line, node).line_target(&*lines)) {
                        sim.body.runtime.set_respawn_destination(spawn_matrix(&line_spawn(&target)), true);
                    }
                    sim.placements_at_bail = sim.body.runtime.placements();
                    let at = projected_point(sim, state_now).0;
                    // The deck body's speed. Not the projected point's step: on the wipeout tick the point switches from
                    // the deck to the character root, and that one offset read as 20-40 m/s (laptop 2026-10-19: four
                    // bails at "40 m/s" on one line, b338).
                    let v = GamePhysics::context_deck_velocity(&sim.context);
                    let speed = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                    let (on_path, dist, a) = sim.follow.map_or((true, 0.0, (0, 0.0)), |(_, f)| (f.on_path, f.dist, f.a));
                    // The skater it touched within the last second, if any (NPC_SKATER_CONTACT has the details).
                    let hz = skate_core::living_world::clock::RETAIL_TICK_HZ;
                    let contact = sim.skater_contact.filter(|(_, age)| (*age as f64) <= hz)
                        .map_or("none".to_owned(), |(o, age)| format!("{} {:.2} s ago", super::npc_skaters::peer_name(o), age as f64 / hz));
                    info!(
                        "NPC_SKATER_BAIL #{} {} respawn in {:.2} s (from {:?} -> {:?}, {speed:.1} m/s, node {}, A {}+{:.2}, on_path {on_path}, {dist:.2} m, at [{:.1}, {:.1}, {:.1}], skater contact {contact})",
                        npc.id.serial, npc.character, ticks as f32 / 60.0, sim.last_state, state_now, replay.cursor.node, a.0, a.1, at[0], at[1], at[2]
                    );
                    events.write(super::npc_skaters::NpcSkaterEvent::Bail { id: npc.id, respawn_seconds: ticks as f32 / 60.0 });
                }
                if let Some(left) = sim.respawn_in {
                    // Retail fades a bailed skater out over the last second before it is placed again (b715).
                    sim.bail_ticks = sim.bail_ticks.saturating_add(1);
                    sim.bail_alpha = state.world.config.skaters.leave_fade.bail_alpha(u64::from(sim.bail_ticks), u64::from(rules.respawn_ticks()));
                    replay.avoid.lag += super::npc_skaters::FRAMES_PER_TICK as f32;
                    // The line waits for the respawn.
                    replay.follow_target = Some(replay.cursor.frames);
                    let respawned = sim.body.runtime.placements() != sim.placements_at_bail;
                    if respawned {
                        sim.bail_alpha = 1.0;
                        // The wipeout-end teleport put the skater on its node: the line state follows (no teleport here).
                        match sim.respawn_node.take() {
                            Some((line, node)) => restart_cursor(replay, &*lines, line, node),
                            None => replay.cursor.frame_in_segment = 0,
                        }
                        replay.follow_target = Some(replay.cursor.frames);
                        // Placed again: the full reset (`sub_82468C10`; [inferred] the bail respawner places through the
                        // same `sub_8246E558` as stage 6), so a stage 5 bail does not repeat after the respawn.
                        skate_core::living_world::ai_controller::full_reset(&mut replay.avoid.controller);
                        settle(sim, rules.respawn_hold_ticks());
                        let len = lines.get(&replay.cursor.line).map_or(0, |l| l.nodes.len());
                        info!("NPC_SKATER_RESPAWN #{} {} at line node {} of {len} (wipeout-end teleport {:.2} s after the bail)", npc.id.serial, npc.character, replay.cursor.node, sim.bail_ticks as f32 / 60.0);
                        events.write(super::npc_skaters::NpcSkaterEvent::Respawned { id: npc.id, node: replay.cursor.node });
                        continue;
                    }
                    // Retail's respawner (`8246EA90`) places the skater itself once the bail time passes the delay
                    // (t = ticks / 60 > 5.0 s), whatever its wipeout does: an AI skater's own auto reset only comes
                    // after 8.0 s, so ours waited 8.57 s (b810). The placement teleport ends the wipeout (b811).
                    if sim.bail_ticks > rules.respawn_ticks() || left == 0 {
                        sim.bail_alpha = 1.0;
                        if left == 0 {
                            warn!("NPC_SKATER_RESPAWN #{} {}: still not placed 10 s after the delay", npc.id.serial, npc.character);
                        }
                        sim.body.runtime.clear_respawn_destination();
                        skate_core::living_world::ai_controller::full_reset(&mut replay.avoid.controller);
                        // The same skater placed again on its line, on the node (retail places its AI skater object
                        // again at the chosen node).
                        match sim.respawn_node.take() {
                            Some((line, node)) => restart_cursor(replay, &*lines, line, node),
                            None => replay.cursor.frame_in_segment = 0,
                        }
                        // The new cursor counts its own frames: hold it there (the old target ran it 38 nodes ahead).
                        replay.follow_target = Some(replay.cursor.frames);
                        let Some(target) = replay.cursor.line_target(&*lines) else { continue };
                        match place(sim, &target, on_board_at(&*lines, &replay.cursor), rules.respawn_hold_ticks()) {
                            Ok(v) => {
                                let len = lines.get(&replay.cursor.line).map_or(0, |l| l.nodes.len());
                                info!(
                                    "NPC_SKATER_RESPAWN #{} {} at line node {} of {len} ({:.2} s after the bail; push [{:.1}, {:.1}, {:.1}] = {:.1} m/s, at [{:.1}, {:.1}, {:.1}], board up y {:.2}, node flags {:02X})",
                                    npc.id.serial, npc.character, replay.cursor.node, sim.bail_ticks as f32 / 60.0, v[0], v[1], v[2], (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt(), target.position[0], target.position[1], target.position[2],
                                    { let q = target.frame; 1.0 - 2.0 * (q[0] * q[0] + q[2] * q[2]) },
                                    lines.get(&replay.cursor.line).and_then(|l| l.nodes.get(replay.cursor.node as usize)).map_or(0, |n| n.flags)
                                );
                                events.write(super::npc_skaters::NpcSkaterEvent::Respawned { id: npc.id, node: replay.cursor.node });
                            }
                            Err(error) => {
                                warn!("NPC_SKATER_SIM #{} {}: respawn failed, back to replay: {error}", npc.id.serial, npc.character);
                                // Back to the pool like the physics-error hand-back (dropped, the next switch loads one here).
                                commands.queue(move |world: &mut World| pool_from(world, e));
                                count -= 1;
                            }
                        }
                        continue;
                    }
                    sim.respawn_in = Some(left - 1);
                }
                let deck = GamePhysics::context_deck(&sim.context);
                // The effective deck forward (PhysOut +64, what the AI record, the steer and the facing gates read;
                // b200). The raw deck pointed backwards after a flip trick, so the steer turned the wrong end toward
                // the line and the skater rolled away from it (b199 #7, #8).
                let forward = effective_deck_forward(sim);
                let direct = skate_core::living_world::controller_b::DirectPath;
                let on_navmesh = peds.as_deref().and_then(|p| p.nav.as_deref()).is_some();
                let paths: &dyn skate_core::living_world::controller_b::PathService = match peds.as_deref().and_then(|p| p.nav.as_deref()) {
                    Some(mesh) => mesh,
                    None => &direct,
                };
                // Mode 7 (b66 / b70 / b78 / b79): the line controller steps off (sub-mode 3 and a one-shot
                // reposition; a second tick in mode 7 sets sub-mode 5 and posts WipeOutRequest), and the
                // reposition swaps to controller B, which walks the skater back to the chosen line node.
                let mut walk_intents: Vec<(String, f32)> = Vec::new();
                // The stuck tracker's stage actions (`sub_8246F818`, b111), on the stage the previous tick set.
                let (mut stuck_resnap, mut stuck_respawn) = (false, false);
                // The walk-back's inputs; it starts after the follow block, which raises its request (b701 2g).
                let mut reposition: Option<(bool, u32)> = None;
                if rules.walk_back && sim.walker.is_none() {
                    use skate_core::living_world::{ai_controller, avoid::AvoidMode};
                    let a = &mut replay.avoid;
                    let mode_7 = a.last.mode == AvoidMode::StepOff;
                    let prop_node = a.last.steer_target.and_then(|id| a.last.entries.iter().find(|x| x.id == id)).and_then(|x| x.path).map_or(replay.cursor.node, |p| p.node);
                    let airborne = matches!(state_now as u32, 200..300);
                    if let Some(w) = ai_controller::step_off(&mut a.controller, &rules.controller, mode_7, airborne, false, prop_node) {
                        walk_intents.push(("WipeOutRequest".into(), w));
                        info!("NPC_STEP_OFF #{} {} wipe-out request (still blocked, node {prop_node})", npc.id.serial, npc.character);
                    }
                    match ai_controller::stuck_action(&mut a.controller, &rules.controller, false) {
                        ai_controller::StuckAction::WipeOut(w) => walk_intents.push(("WipeOutRequest".into(), w)),
                        ai_controller::StuckAction::Resnap => stuck_resnap = true,
                        ai_controller::StuckAction::Respawn => stuck_respawn = true,
                        ai_controller::StuckAction::None => {}
                    }
                    reposition = Some((mode_7, prop_node));
                }
                if let Some(w) = sim.walker.as_mut() {
                    use skate_core::living_world::controller_b::BTick;
                    let input = walker_input(&sim.body.runtime);
                    match w.b.tick(&rules.navmesh, paths, &input) {
                        BTick::Idle => {}
                        BTick::Controls(c) => {
                            // NPC_WALK_PROGRESS once a second while walking back: where it is, how fast, what it asks for.
                            if *calls % 60 == npc.id.serial as u64 % 60 {
                                let i = walker_input(&sim.body.runtime);
                                info!(
                                    "NPC_WALK_PROGRESS #{} {} at [{:.2}, {:.2}, {:.2}] off_board {} toggling {} can_move {} state {:?} steer {:.2} throttle {:?} wipe_out {} intents {:?}",
                                    npc.id.serial, npc.character, i.position[0], i.position[1], i.position[2], i.byte_161, i.byte_160, i.can_move, state_now,
                                    c.steer, c.throttle, c.wipe_out, c.intents().iter().map(|x| x.0).collect::<Vec<_>>()
                                );
                            }
                            walk_intents.extend(c.intents().into_iter().map(|(k, v)| (k.to_string(), v)));
                        }
                        BTick::HandBack { reason, wipe_out } => {
                            if wipe_out {
                                walk_intents.push(("WipeOutRequest".into(), 1.0));
                            }
                            // A re-attaches by its saved line id at the node (824689C0, b66 / b115; search 82455108): docs/hails-additions/26e-living-world-skater-ai.md#controller-a-activate
                            let at = walker_input(&sim.body.runtime).position;
                            let node = lines.get(&w.line).map_or(w.node, |line| skate_core::living_world::path_follow::nearest_node(line, at));
                            restart_cursor(replay, &*lines, w.line, node);
                            replay.follow_target = Some(replay.cursor.frames);
                            let reset = match sim.follow {
                                Some((_, f)) => f.switch_to(node),
                                None => skate_core::living_world::path_follow::PathFollow::reattach(node, &rules.path),
                            };
                            sim.follow = Some((w.line, reset));
                            sim.toggle = Default::default();
                            // The walk-back worked: the stuck watch clears (stage -> 0); say so, or a recovered skater
                            // reads as still stuck in the log (b319).
                            let stage = replay.avoid.controller.sub_mode;
                            if stage > 0 {
                                info!("NPC_SKATER_STUCK #{} {} stage {stage} -> 0 after walk-back to node {node}", npc.id.serial, npc.character);
                            }
                            skate_core::living_world::ai_controller::reposition_done(&mut replay.avoid.controller);
                            info!("NPC_WALK_BACK #{} {} hand back ({reason:?}) at line node {node} (walked to {})", npc.id.serial, npc.character, w.node);
                            events.write(super::npc_skaters::NpcSkaterEvent::WalkBack { id: npc.id, node: w.node, started: false });
                            sim.walker = None;
                            // The activate's reset (82468AC8) also clears flip latch, trick slot and board yaw (b291): docs/hails-additions/26e-living-world-skater-ai.md#activate-reset
                            clear_line_state(sim);
                        }
                    }
                }
                if sim.walker.is_some() {
                    // Controller B is the active controller: no PathController record, signals or spawn push (the
                    // AI source stays present but not fresh: +10689 set, +10688 clear, so the biped runs in the AI's
                    // direct mode on OB_Mag / OB_Turn, `82D310F8`; b80); the line waits.
                    if let Some(source) = sim.body.runtime.ai_physics.as_mut() {
                        source.fresh = false;
                    }
                    replay.avoid.lag += super::npc_skaters::FRAMES_PER_TICK as f32;
                    sim.last_state = Some(state_now);
                    sim.last_deck = Some(projected_point(sim, state_now).0);
                    lift_pending(sim, npc);
                    // The step itself runs after the loop, with the other NPCs' steps (in parallel).
                    jobs.push(Job { entity: e, serial: npc.id.serial, character: npc.character.clone(), intents: walk_intents });
                    continue;
                }
                // Avoider answer (82470830, 82464E30) and retail projection 8246D560 (b110): docs/hails-additions/26e-living-world-skater-ai.md#projection-and-avoider
                let shape_ratio = {
                    let v = target.step.map(|x| x * ai_record::FRAMES_PER_SECOND);
                    let s = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                    if s > 1e-3 && !replay.avoid.low_prop() { replay.avoid.last.shape_speed(s, false) / s } else { 1.0 }
                };
                // The skater state byte `+438` (b119): airborne = an air state with no wheel touching. DB50 counts an
                // airborne skater as off the path for the steering record.
                let airborne_now = (200..300).contains(&(state_now as u32)) && sim.body.runtime.player_input.processed.wheel_count_2556 == 0;
                // This tick's ground-steer heading error (`pc+824`) for the powerslide writer; 0 when not computed.
                let mut slide_error = 0.0f32;
                let mut ground_steered = false;
                // The node-change handler's gate and body point for this tick's cursor steps (sub_8246BEE0, b674). NOT
                // RETAIL YET: the upcoming-trick test (pc+796 / +797) has no stand-in.
                replay.body_gate = Some(skate_core::living_world::replay::BodyGate {
                    point: projected_point(sim, state_now).0,
                    may_branch: !airborne_now && sim.walker.is_none()
                        && !skate_core::living_world::replay::NO_BRANCH_CATEGORIES.contains(&sim.slot.category),
                    // Always the deck's forward, also biped or wiped (b706), about the skater frame's up row.
                    heading: Some((effective_deck_forward(sim), {
                        let root = effective_root(&sim.body.runtime);
                        [root[1][0], root[1][1], root[1][2]]
                    })),
                });
                // Cursor B after this tick's projection: the steering record's target (`sub_8246DE38`, b174 3).
                let mut left_path = false;
                let (on_path, past_start, b_target) = match lines.get(&replay.cursor.line) {
                    Some(line) if stuck_respawn && sim.respawn_in.is_none() && !sim.teleport_pending => {
                        let search_start = sim.follow.map_or(replay.cursor.node, |(_, f)| f.search_start);
                        // Kept off the last two nodes like the bail respawn (see there).
                        let node = skate_core::living_world::ai_controller::respawn_node(&replay.avoid.controller, &rules.controller, search_start, line).min(line.nodes.len().saturating_sub(3) as u32);
                        let line = replay.cursor.line;
                        restart_cursor(replay, &*lines, line, node);
                        replay.follow_target = Some(replay.cursor.frames);
                        skate_core::living_world::ai_controller::full_reset(&mut replay.avoid.controller);
                        sim.follow = None;
                        if let Some(target) = replay.cursor.line_target(&*lines) {
                            match place(sim, &target, on_board_at(&*lines, &replay.cursor), rules.placement_hold_ticks()) {
                                Ok(_) => {
                                    info!("NPC_SKATER_STUCK #{} {} respawn at line node {node} (stage 6, from node {search_start})", npc.id.serial, npc.character);
                                    events.write(super::npc_skaters::NpcSkaterEvent::Respawned { id: npc.id, node });
                                }
                                Err(error) => warn!("NPC_SKATER_STUCK #{} {} respawn at node {node} failed: {error}", npc.id.serial, npc.character),
                            }
                        }
                        (false, false, None)
                    }
                    Some(line) => {
                        // During the bail the line waits (above): no projection, so the cursor and its trick events stay
                        // where the skater fell (it kept running before: node 6 -> 14 and a trick dispatched mid-bail).
                        // Nor during the placement hold: retail's hold branch (`824701F8`) runs no path branch, so the
                        // cursor and the spawn push stay on the placed node (b807; ours ran 62 -> 64 and pushed from 64).
                        if sim.respawn_in.is_some() || sim.teleport_pending || sim.hold_ticks > 0 {
                            (false, false, None)
                        } else {
                            let mut line = line;
                            let mut f = match sim.follow {
                                Some((id, f)) if id == replay.cursor.line && replay.cursor.node + rules.path.window >= f.search_start => f,
                                // A line switch (a junction branch in the cursor): retail's reset `sub_82468AC8` leaves the skater off
                                // the path with the start gate kept (b118).
                                Some((_, f)) => {
                                    clear_line_state(sim);
                                    skate_core::living_world::ai_controller::path_reset(&mut replay.avoid.controller);
                                    f.switch_to(replay.cursor.node)
                                }
                                None => skate_core::living_world::path_follow::PathFollow::start(replay.cursor.node, &rules.path),
                            };
                            let (pos, heading) = projected_point(sim, state_now);
                            if stuck_resnap {
                                let node = skate_core::living_world::ai_controller::resnap_node(line, &rules.controller, f.search_start, pos);
                                info!("NPC_SKATER_STUCK #{} {} re-snap node {} -> {node} (stage 2) at=[{:.1}, {:.1}, {:.1}] dist={:.1}", npc.id.serial, npc.character, f.search_start, pos[0], pos[1], pos[2], f.dist);
                                // Retail's re-snap is a full path reset that keeps the stuck tracker (b207); before,
                                // ours kept a stale air / trick slot / board yaw across the jump.
                                clear_line_state(sim);
                                f = f.switch_to(node);
                            }
                            let was_on = f.on_path;
                            let was_lost = f.lost;
                            // `pc+808` (`sub_8246BCF8`): the speed of the projected point, negative when above 0.01 and
                            // moving against the heading.
                            let speed = sim.last_deck.map_or(0.0, |p| {
                                let v = [(pos[0] - p[0]) * 60.0, (pos[1] - p[1]) * 60.0, (pos[2] - p[2]) * 60.0];
                                let s = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                                if s > 0.01 && v[0] * heading[0] + v[1] * heading[1] + v[2] * heading[2] < 0.0 { -s } else { s }
                            });
                            // The line end (`sub_8246D3C0`, b701): on the trigger tick chain from the body (`sub_8246C7F8`, mode 1
                            // fallback), reset onto the new line, then the 4.5 m lost test on whichever line it is on.
                            if f.search(line, pos, speed, &rules.path) {
                                super::npc_skaters::fill_others(order, npc.id, others, in_use);
                                let profile = super::npc_skaters::npc_trick_profile(&state.npc.tricks, &settings.skater_trick_profiles, &npc.character);
                                let tricks = skate_core::living_world::npc_tricks::TrickContext { mode: settings.npc_tricks.mode, params: settings.npc_tricks.params, profile: profile.as_deref(), seed: npc.seed, slot_tricks: state.npc.tricks.slot_tricks_for(&npc.character) };
                                let ctx = skate_core::living_world::replay::BranchContext { position: pos, forward: heading, speed: speed.abs(), players, others, in_use, preferred_skill: -1, online: observers.online, chain: line_chain, tricks, body: replay.body_gate, valid_lines: valid.as_ref().and_then(|v| v.per_character.get(&npc.character)).map(Vec::as_slice) };
                                out.clear();
                                if let Some((to, node)) = replay.cursor.line_end_chain(&*lines, &ctx, out) {
                                    for ev in out.drain(..) {
                                        if let skate_core::living_world::replay::CursorEvent::Branch(record) = ev {
                                            replay.branches.push(record.clone());
                                            events.write(super::npc_skaters::NpcSkaterEvent::Branch { id: npc.id, record });
                                        }
                                    }
                                    info!("NPC_SKATER_CHAIN #{} {} line end: next line at node {node} (A {}+{:.2})", npc.id.serial, npc.character, f.a.0, f.a.1);
                                    if race.is_some() {
                                        log_far_chain(&*lines, npc, pos, &to, line_chain.radius);
                                    }
                                    if let Some(next) = lines.get(&to) {
                                        line = next;
                                    }
                                    clear_line_state(sim);
                                    skate_core::living_world::ai_controller::path_reset(&mut replay.avoid.controller);
                                    f = f.switch_to(node);
                                    sim.chain_trace = 240;
                                    if let Some(o) = order.iter_mut().find(|o| o.0 == npc.id) {
                                        o.1 = to;
                                        o.2 = node;
                                    }
                                }
                                f.line_end_lost(line, pos, speed, &rules.path);
                            }
                            f.project(line, pos, shape_ratio, airborne_now, &rules.path);
                            {
                                use skate_core::living_world::ai_controller;
                                let c = &mut replay.avoid.controller;
                                let (before, anchor) = (c.sub_mode, c.stuck.anchor);
                                // `sub_82468070`: on the board and not airborne (the off-board walk is controller B's).
                                let can_track = !airborne_now && sim.walker.is_none();
                                let a_pos = skate_core::living_world::path_follow::point(line, f.a);
                                // Retail measures from the animation root always (Skeleton+0x1A0, `sub_82468240`, b674).
                                let root = effective_root(&sim.body.runtime);
                                let pos = [root[3][0], root[3][1], root[3][2]];
                                ai_controller::track_stuck(c, &rules.controller, pos, f.a, a_pos, f.progress, can_track);
                                if c.sub_mode != before {
                                    let anchor = if before == ai_controller::sub_mode::NORMAL { c.stuck.anchor } else { anchor };
                                    let away = ((pos[0] - anchor[0]).powi(2) + (pos[2] - anchor[2]).powi(2)).sqrt();
                                    // Stage 1 arms on one no-progress tick and does nothing (retail's stage actions start at 2, b309): its
                                    // entry and clear are debug, the escalations from stage 2 up stay info.
                                    if before.max(c.sub_mode) >= 2 {
                                        info!("NPC_SKATER_STUCK #{} {} stage {before} -> {} at node {} ({:.1} m from where it stalled, {speed:.1} m/s, on_path {})", npc.id.serial, npc.character, c.sub_mode, f.search_start, away, f.on_path);
                                    } else {
                                        debug!("NPC_SKATER_STUCK #{} {} stage {before} -> {} at node {} ({:.1} m from where it stalled, {speed:.1} m/s, on_path {})", npc.id.serial, npc.character, c.sub_mode, f.search_start, away, f.on_path);
                                    }
                                }
                            }
                            // Lost (`sub_82468DD8`): the same one-shot reposition request mode 7 raises (`ctrl+941`); the
                            // next tick hands the skater to controller B, which walks it back (b115).
                            if f.lost && rules.walk_back && sim.walker.is_none() {
                                if !was_lost {
                                    info!("NPC_SKATER_LOST #{} {} at node {} ({:.1} m from the path, {speed:.1} m/s) at=[{:.1}, {:.1}, {:.1}]", npc.id.serial, npc.character, f.search_start, f.dist, pos[0], pos[1], pos[2]);
                                }
                                replay.avoid.controller.reposition_request = true;
                            }
                            replay.follow_target = Some(replay.cursor.frames + f.frames_to_b(line, replay.cursor.node, replay.cursor.frame_in_segment));
                            if f.on_path != was_on {
                                left_path = was_on;
                                info!("NPC_SKATER_PATH #{} {} {} at node {} (A {}+{:.2}, {:.2} m, lane {})", npc.id.serial, npc.character, if f.on_path { "rejoined the path" } else { "left the path" }, replay.cursor.node, f.a.0, f.a.1, f.dist, f.inside_lane);
                            }
                            sim.follow = Some((replay.cursor.line, f));
                            (f.on_path, f.past_start, skate_core::living_world::replay::line_target_at(line, f.b))
                        }
                    }
                    None => (true, true, None),
                };
                // Walk-back on the tick the lost test raised it (`sub_8246FE38` after `sub_8246D560`, b701 2g; one tick
                // late before): the walker first ticks next tick.
                if let Some((mode_7, prop_node)) = reposition.filter(|_| sim.walker.is_none()) {
                    use skate_core::living_world::ai_controller;
                    let a = &mut replay.avoid;
                    if let Some(line) = lines.get(&replay.cursor.line) {
                        let nodes = line.nodes.len() as u32;
                        // `sub_8246FE38` counts from the projection's search node (`pc+816`).
                        let search_start = sim.follow.map_or(replay.cursor.node, |(_, f)| f.search_start);
                        if let Some(node) = ai_controller::take_reposition(&mut a.controller, &rules.controller, false, *calls, mode_7, search_start, prop_node, nodes) {
                            // 8246FE38 -> 82455728: the first calm node from there.
                            let node = ai_controller::safe_node(line, node, &rules.controller);
                            let n = &line.nodes[node as usize];
                            let l = (n.step[0] * n.step[0] + n.step[1] * n.step[1] + n.step[2] * n.step[2]).sqrt();
                            let dir = if l > 1e-6 { n.step.map(|x| x / l) } else { [0.0; 3] };
                            let input = walker_input(&sim.body.runtime);
                            match skate_core::living_world::controller_b::ControllerB::activate(&rules.navmesh, paths, n.position, dir, &input) {
                                Some(b) => {
                                    info!("NPC_WALK_BACK #{} {} start: node {node} at [{:.1}, {:.1}, {:.1}], {} waypoints", npc.id.serial, npc.character, n.position[0], n.position[1], n.position[2], b.path.len());
                                    events.write(super::npc_skaters::NpcSkaterEvent::WalkBack { id: npc.id, node, started: true });
                                    sim.walker = Some(Walker { b, line: replay.cursor.line, node });
                                }
                                None => {
                                    if sim.no_plan != Some(node) {
                                        // Why the plan failed (b207): which path service, from where to where.
                                        let (from, to) = ([deck.translation.x, deck.translation.y, deck.translation.z], n.position);
                                        let d = ((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2) + (to[2] - from[2]).powi(2)).sqrt();
                                        info!("NPC_WALK_BACK #{} {} no plan to node {node}: stays with the line controller (service {}, from [{:.1}, {:.1}, {:.1}] to [{:.1}, {:.1}, {:.1}], {d:.1} m)", npc.id.serial, npc.character, if on_navmesh { "navmesh" } else { "direct" }, from[0], from[1], from[2], to[0], to[1], to[2]);
                                    }
                                    sim.no_plan = Some(node);
                                }
                            }
                        }
                    }
                }
                let avoid = &replay.avoid;
                // Retail steers to B of this tick (frame, position, displacement). Before 2026-10-17 ours used the
                // replay cursor from the top of the tick, which never moves back: after a rejoin behind it the target
                // stayed up to 1 m ahead of retail's (b174 3).
                let base = b_target.as_ref().unwrap_or(&target);
                let mut steered = base.clone();
                let v = base.step.map(|x| x * ai_record::FRAMES_PER_SECOND);
                let speed = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                if speed > 1e-3 && !avoid.low_prop() {
                    let k = avoid.last.shape_speed(speed, false) / speed;
                    steered.step = base.step.map(|x| x * k);
                }
                let right = super::npc_avoid::right_of(base.step);
                steered.position = std::array::from_fn(|i| base.position[i] + right[i] * avoid.offset);
                let byte_933 = matches!(avoid.last.mode, skate_core::living_world::avoid::AvoidMode::Steer | skate_core::living_world::avoid::AvoidMode::Skitch);
                if let Some((_, f)) = sim.follow.filter(|(id, _)| !airborne_now && *id == replay.cursor.line) {
                    // Not mapped yet: the pending node trick's category 9 (b121 open), as the ground steer.
                    sim.lane_flags = skate_core::living_world::ground_steer::lane_flags(f.inside_lane, f.lateral, f.dist, byte_933, false, &rules.ground_steer);
                }
                let steer = ai_record::SteerState { byte_933, byte_944: sim.lane_flags.0, byte_945: sim.lane_flags.1, ..Default::default() };
                // `sub_8246DB50`: off the path or before the start nodes `sub_8246DE38` is skipped and the record
                // carries no target; the skater coasts on its own physics until the projection commits again.
                // Retail's hold branch after a placement (`824701F8`) sends a record with no target (no steering).
                let mut record = if sim.hold_ticks == 0 && ai_record::steers(&steer, on_path && !airborne_now, past_start) { ai_record::build(&steered, forward, &steer) } else { ai_record::without_steering(&steer) };
                // The recorded jump at the cursor (`sub_8246DA18`): its arc and whether it lands in a grind (b294).
                let mut cursor = None;
                if let Some(line) = lines.get(&replay.cursor.line) {
                    let (node, frame_in_segment) = (replay.cursor.node, replay.cursor.frame_in_segment);
                    let jump = ai_record::current_trajectory(line, node, frame_in_segment);
                    if let Some(jump) = jump {
                        ai_record::with_trajectory(&mut record, jump, ai_record::lands_in_grind(line, node.saturating_sub(1)));
                    }
                    cursor = Some(crate::physics::AiRecordCursor { node, frame_in_segment, ext_node: ai_record::trajectory_node(line, node, frame_in_segment), has_trajectory: jump.is_some(), lands_node: node.saturating_sub(1) });
                }
                sim.body.runtime.ai_physics = Some(crate::physics::AiPhysicsSource { record, fresh: true, cursor });
                // NPC_SKATER_SPEED: the deck body's speed changed by 2 m/s within half a second (a line per skater per
                // half second at most): with the recorded speed the board path pulls toward (`82C05EC0`, b338) and the
                // slope along the motion, so a speed-up is placed and explained from the log.
                {
                    let w = GamePhysics::context_deck_velocity(&sim.context);
                    let now_speed = (w[0] * w[0] + w[1] * w[1] + w[2] * w[2]).sqrt();
                    let (start, ticks, cooldown) = sim.speed_watch;
                    let (start, ticks) = if ticks == 0 || ticks >= 30 { (now_speed, 1) } else { (start, ticks + 1) };
                    let cooldown = cooldown.saturating_sub(1);
                    sim.speed_watch = (start, ticks, cooldown);
                    if cooldown == 0 && (now_speed - start).abs() >= 2.0 {
                        let flat = (w[0] * w[0] + w[2] * w[2]).sqrt();
                        let grade = if flat > 0.1 { w[1] / flat * 100.0 } else { 0.0 };
                        info!(
                            "NPC_SKATER_SPEED #{} {} node {} state {:?} speed {start:.1} -> {now_speed:.1} m/s over {ticks} ticks (recorded {speed:.1} m/s, slope {grade:.0} %, on_path {on_path}, hold {})",
                            npc.id.serial, npc.character, replay.cursor.node, state_now, sim.hold_ticks
                        );
                        sim.speed_watch = (now_speed, 1, 30);
                    }
                }
                // Spawn push (824701F8 hold branch, 82C04168): once on the hold's last tick near the placed node (b178).
                if !sim.teleport_pending && sim.hold_ticks > 0 {
                    let at = projected_point(sim, state_now).0;
                    // A pinned racer settles on its slot while it waits; the push test measures from where it stands at
                    // the release (laptop run C: 0.3 to 0.6 m of settling refused every start push).
                    if race.is_some_and(|c| !c.moving) {
                        sim.placed_at = Some(at);
                    }
                    let placed = *sim.placed_at.get_or_insert(at);
                    if sim.hold_ticks == 1 && !race.is_some_and(|c| !c.moving) {
                        if let Some(line) = lines.get(&replay.cursor.line) {
                            let node = &line.nodes[replay.cursor.node as usize];
                            if node.flags & node_flags::OFF_BOARD == 0 {
                                let moved = (at[0] - placed[0]).hypot(at[2] - placed[2]);
                                let push = ai_record::spawn_push(at, placed, target.step, replay.cursor.node == 0, false);
                                if let Some(v) = push {
                                    GamePhysics::push_context_velocity(&mut sim.context, v);
                                }
                                info!(
                                    "NPC_SKATER_PUSH #{} {} node {} {} (moved {moved:.3} m since the hold started)",
                                    npc.id.serial, npc.character, replay.cursor.node,
                                    push.map_or("none: left its place".to_string(), |v| format!("{:.1} m/s", (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()))
                                );
                            }
                        }
                    }
                }
                // The PathController's ActionGraph signals: anticipation and the trick at its node. The slot is restored
                // below when the dispatcher would not have run this tick.
                let slot_before = sim.slot;
                let mut anticipated = None;
                sim.since_landing = if sim.air.airborne && !airborne_now { 0 } else { sim.since_landing.saturating_add(1) };
                let intents = match lines.get(&replay.cursor.line) {
                    Some(line) => {
                        let at = [deck.translation.x, deck.translation.y, deck.translation.z];
                        let off_line = (at[0] - target.position[0]).hypot(at[2] - target.position[2]);
                        let along = target.step;
                        let facing = if sim.body.runtime.animation.packet.riding_fakie { forward.map(|x| -x) } else { forward };
                        // `AnticMag`'s gate reads `pc+824`; the steer runs later in this function, so last tick's.
                        let heading_error = sim.ground_error.unwrap_or_else(|| if along[0].hypot(along[2]) > 1e-5 { (facing[0] * along[2] - facing[2] * along[0]).atan2(facing[0] * along[0] + facing[2] * along[2]) } else { 0.0 });
                        let input = skate_core::living_world::ai_signals::SignalInput {
                            node: replay.cursor.node,
                            frame_in_segment: replay.cursor.frame_in_segment,
                            last_node: sim.last_node,
                            off_line,
                            heading_error,
                            manual_window: airborne_now || sim.since_landing < 5,
                        };
                        let cursor_line = replay.cursor.line;
                        let tricks = &replay.tricks;
                        let chosen = |n: u32| tricks.iter().rev().find(|r| r.line == cursor_line && r.node == n).map_or_else(|| line.node_trick(&line.nodes[n as usize]), |r| r.chosen);
                        let slot_tricks = state.npc.tricks.slot_tricks_for(&npc.character);
                        let v = skate_core::living_world::ai_signals::signals(line, &rules.signals, &input, &chosen, &mut sim.slot, slot_tricks);
                        anticipated = skate_core::living_world::ai_signals::anticipated(line, &rules.signals, &input);                        v
                    }
                    None => Vec::new(),
                };
                // Mode 4 (skitch; b65, main checked 0x8246FC78): GrabWorld every tick on the ground, no tricks.
                let state = sim.body.runtime.player_state.current();
                let mut intents = intents;
                intents.extend(walk_intents);
                // A's path tick presses the off-board toggle first (`824712E0`): a skater left off its board after a
                // walk-back gets back on (b117); then the mode for the next tick.
                {
                    let p = &sim.body.runtime.player_input.processed;
                    if sim.toggle.press(rules.navmesh.push_period, p.flags_2484 & 0x0800_0000 != 0, p.flags_2480 & 0x4 != 0) {
                        intents.push(("NewToggleOffBoardState".into(), 1.0));
                        intents.push(("ToggleOffBoardState".into(), 1.0));
                    }
                    sim.toggle.line_mode(false);
                }
                // The takeoff latch (`sub_8246A700`, every path tick): the character's forward against the current node's
                // recorded skater forward.
                let character = sim.body.runtime.animated_skeleton.roots.animation_to_world;
                let (char_forward, char_up) = ([character[2][0], character[2][1], character[2][2]], [character[1][0], character[1][1], character[1][2]]);
                if let Some(line) = lines.get(&replay.cursor.line) {
                    // Cursor A (`pc+608`, `pc+652`), as `sub_8246A700` reads it; the replay cursor when off its line.
                    let cursor_a = sim.follow.filter(|(id, _)| *id == replay.cursor.line).map_or(replay.cursor.node, |(_, f)| f.a.0);
                    let node_forward = skate_core::living_world::air_steer::node_skater_forward(line, cursor_a);
                    if sim.air.airborne && !airborne_now {
                        // The air steer's check (b616): the landing facing against the recorded one, sign expected = latch.
                        let d = char_forward[0] * node_forward[0] + char_forward[2] * node_forward[2];
                        info!("NPC_SKATER_LAND #{} {} node {} flip {} char.node {d:.2} aligned {}", npc.id.serial, npc.character, replay.cursor.node, sim.air.flip, (d < 0.0) == sim.air.flip);
                    }
                    if sim.air.tick(airborne_now, char_forward, node_forward) {
                        // `sub_8246A700` -> `sub_824566C8` -> `sub_8246B1F8`: the recorded board-vs-skater yaw of the node
                        // the jump lands on, posted while airborne.
                        use skate_core::living_world::{ai_signals, replay::decode_orientation};
                        let landing = ai_signals::landing_node(line, cursor_a);
                        sim.board_yaw = landing.map_or_else(ai_signals::BoardYaw::default, |i| {
                            let n = &line.nodes[i as usize];
                            ai_signals::BoardYaw::takeoff(decode_orientation(n.board), decode_orientation(n.skater), &rules.board_yaw)
                        });
                        if sim.board_yaw.set {
                            let nodes: Vec<String> = (cursor_a.saturating_sub(4)..=landing.unwrap_or(cursor_a) + 1).filter_map(|i| line.nodes.get(i as usize).map(|n| {
                                let y = ai_signals::BoardYaw::takeoff(decode_orientation(n.board), decode_orientation(n.skater), &rules.board_yaw);
                                format!("{i}:{}{}e{}m{:.1}", if n.flags & skate_core::living_world::replay::node_flags::AIRBORNE != 0 { 'A' } else { 'g' }, n.frames, n.event, y.mag)
                            })).collect();
                            info!("NPC_SKATER_YAW_NODES #{} {}", npc.id.serial, nodes.join(" "));
                            info!("NPC_SKATER_BOARD_YAW #{} {} node {} landing {:?} angle {:.2} mag {:.2} slot {} {}", npc.id.serial, npc.character, cursor_a, landing, sim.board_yaw.angle, sim.board_yaw.mag, sim.slot.category, super::npc_skaters::trick_name(sim.slot.trick));
                        }
                        let d = GamePhysics::context_deck(&sim.context).basis.columns[2];
                        let v = sim.last_deck.map(|q| { let p = projected_point(sim, state_now).0; [p[0] - q[0], p[1] - q[1], p[2] - q[2]] }).unwrap_or([0.0; 3]);
                        let vl = (v[0] * v[0] + v[2] * v[2]).sqrt().max(1e-6);
                        debug!(
                            "NPC_SKATER_TAKEOFF #{} node {} flip {} char.deck {:.2} char.travel {:.2} deck.travel {:.2} char.node {:.2}",
                            npc.id.serial, replay.cursor.node, sim.air.flip,
                            char_forward[0] * d[0] + char_forward[2] * d[2],
                            (char_forward[0] * v[0] + char_forward[2] * v[2]) / vl,
                            (d[0] * v[0] + d[2] * v[2]) / vl,
                            char_forward[0] * node_forward[0] + char_forward[2] * node_forward[2]
                        );
                    }
                }
                // The ground steer (`sub_8246ACF0` -> `sub_8246AA00` / `sub_82471070`, b121): on the ground and on the
                // board (not toggling), not skitching, every path tick, on or off the path; it turns a skater that left
                // its line back toward it.
                {
                    use skate_core::living_world::{avoid::AvoidMode, ground_steer, path_follow};
                    let p = &sim.body.runtime.player_input.processed;
                    let on_foot = p.flags_2484 & 0x0800_0000 != 0 || p.flags_2480 & 0x4 != 0;
                    // Retail's hold branch (`824701F8`) runs no steer right after a placement.
                    let hold = sim.hold_ticks > 0;
                    let waiting = sim.respawn_in.is_some() || sim.teleport_pending || hold;
                    // The air arm's board yaw post (`sub_8246B358` -> `sub_82471008`), after its steer.
                    if airborne_now && rules.board_yaw_on && !on_foot && !waiting && avoid.last.mode != AvoidMode::Skitch {
                        use skate_core::living_world::ai_signals::{manual_value, BoardYaw};
                        let ahead = anticipated.filter(|&t| skate_core::scoring::catalog::category(t) == Some(8) && sim.slot.category != 8);
                        let post: Option<[_; 2]> = BoardYaw::post(&sim.board_yaw, ahead.map(|t| manual_value(t, rules.signals.manual_value)), &rules.board_yaw);
                        intents.extend(post.into_iter().flatten());
                    }
                    if airborne_now && !rules.air_steer_on {
                    } else if airborne_now && !on_foot && !waiting && avoid.last.mode != AvoidMode::Skitch {
                        // The air arm (`sub_8246B358`, b123): toward the recorded skater forward at the near look-ahead,
                        // reversed when latched; nothing inside the dead zone.
                        if let (Some(line), Some((id, f))) = (lines.get(&replay.cursor.line), sim.follow) {
                            if id == replay.cursor.line {
                                use skate_core::living_world::air_steer;
                                let near = path_follow::advance(line, f.a, rules.ground_steer.near_ticks);
                                let e = air_steer::error(char_forward, char_up, air_steer::skater_forward_at(line, near), sim.air.flip);
                                if let Some(s) = air_steer::stick(e, &rules.air_steer) {
                                    intents.extend(ground_steer::intents(s).into_iter().map(|(k, v)| (k.to_string(), v)));
                                }
                            }
                        }
                    } else if !airborne_now && !on_foot && !waiting && avoid.last.mode != AvoidMode::Skitch {
                        if let (Some(line), Some((id, f))) = (lines.get(&replay.cursor.line), sim.follow) {
                            if id == replay.cursor.line {
                                let t = &rules.ground_steer;
                                let near = path_follow::advance(line, f.a, t.near_ticks);
                                let far = path_follow::advance(line, f.a, t.far_ticks);
                                let deck = GamePhysics::context_deck(&sim.context);
                                let up = deck.basis.columns[1];
                                let input = ground_steer::GroundSteerInput {
                                    forward,
                                    fakie: sim.body.runtime.animation.packet.riding_fakie,
                                    up: [up[0], up[1], up[2]],
                                    path_forward: path_follow::forward(line, near),
                                    on_path,
                                    position: projected_point(sim, state).0,
                                    near: path_follow::point(line, near),
                                    far: avoid_far(line, f.a, far, &avoid.last, t),
                                    dist: f.dist,
                                    inside_lane: f.inside_lane,
                                    lateral: f.lateral,
                                    avoiding: steer.byte_933,
                                    category_9: sim.slot.category == 9,
                                };
                                let g = ground_steer::heading_error(&input, t);
                                slide_error = g.error;
                                ground_steered = true;
                                sim.ground_error = Some(g.error);
                                let s = ground_steer::stick(g.error, t.full_deg);
                                intents.extend(ground_steer::intents(s).into_iter().map(|(k, v)| (k.to_string(), v)));
                                if sim.chain_trace > 0 {
                                    if sim.chain_trace % 30 == 0 {
                                        info!(
                                            "NPC_SKATER_REJOIN #{} left {} error {:.1} deg stick {s:.2} rejoining {} on_path {on_path} dist {:.2} A {}+{:.2} B {}+{:.2} at [{:.2}, {:.2}, {:.2}] stuck {}",
                                            npc.id.serial, sim.chain_trace, g.error.to_degrees(), g.rejoining, f.dist, f.a.0, f.a.1, f.b.0, f.b.1,
                                            input.position[0], input.position[1], input.position[2], replay.avoid.controller.sub_mode,
                                        );
                                    }
                                    sim.chain_trace -= 1;
                                }
                                if sim.trace_ticks > 0 {
                                    let h = |v: [f32; 3]| v[0].atan2(v[2]).to_degrees();
                                    let pf = path_follow::forward(line, near);
                                    let w = GamePhysics::context_deck_velocity(&sim.context);
                                    debug!(
                                        "NPC_SKATER_STEER #{} error {:.2} deg stick {s:.3} rejoining {} dist {:.2} deck_yaw {:.1} fakie {} line_yaw {:.1} up_y {:.2} vel_yaw {:.1} speed {:.2} state {:?} flipped {} root_yaw {:.1} eff_yaw {:.1} cat {:?}",
                                        npc.id.serial, g.error.to_degrees(), g.rejoining, f.dist, h(forward), input.fakie, h(pf), up[1],
                                        h(w), (w[0] * w[0] + w[2] * w[2]).sqrt(), state,
                                        // b201 decision fields: the board-flipped bit, the fakie rule's exact input (effective root Z) and the effective
                                        // board forward, and the filtered category.
                                        sim.body.runtime.animation.packet.board_flipped, { let z = effective_root(&sim.body.runtime)[2]; h([z[0], z[1], z[2]]) },
                                        h(GamePhysics::context_effective_forward(&sim.context)),
                                        sim.body.runtime.player_state.filtered_output.as_ref().map(|o| o.category)
                                    );
                                }
                            }
                        }
                    }
                }
                // The push below the line's speed (`sub_8246BAA8` -> `sub_8246B4C8`, b341): ground ticks on the board,
                // not in a hold or skitching, like the ground steer above.
                {
                    use skate_core::living_world::{ai_push, avoid::AvoidMode};
                    let p = &sim.body.runtime.player_input.processed;
                    let on_foot = p.flags_2484 & 0x0800_0000 != 0 || p.flags_2480 & 0x4 != 0;
                    let waiting = sim.respawn_in.is_some() || sim.teleport_pending || sim.hold_ticks > 0;
                    if let (false, false, false, true, Some(line)) = (airborne_now, on_foot, waiting, avoid.last.mode != AvoidMode::Skitch, lines.get(&replay.cursor.line)) {
                        let t = &rules.push;
                        let at = [deck.translation.x, deck.translation.y, deck.translation.z];
                        let off_line = (at[0] - target.position[0]).hypot(at[2] - target.position[2]);
                        let along = target.step;
                        let riding_fakie = sim.body.runtime.animation.packet.riding_fakie;
                        let facing = if riding_fakie { forward.map(|x| -x) } else { forward };
                        // `pc+824`, the ground steer's error, when it ran this tick: docs/hails-additions/26e-living-world-skater-ai.md#push-while-riding-fakie
                        let heading_error = if ground_steered { slide_error } else if along[0].hypot(along[2]) > 1e-5 { (facing[0] * along[2] - facing[2] * along[0]).atan2(facing[0] * along[0] + facing[2] * along[2]) } else { 0.0 };
                        let w = GamePhysics::context_deck_velocity(&sim.context);
                        let near = off_line <= t.look_ahead_distance;
                        let (node, into) = (replay.cursor.node, replay.cursor.frame_in_segment);
                        let input = ai_push::PushInput {
                            speed: (w[0] * w[0] + w[1] * w[1] + w[2] * w[2]).sqrt(),
                            // A racer's rubber band scales the line speed before the min / max clamp (82470830, b969).
                            target: avoid.last.shape_speed(ai_push::target_speed(target.step, off_line, t) * race.map_or(1.0, |c| c.speed_scale()), false),
                            category: sim.slot.category,
                            stuck_stage: replay.avoid.controller.sub_mode,
                            against_forward: ai_push::against_line(forward, riding_fakie, along),
                            heading_error,
                            trick_frames: near.then(|| ai_push::trick_frames(line, node, into, t)).flatten(),
                            off_board_frames: near.then(|| ai_push::off_board_frames(line, node, into, t)).flatten(),
                            slope: ai_push::slope(target.step),
                            mirrored: sim.body.runtime.animation.stance().1,
                            fakie: sim.body.runtime.animation.stance().0,
                        };
                        // At or above the target retail brakes instead (only far above it, `sub_8246B810`).
                        if let Some((key, magnitude)) = ai_push::brake(&input, replay.cursor.node >= 1, &rules.brake, t) {
                            intents.push((key.to_string(), magnitude));
                        }
                        if let Some(push) = ai_push::push(&input, t) {
                            if !sim.pushing {
                                debug!("NPC_SKATER_AI_PUSH #{} node {node} speed {:.2} target {:.2} slope {:.2}", npc.id.serial, input.speed, input.target, input.slope);
                            }
                            sim.pushing = true;
                            skate_core::living_world::ai_signals::drop_for_push(&mut intents);
                            intents.extend(push.into_iter().map(|(k, v)| (k.to_string(), v)));
                        } else {
                            sim.pushing = false;
                        }
                    } else {
                        sim.pushing = false;
                    }
                }
                skate_core::living_world::ai_signals::apply_skitch_mode(
                    &mut intents,
                    &rules.signals,
                    avoid.last.mode == skate_core::living_world::avoid::AvoidMode::Skitch,
                    (200..300).contains(&(state as u32)),
                    state == skate_core::player::state::PhysicalStateId::Skitching,
                );
                // Off the board: no recorded trick (pc +0xA1 gates 8246A2E0); off the path it still goes out (b279).
                let off_board = sim.body.runtime.player_input.processed.flags_2484 & 0x0800_0000 != 0;
                if off_board {
                    if intents.iter().any(|x| x.0 == "Trick") {
                        info!("NPC_SKATER_SIM_TRICK #{} {} dropped at node {}: off the board", npc.id.serial, npc.character, replay.cursor.node);
                    }
                    skate_core::living_world::ai_signals::drop_trick_dispatch(&mut intents);
                }
                // Sub-mode 4 (mode 6): the recorded node action is dropped (`ctrl+832 = 0`; b66).
                if avoid.low_prop() {
                    skate_core::living_world::ai_signals::drop_trick_dispatch(&mut intents);
                }
                let held = sim.hold_ticks > 0;
                if sim.hold_ticks > 0 {
                    // The hold counts once the placement has landed (retail places at once), and not while a racer is
                    // pinned (PinAI): its last tick, with the spawn push, comes after the release.
                    if !sim.teleport_pending && !race.is_some_and(|c| !c.moving) {
                        sim.hold_ticks -= 1;
                    }
                    skate_core::living_world::ai_signals::drop_trick_dispatch(&mut intents);
                }
                // The per-tick writer's slot arm (`sub_824696B8`, b113): the slot trick's timed intents. It runs inside
                // the dispatcher, so not when the dispatch was skipped above (skitch, off the board, hold).
                {
                    let skitch_skip = (avoid.last.mode == skate_core::living_world::avoid::AvoidMode::Skitch && !(200..300).contains(&(state as u32))) || state == skate_core::player::state::PhysicalStateId::Skitching;
                    if avoid.low_prop() {
                        sim.slot.category = 0;
                    }
                    if skitch_skip || off_board || held {
                        sim.slot = slot_before;
                    } else {
                        let opened = sim.slot.ticks == 0 && sim.slot.category != 0 && sim.slot != slot_before;
                        let schedule = usize::try_from(sim.slot.trick).ok().and_then(|t| sim.body.runtime.scoring.data.by_id(t)).map(|d| d.intents.clone()).unwrap_or_default();
                        if opened && !intents.iter().any(|x| x.0 == "Trick") {
                            if let Some(n) = lines.get(&replay.cursor.line).and_then(|l| l.nodes.get(replay.cursor.node as usize)) {
                                use skate_core::living_world::replay::{decode_orientation, rotate};
                                let z = [0.0, 0.0, 1.0];
                                let (b, k) = (rotate(decode_orientation(n.board), z), rotate(decode_orientation(n.skater), z));
                                info!("NPC_SKATER_SLOT_POSE #{} node {} recorded at [{:.2}, {:.2}, {:.2}] board [{:.2}, {:.2}] skater [{:.2}, {:.2}] actual at [{:.2}, {:.2}, {:.2}] board [{:.2}, {:.2}]", npc.id.serial, replay.cursor.node, n.position[0], n.position[1], n.position[2], b[0], b[2], k[0], k[2], deck.translation.x, deck.translation.y, deck.translation.z, forward[0], forward[2]);
                            }
                            info!("NPC_SKATER_SIM_SLOT #{} {} category {} {} node {} schedule {}", npc.id.serial, npc.character, sim.slot.category, super::npc_skaters::trick_name(sim.slot.trick), replay.cursor.node, schedule.len());
                        }
                        let slide = skate_core::living_world::ai_signals::Slide { on_path: on_path || left_path, heading_error: slide_error, values: rules.powerslide };
                        intents.extend(skate_core::living_world::ai_signals::slot_signals(&mut sim.slot, &schedule, 1.0, slide));
                    }
                    // Leaving the path clears the pending trick category once, after this tick's dispatch
                    // (`sub_8246DB50`: `pc+920 = 0` and `pc+832 = 0` in the same block; b279).
                    if left_path {
                        sim.slot.category = 0;
                    }
                }
                // Logged after mode 4 so a dropped trick dispatch is not reported.
                if let Some(name) = intents.iter().position(|x| x.0 == "Trick").and_then(|i| intents.get(i + 1)) {
                    info!("NPC_SKATER_SIM_TRICK #{} {} {} node {} state {:?}", npc.id.serial, npc.character, name.0, replay.cursor.node, state);
                    sim.takeoff_trace = 40;
                    sim.takeoff_logged = false;
                    sim.takeoff_launch = f32::NAN;
                }
                // NPC_SKATER_DRAWN once a second: where the drawn body is (render pose joints) against the board, so a
                // skater that vanishes on screen can be told apart from one that bails.
                if *calls % 60 == npc.id.serial as u64 % 60 {
                    let pose = sim.render_pose();
                    let deck = GamePhysics::context_deck(&sim.context).translation;
                    let bad = pose.iter().filter(|m| m.iter().flatten().any(|x| !x.is_finite())).count();
                    let (lo, hi) = pose.iter().fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), m| {
                        (std::array::from_fn(|i| lo[i].min(m[3][i])), std::array::from_fn(|i| hi[i].max(m[3][i])))
                    });
                    info!(
                        "NPC_SKATER_DRAWN #{} {} state {:?} deck [{:.1}, {:.1}, {:.1}] joints {} bad {bad} joint box [{:.1}, {:.1}, {:.1}]..[{:.1}, {:.1}, {:.1}]",
                        npc.id.serial, npc.character, state, deck.x, deck.y, deck.z, pose.len(), lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]
                    );
                }
                if sim.takeoff_trace > 0 {
                    sim.takeoff_trace -= 1;
                    let (p, _) = projected_point(sim, state);
                    let v = sim.last_deck.map_or(0.0, |q| ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt() * 60.0);
                    let anim = &sim.body.runtime.animation;
                    let ag = anim.action_controller.frame.current.and_then(|id| graphs.action.binding.states.get(id)).map_or("?", |s| s.name.as_str());
                    let mg = anim.motion_controller.frame.current.and_then(|id| graphs.motion.binding.states.get(id)).map_or("?", |s| s.name.as_str());
                    let processed = &sim.body.runtime.player_input.processed;
                    let record = sim.body.runtime.ai_physics.as_ref().map(|a| a.record.flags);
                    info!(
                        "NPC_TAKEOFF_TRACE #{} t{} {:?} {v:.2} m/s node {} prepare_jump {} record {:08X} ag {ag} mg {mg}",
                        npc.id.serial, 40 - sim.takeoff_trace, state, replay.cursor.node, processed.flags_2468 & (1 << 21) != 0, record.unwrap_or(0)
                    );
                    // NPC_TAKEOFF: the first air tick after the dispatch, our deck speed against the recording's own
                    // speed at this node and the one before, and the nearest recorded jump's start speed, so a slow
                    // take-off shows whether ours lost speed or the recording slows down there too.
                    let jump_out = &sim.body.runtime.ground_animation.jump;
                    if state == PhysicalStateId::GroundAnimation && jump_out.active {
                        let w = jump_out.velocity;
                        sim.takeoff_launch = (w[0] * w[0] + w[1] * w[1] + w[2] * w[2]).sqrt();
                    }
                    if !sim.takeoff_logged && matches!(state, PhysicalStateId::KnownAir | PhysicalStateId::PhysicsAir) {
                        sim.takeoff_logged = true;
                        if let Some(line) = lines.get(&replay.cursor.line) {
                            let node = replay.cursor.node as usize;
                            let rec = |i: usize| line.nodes.get(i).map_or(f32::NAN, |n| (n.step[0] * n.step[0] + n.step[1] * n.step[1] + n.step[2] * n.step[2]).sqrt() * 60.0);
                            let jump = (0..=node.min(line.nodes.len().saturating_sub(1))).rev().take(6).find_map(|i| line.nodes[i].jump.map(|j| (i, j)));
                            let (jump_node, jump_speed) = jump.and_then(|(i, j)| line.jumps.get(j as usize).map(|j| (i as i64, (j.start_velocity[0] * j.start_velocity[0] + j.start_velocity[1] * j.start_velocity[1] + j.start_velocity[2] * j.start_velocity[2]).sqrt()))).unwrap_or((-1, f32::NAN));
                            // The GroundAnimation launch (`82D93618`), kept in the body until its next GroundAnimation tick;
                            // `from` says whether this take-off came through GroundAnimation.
                            let launch = sim.takeoff_launch;
                            info!(
                                "NPC_TAKEOFF #{} {} {:?} from {} node {node} speed {v:.2} launch {launch:.2} recorded_here {:.2} recorded_before {:.2} jump_node {jump_node} jump_speed {jump_speed:.2}",
                                npc.id.serial, npc.character, state, sim.last_state.map_or("-".to_string(), |s| format!("{s:?}")), rec(node), node.checked_sub(1).map_or(f32::NAN, rec)
                            );
                        }
                    }
                }
                if sim.trace_ticks > 0 {
                    sim.trace_ticks -= 1;
                    let (p, _) = projected_point(sim, state);
                    let v = sim.last_deck.map_or(0.0, |q| ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt() * 60.0);
                    let names: Vec<&str> = intents.iter().map(|x| x.0.as_str()).filter(|n| !n.starts_with("OB_")).collect();
                    // The action / motion graph states, to see why an intent was or was not taken.
                    let anim = &sim.body.runtime.animation;
                    let ag = anim.action_controller.frame.current.and_then(|id| graphs.action.binding.states.get(id)).map_or("?", |s| s.name.as_str());
                    let mg = anim.motion_controller.frame.current.and_then(|id| graphs.motion.binding.states.get(id)).map_or("?", |s| s.name.as_str());
                    debug!(
                        "NPC_SKATER_PLACED #{} t{} {:?} at [{:.2}, {:.2}, {:.2}] {v:.1} m/s node {} on_path {on_path} past {past_start} air {airborne_now} flags {:08X} ag {ag} mg {mg} intents {names:?}",
                        npc.id.serial, trace_ticks() - sim.trace_ticks, state, p[0], p[1], p[2], replay.cursor.node, sim.body.runtime.ai_physics.as_ref().map_or(0, |a| a.record.flags)
                    );
                }
                // The handover's start speed, once: the first tick with a measured speed after its teleport landed.
                if sim.handover_push.is_some() && !sim.teleport_pending && sim.last_deck.is_some() {
                    let push = sim.handover_push.take().unwrap_or(0.0);
                    let (p, _) = projected_point(sim, state);
                    let v = sim.last_deck.map_or(0.0, |q| ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt() * 60.0);
                    let deck = GamePhysics::context_deck(&sim.context).translation;
                    let off = lines.get(&replay.cursor.line).and_then(|l| l.nodes.get(replay.cursor.node as usize)).map_or(f32::NAN, |n| (deck.x - n.position[0]).hypot(deck.z - n.position[2]));
                    info!(
                        "NPC_SKATER_SIM #{} {} start speed {v:.1} m/s after the handover (push {push:.1} m/s, deck {off:.2} m from node {}, state {:?})",
                        npc.id.serial, npc.character, replay.cursor.node, state
                    );
                }
                sim.last_node = Some(replay.cursor.node);
                sim.last_state = Some(state);
                sim.last_deck = Some(projected_point(sim, state).0);
                lift_pending(sim, npc);
                // The step itself runs after the loop, with the other NPCs' steps (in parallel).
                jobs.push(Job { entity: e, serial: npc.id.serial, character: npc.character.clone(), intents });
            }
            None => {
                // Clients draw what the host sends; only the authority simulates.
                let Some(line) = lines.get(&replay.cursor.line) else { continue };
                let flags = line.nodes[replay.cursor.node as usize].flags;
                let wait = if !rules.enabled {
                    Some("off")
                } else if pool.retry_after.contains_key(&e) {
                    Some("after a physics error")
                } else if settings.net_role == NetRole::Client {
                    Some("client")
                } else if race.is_some_and(|c| !c.moving && c.start.is_none()) {
                    // A challenge racer is a full skater at any distance and over the ambient cap (race_ai_host; ambient
                    // skaters stop while racers ride); a pinned one is placed at its start slot below.
                    Some("race rider pinned")
                } else if count >= rules.max && race.is_none() {
                    Some("at max")
                } else if race.is_none() && distance > rules.radius &&!(rules.walk_back && replay.avoid.last.mode == skate_core::living_world::avoid::AvoidMode::StepOff) {
                    // Retail simulates every ambient skater: one a prop blocks (mode 7) is simulated at any distance so
                    // it can step off and walk round (controller B); our distance switch is an engine cost choice.
                    Some("too far")
                } else if race.is_some_and(|c| !c.moving) {
                    // Its body comes from the loader thread's top-up (above), not a load here.
                    pool.poll_load();
                    // spawn_sim joins a running load, so the wait covers the whole load, not only an empty pool.
                    pool.loading.is_some().then_some("race rider waiting for a loaded body")
                } else if flags & (node_flags::AIRBORNE | node_flags::OFF_BOARD) != 0 || replay.cursor.current_trick() >= 0 {
                    Some("in the air, off board or in a trick")
                } else if replay.cursor.frame_in_segment != 0 {
                    // Retail starts an AI skater on a path node (node 0, or the chosen node on a respawn) with the
                    // spawn push; between nodes the push never applies and the sim dropped in from rest (b109:
                    // 21 of 44 bails within 1 s of the start).
                    Some("between nodes")
                } else if skate_core::living_world::ai_controller::safe_node(line, replay.cursor.node, &rules.controller) != replay.cursor.node {
                    // Retail only ever places an AI skater on a node its safe-node pick returns (`sub_82455728`: a run
                    // of calm nodes, no trick, flag or steep slope ahead). Without it the handover landed one node
                    // before a recorded lip and the skater stalled in the grind (laptop 2026-10-17, node 33, b178).
                    Some("not a safe node")
                } else {
                    None
                };
                if let Some(reason) = wait {
                    if report {
                        info!("NPC_SKATER_SIM #{} {} replay: {reason} (distance {distance:.1} m, radius {:.0} m)", npc.id.serial, npc.character, rules.radius);
                    }
                    continue;
                }
                // One hold tick at a handover: retail's order (placed at rest, pushed, then steered) without a visible stop.
                // A pinned racer: at its slot facing the slot heading, pushed along its line on release (PinAI, b972 §4).
                let at_slot = race.is_some_and(|c| !c.moving && c.start.is_some());
                let target = match race.and_then(|c| (!c.moving).then_some(c.start).flatten()) {
                    Some((at, heading)) => {
                        let (s, c) = (heading * 0.5).sin_cos();
                        ai_record::LineTarget { position: at, frame: [0.0, s, 0.0, c], step: target.step }
                    }
                    None => target,
                };
                match spawn_sim(&mut physics, &config, &graphs, &target, &mut pool, HANDOVER_HOLD_TICKS, at_slot) {
                    Ok(sim) => {
                        let why = if replay.avoid.last.mode == skate_core::living_world::avoid::AvoidMode::StepOff { ", blocked by a prop (mode 7)" } else { "" };
                        let push = sim.handover_push.unwrap_or(0.0);
                        info!("NPC_SKATER_SIM #{} {} <- replay (distance {distance:.1} m{why}, node {}, push {push:.1} m/s)", npc.id.serial, npc.character, replay.cursor.node);
                        commands.entity(e).insert(sim);
                        count += 1;
                    }
                    Err(error) => warn!("NPC_SKATER_SIM #{} {}: cannot simulate: {error}", npc.id.serial, npc.character),
                }
            }
        }
    }
    let player_body = (rules.bump && net.is_some()).then(|| crate::physics::network::capture_body(&physics, &player));
    let bump = player_body.as_ref().zip(net.as_ref().map(|n| &n.schema));
    run_steps(&mut physics, &mut npcs, bump, &graphs, &mut workers, jobs, &mut commands, &mut count);

}

/// The reaction manager (8245A7E8 per tick): players and every NPC skater in stable order; host only.
pub(crate) fn reactions(
    settings: Res<LivingWorldSettings>,
    observers: Res<LivingWorldObservers>,
    npcs: Query<(Entity, &NpcSkater, &Transform, Option<&NpcSim>)>,
    mut state: Local<skate_core::living_world::npc_reactions::ReactionState>,
    mut out: MessageWriter<crate::world_audio::NpcSkaterReactionEvent>,
) {
    use skate_core::living_world::npc_reactions::{ReactionSkater, NO_MODEL};
    if settings.net_role == NetRole::Client {
        return;
    }
    let mut skaters: Vec<ReactionSkater> = observers
        .observers
        .iter()
        .enumerate()
        .map(|(i, o)| ReactionSkater { key: i as u64, position: o.position, wiping: observers.wiping.get(i).copied().unwrap_or(false), model: 0, listens: false, score: observers.scores.get(i).copied().unwrap_or(0) })
        .collect();
    let mut entities = std::collections::BTreeMap::new();
    let mut list: Vec<_> = npcs.iter().collect();
    list.sort_by_key(|(_, npc, ..)| npc.id.serial);
    for (e, npc, transform, sim) in list {
        // NPC keys after every player slot.
        let key = (1u64 << 32) + u64::from(npc.id.serial);
        let wiping = sim.is_some_and(|s| s.body.runtime.player_input.processed.flags_2468 & WIPEOUT_BIT != 0);
        skaters.push(ReactionSkater { key, position: transform.translation.to_array(), wiping, model: npc.voice.unwrap_or(NO_MODEL), listens: true, score: 0 });
        entities.insert(key, e);
    }
    use crate::world_audio::{NpcSkaterReactionEvent, SkaterReaction};
    for (key, r) in state.step(&settings.npc_simulated.reactions, &skaters) {
        let Some(&skater) = entities.get(&key) else { continue };
        if let Some(by) = r.slam {
            out.write(NpcSkaterReactionEvent { skater, reaction: SkaterReaction::Slam, by });
        }
        if let Some(by) = r.slam_b {
            out.write(NpcSkaterReactionEvent { skater, reaction: SkaterReaction::SlamB, by });
        }
        if let Some(by) = r.trick {
            out.write(NpcSkaterReactionEvent { skater, reaction: SkaterReaction::Trick, by });
        }
        if r.crash {
            out.write(NpcSkaterReactionEvent { skater, reaction: SkaterReaction::Crash, by: 0 });
        }
        if r.slam.is_some() || r.slam_b.is_some() {
            debug!("NPC_SKATER_REACTION key={key} slam={:?} slam_b={:?} crash={}", r.slam, r.slam_b, r.crash);
        }
    }
}

#[cfg(test)]
mod pool_loader_tests {
    use super::*;
    use skate_core::physics::board::BodyId;

    /// The pool loader thread (`simulate`'s prewarm: a released `npc_worker`, `load_body_from` on its own thread,
    /// the shared source carried along) gives the bodies a load on the game thread gives: two of each, in order,
    /// ride the same push for 240 ticks and their decks, skeleton roots and states match bit for bit every tick.
    #[test]
    #[ignore = "requires private stock graphs and an installed map (SKATE3_ASSET_ROOT, SKATE3_MAP=<maps/DownTown.skate>)"]
    fn pool_bodies_loaded_on_the_loader_thread_ride_like_ones_loaded_here() {
        let root_dir = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").unwrap());
        let map_path = std::path::PathBuf::from(std::env::var_os("SKATE3_MAP").unwrap());
        let map = skate_data::skate_map::SkateMap::load(&map_path).unwrap();
        let assets = skate_data::GameAssets::load(&root_dir).unwrap();
        let graphs = crate::graph_runtime::StockGraphs::load(&root_dir, &assets).unwrap();
        let mut physics = GamePhysics::load_with_difficulty(&root_dir, Some(&map), crate::difficulty::Difficulty::Easy).unwrap();
        let spawn = physics.board.part_transforms()[BodyId::Deck.index()];
        let mut here_source = None;
        let mut here: Vec<_> = (0..2).map(|_| load_body_from(&mut physics, &root_dir, &graphs, "easy", spawn, &mut here_source).unwrap()).collect();
        let mut worker = physics.npc_worker().unwrap();
        GamePhysics::release_npc_worker(&mut worker);
        let (root, thread_graphs) = (root_dir.clone(), graphs.clone());
        let mut there: Vec<_> = std::thread::spawn(move || {
            let mut source = None;
            (0..2).map(|_| load_body_from(&mut worker, &root, &thread_graphs, "easy", spawn, &mut source).unwrap()).collect::<Vec<_>>()
        })
        .join()
        .unwrap();
        for t in 0..240 {
            for (i, ((ca, a), (cb, b))) in here.iter_mut().zip(there.iter_mut()).enumerate() {
                if t < 30 {
                    GamePhysics::set_context_velocity(ca, [0.0, 0.0, 5.0]);
                    GamePhysics::set_context_velocity(cb, [0.0, 0.0, 5.0]);
                }
                physics.advance_npc_skater(ca, &mut a.runtime, &mut a.controls, &graphs, &mut a.camera, &[]).unwrap();
                physics.advance_npc_skater(cb, &mut b.runtime, &mut b.controls, &graphs, &mut b.camera, &[]).unwrap();
                assert_eq!(format!("{:?}", GamePhysics::context_deck(ca)), format!("{:?}", GamePhysics::context_deck(cb)), "body {i}: deck differs at tick {t}");
                assert_eq!(
                    format!("{:?}", a.runtime.animated_skeleton.roots.animation_to_world),
                    format!("{:?}", b.runtime.animated_skeleton.roots.animation_to_world),
                    "body {i}: skeleton root differs at tick {t}"
                );
                assert_eq!(a.runtime.player_state.current(), b.runtime.player_state.current(), "body {i}: state differs at tick {t}");
            }
        }
    }
}
