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
//! - per MixMap evaluation: instances go to the nearest owners (`owners::Pool`, provisional rule),
//!   each instance's objects `update` from this evaluation's outputs, then `process` writes the
//!   3DObjPos blocks and posts for the next one. Retail runs process before its tick and update
//!   after; here both follow `native::mixmap_frame`'s tick, so the inputs an evaluation sees are
//!   one console frame old (33 ms) — the seam to move into `mixmap_frame` once a system exists.
//! - ped speech requests are logged (`AUDIO_WORLD speech`): the speech archive's playback needs
//!   the opt-in speech export and the speech manager's level mapping (`world-speech.md`).
use std::collections::HashMap;

use bevy::prelude::*;
use serde::Deserialize;
use skate_audio::eval::NodeId;
use skate_audio::mixmap::cadence::CONSOLE_DT;
use skate_audio::player::objpos::Listener;
use skate_audio::world::owners::{Pool, Positions};
use skate_audio::world::peds::{PedFootstepTuning, PedSfx, PedSpeech, PedState};
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
}

/// The install's world tuning (`audio_manifest.json` `world_tuning`, setup
/// `tools/asset_pipeline/world_audio.py`; absent in older installs: the defaults apply).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct WorldTuningJson {
    /// `aud_traffic_engine` records by name (`default`, `c01_family01`, …).
    traffic_engine: HashMap<String, EngineJson>,
    ped_footsteps: Option<PedFootstepsJson>,
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
    ped_objects: HashMap<u64, (PedSfx, PedSpeech, Positions)>,
    nodes: HashMap<(u64, WorldSlot), NodeId>,
    classes: HashMap<&'static str, usize>,
    last_tick: u64,
    rng: Lcg,
    ped_tuning: Option<PedFootstepTuning>,
    player_tuning: Option<skate_audio::player::tuning::PlayerTuning>,
    /// The camera at the last evaluation and the host's cut count then (`Native::cuts`: no
    /// velocity across a teleport / map change).
    last_camera: Option<([f32; 3], u64)>,
    prefetch: WorldPrefetch,
    /// `Native::map_epoch` this host last ran in (None: never ran; [`WorldHost::reset`]).
    epoch: Option<u64>,
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
            last_tick: 0,
            rng: Lcg(0x5EED),
            ped_tuning: None,
            player_tuning: None,
            last_camera: None,
            prefetch: WorldPrefetch::default(),
            epoch: None,
        }
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
        .add_systems(Update, frame.after(super::native::mixmap_frame));
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
                host.nodes.insert((owner, slot), rt.post(id, &words));
            }
            WorldCommand::Redeliver { owner, slot, words } => {
                if let Some(&node) = host.nodes.get(&(owner, slot)) {
                    rt.redeliver(node, &words);
                }
            }
            WorldCommand::Release { owner, slot } => {
                if let Some(node) = host.nodes.remove(&(owner, slot)) {
                    rt.release(node);
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

/// A candidate inside the list radius keeps its distance; one outside never gets an instance.
fn within(d: f32, radius: f32) -> f32 {
    if d < radius { d } else { f32::INFINITY }
}

/// The seconds one MixMap evaluation covers (`native::mixmap_frame`: the console cadence, 1/30).
pub(super) fn evaluation_dt() -> f32 {
    CONSOLE_DT
}

impl WorldHost {
    /// The map changed (`Native::map_epoch`, bumped by `unload_map_banks`), or the host runs for
    /// the first time: the unload destroyed every instance of the world banks, so every held node
    /// is released (harmless on a dead node; it frees them), the pools forget their holders, the
    /// held instances' 3DObjPos blocks go inactive, the ped Splice steps stop, and the per-owner
    /// objects are dropped. An owner that is still published is claimed again and posts afresh.
    /// The banks reload at the next owner (`ensure_bank`, through the prefetch when expected).
    /// The pools take the MixMap's instance counts (`Native::world`).
    fn reset(&mut self, native: &mut Native) {
        self.epoch = Some(native.map_epoch);
        let traffic = self.traffic.clear();
        let peds = self.peds.clear();
        let Native { mixmap, shared, world, .. } = native;
        if !self.nodes.is_empty() || !self.ped_objects.is_empty() {
            if let Ok(mut runtime) = super::timing::lock(shared, &super::timing::GAME_LOCK) {
                let rt = &mut *runtime;
                for (owner, (mut sfx, _, _)) in std::mem::take(&mut self.ped_objects) {
                    let _ = sfx.release(owner, &mut rt.splice_host());
                }
                for (_, node) in self.nodes.drain() {
                    rt.release(node);
                }
            }
        }
        self.ped_objects.clear();
        self.vehicles.clear();
        self.nodes.clear();
        if let Some(m) = mixmap.as_mut() {
            let l = Listener::default();
            for (_, g) in traffic {
                Positions::new(&[keys::traffic_pos(g as u32, 1), keys::traffic_pos(g as u32, 2), keys::traffic_pos(g as u32, 3)]).deactivate(m, &l);
            }
            for (_, g) in peds {
                Positions::new(&[keys::ped_pos(g as u32)]).deactivate(m, &l);
            }
            self.last_tick = self.last_tick.min(m.ticks);
        }
        if self.traffic.len() != world.traffic {
            self.traffic = Pool::new(world.traffic);
        }
        if self.peds.len() != world.peds {
            self.peds = Pool::new(world.peds);
        }
        self.banks = None;
        self.last_camera = None;
    }

    fn held(&self) -> (Vec<(u64, u32)>, Vec<(u64, u32)>) {
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
    run(&mut host, &owners, &mut native, &library, camera, &cues.riding.audio);
    let (traffic, peds) = host.held();
    if held.traffic != traffic || held.peds != peds {
        held.traffic = traffic;
        held.peds = peds;
    }
}

/// One frame of the host (after the inert checks): the prefetch, the map-change reset, the banks,
/// and per MixMap evaluation the instance assignment, update and process. `camera` = the listener
/// (position, forward); `local` = the local player's audio state (the followed point).
pub(crate) fn run(host: &mut WorldHost, owners: &WorldOwners, native: &mut Native, library: &Library, camera: Option<([f32; 3], [f32; 3])>, local: &skate_audio::player::AudioState) {
    if prefetch_on() {
        prefetch_world_banks(&mut host.prefetch, native, library, owners.expected);
    }
    if host.epoch != Some(native.map_epoch) {
        host.reset(native);
    }
    let idle = owners.vehicles.is_empty() && owners.peds.is_empty() && host.vehicles.is_empty() && host.ped_objects.is_empty();
    if idle {
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
        host.player_tuning = native.player.as_ref().map(|p| p.tuning.clone());
        info!("AUDIO_WORLD on: {} vehicles, {} peds published", owners.vehicles.len(), owners.peds.len());
    }
    host.banks = Some(!missing);
    let Some((cam, view)) = camera else { return };
    let native = &mut *native;
    let Some(m) = native.mixmap.as_mut() else { return };
    if m.ticks == host.last_tick {
        return;
    }
    // The MixMap is built once and never rebuilt, so its tick count only grows; the saturating
    // difference keeps a host that ran ahead (a reset, a new MixMap in a test) from underflowing.
    let evaluations = m.ticks.saturating_sub(host.last_tick).max(1);
    host.last_tick = m.ticks;
    let dt = evaluation_dt() * evaluations.min(4) as f32;
    let cam_velocity = host.last_camera.filter(|l| l.1 == native.cuts).map_or([0.0; 3], |(last, _)| std::array::from_fn(|i| (cam[i] - last[i]) / dt));
    host.last_camera = Some((cam, native.cuts));
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
    // Taken out for the frame (no per-frame clone) and put back at the end.
    let ped_tuning = host.ped_tuning.take().unwrap_or_default();
    let player_tuning = host.player_tuning.take().unwrap_or_default();

    // Instances: the nearest N inside retail's list radii (traffic: horizontal, 40 m; peds: 3-D,
    // 50 m). `owners::Pool`.
    let traffic_candidates: Vec<(u64, f32)> = owners.vehicles.iter().map(|(&id, v)| (id, within(horizontal(v.position, cam), TRAFFIC_LIST_RADIUS))).collect();
    let ped_candidates: Vec<(u64, f32)> = owners.peds.iter().map(|(&id, p)| (id, within(distance(p.position, cam), PED_LIST_RADIUS))).collect();
    let traffic = host.traffic.assign(&traffic_candidates);
    let peds = host.peds.assign(&ped_candidates);
    for (owner, _) in traffic.released {
        if let Some((mut vehicle, mut pos)) = host.vehicles.remove(&owner) {
            pos.deactivate(m, &l);
            let cmds = vehicle.release(owner);
            apply(host, rt, cmds);
        }
    }
    for (owner, _) in peds.released {
        if let Some((mut sfx, _, mut pos)) = host.ped_objects.remove(&owner) {
            pos.deactivate(m, &l);
            let cmds = sfx.release(owner, &mut rt.splice_host());
            apply(host, rt, cmds);
        }
    }
    for (owner, g) in traffic.claimed {
        let g = g as u32;
        host.vehicles.insert(owner, (Vehicle::default(), Positions::new(&[keys::traffic_pos(g, 1), keys::traffic_pos(g, 2), keys::traffic_pos(g, 3)])));
    }
    for (owner, g) in peds.claimed {
        host.ped_objects.insert(owner, (PedSfx::default(), PedSpeech::default(), Positions::new(&[keys::ped_pos(g as u32)])));
    }

    // Update (this evaluation's outputs), then process (inputs and posts for the next one).
    let holders: Vec<(usize, u64)> = host.traffic.holders().collect();
    for (g, owner) in holders {
        let Some(v) = owners.vehicles.get(&owner) else { continue };
        let Some((mut vehicle, mut pos)) = host.vehicles.remove(&owner) else { continue };
        let mut cmds = vehicle.update(owner, g as u32, v, m, cam, dt);
        let p = Some((v.position, v.velocity));
        pos.write(m, &l, &[p, p, p]);
        cmds.extend(vehicle.process(owner, v, &mut host.rng));
        apply(host, rt, cmds);
        host.vehicles.insert(owner, (vehicle, pos));
    }
    let holders: Vec<(usize, u64)> = host.peds.holders().collect();
    for (g, owner) in holders {
        let Some(p) = owners.peds.get(&owner) else { continue };
        let Some((mut sfx, mut speech, mut pos)) = host.ped_objects.remove(&owner) else { continue };
        let out = OutputsSnapshot::take(m, keys::ped_sfx(g as u32), &[7, 8]);
        let mut cmds = sfx.update(owner, p, &ped_tuning, &player_tuning, &out, &mut rt.splice_host(), dt);
        pos.write(m, &l, &[Some((p.position, p.velocity))]);
        cmds.extend(sfx.process(owner, p, &ped_tuning, &mut rt.splice_host(), dt));
        if let Some(r) = speech.process(owner, p) {
            info!(
                "AUDIO_WORLD speech owner={} value={} ({}) flag={}",
                r.owner,
                r.value,
                skate_audio::world::speech::speech_value_name(r.value).unwrap_or("?"),
                r.flag
            );
        }
        apply(host, rt, cmds);
        host.ped_objects.insert(owner, (sfx, speech, pos));
    }
    host.ped_tuning = Some(ped_tuning);
    host.player_tuning = Some(player_tuning);
}

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

    /// The map-change regression (spec §2.1, data-gated): a vehicle and a ped publish, their
    /// instances post and the C04 engine sounds; `unload_map_banks` destroys the world banks'
    /// instances; the same ids keep publishing and must post again (before the epoch reset the
    /// engine kept redelivering to its dead node and stayed silent for the rest of its life).
    /// Also: with no owners a map change costs nothing but the epoch bookkeeping.
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
                m.tick(CONSOLE_DT);
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
        native.mixmap.as_mut().unwrap().tick(CONSOLE_DT);
        run(&mut host, &owners, &mut native, &library, camera, &local);
        assert!(host.nodes.is_empty() && host.vehicles.is_empty() && host.ped_objects.is_empty());
        // A map change with nothing held.
        native.unload_map_banks();
        run(&mut host, &owners, &mut native, &library, camera, &local);
        assert_eq!(host.epoch, Some(native.map_epoch));
    }
}
