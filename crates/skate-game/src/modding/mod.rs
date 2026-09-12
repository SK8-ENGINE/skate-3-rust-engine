//! Main-thread SDK adapter. Lua never receives World, entity IDs, or asset handles.
mod audio;
mod canvas;
mod graphics_dynamic;
mod vehicle_camera;
pub(crate) mod bridge;
pub(crate) mod replication;
mod graphics;
pub(crate) mod attachment;
mod glb;
mod menu;

pub(crate) use menu::ModMenu;

use bevy::{
    asset::io::{AssetSourceBuilder, file::FileAssetReader},
    prelude::*,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use skate_dynamics::{
    BodyDesc, ContactEvent, DynamicsWorld, GROUND_BODY_ID, RevoluteJointDesc, Shape,
};
use skate_mods::{
    with_host, Command, DynamicsHost, Manager, RaycastFilter, RaycastOptions,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Resource)]
pub(crate) struct Mods {
    pub manager: Manager,
    world: DynamicsWorld,
    bodies: BTreeMap<(String, String), u64>,
    joints: BTreeMap<(String, String), u64>,
    graphics: BTreeMap<(String, String), GraphicsOwned>,
    overlays: BTreeMap<(String, String), Entity>,
    canvases: BTreeMap<(String, String), canvas::Canvas>,
    attach: Option<AttachState>,
    detach_error: Option<String>,
    detach_pending: Option<(Transform,std::time::Instant)>,
    camera: CameraOverride,
    generation: u64,
    graphics_serial: u64,
    debug_owners: BTreeSet<String>,
    ground_ready: bool,
    /// Passive finite-mass Rapier shadows of remote native actor bodies.
    skater_proxies: BTreeMap<usize, u64>,
    /// APPLICATION keys currently published for local dynamics bodies.
    dyn_published: BTreeSet<String>,
    replication: replication::State,
    /// Local Lua `sdk.net.publish` values: (mod_id, key) → JSON.
    net_states: BTreeMap<(String, String), Value>,
    /// Wire keys currently published for local net states.
    net_published: BTreeSet<String>,
    /// Remote Lua net values: (peer, mod_id, key) → JSON.
    net_remote: BTreeMap<(u64, String, String), Value>,
    /// Maps (peer, wire_key) → (mod_id, key) for empty-value cleanup.
    net_remote_wire: BTreeMap<(u64, String), (String, String)>,
    net_status: String,
    /// Contacts from the last DynamicsWorld::step (before drain).
    last_contacts: Vec<ContactEvent>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NetWire {
    owner: String,
    fingerprint: u64,
    key: String,
    value: Value,
}

type GraphicsOwned = graphics::Owned;

#[derive(Clone)]
struct AttachState {
    owner: String,
    body: String,
    offset: Vec3,
    hidden: Vec<(Entity, Visibility)>,
    fallback: Transform,
}

#[derive(Default)]
struct CameraOverride {
    owner: Option<String>,
    rig: Option<vehicle_camera::Rig>,
    saved_near: Option<f32>,
    follow: Option<(String, String, Vec3)>,
    fixed: Option<(Vec3, Option<Vec3>)>,
}

impl CameraOverride {
    fn clear(&mut self) {
        // Restore the camera lens on the next presentation frame, even on unload.
        let saved_near = self.saved_near.take();
        *self = Self { saved_near, ..Default::default() };
    }
    fn claim(&mut self, owner: &str) -> Result<(), String> {
        if self.owner.as_deref().is_some_and(|o| o != owner) {
            return Err("camera is owned by another mod".into());
        }
        self.owner = Some(owner.to_owned());
        Ok(())
    }
    fn follows(&self, owner: &str, body: &str) -> bool {
        self.owner.as_deref() == Some(owner) &&
            (self.rig.as_ref().is_some_and(|r| r.body == body) ||
             self.follow.as_ref().is_some_and(|(_, b, _)| b == body))
    }
}

pub(crate) struct ModdingPlugin;

impl Plugin for ModdingPlugin {
    fn build(&self, app: &mut App) {
        let root = package_root();
        if let Err(e) = std::fs::create_dir_all(&root) {
            warn!("Cannot create mods folder {}: {e}", root.display());
        }
        info!("Mod packages folder: {}", root.display());
        let settings = std::env::var_os("SKATE3_MOD_SETTINGS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                app.world()
                    .resource::<crate::config::Config>()
                    .asset_root
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .join("settings/mods")
            });
        app.insert_resource(Mods {
            manager: Manager::new(root, settings),
            world: DynamicsWorld::default(),
            bodies: BTreeMap::new(),
            joints: BTreeMap::new(),
            graphics: BTreeMap::new(),
            overlays: BTreeMap::new(),
            canvases: BTreeMap::new(),
            attach: None,
            detach_error: None,
            detach_pending: None,
            camera: CameraOverride::default(),
            generation: u64::MAX,
            graphics_serial: 0,
            debug_owners: BTreeSet::new(),
            ground_ready: false,
            skater_proxies: BTreeMap::new(),
            dyn_published: BTreeSet::new(),
            replication: replication::State::default(),
            net_states: BTreeMap::new(),
            net_published: BTreeSet::new(),
            net_remote: BTreeMap::new(),
            net_remote_wire: BTreeMap::new(),
            net_status: String::new(),
            last_contacts: Vec::new(),
        })
        .init_resource::<ModMenu>();
        menu::install(app);
        audio::install(app);
        graphics_dynamic::install(app);
        app.add_systems(
            PreUpdate,
            maintenance.after(crate::map_transition::MapTransitionSet),
        )
        .add_systems(
            FixedUpdate,
            (replication::sample_fixed, bridge::dynamics_to_board).chain()
                .after(crate::app::SimulationSet::Controls)
                .after(crate::multiplayer::prepare)
                .before(crate::app::SimulationSet::Physics)
                .run_if(crate::graphics_menu::gameplay_active),
        )
        .add_systems(
            FixedUpdate,
            fixed
                .after(crate::app::SimulationSet::Physics)
                .run_if(crate::graphics_menu::gameplay_active),
        )
        .add_systems(
            Update,
            (
                update.after(crate::app::FrameSet::Animation),
                bridge::sync_network.after(crate::multiplayer::send_pose).after(update),
                sync_net.after(bridge::sync_network),
                graphics::debug.after(bridge::sync_network).after(update),
                present_camera.after(crate::camera::present).after(update).after(bridge::sync_network)
                    .before(crate::app::FrameSet::Verification),
            ),
        );
    }
}

pub(crate) fn register_source(app: &mut App) {
    let root = package_root();
    app.register_asset_source(
        "mods",
        AssetSourceBuilder::new(move || Box::new(FileAssetReader::new(root.clone()))),
    );
}

pub(crate) fn package_root() -> std::path::PathBuf {
    std::env::var_os("SKATE3_MODS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.join("mods")))
                .unwrap_or_else(|| "mods".into())
        })
}

pub(crate) fn player_attached(mods: &Mods) -> bool {
    mods.attach.is_some()
}

fn body_snapshot_for(mods: &Mods, owner: &str) -> serde_json::Value {
    let mut bodies = serde_json::Map::new();
    let mut reverse = BTreeMap::<u64, String>::new();
    for ((o, key), id) in &mods.bodies {
        if o != owner {
            continue;
        }
        reverse.insert(*id, key.clone());
        if let Some(snap) = mods.world.read(*id) {
            bodies.insert(
                key.clone(),
                json!({
                    "position": snap.position,
                    "rotation": snap.rotation,
                    "linvel": snap.linvel,
                    "angvel": snap.angvel,
                    "force": snap.force,
                    "torque": snap.torque,
                    "mass": snap.mass,
                    "speed": (snap.linvel[0]*snap.linvel[0]
                        + snap.linvel[1]*snap.linvel[1]
                        + snap.linvel[2]*snap.linvel[2]).sqrt(),
                }),
            );
        }
    }
    let skip: BTreeSet<u64> = mods.skater_proxies.values().copied().collect();
    let label = |id: u64| -> Option<String> {
        if id == GROUND_BODY_ID {
            Some("ground".into())
        } else {
            reverse.get(&id).cloned()
        }
    };
    let owned = |id: u64| reverse.contains_key(&id);
    let contacts: Vec<_> = mods
        .last_contacts
        .iter()
        .filter(|c| !skip.contains(&c.body_a) && !skip.contains(&c.body_b))
        .filter(|c| owned(c.body_a) || owned(c.body_b))
        .map(|c| {
            json!({
                "a": label(c.body_a),
                "b": label(c.body_b),
                "started": c.started,
            })
        })
        .collect();
    let touching: Vec<_> = mods
        .world
        .active_contact_pairs()
        .into_iter()
        .filter(|(a, b)| !skip.contains(a) && !skip.contains(b))
        .filter(|(a, b)| owned(*a) || owned(*b))
        .map(|(a, b)| json!({"a": label(a), "b": label(b)}))
        .collect();
    json!({"bodies": bodies, "contacts": contacts, "touching": touching})
}

fn network_snapshot(world: &World, mods: &Mods) -> Value {
    let (active, id, host) = world
        .get_resource::<crate::multiplayer::Multiplayer>()
        .map_or((false, 0, true), |n| n.mod_identity());
    let mut states = serde_json::Map::new();
    let insert = |states: &mut serde_json::Map<String, Value>,
                  mod_id: &str,
                  peer: &str,
                  key: &str,
                  value: &Value| {
        if value.is_null() {
            return;
        }
        let mod_entry = states
            .entry(mod_id.to_owned())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        let Some(mod_map) = mod_entry.as_object_mut() else {
            return;
        };
        let peer_entry = mod_map
            .entry(peer.to_owned())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        let Some(peer_map) = peer_entry.as_object_mut() else {
            return;
        };
        peer_map.insert(key.to_owned(), value.clone());
    };
    let local = id.to_string();
    for ((mod_id, key), value) in &mods.net_states {
        insert(&mut states, mod_id, &local, key, value);
    }
    for ((peer, mod_id, key), value) in &mods.net_remote {
        insert(&mut states, mod_id, &peer.to_string(), key, value);
    }
    json!({
        "active": active,
        "local_id": local,
        "is_host": host,
        "states": states,
        "status": mods.net_status,
    })
}

fn net_wire_key(owner: &str, key: &str) -> String {
    let full = format!("net:{owner}:{key}");
    if full.len() <= 128 {
        full
    } else {
        format!(
            "net:{:016x}",
            skate_net::hash(format!("{owner}/{key}").as_bytes())
        )
    }
}

fn ensure_ground(world: &World, mods: &mut Mods) -> Result<(), String> {
    if mods.ground_ready {
        return Ok(());
    }
    mods.world.set_ground(
        world
            .resource::<crate::physics::GamePhysics>()
            .world_triangles()
            .iter()
            .map(|t| t.triangle.vertices.map(|p| [p.x, p.y, p.z])),
    )?;
    mods.ground_ready = true;
    Ok(())
}

fn maintenance(world: &mut World) {
    world.resource_scope(|world, mut mods: Mut<Mods>| {
        let camera = camera_position(world);
        let snap = snapshot_ro(world, &mods, camera);
        mods.manager.snapshot = snap;
        let generation = world
            .resource::<crate::map_transition::CurrentMap>()
            .generation;
        if mods.generation != generation {
            clear_runtime(world, &mut mods);
            mods.generation = generation;
            mods.manager.commands.clear();
            let map = mods.manager.snapshot["map"].clone();
            mods.manager
                .dispatch("on_event", json!({"name":"world_changed","map":map}));
        }
        mods.manager.scan(false);
        apply(world, &mut mods);
    });
}

fn camera_position(world: &mut World) -> Option<[f32; 3]> {
    world
        .query_filtered::<&Transform, With<crate::camera::GameplayCamera>>()
        .iter(world)
        .next()
        .map(|t| t.translation.to_array())
}

fn snapshot_ro(world: &World, mods: &Mods, camera: Option<[f32; 3]>) -> serde_json::Value {
    let s = world.resource::<crate::physics::SkaterRuntime>();
    let p = &s.player_input.physical;
    let map = world.resource::<crate::map_transition::CurrentMap>();
    let physics = world.resource::<crate::physics::GamePhysics>();
    let keys: BTreeMap<_, _> = world
        .resource::<ButtonInput<KeyCode>>()
        .get_pressed()
        .map(|k| (format!("{k:?}"), true))
        .collect();
    let actions = *world
        .resource::<crate::input::PublishedTickInput>()
        .0
        .actions()
        .values();
    let pad = world.resource::<crate::input::ControllerInput>().raw_input();
    let root = s.animated_skeleton.roots.animation_to_world;
    let override_root=attachment::local_root(mods);
    let player_position=override_root.map_or([root[3][0],root[3][1],root[3][2]],|t|t.translation.to_array());
    let heading=override_root.map_or(root[2][0].atan2(root[2][2]),|t| {let f=t.rotation*Vec3::Z; f.x.atan2(f.z)});
    json!({
        "player": {
            "position": player_position,
            "velocity": &p.skateboard.vector_80.map(f32::from_bits)[..3],
            "heading": heading,
            "on_board": p.state.category_12 != 500,
            "state": p.state.state_16,
            "category": p.state.category_12,
            "bailing": physics.board_wiping_out,
        },
        "attach": mods.attach.as_ref().map(|a| json!({"body": a.body, "owner": a.owner})),
        "detach_error": mods.detach_error,
        "detach_pending": mods.detach_pending.is_some(),
        "map": {"name": map.name, "generation": map.generation},
        "tick": physics.ticks,
        "keys": keys,
        "actions": actions,
        "pad": {
            "buttons": pad.buttons,
            "triggers": pad.triggers,
            "left": pad.left,
            "right": pad.right,
        },
        "paused": world.resource::<crate::graphics_menu::Menu>().open,
        "replay": world.resource::<crate::replay::Replay>().active,
        "camera": camera.map(|position| json!({"position": position})),
        "physics": {"bodies": {}, "contacts": []},
        "network": network_snapshot(world, mods),
    })
}

fn fixed(world: &mut World) {
    if world.resource::<crate::replay::Replay>().active {
        return;
    }
    let dt = world.resource::<Time<Fixed>>().delta_secs_f64();
    world.resource_scope(|world, mut mods: Mut<Mods>| {
        bridge::take_reactions(&mut mods, &mut world.resource_mut::<crate::physics::GamePhysics>());
        let camera = camera_position(world);
        mods.manager.snapshot = snapshot_ro(world, &mods, camera);
        if let Err(e) = ensure_ground(world, &mut mods) {
            warn!("dynamics ground: {e}");
        }
        let ids: Vec<_> = mods
            .manager
            .packages
            .iter()
            .filter(|(_, p)| p.running())
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            let physics = body_snapshot_for(&mods, &id);
            if let Some(obj) = mods.manager.snapshot.as_object_mut() {
                obj.insert("physics".into(), physics);
            }
            // Sync dynamics queries for this mod while on_fixed_update runs.
            struct Host<'a> {
                world: &'a mut DynamicsWorld,
                bodies: &'a BTreeMap<(String, String), u64>,
                owner: &'a str,
            }
            impl DynamicsHost for Host<'_> {
                fn raycast(
                    &mut self,
                    origin: [f32; 3],
                    direction: [f32; 3],
                    options: &RaycastOptions,
                ) -> Option<serde_json::Value> {
                    let hit = match options.filter {
                        RaycastFilter::Ground => self.world.raycast_ground(
                            origin,
                            direction,
                            options.max_distance,
                        )?,
                        RaycastFilter::All => {
                            let excluded: Vec<_> = options
                                .exclude
                                .iter()
                                .filter_map(|key| {
                                    self.bodies
                                        .get(&(self.owner.to_owned(), key.clone()))
                                        .copied()
                                })
                                .collect();
                            self.world.raycast_excluding_bodies(
                                origin,
                                direction,
                                options.max_distance,
                                &excluded,
                            )?
                        }
                    };
                    let body_key = self
                        .bodies
                        .iter()
                        .find(|((o, _), id)| o.as_str() == self.owner && **id == hit.body)
                        .map(|((_, k), _)| k.clone())
                        .or_else(|| (hit.body == GROUND_BODY_ID).then(|| "ground".into()));
                    Some(serde_json::json!({
                        "body": body_key,
                        "point": hit.point,
                        "normal": hit.normal,
                        "toi": hit.toi,
                    }))
                }
                fn velocity_at(&self, key: &str, point: [f32; 3]) -> Option<[f32; 3]> {
                    let id = *self.bodies.get(&(self.owner.to_owned(), key.to_owned()))?;
                    self.world.velocity_at(id, point)
                }
                fn effective_inv_mass(
                    &self,
                    key: &str,
                    point: [f32; 3],
                    direction: [f32; 3],
                ) -> Option<f32> {
                    let id = *self.bodies.get(&(self.owner.to_owned(), key.to_owned()))?;
                    self.world.effective_inv_mass(id, point, direction)
                }
                fn spring_ray(
                    &mut self,
                    key: &str,
                    desc: skate_dynamics::SpringRayDesc,
                ) -> Option<skate_dynamics::SpringRayHit> {
                    let id = *self.bodies.get(&(self.owner.to_owned(), key.to_owned()))?;
                    self.world.spring_ray(id, desc)
                }
                fn local_ang_accel_impulse(
                    &self,
                    key: &str,
                    local_accel: [f32; 3],
                    dt: f32,
                ) -> Option<[f32; 3]> {
                    let id = *self.bodies.get(&(self.owner.to_owned(), key.to_owned()))?;
                    self.world.local_ang_accel_impulse(id, local_accel, dt)
                }
            }
            // Split-borrow world/bodies for the query host while Manager runs Lua.
            let Mods {
                manager,
                world: dyn_world,
                bodies,
                ..
            } = &mut *mods;
            let mut host = Host {
                world: dyn_world,
                bodies,
                owner: &id,
            };
            with_host(&mut host, || {
                manager.call(&id, "on_fixed_update", json!({"dt": dt}));
            });
        }
        // New command frame: drop last tick's user forces so they cannot stack in Rapier.
        mods.world.begin_force_frame();
        apply(world, &mut mods);
        if let Err(e) = ensure_ground(world, &mut mods) {
            warn!("dynamics ground: {e}");
        }
        {
            let physics = world.resource::<crate::physics::GamePhysics>();
            let skater = world.resource::<crate::physics::SkaterRuntime>();
            bridge::push_skater_into_rapier(&mut mods, physics, skater);
        }
        if !mods.bodies.is_empty() || mods.ground_ready || !mods.skater_proxies.is_empty() {
            mods.world.step(dt as f32);
            mods.last_contacts = mods.world.drain_contacts();
        }
        sync_graphics(world, &mut mods);
        sync_attach(world, &mut mods);
        // Record native post-step poses for render interpolation, including hood view.
        record_camera(&mut mods, dt as f32);
    });
}

fn update(world: &mut World) {
    let camera = camera_position(world);
    let snap = {
        let mods = world.resource::<Mods>();
        snapshot_ro(world, mods, camera)
    };
    let paused =
        snap["paused"].as_bool().unwrap_or(true) || snap["replay"].as_bool().unwrap_or(false);
    let dt = world.resource::<Time<Real>>().delta_secs_f64().min(0.25);
    world.resource_scope(|world, mut mods: Mut<Mods>| {
        mods.manager.snapshot = snap;
        if !paused {
            mods.manager.dispatch("on_update", json!({"dt": dt}));
        }
        apply(world, &mut mods);
        sync_graphics(world, &mut mods);
        sync_attach(world, &mut mods);
        let hidden = paused || world.get_resource::<crate::customiser::Customiser>()
            .is_some_and(|c| c.open);
        canvas::present(world, &mods.canvases, hidden);
    });
}

fn clear_runtime(world: &mut World, mods: &mut Mods) {
    replication::reset(world,mods);
    audio::clear(world);
    graphics_dynamic::clear(world);
    canvas::clear_owner(world, &mut mods.canvases, None);
    detach_player(world, mods, true);
    let keys: Vec<_> = mods.graphics.keys().cloned().collect();
    for key in keys {
        retire_graphics(world, mods, &key);
    }
    let overlays: Vec<_> = mods.overlays.keys().cloned().collect();
    for key in overlays {
        if let Some(e) = mods.overlays.remove(&key) {
            world.despawn(e);
        }
    }
    mods.bodies.clear();
    mods.joints.clear();
    mods.skater_proxies.clear();
    mods.dyn_published.clear();
    mods.net_states.clear();
    mods.net_published.clear();
    mods.net_remote.clear();
    mods.net_remote_wire.clear();
    mods.net_status.clear();
    mods.last_contacts.clear();
    mods.debug_owners.clear();
    world.resource_mut::<crate::physics::GamePhysics>().set_external_queries(None);
    mods.world = DynamicsWorld::default();
    mods.ground_ready = false;
    mods.camera.clear();
    mods.detach_pending=None;
    mods.detach_error=None;
}

fn retire_graphics(world: &mut World, mods: &mut Mods, key: &(String, String)) {
    if let Some(owned) = mods.graphics.remove(key) {
        world.despawn(owned.entity);
        if let Some(id) = owned.mesh {
            world.resource_mut::<Assets<Mesh>>().remove(id);
        }
        if let Some(id) = owned.material {
            world.resource_mut::<Assets<StandardMaterial>>().remove(id);
        }
    }
}

fn apply(world: &mut World, mods: &mut Mods) {
    let retired = std::mem::take(&mut mods.manager.retired);
    for id in &retired {
        audio::stop_owner(world, id, true);
        graphics_dynamic::clear_owner(world, id);
        canvas::clear_owner(world, &mut mods.canvases, Some(id));
        detach_if_owner(world, mods, id);
        if mods.camera.owner.as_ref() == Some(id) {
            mods.camera.clear();
        }
        let gkeys: Vec<_> = mods
            .graphics
            .keys()
            .filter(|(o, _)| o == id)
            .cloned()
            .collect();
        for key in gkeys {
            retire_graphics(world, mods, &key);
        }
        let okeys: Vec<_> = mods
            .overlays
            .keys()
            .filter(|(o, _)| o == id)
            .cloned()
            .collect();
        for key in okeys {
            if let Some(e) = mods.overlays.remove(&key) {
                world.despawn(e);
            }
        }
        let bkeys: Vec<_> = mods
            .bodies
            .keys()
            .filter(|(o, _)| o == id)
            .cloned()
            .collect();
        for key in bkeys {
            if let Some(body) = mods.bodies.remove(&key) {
                mods.world.remove(body);
            }
        }
        let jkeys: Vec<_> = mods
            .joints
            .keys()
            .filter(|(o, _)| o == id)
            .cloned()
            .collect();
        for key in jkeys {
            if let Some(j) = mods.joints.remove(&key) {
                mods.world.remove_joint(j);
            }
        }
        mods.net_states.retain(|(o, _), _| o != id);
    }
    mods.manager
        .commands
        .retain(|(owner, _)| !retired.iter().any(|id| id == owner));
    let mut batches = BTreeMap::<String, Vec<Command>>::new();
    for (id, command) in std::mem::take(&mut mods.manager.commands) {
        batches.entry(id).or_default().push(command);
    }
    for (id, mut commands) in batches {
        if !mods.manager.packages.get(&id).is_some_and(|p| p.running()) {
            continue;
        }
        if let Err(e) = ensure_ground(world, mods) {
            warn!("Lua mod {id}: {e}");
            mods.manager.fail(&id, e);
            audio::stop_owner(world, &id, true);
            graphics_dynamic::clear_owner(world, &id);
            continue;
        }
        commands.sort_by_key(|command| match command {
            Command::PlayerDetach { .. } => 0,
            Command::PhysicsRemove { .. } | Command::PhysicsRemoveJoint { .. } => 1,
            Command::PhysicsSpawn { .. } => 2,
            _ => 3,
        });
        let result = (|| {
            for command in commands {
                apply_one(world, mods, &id, command)?;
            }
            Ok::<(), String>(())
        })();
        if let Err(e) = result {
            warn!("Lua mod {id}: {e}");
            mods.manager.fail(&id, e);
            audio::stop_owner(world, &id, true);
            graphics_dynamic::clear_owner(world, &id);
        }
    }
    let mut row = 0;
    for entity in mods.overlays.values() {
        if let Some(mut node) = world.get_mut::<Node>(*entity) {
            node.top = px(16. + row as f32 * 28.);
            row += 1;
        }
    }
}

fn apply_one(
    world: &mut World,
    mods: &mut Mods,
    id: &str,
    command: Command,
) -> Result<(), String> {
    match command {
        Command::UiCanvas { key, options } => canvas::set(world, &mut mods.canvases, id, key, options)?,
        Command::UiRemove { key } => {
            let slot = (id.to_owned(), key);
            canvas::remove(world, &mut mods.canvases, &slot);
            if let Some(e) = mods.overlays.remove(&slot) { world.despawn(e); }
        }
        Command::CameraRig { body, options } => {
            resolve_body(mods, id, &body)?;
            mods.camera.claim(id)?;
            mods.camera.follow = None;
            mods.camera.fixed = None;
            if let Some(rig) = mods.camera.rig.as_mut().filter(|r| r.body == body) {
                rig.configure(options);
            } else {
                mods.camera.rig = Some(vehicle_camera::Rig::new(body, options));
            }
        }
        Command::AudioPreload { path } => audio::preload(world, mods, id, &path)?,
        Command::AudioPlay { key, options } => audio::play(world, mods, id, key, options)?,
        Command::AudioUpdate { key, options } => audio::update_voice(world, id, &key, options),
        Command::AudioStop { key, fade_out } => audio::stop(world, id, &key, fade_out),
        Command::AudioStopAll {} => audio::stop_owner(world, id, false),
        Command::GraphicsMeshBuffer { key, options } => {
            graphics_dynamic::mesh_buffer(world, mods, id, key, options)?;
        }
        Command::GraphicsMeshBufferWrite { key, data } => {
            graphics_dynamic::mesh_buffer_write(world, mods, id, &key, data)?;
        }
        Command::GraphicsMeshBufferAppend { key, data } => {
            graphics_dynamic::mesh_buffer_append(world, mods, id, &key, data)?;
        }
        Command::GraphicsLight { key, options } => graphics_dynamic::light(world, mods, id, key, options)?,
        Command::Log { text } => info!("Lua [{id}]: {text}"),
        Command::Overlay { key, text } => {
            let k = (id.to_owned(), key);
            // Empty overlay text removes its row rather than leaving a blank slot.
            if text.is_empty() {
                if let Some(e) = mods.overlays.remove(&k) { world.despawn(e); }
                return Ok(());
            }
            if let Some(entity) = mods.overlays.get(&k) {
                if let Some(mut t) = world.get_mut::<Text>(*entity) {
                    **t = text;
                    return Ok(());
                }
            }
            if let Some(e) = mods.overlays.remove(&k) {
                world.despawn(e);
            }
            let entity = world
                .spawn((
                    Text::new(text),
                    TextFont {
                        font_size: 19.,
                        ..default()
                    },
                    TextColor(Color::WHITE),
                    GlobalZIndex(3),
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(16.),
                        ..default()
                    },
                ))
                .id();
            mods.overlays.insert(k, entity);
        }
        Command::PhysicsSpawn { key, body } => {
            let k = (id.to_owned(), key);
            if let Some(old) = mods.bodies.remove(&k) {
                mods.world.remove(old);
            }
            if mods.bodies.keys().filter(|(o, _)| o == id).count() >= 64 {
                return Err("64 physics bodies per mod maximum".into());
            }
            let body = resolve_body_desc(mods, id, body)?;
            let body_id = mods.world.spawn(body)?;
            mods.bodies.insert(k, body_id);
        }
        Command::PhysicsRemove { key } => {
            if mods.attach.as_ref().is_some_and(|a|a.owner==id && a.body==key) { detach_player(world,mods,true); }
            let bound:Vec<_>=mods.graphics.iter().filter(|((o,_),g)|o==id && g.body.as_deref()==Some(key.as_str())).map(|(k,_)|k.clone()).collect();
            for slot in bound { retire_graphics(world,mods,&slot); }
            if mods.camera.follows(id, &key) { mods.camera.clear(); }
            audio::stop_body(world, id, &key);
            graphics_dynamic::remove(world, id, &key);
            let k = (id.to_owned(), key);
            if let Some(body) = mods.bodies.remove(&k) {
                mods.world.remove(body);
            }
        }
        Command::PhysicsAddCollider {
            key,
            shape,
            position,
            friction,
        } => {
            let body = resolve_body(mods, id, &key)?;
            let points = resolve_convex_points(mods, id, shape)?;
            mods.world
                .add_convex_collider(body, &points, position, friction)?;
        }
        Command::PhysicsForce { key, force, point } => {
            let body = resolve_body(mods, id, &key)?;
            if !mods.world.apply_force(body, force, point) {
                return Err(format!("force failed for {key}"));
            }
        }
        Command::PhysicsImpulse {
            key,
            impulse,
            point,
        } => {
            let body = resolve_body(mods, id, &key)?;
            if !mods.world.apply_impulse(body, impulse, point) {
                return Err(format!("impulse failed for {key}"));
            }
        }
        Command::PhysicsTorque { key, torque } => {
            let body = resolve_body(mods, id, &key)?;
            if !mods.world.apply_torque(body, torque) {
                return Err(format!("torque failed for {key}"));
            }
        }
        Command::PhysicsTorqueImpulse { key, torque } => {
            let body = resolve_body(mods, id, &key)?;
            if !mods.world.apply_torque_impulse(body, torque) {
                return Err(format!("torque_impulse failed for {key}"));
            }
        }
        Command::PhysicsSetLinvel { key, linvel } => {
            let body = resolve_body(mods, id, &key)?;
            if !mods.world.set_linvel(body, linvel) {
                return Err(format!("set_linvel failed for {key}"));
            }
        }
        Command::PhysicsSetAngvel { key, angvel } => {
            let body = resolve_body(mods, id, &key)?;
            if !mods.world.set_angvel(body, angvel) {
                return Err(format!("set_angvel failed for {key}"));
            }
        }
        Command::PhysicsSetPose {
            key,
            position,
            rotation,
        } => {
            let body = resolve_body(mods, id, &key)?;
            if !mods.world.set_pose(body, position, rotation) {
                return Err(format!("set_pose failed for {key}"));
            }
        }
        Command::PhysicsRevolute {
            key,
            body_a,
            body_b,
            anchor_a,
            anchor_b,
            axis,
            limits,
            contacts_enabled,
        } => {
            let a = resolve_body(mods, id, &body_a)?;
            let b = resolve_body(mods, id, &body_b)?;
            let k = (id.to_owned(), key);
            if let Some(old) = mods.joints.remove(&k) {
                mods.world.remove_joint(old);
            }
            let jid = mods.world.add_revolute_joint(RevoluteJointDesc {
                body_a: a,
                body_b: b,
                anchor_a,
                anchor_b,
                axis,
                limits,
                contacts_enabled,
            })?;
            mods.joints.insert(k, jid);
        }
        Command::PhysicsJointMotor { key, motor } => {
            let j = *mods
                .joints
                .get(&(id.to_owned(), key.clone()))
                .ok_or_else(|| format!("unknown joint {key}"))?;
            if !mods.world.set_joint_motor(j, motor) {
                return Err(format!("joint motor failed for {key}"));
            }
        }
        Command::PhysicsPrismatic {
            key,
            body_a,
            body_b,
            anchor_a,
            anchor_b,
            axis,
            limits,
            contacts_enabled,
        } => {
            let a = resolve_body(mods, id, &body_a)?;
            let b = resolve_body(mods, id, &body_b)?;
            let k = (id.to_owned(), key);
            if let Some(old) = mods.joints.remove(&k) {
                mods.world.remove_joint(old);
            }
            let jid = mods.world.add_prismatic_joint(skate_dynamics::PrismaticJointDesc {
                body_a: a,
                body_b: b,
                anchor_a,
                anchor_b,
                axis,
                limits,
                contacts_enabled,
            })?;
            mods.joints.insert(k, jid);
        }
        Command::PhysicsJointSpring { key, spring } => {
            let j = *mods
                .joints
                .get(&(id.to_owned(), key.clone()))
                .ok_or_else(|| format!("unknown joint {key}"))?;
            if !mods.world.set_joint_spring(j, spring) {
                return Err(format!("joint spring failed for {key}"));
            }
        }
        Command::PhysicsRemoveJoint { key } => {
            let k = (id.to_owned(), key);
            if let Some(j) = mods.joints.remove(&k) {
                mods.world.remove_joint(j);
            }
        }
        Command::GraphicsMesh { key,path,body,position,rotation,scale,color,visible } => {
            graphics::spawn(world,mods,id,id,key,
                skate_mods::scene::GraphicsDefinition { path,body,color },
                skate_mods::scene::TransformState { position:position.unwrap_or([0.;3]),rotation:rotation.unwrap_or([0.,0.,0.,1.]),scale },visible,None)?;
        }
        Command::GraphicsTransform { key, options } => {
            let owned=mods.graphics.get_mut(&(id.to_owned(),key)).ok_or("unknown graphics key")?;
            owned.transform.apply(&options);
        }
        Command::GraphicsNode { key,node,options } => graphics::set_node(mods,id,&key,node,options)?,
        Command::GraphicsResetNode { key,node } => graphics::reset_node(world,mods,id,&key,&node),
        Command::PhysicsDebug { enabled } => {
            if enabled { mods.debug_owners.insert(id.to_owned()); }
            else { mods.debug_owners.remove(id); }
        }
        Command::GraphicsRemove { key } => {
            graphics_dynamic::remove(world, id, &key);
            retire_graphics(world, mods, &(id.to_owned(), key));
        }
        Command::GraphicsVisibility { key,visible } => {
            if let Some(owned)=mods.graphics.get_mut(&(id.to_owned(),key)) { owned.visible=visible; }
        }
        Command::PlayerAttach { body,offset } => attachment::attach(world,mods,id,body,offset)?,
        Command::PlayerDetach { options } => {
            if mods.attach.as_ref().is_some_and(|a|a.owner==id) { attachment::detach(world,mods,false,&options); }
        }
        Command::CameraFollow { body, offset } => {
            if let Some(body) = body {
                resolve_body(mods, id, &body)?;
                mods.camera.claim(id)?;
                mods.camera.fixed = None;
                mods.camera.rig = None;
                mods.camera.follow = Some((id.to_owned(), body, Vec3::from_array(offset)));
            } else if mods.camera.owner.as_deref() == Some(id) {
                mods.camera.clear();
            }
        }
        Command::CameraSet { position, look_at } => {
            mods.camera.claim(id)?;
            mods.camera.rig = None;
            mods.camera.follow = None;
            mods.camera.fixed = Some((
                Vec3::from_array(position),
                look_at.map(Vec3::from_array),
            ));
        }
        Command::NetworkState { key, value } => {
            let slot = (id.to_owned(), key);
            if value.is_null() {
                mods.net_states.remove(&slot);
            } else {
                if mods.net_states.len() >= 128 && !mods.net_states.contains_key(&slot) {
                    return Err("128 shared net states maximum".into());
                }
                mods.net_states.insert(slot, value);
            }
        }
    }
    Ok(())
}

fn resolve_body(mods: &Mods, owner: &str, key: &str) -> Result<u64, String> {
    mods.bodies
        .get(&(owner.to_owned(), key.to_owned()))
        .copied()
        .ok_or_else(|| format!("unknown body {key}"))
}

fn resolve_body_desc(mods: &Mods, owner: &str, mut body: BodyDesc) -> Result<BodyDesc, String> {
    match &body.shape {
        Shape::Mesh { .. } => {
            let points = resolve_convex_points(mods, owner, body.shape.clone())?;
            body.shape = Shape::Convex { points };
        }
        Shape::Model { path, object, options } => {
            let package = mods.manager.packages.get(owner).ok_or("missing package")?;
            if path.is_empty() || !skate_mods::scene::valid_asset(path) {
                return Err("invalid model GLB path".into());
            }
            let root = package.root.canonicalize().map_err(|e| e.to_string())?;
            let full = root.join(path).canonicalize().map_err(|e| e.to_string())?;
            if !full.starts_with(&root) { return Err("model GLB escapes its package".into()); }
            body.shape = skate_mods::model_shape_file(&full, object, options)?;
        }
        _ => {}
    }
    Ok(body)
}

fn resolve_convex_points(mods: &Mods, owner: &str, shape: Shape) -> Result<Vec<[f32; 3]>, String> {
    match shape {
        Shape::Convex { points } => {
            if points.len() < 4 || points.len() > 512 {
                return Err("convex needs 4..=512 points".into());
            }
            Ok(points)
        }
        Shape::Mesh { path, object } => {
            let package = mods
                .manager
                .packages
                .get(owner)
                .ok_or("missing package")?;
            if !skate_mods::scene::valid_asset(&path) || path.is_empty() { return Err("invalid collider GLB path".into()); }
            let root=package.root.canonicalize().map_err(|e|e.to_string())?;
            let full=root.join(&path).canonicalize().map_err(|e|e.to_string())?;
            if !full.starts_with(&root) { return Err("collider GLB escapes its package".into()); }
            glb::convex_points(&full, &object)
        }
        _ => Err("add_collider shape must be mesh or convex".into()),
    }
}

fn sync_graphics(world: &mut World, mods: &mut Mods) { graphics::sync(world,mods); }

fn sync_attach(world:&mut World,mods:&mut Mods) { attachment::sync(world,mods); }
fn detach_if_owner(world:&mut World,mods:&mut Mods,owner:&str) {
    if mods.attach.as_ref().is_some_and(|a|a.owner==owner) { detach_player(world,mods,true); }
}
fn detach_player(world:&mut World,mods:&mut Mods,forced:bool) {
    attachment::detach(world,mods,forced,&skate_mods::scene::DetachOptions::default());
}

fn sync_net(world: &mut World) {
    world.resource_scope(|world, mut mods: Mut<Mods>| {
        let packages: BTreeMap<_, _> = mods
            .manager
            .packages
            .iter()
            .filter(|(_, p)| p.running())
            .map(|(id, p)| (id.clone(), p.content_fingerprint()))
            .collect();
        mods.net_states
            .retain(|(owner, _), _| packages.contains_key(owner));

        let Some(mut net) = world.get_resource_mut::<crate::multiplayer::Multiplayer>() else {
            mods.net_published.clear();
            mods.net_remote.clear();
            mods.net_remote_wire.clear();
            mods.net_status.clear();
            return;
        };
        if !net.active() {
            mods.net_published.clear();
            mods.net_remote.clear();
            mods.net_remote_wire.clear();
            mods.net_status.clear();
            return;
        }

        let mut published = BTreeSet::new();
        let mut failures = 0u32;
        for ((owner, key), value) in &mods.net_states {
            let Some(&fingerprint) = packages.get(owner) else {
                continue;
            };
            let wire = net_wire_key(owner, key);
            let bytes = match serde_json::to_vec(&NetWire {
                owner: owner.clone(),
                fingerprint,
                key: key.clone(),
                value: value.clone(),
            }) {
                Ok(b) if b.len() <= skate_net::lobby::MAX_APP_VALUE => b,
                _ => {
                    failures += 1;
                    continue;
                }
            };
            if !net.publish_application(&wire, bytes) {
                failures += 1;
            }
            published.insert(wire);
        }
        for key in mods.net_published.difference(&published).cloned().collect::<Vec<_>>() {
            net.publish_application(&key, vec![]);
        }
        mods.net_published = published;

        let records = net.application_records();
        drop(net);

        let mut live_wire = BTreeSet::new();
        let mut mismatches = BTreeSet::new();
        for (peer, wire, _seq, bytes) in records {
            if !wire.starts_with("net:") {
                continue;
            }
            if bytes.is_empty() {
                if let Some((owner, key)) = mods.net_remote_wire.remove(&(peer, wire)) {
                    mods.net_remote.remove(&(peer, owner, key));
                }
                continue;
            }
            let Ok(record) = serde_json::from_slice::<NetWire>(&bytes) else {
                continue;
            };
            let probe = Command::NetworkState {
                key: record.key.clone(),
                value: record.value.clone(),
            };
            if !probe.validate() {
                continue;
            }
            let Some(&fp) = packages.get(&record.owner) else {
                mismatches.insert(record.owner);
                continue;
            };
            if fp != record.fingerprint {
                mismatches.insert(record.owner);
                continue;
            }
            if record.value.is_null() {
                mods.net_remote
                    .remove(&(peer, record.owner.clone(), record.key.clone()));
                mods.net_remote_wire.remove(&(peer, wire));
                continue;
            }
            mods.net_remote.insert(
                (peer, record.owner.clone(), record.key.clone()),
                record.value,
            );
            mods.net_remote_wire
                .insert((peer, wire.clone()), (record.owner, record.key));
            live_wire.insert((peer, wire));
        }

        let stale: Vec<_> = mods
            .net_remote_wire
            .keys()
            .filter(|id| !live_wire.contains(*id))
            .cloned()
            .collect();
        for id in stale {
            if let Some((owner, key)) = mods.net_remote_wire.remove(&id) {
                mods.net_remote.remove(&(id.0, owner, key));
            }
        }
        mods.net_remote
            .retain(|(_, owner, _), _| packages.contains_key(owner));

        mods.net_status = if failures > 0 {
            format!("Mod network limit reached: {failures} records")
        } else if !mismatches.is_empty() {
            format!(
                "Mods missing, disabled or different: {}",
                mismatches.into_iter().collect::<Vec<_>>().join(", ")
            )
        } else {
            format!(
                "Matching mods synchronized | {} remote net keys",
                mods.net_remote.len()
            )
        };
        let solid_status=mods.replication.status.clone();
        mods.net_status.push_str(" | ");mods.net_status.push_str(&solid_status);
    });
}

fn camera_sample(mods: &Mods, owner: &str, body: &str) -> Option<vehicle_camera::Sample> {
    let id = *mods.bodies.get(&(owner.to_owned(), body.to_owned()))?;
    let snap = mods.world.read(id)?;
    Some(vehicle_camera::Sample {
        position: Vec3::from_array(snap.position),
        rotation: Quat::from_xyzw(snap.rotation[0], snap.rotation[1], snap.rotation[2], snap.rotation[3]).normalize(),
        velocity: Vec3::from_array(snap.linvel),
    })
}
fn record_camera(mods: &mut Mods, dt: f32) {
    let sample = mods.camera.owner.as_ref().and_then(|o| {
        mods.camera.rig.as_ref().and_then(|r| camera_sample(mods, o, &r.body))
    });
    if let Some(sample) = sample {
        if let Some(r) = mods.camera.rig.as_mut() { r.record(sample, dt); }
    } else if mods.camera.rig.is_some() { mods.camera.clear(); }
}

fn present_camera(world: &mut World) {
    let camera = world.query_filtered::<Entity, With<crate::camera::GameplayCamera>>()
        .iter(world).next();
    let Some(camera) = camera else { return; };
    let replay = world.resource::<crate::replay::Replay>().active;
    let customizing = world.get_resource::<crate::customiser::Customiser>().is_some_and(|c| c.open);
    let paused = world.resource::<crate::graphics_menu::Menu>().open;
    let dt = if paused {0.0} else {world.resource::<Time<Real>>().delta_secs().clamp(0.0,0.1)};
    let alpha = if paused {1.0} else {world.resource::<Time<Fixed>>().overstep_fraction()};
    world.resource_scope(|world, mut mods: Mut<Mods>| {
        let suppress = replay || customizing;
        if mods.camera.rig.is_none() || suppress {
            if let Some(near) = mods.camera.saved_near.take() {
                if let Some(mut projection) = world.get_mut::<Projection>(camera) {
                    if let Projection::Perspective(p) = &mut *projection { p.near = near; }
                }
            }
        }
        if suppress { return; }
        if let Some(mut rig) = mods.camera.rig.take() {
            let owner = mods.camera.owner.clone().unwrap_or_default();
            // A rig may be created during on_update, between fixed steps.
            if rig.sample(alpha).is_none() {
                if let Some(s) = camera_sample(&mods, &owner, &rig.body) {
                    rig.record(s, world.resource::<Time<Fixed>>().timestep().as_secs_f32());
                }
            }
            if let Some(sample) = rig.sample(alpha) {
                let view = rig.view(sample, dt, &mut mods.world);
                // Match camera and owned car visuals to the SAME interpolation sample.
                // Physics remains at the native current pose; only Transform is changed.
                let entities: Vec<_> = mods.graphics.iter()
                    .filter(|((o,_),g)| o == &owner && g.body.as_ref() == Some(&rig.body))
                    .map(|(_,g)|(g.entity,g.transform.clone())).collect();
                for (e,local) in entities {
                    if let Some(mut t) = world.get_mut::<Transform>(e) {
                        t.translation = sample.position + sample.rotation*Vec3::from_array(local.position);
                        t.rotation = (sample.rotation*Quat::from_array(local.rotation).normalize()).normalize();
                        t.scale=Vec3::from_array(local.scale);
                    }
                }
                if let Some(mut transform) = world.get_mut::<Transform>(camera) { *transform = view.transform; }
                if let Some(mut projection) = world.get_mut::<Projection>(camera) {
                    if let Projection::Perspective(p) = &mut *projection {
                        if mods.camera.saved_near.is_none() { mods.camera.saved_near = Some(p.near); }
                        p.near = view.near; p.fov = view.fov;
                    }
                }
            }
            mods.camera.rig = Some(rig);
            return;
        }
        let follow = mods.camera.follow.clone();
        if let Some((owner, body, offset)) = follow {
            if let Some(sample) = camera_sample(&mods, &owner, &body) {
                let eye = sample.position + sample.rotation * offset;
                if let Some(mut t) = world.get_mut::<Transform>(camera) {
                    *t = Transform::from_translation(eye).looking_at(sample.position + Vec3::Y * 0.5, Vec3::Y);
                }
            } else { mods.camera.clear(); }
        } else if let Some((position, look_at)) = mods.camera.fixed {
            if let Some(mut t) = world.get_mut::<Transform>(camera) {
                if let Some(target) = look_at { *t = Transform::from_translation(position).looking_at(target, Vec3::Y); }
                else { t.translation = position; }
            }
        }
    });
}
