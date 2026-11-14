//! Named trigger volumes in the world and their enter/exit events.
//!
//! Retail (TU3, notes `triggers-volumes-re.md`): the trigger manager keeps three
//! groups (Challenge, Stairs, Camera). Each sim tick a group gives every tracked
//! entity a query cylinder, tests it against each volume's oriented box and
//! posts `cMsgTriggerEnterCollision` / `cMsgTriggerExitCollision` for the
//! differences, once with the volume's link GUID and once with its instance id.
//! The geometry and bookkeeping live in `skate_core::triggers`; this module
//! loads a map's volumes (`<Map>.triggers` sidecar or a `TVOL` extension),
//! tracks the player and any registered body, and publishes
//! [`TriggerEntered`] / [`TriggerExited`] messages. Mods reach the same data
//! through `sdk.triggers` (`modding/triggers.rs`).
use bevy::prelude::*;
use skate_core::triggers::{Cylinder, OrientedBox, QueryShape, Tracker, Transition};
use skate_data::trigger_volumes::{TriggerGroup, TriggerVolumeRecord};
use std::collections::BTreeMap;
use std::path::Path;

/// The local player's body id in events and snapshots.
pub(crate) const PLAYER: &str = "player";
/// Ids reserved for mod-owned volumes and bodies.
pub(crate) const MOD_PREFIX: &str = "mod:";

/// Where a volume came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum VolumeSource {
    /// The loaded map (retail export or a custom map's data).
    Map,
    /// Added at runtime by a mod (`sdk.triggers.box`), removed with it.
    Mod(String),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TriggerVolume {
    pub id: String,
    pub name: String,
    pub full_name: Option<String>,
    pub group: TriggerGroup,
    pub shape: OrientedBox,
    pub instance_id: Option<u64>,
    pub link_guid: Option<u64>,
    pub source: VolumeSource,
}

impl TriggerVolume {
    pub(crate) fn from_record(record: &TriggerVolumeRecord) -> Self {
        Self {
            id: record.id.clone(),
            name: record.name.clone(),
            full_name: record.full_name.clone(),
            group: record.group,
            shape: record.shape.oriented_box(),
            instance_id: record.instance(),
            link_guid: record.link(),
            source: VolumeSource::Map,
        }
    }
}

/// All trigger volumes of the current world: the map's (in file order, the
/// retail registration order) and mod-owned ones. Mods can switch map
/// volumes off; the switch belongs to the mod and ends with it.
#[derive(Resource, Clone, Debug, Default)]
pub(crate) struct TriggerVolumes {
    pub map: Vec<TriggerVolume>,
    /// (owner mod, key) -> volume with id `mod:<owner>:<key>`.
    pub mods: BTreeMap<(String, String), TriggerVolume>,
    /// Map volume id -> mod that disabled it.
    pub disabled: BTreeMap<String, String>,
    /// Human-readable source for logs (`sidecar <path>`, `embedded`, `none`).
    pub origin: String,
}

impl TriggerVolumes {
    pub(crate) fn from_records(records: &[TriggerVolumeRecord], origin: String) -> Result<Self, String> {
        if let Some(bad) = records.iter().find(|r| r.id.starts_with(MOD_PREFIX)) {
            return Err(format!("Trigger volume id {:?} uses the reserved prefix {MOD_PREFIX}", bad.id));
        }
        Ok(Self { map: records.iter().map(TriggerVolume::from_record).collect(), origin, ..Default::default() })
    }

    /// The map's volumes; a missing source is no volumes, a broken one is an error.
    pub(crate) fn load(map_path: Option<&Path>, map: Option<&skate_data::skate_map::SkateMap>) -> Result<Self, String> {
        let (records, origin) = skate_data::trigger_volumes::load_for_map(map_path, map)?;
        let origin = match origin {
            skate_data::trigger_volumes::Origin::Sidecar(path) => format!("sidecar {}", path.display()),
            skate_data::trigger_volumes::Origin::Embedded => "embedded TVOL".into(),
            skate_data::trigger_volumes::Origin::None => "none".into(),
        };
        Self::from_records(&records, origin)
    }

    /// Runtime loading never refuses a map over optional trigger data.
    pub(crate) fn load_or_warn(map_path: Option<&Path>, map: Option<&skate_data::skate_map::SkateMap>) -> Self {
        match Self::load(map_path, map) {
            Ok(volumes) => volumes,
            Err(error) => {
                warn!("SKATE_TRIGGERS unavailable: {error}");
                Self { origin: format!("unavailable: {error}"), ..Default::default() }
            }
        }
    }

    pub(crate) fn get(&self, id: &str) -> Option<&TriggerVolume> {
        self.map.iter().find(|v| v.id == id).or_else(|| self.mods.values().find(|v| v.id == id))
    }

    pub(crate) fn enabled(&self, id: &str) -> bool {
        !self.disabled.contains_key(id)
    }

    /// Active volumes in registration order: map volumes, then mod volumes.
    /// Only groups with entity slots (Challenge) take part; retail's Stairs and
    /// Camera groups never post enter/exit (`TriggerGroup::tracks_bodies`).
    pub(crate) fn active(&self) -> Vec<(String, OrientedBox)> {
        self.map.iter().filter(|v| self.enabled(&v.id))
            .chain(self.mods.values())
            .filter(|v| v.group.tracks_bodies())
            .map(|v| (v.id.clone(), v.shape))
            .collect()
    }

    pub(crate) fn mod_id(owner: &str, key: &str) -> String {
        format!("{MOD_PREFIX}{owner}:{key}")
    }

    /// Drop everything a mod added or switched off.
    pub(crate) fn clear_owner(&mut self, owner: &str) {
        self.mods.retain(|(o, _), _| o != owner);
        self.disabled.retain(|_, o| o != owner);
    }
}

/// Bodies tracked besides the local player: id -> query cylinder for the next
/// update. Engine systems and the mod bridge write here; an entry stays until
/// removed, and removing it posts exits (retail entity removal).
#[derive(Resource, Clone, Debug, Default)]
pub(crate) struct TriggerBodies {
    pub bodies: BTreeMap<String, Cylinder>,
}

/// Tracker state plus the query-shape constants (retail defaults, data a mod
/// can change through `sdk.triggers.configure`).
#[derive(Resource, Clone, Debug, Default)]
pub(crate) struct TriggerState {
    pub tracker: Tracker<String, String>,
    pub shape: QueryShape,
    /// Owner of a changed query shape, so it resets when that mod goes away.
    pub shape_owner: Option<String>,
    /// Events not yet delivered to Lua mods (drained by the mod runtime).
    pub mod_queue: Vec<TriggerEvent>,
    pub player_tracked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TriggerEvent {
    pub body: String,
    pub volume: String,
    pub entered: bool,
}

/// A tracked body's query shape now overlaps a volume it did not overlap last tick.
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub(crate) struct TriggerEntered {
    pub body: String,
    pub volume: String,
}

/// A tracked body left a volume (or stopped being tracked while inside it).
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub(crate) struct TriggerExited {
    pub body: String,
    pub volume: String,
}

/// Engine system sets other systems can order against.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct TriggerSet;

pub(crate) struct TriggerVolumesPlugin;

impl Plugin for TriggerVolumesPlugin {
    fn build(&self, app: &mut App) {
        let config = app.world().resource::<crate::config::Config>();
        let volumes = TriggerVolumes::load_or_warn(config.map_path.as_deref(), config.map.as_ref());
        info!("SKATE_TRIGGERS map={:?} volumes={} source={}", config.map.as_ref().map(|m| m.name.as_str()),
            volumes.map.len(), volumes.origin);
        app.insert_resource(volumes)
            .init_resource::<TriggerBodies>()
            .insert_resource(TriggerState { player_tracked: true, ..Default::default() })
            .add_message::<TriggerEntered>()
            .add_message::<TriggerExited>()
            .add_systems(PreUpdate, reset_on_world_change.after(crate::map_transition::MapTransitionSet))
            .add_systems(FixedUpdate, update.in_set(TriggerSet)
                .after(crate::app::SimulationSet::Physics)
                .run_if(crate::graphics_menu::gameplay_active));
    }
}

/// A committed map replaces the volume list; forget insides without events.
fn reset_on_world_change(
    map: Res<crate::map_transition::CurrentMap>,
    mut state: ResMut<TriggerState>,
    mut bodies: ResMut<TriggerBodies>,
    mut generation: Local<u64>,
) {
    if map.generation != *generation {
        *generation = map.generation;
        state.tracker.clear();
        state.mod_queue.clear();
        bodies.bodies.clear();
    }
}

/// The player's three query points, as the retail skater entity reports them
/// (vtable +12 / +16 / +20, measured in the recomp with a hook on `82DD80B8`):
/// the feet (the character's ground point: the animation root, which the
/// recomp put exactly on the ground plane while standing), the head and the
/// hips. Recomp: head 1.61-1.65 m and hips 0.97-1.00 m above the feet.
pub(crate) fn player_points(skater: &crate::physics::SkaterRuntime) -> [[f32; 3]; 3] {
    const HEAD: usize = 1;
    const HIPS: usize = 23;
    let parts = skater.skeleton.part_transforms();
    let p = |i: usize| -> [f32; 3] { let t = parts[i][3]; [t[0], t[1], t[2]] };
    let root = skater.animated_skeleton.roots.animation_to_world[3];
    [[root[0], root[1], root[2]], p(HEAD), p(HIPS)]
}

/// Player query cylinder: feet to just above the head (`player_points`).
pub(crate) fn player_cylinder(skater: &crate::physics::SkaterRuntime, shape: &QueryShape) -> Option<Cylinder> {
    let [feet, head, hips] = player_points(skater);
    shape.cylinder(feet, head, hips)
}

fn update(
    volumes: Res<TriggerVolumes>,
    bodies: Res<TriggerBodies>,
    mut state: ResMut<TriggerState>,
    skater: Res<crate::physics::SkaterRuntime>,
    entered: MessageWriter<TriggerEntered>,
    exited: MessageWriter<TriggerExited>,
) {
    // Retail moves the entity within the frame; ours moves a tick or two later, so while a teleport is in flight the
    // player's old spot is not checked (a challenge start entered and left its spot volumes from the sign-up point).
    if state.player_tracked && skater.teleport_pending() {
        return;
    }
    let player = state.player_tracked.then(|| player_cylinder(&skater, &state.shape)).flatten();
    step(&volumes, &bodies, &mut state, player, entered, exited);
}

/// One trigger update: the player first (retail slot 0), then other bodies.
pub(crate) fn step(
    volumes: &TriggerVolumes,
    bodies: &TriggerBodies,
    state: &mut TriggerState,
    player: Option<Cylinder>,
    mut entered: MessageWriter<TriggerEntered>,
    mut exited: MessageWriter<TriggerExited>,
) {
    let mut tracked: Vec<(String, Cylinder)> = Vec::with_capacity(1 + bodies.bodies.len());
    tracked.extend(player.map(|c| (PLAYER.to_string(), c)));
    tracked.extend(bodies.bodies.iter().filter(|(k, _)| k.as_str() != PLAYER || player.is_none()).map(|(k, c)| (k.clone(), *c)));
    let active = volumes.active();
    if active.is_empty() && state.tracker.bodies().next().is_none() {
        return;
    }
    let events = state.tracker.update(&tracked, &active);
    for event in events {
        let entered_now = event.transition == Transition::Entered;
        if let Some(volume) = volumes.get(&event.volume) {
            info!("SKATE_TRIGGER {} body={} volume={} name={} group={}",
                if entered_now { "enter" } else { "exit" }, event.body, volume.id, volume.name, volume.group.as_str());
        }
        if entered_now {
            entered.write(TriggerEntered { body: event.body.clone(), volume: event.volume.clone() });
        } else {
            exited.write(TriggerExited { body: event.body.clone(), volume: event.volume.clone() });
        }
        state.mod_queue.push(TriggerEvent { body: event.body, volume: event.volume, entered: entered_now });
    }
    // Lua delivery is bounded; a mod runtime that never drains must not grow this.
    let excess = state.mod_queue.len().saturating_sub(1024);
    state.mod_queue.drain(..excess);
}

/// `--check-assets`: a broken trigger sidecar or TVOL extension fails the map.
pub(crate) fn check(map_path: Option<&Path>, map: Option<&skate_data::skate_map::SkateMap>) -> Result<usize, String> {
    TriggerVolumes::load(map_path, map).map(|v| v.map.len())
}

#[cfg(test)]
#[path = "tests/trigger_volumes.rs"]
mod tests;
