//! The bridge from the engine-facing world audio components (`crate::world_audio`) to the hosts'
//! internal seams (`world_sources::WorldOwners`, `npc_skaters::NpcSkaters`), and the read-back
//! (`WorldAudioInstance`, `WorldAudioStats`). Runs before `native::mixmap_frame`; the read-back
//! after both hosts. Inert (returns at once, touches nothing) while no component exists and
//! nothing is held.
//!
//! What the bridge fills where the engine leaves a field open (retail rules from the recomp gap
//! runs G1 / G2, spec `world-audio-hookin` §7.3):
//! - velocity: [`AudioVelocity`] or the transform's change over the frame;
//! - vehicle speed: |velocity|; acceleration (`+144`): the speed change per second;
//! - ped `+68` footsteps on: the 3 nearest peds within 50 m; `+148` / `+156`: the 3-D distance to
//!   the listener / 20 m; materials: 0 (retail's pavements);
//! - NPC skaters: the list order (remote players first, then `list_order`, then spawn order).
//!
//! Remote multiplayer players (non-retail extension, user decision 2026-10-03) get an
//! [`NpcSkaterAudio`] with a lite state ([`AudioState::rolling`]) from their root transform and
//! the ground's audio material under them ([`ground_material`]).
use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use skate_audio::world::traffic::{EngineRecord, VehicleState};

use super::world_sources::{PED_LIST_RADIUS, WorldHeld, WorldOwners};
use crate::world_audio::*;

/// Retail's footsteps-on rule: the first 3 of the nearest-first ped list (`S+68`, gap run G2;
/// `aud_speech/default` field `53364DFA09A499DD`).
pub(crate) const FOOTSTEP_PEDS: usize = 3;
/// The model's far threshold for regular peds (`S+156`, `aud_characteristics` `A27215A909135B62`).
pub(crate) const PED_FAR_THRESHOLD: f32 = 20.0;

/// The bridge's publish step: publishers that write components each frame (the mod host, the
/// ghost) run before it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct WorldAudioPublish;

/// A ghost NPC skater (dev / mods, spec §3.10): replays recorded audio states
/// ([`super::state_replay::ghost_states`]) as its [`NpcSkaterAudio`] state, looping, 60 rows per
/// second of game time, the entity following the board.
#[derive(Component, Clone, Debug)]
pub(crate) struct GhostSkater {
    pub(crate) states: Vec<AudioState>,
    /// Rows played (fractional).
    pub(crate) at: f64,
}

impl GhostSkater {
    /// Load a state log's window as a ghost (see `ghost_states`).
    pub(crate) fn from_log(text: &str, from: f32, seconds: f32, anchor: [f32; 3]) -> Result<Self, String> {
        let states = super::state_replay::ghost_states(text, from, seconds, anchor)?;
        if states.is_empty() {
            return Err("empty window".into());
        }
        Ok(Self { states, at: 0.0 })
    }
}

/// Advance the ghosts: the row for this frame, its one-step push pulse OR-ed over the rows the
/// frame passed (as `skate_events::latch_pulses` does for the local player).
fn ghost_step(time: Res<Time>, mut ghosts: Query<(&mut GhostSkater, &mut NpcSkaterAudio, &mut Transform, &mut GlobalTransform)>) {
    for (mut ghost, mut npc, mut t, mut g) in &mut ghosts {
        let n = ghost.states.len();
        let before = ghost.at as usize;
        ghost.at += f64::from(time.delta_secs()) * 60.0;
        let now = ghost.at as usize;
        let mut s = ghost.states[now % n];
        s.push_trigger |= (before + 1..now).any(|i| ghost.states[i % n].push_trigger);
        if ghost.at >= (n * 1000) as f64 {
            ghost.at -= (n * 1000) as f64;
        }
        t.translation = Vec3::from_array(s.board_position);
        *g = GlobalTransform::from(*t);
        npc.state = Some(s);
    }
}

pub(crate) fn register(app: &mut App) {
    app.init_resource::<LivingWorldAudio>()
        .init_resource::<WorldAudioStats>()
        .init_resource::<Bridge>()
        .add_message::<PedSpeechEvent>()
        .add_message::<VehicleHorn>()
        .add_message::<VehicleAlarm>()
        .add_systems(Update, (tag_remote_players, ghost_step, publish.in_set(WorldAudioPublish)).chain().before(super::native::mixmap_frame).after(crate::app::FrameSet::Animation))
        .add_systems(Update, read_back.after(super::world_sources::frame).after(super::npc_skaters::frame));
}

/// The bridge's memory between frames.
#[derive(Resource, Default)]
pub(crate) struct Bridge {
    /// Last frame's position and speed per entity (velocity / acceleration from the change).
    last: HashMap<Entity, ([f32; 3], f32)>,
    /// Horn holds from [`VehicleHorn`] / [`VehicleAlarm`]: (state, seconds left).
    horns: HashMap<Entity, (HornState, f32)>,
    /// Spawn order for skaters without a list order.
    order: HashMap<Entity, u32>,
    next_order: u32,
    /// Engine records by name, and the names already reported unknown.
    engines: HashMap<String, EngineRecord>,
    unknown: HashSet<String>,
    /// Entities that carry a `WorldAudioInstance` now.
    tagged: HashSet<Entity>,
    /// Something was published last frame (the owners must be cleared once when it stops).
    active: bool,
}

/// The ground's audio material under a point (`material_of_tag` of the first surface a 1.5 m line
/// down from 0.5 m above it hits; `NO_MATERIAL` when none): the same stock line query the wheel
/// lines use.
pub(crate) fn ground_material(physics: &crate::physics::GamePhysics, at: Vec3) -> u32 {
    use skate_core::math::Vector3;
    let start = Vector3::new(at.x, at.y + 0.5, at.z);
    let end = Vector3::new(at.x, at.y - 1.0, at.z);
    match physics.world().query_thin_line(start, end) {
        Ok(Some(hit)) => skate_audio::player::state::material_of_tag(hit.tag & 0x7F),
        _ => skate_audio::player::state::NO_MATERIAL,
    }
}

fn velocity(bridge: &Bridge, e: Entity, at: [f32; 3], given: Option<&AudioVelocity>, dt: f32) -> [f32; 3] {
    if let Some(v) = given {
        return v.0.to_array();
    }
    match bridge.last.get(&e) {
        Some((last, _)) if dt > 0.0 => std::array::from_fn(|i| (at[i] - last[i]) / dt),
        _ => [0.0; 3],
    }
}

fn length(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// The +Z axis of a transform (the game's forward).
fn forward(t: &GlobalTransform) -> [f32; 3] {
    let z = t.affine().matrix3.z_axis;
    let n = z.length();
    if n > 1e-6 { (z / n).to_array() } else { [0.0, 0.0, 1.0] }
}

/// Remote multiplayer players take the NPC skater instance(s) (non-retail extension): tag their
/// roots once; [`publish`] fills their lite state.
fn tag_remote_players(mut commands: Commands, remotes: Query<Entity, (With<crate::multiplayer::appearance::RemoteCharacter>, Without<NpcSkaterAudio>)>) {
    for e in &remotes {
        commands.entity(e).insert(NpcSkaterAudio { remote: true, ..Default::default() });
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn publish(
    mut bridge: ResMut<Bridge>,
    mut owners: ResMut<WorldOwners>,
    mut skaters: ResMut<super::npc_skaters::NpcSkaters>,
    living: Res<LivingWorldAudio>,
    library: Option<Res<super::Library>>,
    physics: Option<Res<crate::physics::GamePhysics>>,
    time: Res<Time>,
    listener: Query<&GlobalTransform, With<super::GameAudioListener>>,
    vehicles: Query<(Entity, &GlobalTransform, &TrafficAudio, Option<&AudioVelocity>)>,
    mut peds: Query<(Entity, &GlobalTransform, &mut PedAudio, Option<&AudioVelocity>)>,
    mut npcs: Query<(Entity, &GlobalTransform, &mut NpcSkaterAudio, Option<&AudioVelocity>)>,
    mut speech: MessageReader<PedSpeechEvent>,
    mut horns: MessageReader<VehicleHorn>,
    mut alarms: MessageReader<VehicleAlarm>,
) {
    let any = !vehicles.is_empty() || !peds.is_empty() || !npcs.is_empty();
    if owners.expected != living.expected {
        owners.expected = living.expected;
    }
    if !any && !bridge.active && speech.is_empty() && horns.is_empty() && alarms.is_empty() {
        return;
    }
    let bridge = &mut *bridge;
    let dt = time.delta_secs();
    for e in speech.read() {
        if let Ok((_, _, mut ped, _)) = peds.get_mut(e.ped) {
            ped.speech_value = e.value.0;
        }
    }
    for h in horns.read() {
        bridge.horns.insert(h.vehicle, (HornState::Honk(h.kind.clamp(1, 5)), h.seconds.max(0.0)));
    }
    for a in alarms.read() {
        bridge.horns.insert(a.vehicle, (HornState::Alarm, ALARM_SECONDS));
    }
    for (_, left) in bridge.horns.values_mut() {
        *left -= dt;
    }
    bridge.horns.retain(|e, (_, left)| *left > 0.0 && vehicles.contains(*e));
    let camera = listener.single().map(|t| t.translation()).unwrap_or(Vec3::ZERO);

    // Vehicles.
    owners.vehicles.clear();
    let mut seen = Vec::with_capacity(vehicles.iter().len());
    for (e, t, car, given) in &vehicles {
        let at = t.translation().to_array();
        let v = velocity(bridge, e, at, given, dt);
        let speed = car.speed.unwrap_or_else(|| length(v)).max(0.0);
        let last_speed = bridge.last.get(&e).map_or(speed, |l| l.1);
        let load = car.load.unwrap_or(if dt > 0.0 { (speed - last_speed) / dt } else { 0.0 });
        let record = engine_record(bridge, library.as_deref(), &car.engine);
        let horn = bridge.horns.get(&e).map_or(car.horn, |h| h.0);
        owners.vehicles.insert(
            e.to_bits(),
            VehicleState { position: at, velocity: v, direction: forward(t), speed, load, horn: horn.word(), skid: i32::from(car.skidding), engine: record },
        );
        seen.push((e, at, speed));
    }

    // Peds: the nearest-first list within 50 m gives the footsteps-on rule.
    owners.peds.clear();
    let mut list: Vec<(f32, Entity)> = peds.iter().map(|(e, t, _, _)| (t.translation().distance(camera), e)).filter(|(d, _)| *d < PED_LIST_RADIUS).collect();
    list.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.to_bits().cmp(&b.1.to_bits())));
    let nearest: HashSet<Entity> = list.iter().take(FOOTSTEP_PEDS).map(|x| x.1).collect();
    for (e, t, ped, given) in &peds {
        let at = t.translation().to_array();
        let v = velocity(bridge, e, at, given, dt);
        let distance = t.translation().distance(camera);
        let (measure, limit) = ped.speech_distance.unwrap_or((distance, PED_FAR_THRESHOLD));
        owners.peds.insert(
            e.to_bits(),
            skate_audio::world::peds::PedState {
                position: at,
                velocity: v,
                speed: length(v),
                feet: ped.feet_down,
                footsteps: ped.footsteps_on.unwrap_or_else(|| nearest.contains(&e)),
                materials: ped.foot_materials.unwrap_or([0, 0]),
                speech_value: ped.speech_value,
                class: i32::from(ped.shoe_class.clamp(1, 5)),
                weight: i32::from(ped.weight.clamp(1, 5)),
                close: ped.close_range,
                speech_measure: measure,
                speech_limit: limit,
            },
        );
        seen.push((e, at, 0.0));
    }

    // NPC / remote skaters, in list order.
    skaters.skaters.clear();
    let mut list: Vec<(bool, u32, u32, u64, AudioState)> = Vec::new();
    for (e, t, mut npc, given) in &mut npcs {
        let at = t.translation().to_array();
        if npc.remote {
            let v = velocity(bridge, e, at, given, dt);
            let f = forward(t);
            let material = physics.as_deref().map_or(skate_audio::player::state::NO_MATERIAL, |p| ground_material(p, t.translation()));
            npc.state = Some(AudioState::rolling(&LiteSkater { position: at, velocity: v, heading: f[0].atan2(f[2]), material, dt: dt.max(1e-4), ..Default::default() }));
        }
        seen.push((e, at, 0.0));
        let Some(state) = npc.state else { continue };
        let spawn = *bridge.order.entry(e).or_insert_with(|| {
            bridge.next_order += 1;
            bridge.next_order
        });
        list.push((!npc.remote, if npc.list_order == 0 { u32::MAX } else { npc.list_order }, spawn, e.to_bits(), state));
    }
    list.sort_by(|a, b| (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2)));
    skaters.skaters.extend(list.into_iter().map(|(_, _, _, id, state)| skate_audio::world::skaters::NpcSkaterAudioState { id, state }));

    bridge.last.clear();
    for (e, at, speed) in seen {
        bridge.last.insert(e, (at, speed));
    }
    bridge.order.retain(|e, _| npcs.contains(*e));
    bridge.active = any;
}

fn engine_record(bridge: &mut Bridge, library: Option<&super::Library>, name: &str) -> EngineRecord {
    if let Some(r) = bridge.engines.get(name) {
        return *r;
    }
    let Some(library) = library else { return EngineRecord::default() };
    let tuning = library.world_tuning();
    let record = match tuning.engine(&name.to_ascii_lowercase()) {
        Some(r) => r,
        None => {
            if bridge.unknown.insert(name.to_owned()) {
                warn!("AUDIO_WORLD unknown traffic engine record {name:?}: the default record (silent)");
            }
            tuning.engine("default").unwrap_or_default()
        }
    };
    bridge.engines.insert(name.to_owned(), record);
    record
}

/// `WorldAudioInstance` on the holders, and the stats.
fn read_back(
    mut commands: Commands,
    mut bridge: ResMut<Bridge>,
    held: Res<WorldHeld>,
    owners: Res<WorldOwners>,
    skaters: Res<super::npc_skaters::NpcSkaters>,
    native: Option<Res<super::native::Native>>,
    mut stats: ResMut<WorldAudioStats>,
    instances: Query<&WorldAudioInstance>,
) {
    if !bridge.active && bridge.tagged.is_empty() {
        return;
    }
    let mut now: HashMap<Entity, WorldAudioInstance> = HashMap::new();
    for (list, slot) in [(&held.traffic, WorldAudioSlot::Traffic), (&held.peds, WorldAudioSlot::Ped), (&held.skaters, WorldAudioSlot::PlayerSlot)] {
        for &(id, instance) in list {
            now.insert(Entity::from_bits(id), WorldAudioInstance { slot, instance });
        }
    }
    for e in bridge.tagged.clone() {
        if !now.contains_key(&e) {
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.try_remove::<WorldAudioInstance>();
            }
            bridge.tagged.remove(&e);
        }
    }
    for (e, tag) in now {
        if instances.get(e).ok() != Some(&tag) {
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.try_insert(tag);
                bridge.tagged.insert(e);
            }
        }
    }
    let world = native.as_deref().map_or(super::native::WorldInstances::RETAIL, |n| n.world);
    let next = WorldAudioStats {
        vehicles: owners.vehicles.len(),
        peds: owners.peds.len(),
        skaters: skaters.skaters.len(),
        traffic_held: held.traffic.len(),
        peds_held: held.peds.len(),
        skaters_held: held.skaters.len(),
        instances: (world.traffic, world.peds, world.npc),
        more_audible: world != super::native::WorldInstances::RETAIL,
    };
    if *stats != next {
        *stats = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<WorldOwners>().init_resource::<super::super::npc_skaters::NpcSkaters>().init_resource::<WorldHeld>();
        app.init_resource::<LivingWorldAudio>().init_resource::<WorldAudioStats>().init_resource::<Bridge>();
        app.add_message::<PedSpeechEvent>().add_message::<VehicleHorn>().add_message::<VehicleAlarm>();
        app.add_systems(Update, (publish, read_back).chain());
        app.world_mut().spawn((super::super::GameAudioListener, Transform::default(), GlobalTransform::default()));
        app
    }

    fn at(x: f32, z: f32) -> (Transform, GlobalTransform) {
        let t = Transform::from_xyz(x, 0.0, z);
        (t, GlobalTransform::from(t))
    }

    /// Components become owners (ids = entity bits), events hold their states for the right
    /// time, despawning releases, and the read-back tags the holders.
    #[test]
    fn components_become_owners_and_events_hold_states() {
        let mut app = app();
        let car = app.world_mut().spawn((TrafficAudio::new("c04_taxi01"), at(0.0, 10.0), AudioVelocity(Vec3::new(0.0, 0.0, 8.0)))).id();
        let peds: Vec<Entity> = (0..5).map(|i| app.world_mut().spawn((PedAudio { voice: Some(59), ..Default::default() }, at(1.0, 2.0 + i as f32))).id()).collect();
        let far = app.world_mut().spawn((PedAudio::default(), at(0.0, 60.0))).id();
        let npc = app.world_mut().spawn((NpcSkaterAudio { state: Some(AudioState::default()), ..Default::default() }, at(3.0, 3.0))).id();
        let remote_like = app.world_mut().spawn((NpcSkaterAudio { state: Some(AudioState::default()), remote: true, ..Default::default() }, at(4.0, 3.0))).id();
        app.update();
        {
            let owners = app.world().resource::<WorldOwners>();
            let v = owners.vehicles[&car.to_bits()];
            assert_eq!(v.velocity, [0.0, 0.0, 8.0]);
            assert_eq!(v.speed, 8.0);
            assert_eq!(v.direction, [0.0, 0.0, 1.0]);
            assert_eq!(owners.peds.len(), 6);
            // The 3 nearest within 50 m step; the others (and the one beyond 50 m) don't.
            let on: Vec<bool> = peds.iter().map(|p| owners.peds[&p.to_bits()].footsteps).collect();
            assert_eq!(on, [true, true, true, false, false]);
            assert!(!owners.peds[&far.to_bits()].footsteps);
            assert_eq!(owners.peds[&far.to_bits()].speech_limit, PED_FAR_THRESHOLD);
            // Remote players first in the list order.
            let s = app.world().resource::<super::super::npc_skaters::NpcSkaters>();
            assert_eq!(s.skaters.iter().map(|s| s.id).collect::<Vec<_>>(), vec![remote_like.to_bits(), npc.to_bits()]);
        }
        // Events.
        app.world_mut().write_message(VehicleAlarm { vehicle: car });
        app.world_mut().write_message(PedSpeechEvent { ped: peds[0], value: SpeechValue::WARN });
        app.update();
        assert_eq!(app.world().resource::<WorldOwners>().vehicles[&car.to_bits()].horn, 6);
        assert_eq!(app.world().get::<PedAudio>(peds[0]).unwrap().speech_value, 11);
        app.world_mut().write_message(VehicleHorn { vehicle: car, kind: 3, seconds: 0.0 });
        app.update();
        assert_eq!(app.world().resource::<WorldOwners>().vehicles[&car.to_bits()].horn, 0, "a zero-length horn ends at once");
        // Read-back.
        app.world_mut().resource_mut::<WorldHeld>().traffic = vec![(car.to_bits(), 2)];
        app.update();
        assert_eq!(app.world().get::<WorldAudioInstance>(car), Some(&WorldAudioInstance { slot: WorldAudioSlot::Traffic, instance: 2 }));
        app.world_mut().resource_mut::<WorldHeld>().traffic.clear();
        app.update();
        assert!(app.world().get::<WorldAudioInstance>(car).is_none());
        // Despawn = release: the owner disappears.
        app.world_mut().despawn(car);
        app.update();
        assert!(app.world().resource::<WorldOwners>().vehicles.is_empty());
        for p in peds.into_iter().chain([far, npc, remote_like]) {
            app.world_mut().despawn(p);
        }
        app.update();
        assert!(app.world().resource::<WorldOwners>().peds.is_empty());
        assert!(app.world().resource::<super::super::npc_skaters::NpcSkaters>().skaters.is_empty());
    }

    /// No component, nothing held: the bridge does not touch the owners (no change detection).
    #[test]
    fn inert_without_components() {
        let mut app = app();
        app.update();
        app.update();
        assert!(app.world().resource::<WorldOwners>().vehicles.is_empty());
        assert!(!app.world().resource::<Bridge>().active);
    }
}
