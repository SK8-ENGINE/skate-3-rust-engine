//! Transactional world replacement: prepare without mutating the live session,
//! then commit at a schedule boundary with gameplay suspended.
use bevy::prelude::*;
use std::{path::PathBuf, sync::{Arc, OnceLock, atomic::{AtomicU8, Ordering}}, thread::JoinHandle, time::Instant};
use crate::{config::Config, map_library::Entry, map_render::PreparedScene,
    physics::{GamePhysics, PlayerControls, SkaterRuntime}};

/// Persistent character customisation can reapply after this set, while the
/// transition still holds the loading overlay and gameplay remains suspended.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct MapTransitionSet;
#[derive(Message)]
pub(crate) struct WorldChanged;


#[cfg(test)]
#[path = "tests/map_transition.rs"]
mod tests;

#[derive(Resource, Debug)]
pub(crate) struct CurrentMap {
    pub path: Option<PathBuf>,
    pub name: String,
    pub spawn: [f32; 3],
    pub heading: f32,
    pub generation: u64,
    /// The map's own audio definition: its `AUDO` extension (schema 1, UTF-8 JSON;
    /// `game_audio::map_audio`), if it has one.
    pub audio_tag: Option<Arc<[u8]>>,
    /// The skate.Park layout the map was loaded with (park_layout.rs), if any.
    pub layout: Option<String>,
}
impl CurrentMap {
    fn from_package(path: Option<PathBuf>, map: Option<&skate_data::skate_map::SkateMap>) -> Self {
        Self { path, name: map.map_or_else(|| "Test world".into(), |m| m.name.clone()),
            spawn: map.map_or([0., crate::physics::ground::HEIGHT, 0.], |m| m.spawn),
            heading: map.map_or(0., |m| m.heading), generation: 0,
            audio_tag: map.and_then(|m| m.extensions.iter().find(|e| e.tag == *b"AUDO" && e.schema == 1)).map(|e| Arc::from(&e.payload[..])),
            layout: map.and_then(crate::park_layout::layout_of).map(str::to_owned) }
    }
}

struct PreparedWorld {
    map_fingerprint: u64,
    scene: PreparedScene,
    physics: GamePhysics,
    skater: SkaterRuntime,
    controls: PlayerControls,
    camera: crate::camera::CameraRuntime,
    metadata: CurrentMap,
    retail: bool,
    triggers: crate::trigger_volumes::TriggerVolumes,
    difficulty: crate::difficulty::Difficulty,
    /// The mods' dynamics ground for the new map, built here so the switch does not build it on the game thread.
    mod_ground: Result<skate_dynamics::GroundCollider, String>,
    /// The new map's living world, ped, traffic and character lighting data (`map_read_ahead`).
    game_data: crate::map_read_ahead::MapData,
}

enum Phase {
    Idle,
    Requested(Entry),
    /// `spawn`: the new map's spawn point, set once the loader has read the map (the audio reads its banks ahead).
    Loading { entry: Entry, progress: Arc<AtomicU8>, spawn: Arc<OnceLock<[f32; 3]>>, job: JoinHandle<Result<PreparedWorld, String>> },
    // Let extraction see the committed scene before releasing the pause menu.
    Publishing { frames: u8, notice: String },
}
#[derive(Resource)]
pub(crate) struct MapTransition { phase: Phase, cover_wait: Option<Instant> }
impl Default for MapTransition {
    fn default() -> Self { Self { phase: Phase::Idle, cover_wait: None } }
}

/// The front end's loading screen for this load (`retail_menu_stage`, FE 52): while it is `active` the commit waits
/// until it `covered` the screen (black or the screen up), at most [`COVER_WAIT`], so the world never changes in view.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LoadingCover {
    /// The front end has its loading screen (the set has the movie): a load started with the PC overlay closed uses it.
    pub available: bool,
    pub active: bool,
    pub covered: bool,
}

/// A loading screen that does not cover in this time no longer holds the commit.
const COVER_WAIT: std::time::Duration = std::time::Duration::from_secs(5);
impl MapTransition {
    pub fn busy(&self) -> bool { !matches!(self.phase, Phase::Idle) }
    /// The map being loaded, until it is committed (the audio reads its zone beds ahead, `map_audio`).
    pub(crate) fn loading(&self) -> Option<&Entry> {
        match &self.phase { Phase::Requested(entry) | Phase::Loading { entry, .. } => Some(entry), _ => None }
    }
    /// The spawn point of the map being loaded, once the loader has read it.
    pub(crate) fn loading_spawn(&self) -> Option<[f32; 3]> {
        match &self.phase { Phase::Loading { spawn, .. } => spawn.get().copied(), _ => None }
    }
    pub fn request(&mut self, entry: Entry) {
        if !self.busy() { self.phase = Phase::Requested(entry); }
    }
    pub fn label(&self) -> String {
        match &self.phase {
            Phase::Idle => String::new(),
            Phase::Requested(entry) => format!("Loading {} — preparing…", entry.label),
            Phase::Loading { entry, progress, .. } => format!("Loading {} — {}…", entry.label,
                match progress.load(Ordering::Relaxed) {
                    0 => "reading and validating map", 1 => "building collision and rendering",
                    2 => "initializing skater, camera and rendering", _ => "finishing meshes, textures and sky",
                }),
            Phase::Publishing { .. } => "Loading — publishing the new world…".into(),
        }
    }
}

pub(crate) struct MapTransitionPlugin;
impl Plugin for MapTransitionPlugin {
    fn build(&self, app: &mut App) {
        let config = app.world().resource::<Config>();
        let current = CurrentMap::from_package(config.map_path.clone(), config.map.as_ref());
        app.insert_resource(current)
            .init_resource::<MapTransition>()
            .init_resource::<LoadingCover>()
            .add_message::<WorldChanged>()
            .add_systems(PreUpdate, poll.in_set(MapTransitionSet).after(crate::graphics_menu::MenuInput)
                .before(crate::input::poll_controllers))
            .add_systems(PreUpdate, scripted_switch.before(MapTransitionSet))
            .add_systems(PreUpdate, crate::map_render::upload_slices.after(MapTransitionSet))
            .configure_sets(FixedUpdate, (
                crate::app::SimulationSet::Input, crate::app::SimulationSet::Controls,
                crate::app::SimulationSet::Physics,
            ).run_if(crate::graphics_menu::gameplay_active));
    }
}

/// Diagnostics: `SKATE_MAP_SWITCH=<seconds>:<map .skate path>` requests one map switch that many seconds after
/// start, so an unattended run (`--trace`, frame log) can measure a switch (2026-10-17: a 1 to 2 s "audio" stage
/// frame after every switch). Unset, nothing happens.
fn scripted_switch(time: Res<Time<Real>>, mut transition: ResMut<MapTransition>, mut done: Local<bool>) {
    static PLAN: std::sync::OnceLock<Option<(f32, std::path::PathBuf)>> = std::sync::OnceLock::new();
    let plan = PLAN.get_or_init(|| parse_scripted_switch(&std::env::var("SKATE_MAP_SWITCH").ok()?));
    let Some((after, path)) = plan else { return };
    if *done || time.elapsed_secs() < *after || transition.busy() {
        return;
    }
    *done = true;
    info!("SKATE_MAP_SWITCH requesting {} after {:.1} s", path.display(), time.elapsed_secs());
    transition.request(Entry { label: path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned()), path: Some(path.clone()), world: None });
}

/// `<seconds>:<path>` (the path may hold ':' itself, e.g. a drive letter).
fn parse_scripted_switch(value: &str) -> Option<(f32, std::path::PathBuf)> {
    let (seconds, path) = value.split_once(':')?;
    let seconds = seconds.trim().parse::<f32>().ok().filter(|s| s.is_finite() && *s >= 0.0)?;
    (!path.trim().is_empty()).then(|| (seconds, std::path::PathBuf::from(path.trim())))
}

fn start(world: &World, entry: Entry) -> Result<Phase, String> {
    let config = world.resource::<Config>();
    let root = config.asset_root.clone();
    let difficulty = config.difficulty;
    let graphs = world.resource::<crate::graph_runtime::StockGraphs>().clone();
    let source = world.resource::<SkaterRuntime>().animation.source.clone();
    let preferences = world.resource::<PlayerControls>().preferences;
    let nav_rules = world.get_resource::<crate::living_world::peds::PedNavSettings>().map(|n| n.rules.clone());
    let mut scene = PreparedScene::new(world);
    let selected = entry.clone();
    let progress = Arc::new(AtomicU8::new(0));
    let stage = progress.clone();
    let spawn = Arc::new(OnceLock::new());
    let spawn_known = spawn.clone();
    let job = std::thread::Builder::new().name("map-loader".into()).spawn(move || {
        let _span = info_span!("load_map_transition").entered();
        let started = Instant::now();
        let mut map = info_span!("read_map").in_scope(|| selected.path.as_deref().map(skate_data::skate_map::SkateMap::load).transpose())?;
        // The world row's skate.Park layout rides on the loaded map for the render and physics builders.
        if let Some(map) = map.as_mut() {
            crate::park_layout::attach(map, &root, selected.world.as_deref());
        }
        let map_fingerprint = crate::config::map_fingerprint(selected.path.as_deref())?;
        let read_time = started.elapsed();
        let validation_started = Instant::now();
        if let Some(map) = &map { crate::skate_world::validate_runtime(map)?; }
        let validation_time = validation_started.elapsed();
        let triggers = crate::trigger_volumes::TriggerVolumes::load_or_warn(selected.path.as_deref(), map.as_ref());
        let metadata = CurrentMap::from_package(selected.path, map.as_ref());
        let _ = spawn_known.set(metadata.spawn);
        let retail = map.as_ref().is_some_and(|m| crate::retail_render::RetailScene::for_map(m));
        // Both builders only read the decoded package. Reserve render handles
        // on a second worker while the first constructs fresh simulation state;
        // neither publishes to the live world until both have succeeded.
        let (physics, skater, controls, camera, mod_ground, game_data, simulation_time, render_time) =
            std::thread::scope(|scope| -> Result<_, String> {
                let game_data = std::thread::Builder::new().name("map-data-reader".into())
                    .spawn_scoped(scope, || crate::map_read_ahead::read(&root, &metadata.name, metadata.path.as_deref(), retail, nav_rules))
                    .map_err(|e| format!("Could not start the map data reader: {e}"))?;
                let rendering = std::thread::Builder::new().name("map-render-loader".into())
                    .spawn_scoped(scope, || {
                        let render_started = Instant::now();
                        scene.prepare(map.as_ref(), &root);
                        render_started.elapsed()
                    }).map_err(|e| format!("Could not start render loader: {e}"))?;
                let simulation_started = Instant::now();
                let simulation = (|| -> Result<_, String> {
                    stage.store(1, Ordering::Relaxed);
                    let physics = info_span!("load_physics").in_scope(|| GamePhysics::load_with_difficulty(&root, map.as_ref(), difficulty))?;
                    // The same collider the mods' first fixed tick on the new map built on the game thread (1 to 2 s on a
                    // city map, 2026-10-17), from the same triangles; installed on that same tick.
                    // Its own thread over a copy of the triangles, next to the skater and camera loads below.
                    let triangles = crate::modding::ground_triangles(&physics);
                    let ground_job = std::thread::Builder::new().name("mod-ground".into())
                        .spawn(move || info_span!("build_mod_ground").in_scope(|| skate_dynamics::DynamicsWorld::build_ground(triangles.into_iter())))
                        .map_err(|e| format!("Could not start the mod ground builder: {e}"))?;
                    stage.store(2, Ordering::Relaxed);
                    let skater = SkaterRuntime::load_for_world(&root, &graphs, &physics, difficulty.profile_key(), Some(source))?;
                    let mut controls = PlayerControls::load(&root)?;
                    controls.preferences = preferences;
                    let camera = crate::camera::CameraRuntime::load(&root)?;
                    let mod_ground = ground_job.join().unwrap_or_else(|_| Err("the mod ground builder failed unexpectedly".into()));
                    Ok((physics, skater, controls, camera, mod_ground))
                })();
                let simulation_time = simulation_started.elapsed();
                stage.store(3, Ordering::Relaxed);
                // Explicitly join even on a simulation error: no background
                // render work or reserved scene can outlive a failed request.
                let render_time = rendering.join()
                    .map_err(|_| "Map render preparation failed unexpectedly".to_string())?;
                let game_data = game_data.join().unwrap_or_default();
                let (physics, skater, controls, camera, mod_ground) = simulation?;
                Ok((physics, skater, controls, camera, mod_ground, game_data, simulation_time, render_time))
            })?;
        eprintln!("MAP_LOAD_TIMING name={:?} read_ms={} validation_ms={} simulation_ms={} render_ms={} prepare_ms={} parallel=true",
            metadata.name, read_time.as_millis(), validation_time.as_millis(),
            simulation_time.as_millis(), render_time.as_millis(), started.elapsed().as_millis());
        // Drop the decoded package on this worker. Physics and rendering now
        // own their data; retaining it would double large-city CPU memory.
        Ok(PreparedWorld { map_fingerprint, scene, physics, skater, controls, camera, metadata, retail, triggers, difficulty, mod_ground, game_data })
    }).map_err(|e| format!("Could not start map loader: {e}"))?;
    Ok(Phase::Loading { entry, progress, spawn, job })
}

/// The loading screen (if any) covers the screen, or gave up.
fn covered(world: &World) -> bool {
    let cover = world.get_resource::<LoadingCover>().copied().unwrap_or_default();
    if !cover.active || cover.covered {
        return true;
    }
    let waited = world.resource::<MapTransition>().cover_wait.is_some_and(|t| t.elapsed() >= COVER_WAIT);
    if waited {
        warn!("MAP_TRANSITION the loading screen did not cover in {} s: committing", COVER_WAIT.as_secs());
    }
    waited
}

fn poll(world: &mut World) {
    {
        let finished = matches!(&world.resource::<MapTransition>().phase, Phase::Loading { job, .. } if job.is_finished());
        let mut t = world.resource_mut::<MapTransition>();
        if !finished {
            t.cover_wait = None;
        } else if t.cover_wait.is_none() {
            t.cover_wait = Some(Instant::now());
        }
    }
    let uploading = world.get_resource::<crate::map_render::MapAssets>().is_some_and(crate::map_render::MapAssets::uploading);
    let ready = match &world.resource::<MapTransition>().phase {
        Phase::Idle => false,
        Phase::Loading { job, .. } => job.is_finished() && covered(world),
        // The new map's meshes and textures go in a slice per frame first (`map_render::upload_slices`).
        Phase::Publishing { .. } => !uploading,
        _ => true,
    };
    if !ready { return; }
    let phase = std::mem::replace(&mut world.resource_mut::<MapTransition>().phase, Phase::Idle);
    let next = match phase {
        Phase::Requested(entry) => match start(world, entry) {
            Ok(phase) => phase,
            Err(error) => { failed(world, error); Phase::Idle }
        },
        Phase::Loading { job, .. } => match job.join().unwrap_or_else(|_| Err("Map loader failed unexpectedly".into())) {
            Ok(prepared) => {
                let mut notice = commit(world, prepared);
                let config = world.resource::<Config>();
                match crate::map_library::save_default(&config.asset_root, config.map_path.as_deref()) {
                    Ok(()) => notice.push_str(" Default map saved."),
                    Err(error) => notice.push_str(&format!(" Could not save default: {error}")),
                }
                Phase::Publishing { frames: 3, notice }
            }
            Err(error) => { failed(world, error); Phase::Idle }
        },
        Phase::Publishing { frames, notice } if frames > 0 => Phase::Publishing { frames: frames - 1, notice },
        Phase::Publishing { notice, .. } => {
            world.resource_mut::<crate::graphics_menu::Menu>().transition_finished(notice, true);
            world.resource_mut::<Time<Virtual>>().unpause();
            Phase::Idle
        }
        Phase::Idle => Phase::Idle,
    };
    world.resource_mut::<MapTransition>().phase = next;
}

fn failed(world: &mut World, error: String) {
    warn!("MAP_TRANSITION_FAILED {error}");
    world.resource_mut::<crate::graphics_menu::Menu>()
        .transition_finished(format!("Could not load map: {error}\nPrevious world retained. Choose another map or Resume."), false);
}

fn commit(world: &mut World, mut prepared: PreparedWorld) -> String {
    let publication_started = Instant::now();
    prepared.metadata.generation = world.resource::<CurrentMap>().generation + 1;
    // All fallible decoding/validation/construction finished before this point.
    // No simulation system can observe a mixture of the two worlds.
    crate::map_render::MapAssets::retire(world);
    prepared.scene.publish_sliced(world);
    crate::camera::set_world_environment(world, prepared.retail);
    world.insert_resource(crate::retail_render::RetailScene(prepared.retail));
    info!("SKATE_TRIGGERS map={:?} volumes={} source={}", prepared.metadata.name, prepared.triggers.map.len(), prepared.triggers.origin);
    // Mod-added volumes and switches are world-scoped, like sdk.volumes.
    world.insert_resource(prepared.triggers);
    world.insert_resource(crate::grind_world::GrindGeometry::for_world(prepared.metadata.path.is_none()));
    world.insert_resource(Time::<Fixed>::from_duration(prepared.physics.period()));
    let root_transform = Transform::from_matrix(crate::animation::native_matrix(
        prepared.skater.animated_skeleton.roots.animation_to_world));
    for mut transform in world.query_filtered::<&mut Transform, With<crate::world::PlayerRoot>>().iter_mut(world) {
        *transform = root_transform;
    }
    world.insert_resource(crate::modding::PreparedGround { generation: prepared.metadata.generation, ground: Some(prepared.mod_ground) });
    world.insert_resource(crate::map_read_ahead::PreparedMapData::new(
        prepared.metadata.name.clone(), prepared.metadata.generation, prepared.metadata.path.clone(), prepared.game_data));
    world.insert_resource(prepared.physics);
    world.insert_resource(prepared.skater);
    world.insert_resource(prepared.controls);
    world.insert_resource(prepared.camera);
    world.insert_resource(crate::presentation::Presentation::default());
    world.insert_resource(crate::replay::Replay::default());
    world.insert_resource(crate::input::ControllerInput::default());
    world.insert_resource(crate::input::PublishedTickInput::default());
    world.resource_mut::<ButtonInput<KeyCode>>().reset_all();
    world.resource_mut::<ButtonInput<MouseButton>>().reset_all();
    let mut config = world.resource_mut::<Config>();
    config.map_path = prepared.metadata.path.clone();
    config.map_fingerprint = prepared.map_fingerprint;
    config.difficulty = prepared.difficulty;
    let notice = format!("Loaded {}.", prepared.metadata.name);
    info!("MAP_TRANSITION_COMMITTED generation={} name={:?} spawn={:?} heading={} pid={}",
        prepared.metadata.generation, prepared.metadata.name, prepared.metadata.spawn,
        prepared.metadata.heading, std::process::id());
    if let Some(mut messages) = world.get_resource_mut::<Messages<WorldChanged>>() {
        messages.write(WorldChanged);
    }
    world.insert_resource(prepared.metadata);
    eprintln!("MAP_PUBLISH_TIMING cpu_ms={}", publication_started.elapsed().as_millis());
    notice
}
