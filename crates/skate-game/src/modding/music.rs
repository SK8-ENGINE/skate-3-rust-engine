//! Mod entry for the music (`sdk.music`, capability `music` = 1): the queue (a playlist in the mod's folder, the disc
//! soundtrack, stop, skip), the volume, the playing graph's variables and events, the caption style, and
//! `music_track_started` events. Everything goes through `game_audio::music::MusicRequest` (the same way in as the
//! engine's); a mod's queue, volume and caption end with the mod. Details: docs/hails-additions/11-audio.md#music-banks
use bevy::prelude::*;
use serde_json::{json, Value};

use super::Mods;
use crate::game_audio::music::{Music, MusicRequest};

pub(super) fn snapshot(world: &World) -> Value {
    let Some(m) = world.get_resource::<Music>() else {
        return json!({"on": false, "playing": null, "queued": 0, "owner": null, "volume": 1.0});
    };
    let playing = m.playing.as_ref().map(|p| json!({"title": p.title, "artist": p.artist, "custom": p.custom}));
    json!({
        "on": m.sequencer.is_some(),
        "playing": playing,
        "queued": m.sequencer.as_ref().map_or(0, |s| s.tracks.len()),
        "owner": m.owner,
        "volume": m.volume.as_ref().map_or(1.0, |v| v.1),
    })
}

/// Queued music events as `on_event` to every running mod.
pub(super) fn dispatch(world: &mut World, mods: &mut Mods) {
    let Some(mut music) = world.get_resource_mut::<Music>() else { return };
    for event in std::mem::take(&mut music.events) {
        mods.manager.dispatch("on_event", event);
    }
}

fn request(world: &mut World, r: MusicRequest) -> Result<(), String> {
    let mut music = world.get_resource_mut::<Music>().ok_or("music is not ready")?;
    music.requests.push(r);
    Ok(())
}

/// A playlist file inside the mod's folder (its banks may sit anywhere relative to it, e.g. the player's own music
/// folder: only the playlist must be in the package).
pub(super) fn playlist(world: &mut World, mods: &Mods, id: &str, path: &str, queue: bool) -> Result<(), String> {
    let package = mods.manager.packages.get(id).ok_or("missing package")?;
    let root = package.root.canonicalize().map_err(|e| e.to_string())?;
    let full = root.join(path).canonicalize().map_err(|e| format!("{path}: {e}"))?;
    if !full.starts_with(&root) {
        return Err("playlist escapes its package".into());
    }
    request(world, MusicRequest::Playlist { owner: Some(id.to_owned()), path: full, queue })
}

pub(super) fn soundtrack(world: &mut World, id: &str, track: Option<u32>) -> Result<(), String> {
    request(world, MusicRequest::Soundtrack { owner: Some(id.to_owned()), track })
}

pub(super) fn stop(world: &mut World, id: &str) -> Result<(), String> {
    request(world, MusicRequest::Stop { owner: Some(id.to_owned()) })
}

pub(super) fn skip(world: &mut World) -> Result<(), String> {
    request(world, MusicRequest::Skip)
}

pub(super) fn volume(world: &mut World, id: &str, level: Option<f32>) -> Result<(), String> {
    request(world, MusicRequest::Volume { owner: Some(id.to_owned()), level })
}

pub(super) fn var(world: &mut World, name: String, value: i32) -> Result<(), String> {
    request(world, MusicRequest::Var { name, value })
}

pub(super) fn post(world: &mut World, event: u32) -> Result<(), String> {
    request(world, MusicRequest::Post { event })
}

pub(super) fn caption(world: &mut World, id: &str, options: Option<skate_mods::MusicCaptionOptions>) -> Result<(), String> {
    let mut style = world.get_resource_mut::<crate::game_audio::music_caption::MusicCaptionStyle>().ok_or("music is not ready")?;
    style.set(id, options);
    Ok(())
}

pub(super) fn clear_owner(world: &mut World, id: &str) {
    if let Some(mut music) = world.get_resource_mut::<Music>() {
        music.requests.push(MusicRequest::ClearOwner(id.to_owned()));
    }
    if let Some(mut style) = world.get_resource_mut::<crate::game_audio::music_caption::MusicCaptionStyle>() {
        style.set(id, None);
    }
}

/// Every mod's music changes end (all mods unloaded).
pub(super) fn clear(world: &mut World) {
    let owners: Vec<String> = world.get_resource::<Music>().map_or_else(Vec::new, |m| {
        m.owner.iter().chain(m.volume.iter().map(|v| &v.0)).cloned().collect()
    });
    for id in owners {
        clear_owner(world, &id);
    }
    if let Some(mut style) = world.get_resource_mut::<crate::game_audio::music_caption::MusicCaptionStyle>() {
        style.overrides.clear();
    }
}
