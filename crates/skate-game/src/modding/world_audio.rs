//! Mod-owned world audio objects (API 2, world audio extension 1; `skate_mods::world_audio`):
//! each `sdk.world_audio.spawn` key becomes an entity with the same engine-facing components a
//! game system would add (`crate::world_audio`), so mods and engine systems are equal publishers
//! of the retail world audio. Keys belong to the calling mod; 48 objects per mod, 128 in all; an
//! object not updated for 0.5 s is parked (speed 0, feet up, horn off; ghosts never park);
//! everything is despawned when the mod is disabled, reloaded or fails, and on a runtime reset.
use super::Mods;
use crate::world_audio::*;
use bevy::prelude::*;
use serde_json::{Value, json};
use skate_mods::world_audio::{AlarmRuleOptions, AnnounceOptions, MAX_OBJECTS_PER_MOD, MAX_OBJECTS_TOTAL, ObjectKind, PARK_SECONDS, WorldAudioEventOptions, WorldAudioOptions};
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
    /// The mod that named the announcer character (`LivingWorldAudio::mod_announcer`).
    announcer_owner: Option<String>,
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
    take!(position, velocity, heading, body, engine, speed, load, horn, skidding, parked, voice, shoe_class, weight, close_range, feet, materials, footsteps, tazing, photo_flag, source, from, seconds, wheels, material, grinding, grind_material, air, loose_board,
        bank, patch, volume, falloff, extent, forward, core, preset);
}

/// An emitter's component from its description (the record's defaults: forward +X, no core,
/// volume 1, the squared curve).
fn emitter(s: &WorldAudioOptions) -> WorldEmitter {
    WorldEmitter {
        bank: s.bank.clone().unwrap_or_default(),
        patch: s.patch.unwrap_or(0),
        extent: Vec3::from_array(s.extent.unwrap_or([1.0; 3])),
        forward: Vec3::from_array(s.forward.unwrap_or([1.0, 0.0, 0.0])),
        core: s.core.unwrap_or(0.0),
        volume: s.volume.unwrap_or(1.0),
        falloff: s.falloff.unwrap_or_default().retail_type(),
    }
}

fn zone(s: &WorldAudioOptions) -> ReverbZoneVolume {
    ReverbZoneVolume {
        preset: s.preset.as_deref().and_then(|p| u64::from_str_radix(p, 16).ok()).unwrap_or(0),
        extent: Vec3::from_array(s.extent.unwrap_or([1.0; 3])),
        forward: Vec3::from_array(s.forward.unwrap_or([1.0, 0.0, 0.0])),
        core: s.core.unwrap_or(0.0),
    }
}

/// An emitter's bank must be in the audio (install or a running content overlay) and a reverb
/// zone's preset one of the install's: a zone naming no known preset would end retail's zone walk.
fn check_audio_refs(world: &World, kind: ObjectKind, opts: &WorldAudioOptions) -> Result<(), String> {
    let library = world.get_resource::<crate::game_audio::Library>();
    match kind {
        ObjectKind::Emitter => {
            if let (Some(bank), Some(l)) = (&opts.bank, library) {
                if !l.aems().banks.contains_key(bank) {
                    return Err(format!("world audio emitter: the audio has no AEMS bank {bank}"));
                }
            }
        }
        ObjectKind::ReverbZone => {
            if let (Some(p), Some(l)) = (&opts.preset, library) {
                let key = u64::from_str_radix(p, 16).unwrap_or(0);
                if !l.bus_tuning().0.contains_key(&key) {
                    return Err(format!("world audio reverb zone: the install has no reverb preset {p}"));
                }
            }
        }
        _ => {}
    }
    Ok(())
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
    if !opts.validate_for(kind) || !opts.complete_for(kind) {
        return Err("Invalid world audio options".into());
    }
    check_audio_refs(world, kind, &opts)?;
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
            _ => {}
        }
        // Doc 16 L3 / M3: a mod's car or ped takes its own MixMap instance (`game_audio::mod_world`)
        // unless it asks for retail's pools (`slots = 'shared'`). Engine systems publishing the
        // living world add no `OwnAudioInstance` and keep retail's pools.
        if matches!(kind, ObjectKind::Traffic | ObjectKind::Ped) && skate_mods::world_audio::Slots::resolve(opts.slots) == skate_mods::world_audio::Slots::Own {
            e.insert(crate::world_audio::OwnAudioInstance);
        }
        match kind {
            ObjectKind::Traffic | ObjectKind::Ped => {}
            ObjectKind::Skater => {
                e.insert(NpcSkaterAudio { voice: opts.voice.filter(|v| *v != 0), ..Default::default() });
            }
            ObjectKind::Emitter => {
                e.insert(emitter(&opts));
            }
            ObjectKind::ReverbZone => {
                e.insert(zone(&opts));
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
    let kind = obj.kind;
    drop(audio);
    check_audio_refs(world, kind, &opts)?;
    let mut audio = world.resource_mut::<ModWorldAudio>();
    let Some(obj) = audio.objects.get_mut(&(owner.to_owned(), key.to_owned())) else {
        return Err(format!("unknown world audio object {key}"));
    };
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
        ("impact", ObjectKind::Traffic) => {
            // Retail's car alarm rule decides (crate::game_audio::car_alarm): a parked car, a real contact.
            let by = opts.source.as_deref().and_then(ImpactSource::from_name).unwrap_or_default();
            world.write_message(VehicleImpact::speed(entity, by, opts.speed.ok_or("impact needs its speed")?));
        }
        ("tazer", ObjectKind::Ped) => {
            world.write_message(PedTazerEvent { ped: entity, seconds: opts.seconds });
        }
        ("body_fall", ObjectKind::Ped) => {
            let kind = opts.kind.ok_or("body_fall needs its kind")?;
            world.write_message(PedBodyFallEvent { ped: entity, kind: f32::from(kind) });
        }
        ("reaction", ObjectKind::Skater) => {
            let reaction = match opts.value.as_ref().and_then(Value::as_str) {
                Some("slam") => SkaterReaction::Slam,
                Some("slam_b") => SkaterReaction::SlamB,
                Some("trick") => SkaterReaction::Trick,
                Some("crash") => SkaterReaction::Crash,
                Some("chase") => SkaterReaction::Chase,
                other => return Err(format!("unknown reaction {other:?}")),
            };
            world.write_message(NpcSkaterReactionEvent { skater: entity, reaction, by: opts.by.unwrap_or(0) });
        }
        ("speech", ObjectKind::Ped) => {
            let value = match &opts.value {
                Some(Value::Number(n)) => n.as_i64().map(|v| SpeechValue(v as i32)),
                Some(Value::String(s)) => SpeechValue::from_name(s),
                _ => None,
            }
            .ok_or_else(|| format!("unknown speech value {:?}", opts.value))?;
            world.write_message(PedSpeechEvent { ped: entity, value, topic: None });
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

/// `sdk.world_audio.announcer`: name (or clear) the announcer character for the mods.
pub(super) fn announcer(world: &mut World, owner: &str, character: Option<u32>) -> Result<(), String> {
    if !skate_mods::world_audio::valid_announcer_character(character) {
        return Err(format!("invalid announcer character {character:?}"));
    }
    world.resource_mut::<ModWorldAudio>().announcer_owner = character.map(|_| owner.to_owned());
    if let Some(mut living) = world.get_resource_mut::<LivingWorldAudio>() {
        living.mod_announcer = character;
    }
    Ok(())
}

/// `sdk.world_audio.announce`: an announcer request (retail's `PlayAnnouncerSpeech`).
pub(super) fn announce(world: &mut World, event: &Value, options: AnnounceOptions) -> Result<(), String> {
    if !skate_mods::world_audio::valid_announcer_event(event) || !options.validate() {
        return Err(format!("invalid announcer request {event}"));
    }
    let line = match event {
        Value::Number(n) => AnnouncerLine::Id(n.as_u64().and_then(|v| u16::try_from(v).ok()).ok_or("invalid announcer event")?),
        Value::String(s) => AnnouncerLine::Name(s.clone()),
        _ => return Err("invalid announcer event".into()),
    };
    world.write_message(AnnouncerSpeechEvent { event: line, pro: options.pro, words: options.words().unwrap_or_default() });
    Ok(())
}

/// `sdk.world_audio.alarm_rule`: tune or disable retail's car alarm trigger for every car (None =
/// back to retail's numbers).
pub(super) fn alarm_rule(world: &mut World, owner: &str, options: Option<AlarmRuleOptions>) -> Result<(), String> {
    if !options.as_ref().is_none_or(AlarmRuleOptions::validate) {
        return Err("invalid alarm rule".into());
    }
    let mut rule = world.get_resource_or_insert_with(CarAlarmRule::default);
    match options {
        None => {
            rule.overrides = None;
            rule.override_owner = None;
        }
        Some(o) => {
            let mut t = rule.tuning();
            if let Some(v) = o.enabled {
                t.enabled = v;
            }
            if let Some(v) = o.min_impact {
                t.min_impact = v;
            }
            if let Some(v) = o.seconds {
                t.seconds = v;
            }
            rule.overrides = Some(t);
            rule.override_owner = Some(owner.to_owned());
        }
    }
    Ok(())
}

/// A stopped mod's alarm rule goes (`owner` None = every mod's).
fn clear_alarm_rule(world: &mut World, owner: Option<&str>) {
    let Some(mut rule) = world.get_resource_mut::<CarAlarmRule>() else { return };
    if rule.override_owner.is_some() && owner.is_none_or(|o| rule.override_owner.as_deref() == Some(o)) {
        rule.overrides = None;
        rule.override_owner = None;
    }
}

fn clear_announcer(world: &mut World, owner: Option<&str>) {
    let mut audio = world.resource_mut::<ModWorldAudio>();
    if audio.announcer_owner.is_some() && owner.is_none_or(|o| audio.announcer_owner.as_deref() == Some(o)) {
        audio.announcer_owner = None;
        if let Some(mut living) = world.get_resource_mut::<LivingWorldAudio>() {
            living.mod_announcer = None;
        }
    }
}

pub(super) fn clear_owner(world: &mut World, owner: &str) {
    clear_announcer(world, Some(owner));
    clear_alarm_rule(world, Some(owner));
    let mut audio = world.resource_mut::<ModWorldAudio>();
    let keys: Vec<Key> = audio.objects.keys().filter(|(o, _)| o == owner).cloned().collect();
    let entities: Vec<Entity> = keys.iter().filter_map(|k| audio.objects.remove(k)).map(|o| o.entity).collect();
    for e in entities {
        world.despawn(e);
    }
}

pub(super) fn clear(world: &mut World) {
    clear_announcer(world, None);
    clear_alarm_rule(world, None);
    let objects = std::mem::take(&mut world.resource_mut::<ModWorldAudio>().objects);
    for (_, o) in objects {
        world.despawn(o.entity);
    }
}

/// `snapshot.world_audio[owner]`: per key `{kind, audible, instance}`.
pub(super) fn snapshot(world: &World, owner: &str) -> Value {
    let audio = world.resource::<ModWorldAudio>();
    let stats = world.get_resource::<WorldEmitterStats>();
    let objects = world.get_resource::<WorldAudioStats>();
    let bridge = world.get_resource::<crate::game_audio::world_bridge::Bridge>();
    let mut out = serde_json::Map::new();
    for ((o, key), obj) in &audio.objects {
        if o != owner {
            continue;
        }
        let held = world.get::<WorldAudioInstance>(obj.entity);
        let mut entry = json!({
            "kind": match obj.kind { ObjectKind::Traffic => "traffic", ObjectKind::Ped => "ped", ObjectKind::Skater => "skater", ObjectKind::Emitter => "emitter", ObjectKind::ReverbZone => "reverb_zone" },
            // Emitters: playing now (holding an emitter state); reverb zones: holding the listener.
            "audible": held.is_some() || stats.is_some_and(|s| s.playing.contains(&obj.entity) || s.zones.contains(&obj.entity)),
            "instance": held.map(|h| h.instance),
            // Doc 16 L3: the instance is the object's own (a private MixMap), not one of retail's.
            "own": held.is_some_and(|h| h.own),
            // Doc 16 M3: a car's / ped's instances (`own`, the default, or `shared` = retail's
            // pools; nil for other kinds), and whether it is in reach but waits for one (every
            // instance of its pool is held by a nearer object).
            "slots": matches!(obj.kind, ObjectKind::Traffic | ObjectKind::Ped).then(|| if world.get::<OwnAudioInstance>(obj.entity).is_some() { "own" } else { "shared" }),
            "waiting": objects.is_some_and(|s| s.waiting.contains(&obj.entity) || s.own_waiting.contains(&obj.entity)),
            "parked": world.resource::<Time<Real>>().elapsed_secs_f64() - obj.updated > PARK_SECONDS && !obj.ghost && !matches!(obj.kind, ObjectKind::Emitter | ObjectKind::ReverbZone),
        });
        // Traffic: the car alarm's seconds left while it sounds (an `alarm` / `impact` event, or engine code).
        if let Some(left) = bridge.and_then(|b| b.alarm_left(obj.entity)) {
            entry["alarm"] = json!(left);
        }
        out.insert(key.clone(), entry);
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
        "speech_lines": s.speech_lines,
        "announcer": world.get_resource::<LivingWorldAudio>().and_then(LivingWorldAudio::announcer_character),
        "waiting": s.waiting.len(),
        // Doc 16 M3: the objects with their own instance (a mod's cars and peds by default).
        "own": {
            "instances": {"traffic": s.own_instances.0, "peds": s.own_instances.1},
            "published": {"traffic": s.own_vehicles, "peds": s.own_peds},
            "audible": {"traffic": s.own_traffic_held, "peds": s.own_peds_held},
            "waiting": s.own_waiting.len(),
        },
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
    mut q: Query<(&mut Transform, &mut GlobalTransform, Option<&mut TrafficAudio>, Option<&mut PedAudio>, Option<&mut NpcSkaterAudio>, Option<&mut AudioVelocity>, Option<&mut WorldEmitter>, Option<&mut ReverbZoneVolume>)>,
    mut living: ResMut<LivingWorldAudio>,
    mut commands: Commands,
    parked_now: Query<(), With<VehicleParked>>,
) {
    // The photographer's game flag: raised while any mod ped sets it (cleared with the objects).
    let photo = audio.objects.values().any(|o| o.kind == ObjectKind::Ped && o.state.photo_flag == Some(true));
    if living.mod_photo_flag != photo {
        living.mod_photo_flag = photo;
    }
    if audio.objects.is_empty() {
        return;
    }
    let now = time.elapsed_secs_f64();
    let dt = game_time.delta_secs();
    for ((owner, _), obj) in audio.objects.iter_mut() {
        if obj.ghost {
            continue;
        }
        let Ok((mut t, mut g, car, ped, npc, vel, emit, reverb)) = q.get_mut(obj.entity) else { continue };
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
        // Emitters and reverb zones are records: they never park.
        if let Some(mut e) = emit {
            let want = emitter(s);
            if *e != want {
                *e = want;
            }
        }
        if let Some(mut z) = reverb {
            let want = zone(s);
            if *z != want {
                *z = want;
            }
        }
        if car.is_some() {
            // Retail's StayingParked for the car alarm rule: the `parked` option, else the host's parking.
            let alarm_armed = s.parked.unwrap_or(parked);
            if alarm_armed != parked_now.contains(obj.entity) {
                if alarm_armed {
                    commands.entity(obj.entity).insert(VehicleParked);
                } else {
                    commands.entity(obj.entity).remove::<VehicleParked>();
                }
            }
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
                shoe_class: s.shoe_class,
                weight: s.weight.unwrap_or(1),
                close_range: s.close_range,
                feet_down: if parked { [false; 2] } else { s.feet.unwrap_or([false; 2]) },
                foot_materials: s.materials,
                footsteps_on: s.footsteps,
                speech_distance: None,
                speech_value: ped.speech_value,
                tazing: !parked && s.tazing.unwrap_or(false),
                body_fall: ped.body_fall,
            };
            if *ped != want {
                *ped = want;
            }
        }
        if let Some(mut npc) = npc {
            let voice = s.voice.filter(|v| *v != 0);
            if npc.voice != voice {
                npc.voice = voice;
            }
            let loose = s.loose_board.unwrap_or(0).min(2);
            if npc.loose_board != loose {
                npc.loose_board = loose;
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn world() -> (World, Mods) {
        let mut w = World::new();
        w.insert_resource(Time::<Real>::default());
        w.init_resource::<ModWorldAudio>();
        let dir = std::env::temp_dir().join(format!("skate-world-audio-slots-{}", std::process::id()));
        (w, Mods::new(skate_mods::Manager::new(dir.clone(), dir)))
    }

    fn spawn_with(w: &mut World, mods: &Mods, key: &str, kind: ObjectKind, options: Value) -> Entity {
        let opts: WorldAudioOptions = serde_json::from_value(options).unwrap();
        spawn(w, mods, "m", key.into(), kind, opts).unwrap();
        w.resource::<ModWorldAudio>().objects[&("m".to_owned(), key.to_owned())].entity
    }

    /// Doc 16 M3 (user decision 2026-10-04): a mod's car or ped takes its own MixMap instance by
    /// default, `slots = 'shared'` (alias `retail`) puts it in retail's pools, `slots = 'own'`
    /// stays valid; other kinds never take the marker; an engine-published car (the living
    /// world's path: the components without the marker) is untouched, so it stays in retail's
    /// pools (`world_bridge::tests::own_instance_objects_go_to_their_own_host` routes by the marker).
    #[test]
    fn mod_cars_and_peds_default_to_their_own_instance() {
        let (mut w, mods) = world();
        let own = |w: &World, e: Entity| w.get::<OwnAudioInstance>(e).is_some();
        let car = spawn_with(&mut w, &mods, "car", ObjectKind::Traffic, json!({"engine": "c04_taxi01"}));
        let ped = spawn_with(&mut w, &mods, "ped", ObjectKind::Ped, json!({"voice": 59}));
        assert!(own(&w, car) && own(&w, ped), "default: own");
        let shared_car = spawn_with(&mut w, &mods, "shared_car", ObjectKind::Traffic, json!({"slots": "shared"}));
        let shared_ped = spawn_with(&mut w, &mods, "shared_ped", ObjectKind::Ped, json!({"slots": "retail"}));
        assert!(!own(&w, shared_car) && !own(&w, shared_ped), "shared / retail: retail's pools");
        let own_car = spawn_with(&mut w, &mods, "own_car", ObjectKind::Traffic, json!({"slots": "own"}));
        assert!(own(&w, own_car), "explicit own");
        let skater = spawn_with(&mut w, &mods, "skater", ObjectKind::Skater, json!({}));
        assert!(!own(&w, skater), "NPC skaters stay in retail's Player slot");
        assert!(spawn(&mut w, &mods, "m", "bad".into(), ObjectKind::Skater, serde_json::from_value(json!({"slots": "own"})).unwrap()).is_err());
        // Respawning a key with `shared` drops the marker with the old entity.
        let again = spawn_with(&mut w, &mods, "car", ObjectKind::Traffic, json!({"slots": "shared"}));
        assert!(w.get_entity(car).is_err() && !own(&w, again));
        // The engine's path: the same components, no marker.
        let engine = w.spawn((TrafficAudio::new("c04_taxi01"), Transform::default(), GlobalTransform::default())).id();
        assert!(!own(&w, engine), "engine objects keep retail's pools");
        // Read back: `slots` per car / ped, nil for other kinds.
        let snap = snapshot(&w, "m");
        assert_eq!(snap["ped"]["slots"], "own");
        assert_eq!(snap["car"]["slots"], "shared");
        assert_eq!(snap["own_car"]["slots"], "own");
        assert!(snap["skater"]["slots"].is_null());
        assert_eq!(snap["ped"]["waiting"], false);
    }

    fn world_with_car() -> (World, Entity) {
        let mut world = World::new();
        world.init_resource::<ModWorldAudio>();
        world.init_resource::<Messages<VehicleImpact>>();
        world.init_resource::<Messages<VehicleAlarm>>();
        let car = world.spawn(TrafficAudio::new("c04_taxi01")).id();
        world.resource_mut::<ModWorldAudio>().objects.insert(
            ("m".into(), "taxi".into()),
            Object { entity: car, kind: ObjectKind::Traffic, state: WorldAudioOptions::default(), updated: 0.0, ghost: false, last: None },
        );
        (world, car)
    }

    /// `impact` becomes a `VehicleImpact` (the rule decides later); it is a traffic-only event.
    #[test]
    fn impact_events_reach_the_rule() {
        let (mut world, car) = world_with_car();
        let opts = WorldAudioEventOptions { speed: Some(3.3), source: Some("vehicle".into()), ..Default::default() };
        event(&mut world, "m", "taxi", "impact", opts).unwrap();
        let hits: Vec<VehicleImpact> = world.resource::<Messages<VehicleImpact>>().iter_current_update_messages().copied().collect();
        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].vehicle, hits[0].by, hits[0].impact.length()), (car, ImpactSource::Vehicle, 3.3));
        assert!(event(&mut world, "m", "taxi", "impact", WorldAudioEventOptions::default()).is_err(), "no speed");
        assert!(event(&mut world, "other", "taxi", "impact", WorldAudioEventOptions { speed: Some(1.0), ..Default::default() }).is_err(), "another mod's key");
    }

    /// `alarm_rule`: fields replace the numbers in effect, nil goes back to retail's, and a stopped
    /// mod's rule is cleared (another mod's stays).
    #[test]
    fn alarm_rule_is_set_merged_and_cleared_with_its_mod() {
        let (mut world, _) = world_with_car();
        alarm_rule(&mut world, "m", Some(AlarmRuleOptions { seconds: Some(4.0), ..Default::default() })).unwrap();
        alarm_rule(&mut world, "m", Some(AlarmRuleOptions { min_impact: Some(2.0), ..Default::default() })).unwrap();
        assert_eq!(world.resource::<CarAlarmRule>().tuning(), AlarmTuning { enabled: true, min_impact: 2.0, seconds: 4.0 });
        assert!(alarm_rule(&mut world, "m", Some(AlarmRuleOptions { seconds: Some(-1.0), ..Default::default() })).is_err());
        clear_owner(&mut world, "someone-else");
        assert_eq!(world.resource::<CarAlarmRule>().tuning().seconds, 4.0);
        clear_owner(&mut world, "m");
        assert_eq!(world.resource::<CarAlarmRule>().tuning(), AlarmTuning::default(), "the mod stopped: retail again");
        alarm_rule(&mut world, "m", Some(AlarmRuleOptions { enabled: Some(false), ..Default::default() })).unwrap();
        assert!(!world.resource::<CarAlarmRule>().tuning().enabled);
        alarm_rule(&mut world, "m", None).unwrap();
        assert_eq!(world.resource::<CarAlarmRule>().overrides, None);
    }
}
