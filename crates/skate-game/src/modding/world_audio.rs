//! Mod-owned world audio objects (API 2, world audio extension 1; `skate_mods::world_audio`):
//! each `sdk.world_audio.spawn` key becomes an entity with the same engine-facing components a
//! game system would add (`crate::world_audio`), so mods and engine systems are equal publishers
//! of the retail world audio. Keys belong to the calling mod; 16 objects per mod, 64 in all; an
//! object not updated for 0.5 s is parked (speed 0, feet up, horn off; ghosts never park);
//! everything is despawned when the mod is disabled, reloaded or fails, and on a runtime reset.
use super::Mods;
use crate::world_audio::*;
use bevy::prelude::*;
use serde_json::{Value, json};
use skate_mods::world_audio::{MAX_OBJECTS_PER_MOD, MAX_OBJECTS_TOTAL, ObjectKind, PARK_SECONDS, WorldAudioEventOptions, WorldAudioOptions};
use std::collections::BTreeMap;

type Key = (String, String);

struct Object {
    entity: Entity,
    kind: ObjectKind,
    /// The merged spawn + update fields (the object's current description).
    state: WorldAudioOptions,
    /// `Time<Real>` seconds of the last spawn / update.
    updated: f64,
    /// A ghost (driven by its recorded states, never parked).
    ghost: bool,
    /// The last position and the derived velocity (lite skaters).
    last: Option<Vec3>,
}

#[derive(Resource, Default)]
pub(super) struct ModWorldAudio {
    objects: BTreeMap<Key, Object>,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<ModWorldAudio>();
    app.add_systems(
        Update,
        sync.after(super::update).before(crate::game_audio::world_bridge::WorldAudioPublish),
    );
}

fn merge(into: &mut WorldAudioOptions, from: WorldAudioOptions) {
    macro_rules! take {
        ($($f:ident),*) => { $( if from.$f.is_some() { into.$f = from.$f; } )* };
    }
    take!(position, velocity, heading, body, engine, speed, load, horn, skidding, voice, shoe_class, weight, close_range, feet, materials, footsteps, source, from, seconds, wheels, material, grinding, grind_material, air);
}

/// The ghost's states from `logs/<name>.tsv` in the mod, or `SKATE_AUDIO_STATE_LOGS/<name>.tsv`.
fn ghost(mods: &Mods, owner: &str, name: &str, opts: &WorldAudioOptions, anchor: Vec3) -> Result<crate::game_audio::world_bridge::GhostSkater, String> {
    const MAX_LOG: u64 = 64 * 1024 * 1024;
    let file = format!("logs/{name}.tsv");
    let root = &mods.manager.packages.get(owner).ok_or("Missing world audio owner")?.root;
    let bytes = skate_mods::read_bounded(root, &file, MAX_LOG).or_else(|e| {
        let dir = std::env::var_os("SKATE_AUDIO_STATE_LOGS").ok_or(e)?;
        skate_mods::read_bounded(std::path::Path::new(&dir), &format!("{name}.tsv"), MAX_LOG)
    })?;
    let text = String::from_utf8(bytes).map_err(|_| format!("{file}: not text"))?;
    crate::game_audio::world_bridge::GhostSkater::from_log(&text, opts.from.unwrap_or(0.0), opts.seconds.unwrap_or(20.0), anchor.to_array())
        .map_err(|e| format!("{file}: {e}"))
}

pub(super) fn spawn(world: &mut World, mods: &Mods, owner: &str, key: String, kind: ObjectKind, opts: WorldAudioOptions) -> Result<(), String> {
    if !opts.validate_for(kind) {
        return Err("Invalid world audio options".into());
    }
    if let Some(body) = &opts.body {
        super::resolve_body(mods, owner, body)?;
    }
    let slot = (owner.to_owned(), key);
    let ghost_source = opts.source.as_deref().and_then(|s| s.strip_prefix("state_log:")).map(str::to_owned);
    let now = world.resource::<Time<Real>>().elapsed_secs_f64();
    world.resource_scope(|world, mut audio: Mut<ModWorldAudio>| {
        if !audio.objects.contains_key(&slot)
            && (audio.objects.len() >= MAX_OBJECTS_TOTAL || audio.objects.keys().filter(|(o, _)| o == owner).count() >= MAX_OBJECTS_PER_MOD)
        {
            return Err(format!("World audio object limit reached ({MAX_OBJECTS_PER_MOD}/mod, {MAX_OBJECTS_TOTAL} total)"));
        }
        let position = Vec3::from_array(opts.position.unwrap_or([0.0; 3]));
        let ghost = match &ghost_source {
            Some(name) => Some(ghost(mods, owner, name, &opts, position)?),
            None => None,
        };
        if let Some(old) = audio.objects.remove(&slot) {
            world.despawn(old.entity);
        }
        let t = Transform::from_translation(position).with_rotation(Quat::from_rotation_y(opts.heading.unwrap_or(0.0)));
        let mut e = world.spawn((t, GlobalTransform::from(t), Name::new(format!("world audio {}:{}", slot.0, slot.1))));
        match kind {
            ObjectKind::Traffic => {
                e.insert(TrafficAudio::new(opts.engine.clone().unwrap_or_else(|| "default".into())));
            }
            ObjectKind::Ped => {
                e.insert(PedAudio::default());
            }
            ObjectKind::Skater => {
                e.insert(NpcSkaterAudio::default());
            }
        }
        let is_ghost = ghost.is_some();
        if let Some(g) = ghost {
            e.insert(g);
        }
        let entity = e.id();
        audio.objects.insert(slot, Object { entity, kind, state: opts, updated: now, ghost: is_ghost, last: None });
        Ok(())
    })
}

pub(super) fn update(world: &mut World, mods: &Mods, owner: &str, key: &str, opts: WorldAudioOptions) -> Result<(), String> {
    if let Some(body) = &opts.body {
        super::resolve_body(mods, owner, body)?;
    }
    let now = world.resource::<Time<Real>>().elapsed_secs_f64();
    let mut audio = world.resource_mut::<ModWorldAudio>();
    let Some(obj) = audio.objects.get_mut(&(owner.to_owned(), key.to_owned())) else {
        return Err(format!("unknown world audio object {key}"));
    };
    if !opts.validate_for(obj.kind) {
        return Err(format!("Invalid world audio update for {key}"));
    }
    merge(&mut obj.state, opts);
    obj.updated = now;
    Ok(())
}

pub(super) fn event(world: &mut World, owner: &str, key: &str, event: &str, opts: WorldAudioEventOptions) -> Result<(), String> {
    let audio = world.resource::<ModWorldAudio>();
    let Some(obj) = audio.objects.get(&(owner.to_owned(), key.to_owned())) else {
        return Err(format!("unknown world audio object {key}"));
    };
    let (entity, kind) = (obj.entity, obj.kind);
    match (event, kind) {
        ("horn", ObjectKind::Traffic) => {
            world.write_message(VehicleHorn { vehicle: entity, kind: opts.kind.unwrap_or(1), seconds: opts.seconds.unwrap_or(0.6) });
        }
        ("alarm", ObjectKind::Traffic) => {
            world.write_message(VehicleAlarm { vehicle: entity });
        }
        ("speech", ObjectKind::Ped) => {
            let value = match &opts.value {
                Some(Value::Number(n)) => n.as_i64().map(|v| SpeechValue(v as i32)),
                Some(Value::String(s)) => SpeechValue::from_name(s),
                _ => None,
            }
            .ok_or_else(|| format!("unknown speech value {:?}", opts.value))?;
            world.write_message(PedSpeechEvent { ped: entity, value });
        }
        _ => return Err(format!("event {event} does not apply to a {kind:?} object")),
    }
    Ok(())
}

pub(super) fn remove(world: &mut World, owner: &str, key: &str) {
    let obj = world.resource_mut::<ModWorldAudio>().objects.remove(&(owner.to_owned(), key.to_owned()));
    if let Some(obj) = obj {
        world.despawn(obj.entity);
    }
}

pub(super) fn clear_owner(world: &mut World, owner: &str) {
    let mut audio = world.resource_mut::<ModWorldAudio>();
    let keys: Vec<Key> = audio.objects.keys().filter(|(o, _)| o == owner).cloned().collect();
    let entities: Vec<Entity> = keys.iter().filter_map(|k| audio.objects.remove(k)).map(|o| o.entity).collect();
    for e in entities {
        world.despawn(e);
    }
}

pub(super) fn clear(world: &mut World) {
    let objects = std::mem::take(&mut world.resource_mut::<ModWorldAudio>().objects);
    for (_, o) in objects {
        world.despawn(o.entity);
    }
}

/// `snapshot.world_audio[owner]`: per key `{kind, audible, instance}`.
pub(super) fn snapshot(world: &World, owner: &str) -> Value {
    let audio = world.resource::<ModWorldAudio>();
    let mut out = serde_json::Map::new();
    for ((o, key), obj) in &audio.objects {
        if o != owner {
            continue;
        }
        let held = world.get::<WorldAudioInstance>(obj.entity);
        out.insert(key.clone(), json!({
            "kind": match obj.kind { ObjectKind::Traffic => "traffic", ObjectKind::Ped => "ped", ObjectKind::Skater => "skater" },
            "audible": held.is_some(),
            "instance": held.map(|h| h.instance),
            "parked": world.resource::<Time<Real>>().elapsed_secs_f64() - obj.updated > PARK_SECONDS && !obj.ghost,
        }));
    }
    Value::Object(out)
}

/// `snapshot.world_audio_info`: the instance layout and the published counts.
pub(super) fn info(world: &World) -> Value {
    let s = world.get_resource::<WorldAudioStats>().cloned().unwrap_or_default();
    json!({
        "more_audible": s.more_audible,
        "instances": {"traffic": s.instances.0, "peds": s.instances.1, "skaters": s.instances.2},
        "published": {"traffic": s.vehicles, "peds": s.peds, "skaters": s.skaters},
        "audible": {"traffic": s.traffic_held, "peds": s.peds_held, "skaters": s.skaters_held},
    })
}

/// Write every object's components from its description: body following, parking, the lite
/// skater's state.
#[allow(clippy::type_complexity)]
fn sync(
    mut audio: ResMut<ModWorldAudio>,
    mods: Res<Mods>,
    time: Res<Time<Real>>,
    game_time: Res<Time>,
    physics: Option<Res<crate::physics::GamePhysics>>,
    mut q: Query<(&mut Transform, &mut GlobalTransform, Option<&mut TrafficAudio>, Option<&mut PedAudio>, Option<&mut NpcSkaterAudio>, Option<&mut AudioVelocity>)>,
    mut commands: Commands,
) {
    if audio.objects.is_empty() {
        return;
    }
    let now = time.elapsed_secs_f64();
    let dt = game_time.delta_secs();
    for ((owner, _), obj) in audio.objects.iter_mut() {
        if obj.ghost {
            continue;
        }
        let Ok((mut t, mut g, car, ped, npc, vel)) = q.get_mut(obj.entity) else { continue };
        let parked = now - obj.updated > PARK_SECONDS;
        let s = &obj.state;
        // Where: a body of this mod, or the given position / heading.
        let mut velocity = s.velocity.map(Vec3::from_array);
        if let Some(read) = s.body.as_deref().and_then(|b| super::resolve_body(&mods, owner, b).ok()).and_then(|id| mods.world.read(id)) {
            t.translation = Vec3::from_array(read.position);
            let q = read.rotation;
            t.rotation = Quat::from_xyzw(q[0], q[1], q[2], q[3]);
            velocity = Some(Vec3::from_array(read.linvel));
        } else {
            if let Some(p) = s.position {
                t.translation = Vec3::from_array(p);
            }
            if let Some(h) = s.heading {
                t.rotation = Quat::from_rotation_y(h);
            }
        }
        *g = GlobalTransform::from(*t);
        let derived = match (obj.last, dt > 0.0) {
            (Some(last), true) => (t.translation - last) / dt,
            _ => Vec3::ZERO,
        };
        obj.last = Some(t.translation);
        let v = if parked { Some(Vec3::ZERO) } else { velocity };
        match (v, vel) {
            (Some(v), Some(mut a)) => a.0 = v,
            (Some(v), None) => {
                commands.entity(obj.entity).insert(AudioVelocity(v));
            }
            (None, Some(_)) => {
                commands.entity(obj.entity).remove::<AudioVelocity>();
            }
            (None, None) => {}
        }
        if let Some(mut car) = car {
            let want = TrafficAudio {
                engine: s.engine.clone().unwrap_or_else(|| car.engine.clone()),
                speed: if parked { Some(0.0) } else { s.speed },
                load: if parked { Some(0.0) } else { s.load },
                horn: if parked { HornState::None } else { HornState::from_word(s.horn.unwrap_or(0)) },
                skidding: !parked && s.skidding.unwrap_or(false),
            };
            if *car != want {
                *car = want;
            }
        }
        if let Some(mut ped) = ped {
            let want = PedAudio {
                voice: s.voice.filter(|v| *v != 0),
                shoe_class: s.shoe_class.unwrap_or(2),
                weight: s.weight.unwrap_or(1),
                close_range: s.close_range.unwrap_or(false),
                feet_down: if parked { [false; 2] } else { s.feet.unwrap_or([false; 2]) },
                foot_materials: s.materials,
                footsteps_on: s.footsteps,
                speech_distance: None,
                speech_value: ped.speech_value,
            };
            if *ped != want {
                *ped = want;
            }
        }
        if let Some(mut npc) = npc {
            let v = if parked { Vec3::ZERO } else { velocity.unwrap_or(derived) };
            let v = match s.speed {
                Some(speed) if !parked && v.length() < 1e-4 => t.rotation * Vec3::Z * speed,
                _ => v,
            };
            let material = s.material.unwrap_or_else(|| physics.as_deref().map_or(skate_audio::player::state::NO_MATERIAL, |p| crate::game_audio::world_bridge::ground_material(p, t.translation)));
            let f = t.rotation * Vec3::Z;
            npc.state = Some(AudioState::rolling(&LiteSkater {
                position: t.translation.to_array(),
                velocity: v.to_array(),
                heading: f.x.atan2(f.z),
                wheels: s.wheels.unwrap_or([true; 4]),
                material,
                grinding: s.grinding.unwrap_or(false),
                grind_material: s.grind_material.unwrap_or(skate_audio::player::state::NO_MATERIAL),
                airborne: s.air.unwrap_or(false),
                air_time: 0.0,
                dt: dt.max(1e-4),
            }));
        }
    }
}
