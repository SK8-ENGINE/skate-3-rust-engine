//! Main-thread, mod-owned Bevy audio adapter (API 2, audio extension 1).
//! Uses the existing DefaultPlugins AudioPlugin; Cargo enables bevy_audio + wav.
//! Playback speed changes pitch AND duration. This is not a time-stretch engine.
use super::{Mods, resolve_body};
use bevy::{audio::{AudioSinkPlayback, PlaybackMode, SpatialScale, Volume}, prelude::*};
use skate_mods::audio::{AudioPlayOptions, AudioUpdateOptions, MAX_WAV_BYTES, canonical_pcm_wav};
use std::{collections::BTreeMap, path::Path};

type Key = (String, String);
const MAX_VOICES_PER_MOD: usize = 32;
const MAX_VOICES_TOTAL: usize = 128;
const MAX_CLIPS_PER_MOD: usize = 32;
const MAX_CLIPS_TOTAL: usize = 128;
const MAX_BYTES_PER_MOD: usize = 32 * 1024 * 1024;
const MAX_BYTES_TOTAL: usize = 128 * 1024 * 1024;

struct Clip { handle: Handle<AudioSource>, bytes: usize }
struct Voice {
    entity: Entity,
    body: Option<String>,
    position: Vec3,
    offset: Vec3,
    looping: bool,
    paused: bool,
    volume: f32,
    gain: f32,
    pitch: f32,
    speed: f32,
    attack: f32,
    attack_elapsed: f32,
    stopping: Option<(f32, f32)>, // remaining seconds, total seconds
    pending: f32,
    warned: bool,
}
#[derive(Resource, Default)]
struct ModAudio {
    clips: BTreeMap<Key, Clip>,
    voices: BTreeMap<Key, Voice>,
}
#[derive(Component)]
struct ModAudioListener;

pub(super) fn install(app: &mut App) {
    app.init_resource::<ModAudio>();
    app.add_systems(Update, sync.after(super::update).after(super::present_camera));
}

fn ensure_clip(
    world: &mut World, audio: &mut ModAudio, owner: &str, root: &Path, path: &str,
) -> Result<Handle<AudioSource>, String> {
    let key = (owner.to_owned(), path.to_owned());
    if let Some(clip) = audio.clips.get(&key) { return Ok(clip.handle.clone()); }
    if !skate_mods::audio::valid_audio_path(path) { return Err("Invalid audio path".into()); }
    if audio.clips.len() >= MAX_CLIPS_TOTAL
        || audio.clips.keys().filter(|(o, _)| o == owner).count() >= MAX_CLIPS_PER_MOD {
        return Err("Audio cache limit reached (32 clips/mod, 128 total)".into());
    }
    // Existing sandbox reader canonicalizes paths and rejects symlink escapes.
    // This is done ONCE per clip, at preload/play, never in the update loop.
    let input = skate_mods::read_bounded(root, path, MAX_WAV_BYTES)?;
    let (bytes, _info) = canonical_pcm_wav(&input).map_err(|e| format!("{path}: {e}"))?;
    let total: usize = audio.clips.values().map(|c| c.bytes).sum();
    let owned: usize = audio.clips.iter().filter(|((o, _), _)| o == owner).map(|(_, c)| c.bytes).sum();
    if total + bytes.len() > MAX_BYTES_TOTAL || owned + bytes.len() > MAX_BYTES_PER_MOD {
        return Err("Audio cache byte limit reached (32 MiB/mod, 128 MiB total)".into());
    }
    let size = bytes.len();
    let Some(mut assets) = world.get_resource_mut::<Assets<AudioSource>>() else {
        return Err("AudioPlugin/PCM WAV support unavailable; rebuild with bevy_audio and wav features".into());
    };
    let handle = assets.add(AudioSource { bytes: bytes.into() });
    audio.clips.insert(key, Clip { handle: handle.clone(), bytes: size });
    Ok(handle)
}

pub(super) fn preload(world: &mut World, mods: &Mods, owner: &str, path: &str) -> Result<(), String> {
    let root = &mods.manager.packages.get(owner).ok_or("Missing audio owner")?.root;
    world.resource_scope(|world, mut audio: Mut<ModAudio>| {
        ensure_clip(world, &mut audio, owner, root, path).map(|_| ())
    })
}
fn emitter_position(mods: &Mods, owner: &str, body: Option<&str>, position: Vec3, offset: Vec3) -> Option<Vec3> {
    if let Some(body) = body {
        let snapshot = mods.world.read(resolve_body(mods, owner, body).ok()?)?;
        let q = snapshot.rotation;
        Some(Vec3::from_array(snapshot.position) + Quat::from_xyzw(q[0], q[1], q[2], q[3]) * offset)
    } else { Some(position + offset) }
}
fn remove_voice(world: &mut World, audio: &mut ModAudio, key: &Key) {
    if let Some(voice) = audio.voices.remove(key) {
        // Explicit stop also covers backend semantics where dropping a sink detaches it.
        if let Some(sink) = world.get::<AudioSink>(voice.entity) { sink.stop(); }
        if let Some(sink) = world.get::<SpatialAudioSink>(voice.entity) { sink.stop(); }
        world.despawn(voice.entity);
    }
}

pub(super) fn play(world: &mut World, mods: &Mods, owner: &str, key: String, opts: AudioPlayOptions) -> Result<(), String> {
    if !opts.validate() { return Err("Invalid audio play options".into()); }
    let root = &mods.manager.packages.get(owner).ok_or("Missing audio owner")?.root;
    let origin = Vec3::from_array(opts.position.unwrap_or([0.0; 3]));
    let offset = Vec3::from_array(opts.offset);
    let position = emitter_position(mods, owner, opts.body.as_deref(), origin, offset)
        .ok_or("Audio emitter body does not exist")?;
    world.resource_scope(|world, mut audio: Mut<ModAudio>| {
        let slot = (owner.to_owned(), key);
        if !audio.voices.contains_key(&slot)
            && (audio.voices.len() >= MAX_VOICES_TOTAL
                || audio.voices.keys().filter(|(o, _)| o == owner).count() >= MAX_VOICES_PER_MOD) {
            return Err("Audio voice limit reached (32 voices/mod, 128 total)".into());
        }
        let handle = ensure_clip(world, &mut audio, owner, root, &opts.path)?;
        remove_voice(world, &mut audio, &slot);
        let paused = opts.paused || mods.manager.snapshot["paused"].as_bool().unwrap_or(true)
            || mods.manager.snapshot["replay"].as_bool().unwrap_or(false);
        let initial_gain = if opts.fade_in > 0.0 { 0.0 } else { opts.volume };
        let settings = PlaybackSettings {
            mode: if opts.looping { PlaybackMode::Loop } else { PlaybackMode::Once },
            volume: Volume::Linear(initial_gain), speed: opts.pitch, paused,
            spatial: opts.spatial, spatial_scale: Some(SpatialScale::new(opts.spatial_scale)),
            ..Default::default()
        };
        let entity = world.spawn((AudioPlayer::new(handle), settings, Transform::from_translation(position))).id();
        audio.voices.insert(slot, Voice {
            entity, body: opts.body, position: origin, offset, looping: opts.looping,
            paused: opts.paused, volume: opts.volume, gain: initial_gain,
            pitch: opts.pitch, speed: opts.pitch, attack: opts.fade_in,
            attack_elapsed: 0.0, stopping: None, pending: 0.0, warned: false,
        });
        Ok(())
    })
}

pub(super) fn update_voice(world: &mut World, owner: &str, key: &str, opts: AudioUpdateOptions) {
    let mut audio = world.resource_mut::<ModAudio>();
    let Some(v) = audio.voices.get_mut(&(owner.to_owned(), key.to_owned())) else { return; };
    if v.stopping.is_some() { return; }
    // Commands already passed typed validation. Finished/missing keys are harmless.
    if let Some(x) = opts.volume { v.volume = x; }
    if let Some(x) = opts.pitch { v.pitch = x; }
    if let Some(x) = opts.paused { v.paused = x; }
    // World position is only meaningful for an unattached emitter.
    if v.body.is_none() { if let Some(x) = opts.position { v.position = Vec3::from_array(x); } }
    if let Some(x) = opts.offset { v.offset = Vec3::from_array(x); }
}

pub(super) fn stop(world: &mut World, owner: &str, key: &str, fade: f32) {
    world.resource_scope(|world, mut audio: Mut<ModAudio>| {
        let key = (owner.to_owned(), key.to_owned());
        if fade <= 0.0 { remove_voice(world, &mut audio, &key); }
        else if let Some(v) = audio.voices.get_mut(&key) {
            // Repeated stop calls must not extend the sound's lifetime.
            if v.stopping.is_none() { v.stopping = Some((fade, fade)); }
        }
    });
}

pub(super) fn stop_body(world: &mut World, owner: &str, body: &str) {
    world.resource_scope(|world, mut audio: Mut<ModAudio>| {
        let keys: Vec<_> = audio.voices.iter()
            .filter(|((o, _), v)| o == owner && v.body.as_deref() == Some(body))
            .map(|(k, _)| k.clone()).collect();
        for key in keys { remove_voice(world, &mut audio, &key); }
    });
}

pub(super) fn stop_owner(world: &mut World, owner: &str, release_clips: bool) {
    world.resource_scope(|world, mut audio: Mut<ModAudio>| {
        let keys: Vec<_> = audio.voices.keys().filter(|(o, _)| o == owner).cloned().collect();
        for key in keys { remove_voice(world, &mut audio, &key); }
        if release_clips {
            let keys: Vec<_> = audio.clips.keys().filter(|(o, _)| o == owner).cloned().collect();
            for key in keys {
                if let Some(clip) = audio.clips.remove(&key) {
                    world.resource_mut::<Assets<AudioSource>>().remove(clip.handle.id());
                }
            }
        }
    });
}

pub(super) fn clear(world: &mut World) {
    world.resource_scope(|world, mut audio: Mut<ModAudio>| {
        let keys: Vec<_> = audio.voices.keys().cloned().collect();
        for key in keys { remove_voice(world, &mut audio, &key); }
        for (_, clip) in std::mem::take(&mut audio.clips) {
            world.resource_mut::<Assets<AudioSource>>().remove(clip.handle.id());
        }
    });
}

fn set_sink(sink: &mut impl AudioSinkPlayback, volume: f32, pitch: f32, paused: bool) -> bool {
    sink.set_volume(Volume::Linear(volume));
    sink.set_speed(pitch);
    if paused { if !sink.is_paused() { sink.pause(); } }
    else if sink.is_paused() { sink.play(); }
    sink.empty()
}

fn sync(world: &mut World) {
    let dt = world.resource::<Time<Real>>().delta_secs().clamp(0.0, 0.25);
    let global_gain = world.get_resource::<GlobalVolume>()
        .map_or(1.0, |volume| volume.volume.to_linear());
    let global_gain = if global_gain.is_finite() { global_gain.clamp(0.0, 1.0) } else { 0.0 };
    // Use this frame's camera Transform. Bevy propagates it before audio PostUpdate.
    let camera = world.query_filtered::<&Transform, With<crate::camera::GameplayCamera>>()
        .iter(world).next().copied();
    if let Some(camera) = camera {
        let listener = world.query_filtered::<Entity, With<ModAudioListener>>().iter(world).next();
        if let Some(entity) = listener { world.entity_mut(entity).insert(camera); }
        else {
            // The supplied project has no other audio listener. Do not create two
            // if a later engine version already supplies one.
            let external = world.query_filtered::<Entity, With<SpatialListener>>().iter(world).next();
            if external.is_none() {
                world.spawn((ModAudioListener, SpatialListener::new(0.2), camera));
            }
        }
    }
    let paused = {
        let mods = world.resource::<Mods>();
        camera.is_none() || mods.manager.snapshot["paused"].as_bool().unwrap_or(true)
            || mods.manager.snapshot["replay"].as_bool().unwrap_or(false)
    };
    world.resource_scope(|world, mut audio: Mut<ModAudio>| {
        let mut remove = Vec::new();
        for (key, v) in &mut audio.voices {
            if world.get_entity(v.entity).is_err() { remove.push(key.clone()); continue; }
            let position = emitter_position(world.resource::<Mods>(), &key.0, v.body.as_deref(), v.position, v.offset);
            let Some(position) = position else { remove.push(key.clone()); continue; };
            if let Some(mut transform) = world.get_mut::<Transform>(v.entity) { transform.translation = position; }
            let active = !(paused || v.paused);
            if active { v.attack_elapsed += dt; }
            let attack = if v.attack <= 0.0 { 1.0 } else { (v.attack_elapsed / v.attack).min(1.0) };
            let mut release = 1.0;
            if let Some((remaining, total)) = &mut v.stopping {
                *remaining -= dt; release = (*remaining / *total).max(0.0);
                if *remaining <= 0.0 { remove.push(key.clone()); continue; }
            }
            // This smooths audible parameter steps only. It never touches dynamics.
            v.gain += (v.volume - v.gain) * (1.0 - (-dt / 0.012).exp());
            v.speed += (v.pitch - v.speed) * (1.0 - (-dt / 0.025).exp());
            let gain = (v.gain * attack * release).clamp(0.0, 1.0);
            let speed = v.speed.clamp(0.25, 4.0);
            // Keep initial settings current while Bevy has not yet created a sink.
            // Once started, only the sink calls below change actual playback.
            if let Some(mut settings) = world.get_mut::<PlaybackSettings>(v.entity) {
                settings.volume = Volume::Linear(gain); settings.speed = speed; settings.paused = !active;
            }
            let finished = if let Some(mut sink) = world.get_mut::<AudioSink>(v.entity) {
                Some(set_sink(&mut *sink, gain * global_gain, speed, !active))
            } else if let Some(mut sink) = world.get_mut::<SpatialAudioSink>(v.entity) {
                Some(set_sink(&mut *sink, gain * global_gain, speed, !active))
            } else { None };
            match finished {
                Some(true) => remove.push(key.clone()),
                Some(false) => { v.pending = 0.0; },
                None => {
                    if active { v.pending += dt; }
                    if v.pending > 1.0 && !v.looping { remove.push(key.clone()); }
                    if v.pending > 2.0 && !v.warned {
                        warn!("Lua audio {}:{} has no playback sink; check the output device and AudioPlugin", key.0, key.1);
                        v.warned = true;
                    }
                }
            }
        }
        for key in remove { remove_voice(world, &mut audio, &key); }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn world() -> World {
        let mut w=World::new();
        w.insert_resource(ModAudio::default());
        w.insert_resource(Assets::<AudioSource>::default());
        w
    }
    fn voice(w: &mut World, owner: &str, key: &str, body: Option<&str>) -> Entity {
        let entity=w.spawn_empty().id();
        w.resource_mut::<ModAudio>().voices.insert((owner.into(),key.into()), Voice {
            entity,body:body.map(str::to_owned),position:Vec3::ZERO,offset:Vec3::ZERO,
            looping:true,paused:false,volume:1.0,gain:1.0,pitch:1.0,speed:1.0,
            attack:0.0,attack_elapsed:0.0,stopping:None,pending:0.0,warned:false,
        }); entity
    }
    #[test] fn owners_and_bodies_are_isolated() {
        let mut w=world(); let a=voice(&mut w,"a","engine",Some("car"));
        let b=voice(&mut w,"b","engine",Some("car"));
        stop_body(&mut w,"a","car");
        assert!(w.get_entity(a).is_err()); assert!(w.get_entity(b).is_ok());
        stop_owner(&mut w,"b",true); assert!(w.get_entity(b).is_err());
    }
    #[test] fn update_preserves_entity_and_stop_is_idempotent() {
        let mut w=world(); let entity=voice(&mut w,"a","engine",None);
        update_voice(&mut w,"a","engine",AudioUpdateOptions{volume:Some(0.0),pitch:Some(2.0),..Default::default()});
        let v=&w.resource::<ModAudio>().voices[&("a".into(),"engine".into())];
        assert_eq!(v.entity,entity); assert_eq!(v.volume,0.0); assert_eq!(v.pitch,2.0);
        stop(&mut w,"a","engine",0.1); stop(&mut w,"a","engine",1.0);
        assert_eq!(w.resource::<ModAudio>().voices[&("a".into(),"engine".into())].stopping,Some((0.1,0.1)));
        stop(&mut w,"a","engine",0.0); stop(&mut w,"a","engine",0.0);
        assert!(w.resource::<ModAudio>().voices.is_empty());
    }
    #[test] fn unloading_releases_cached_assets() {
        let mut w=world(); let h=w.resource_mut::<Assets<AudioSource>>().add(AudioSource{bytes:Vec::<u8>::new().into()});
        w.resource_mut::<ModAudio>().clips.insert(("a".into(),"x.wav".into()),Clip{handle:h.clone(),bytes:0});
        stop_owner(&mut w,"a",false); assert!(w.resource::<Assets<AudioSource>>().get(h.id()).is_some());
        stop_owner(&mut w,"a",true); assert!(w.resource::<Assets<AudioSource>>().get(h.id()).is_none());
    }
}
