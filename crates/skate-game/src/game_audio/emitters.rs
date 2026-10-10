//! Retail world sound emitters: the map's `.ems` records (positions, shapes) with
//! their sound attributes from the skatercollections database, exported by
//! setup into the audio manifest (tools/asset_pipeline/audio_export.py).
//!
//! The game side follows TU3 (audio-specs/ems-emitters-re.md; doc 11):
//! - a record is a sphere when its three extents are equal, otherwise an
//!   ellipsoid whose semi-axes are the extents along forward = scalars[1..4],
//!   up and side; the listener's normalised distance `d` must be below 1;
//! - scalars[0] is an inner core: inside it the level is full, outside `d` is
//!   rescaled over the rest;
//! - level = attribute volume x falloff curve ((1-d)^2, 1-d or flat);
//! - at most `MAX_ACTIVE` play, in the order they were reached; a record the
//!   listener leaves stops at once (retail releases it without a fade).
//!
//! What each bank then plays (relay of short pieces or one loop, and its slow
//! level/pitch movement) is retail's patch program for the bank:
//! - with the native AEMS runtime running (`native.rs`), every record whose bank is in
//!   the install takes one of the 5 emitter states (= MixMap Emitter instances), posts `c_emitter`
//!   and the bank's own program plays it. The payload comes from the MixMap (`Native::
//!   emitter_payload`: w1 dry = out4 × level, w2 send = out8 × level, w3 pan = out0, w4 pitch =
//!   out5, w5 low-pass = out6, w8 = the attribute patch = selector); the state's 3-D input gets the
//!   listener's distance and azimuth each frame. Redelivered every frame, released (state freed)
//!   when the listener leaves.
//!
//! Without the native runtime (an install without the AEMS data: `native.rs` logs the error) the
//! emitters are silent; there is no measured fallback (2026-10-03: the `PROFILES` table is gone).
use super::{Library, native::Native};
use bevy::prelude::*;
use skate_audio::eval::NodeId;

/// CSTATEMGR_Emitter's pool size.
const MAX_ACTIVE: usize = 5;

// The `.ems` files a map loads come from its database entry (`F4917ACACAFAF913` field
// `65FA976EF23A314E`, in that order; the districts list five, the parks one), read at run time
// with the map's own definition and mods on top (`map_audio::MapAudio`). The emitter system
// (`sub_824A24F8`) loads them all and dispatches every record by its attribute's eVolumeType
// (sound emitters 1, reverb zones 5, music zones 4); `music_` holds music zones, `speakers_` /
// `crowds_` types 6 / 7.

/// The reverb-zone emitters (`eVolumeType` 5) the listener is inside this frame, in the order
/// they were reached (retail's active node list, which `sub_82488278` walks for `SFXObj_Reverb`).
#[derive(Resource, Default)]
pub(super) struct ReverbZones {
    pub zones: Vec<skate_audio::bus::zones::Zone>,
}

struct ZoneRecord {
    shape: Shape,
    id: u64,
    attribute: u64,
    /// The attribute's reverb preset key (`99FD793BC30CF0FA`, collection key; 0 when it has none).
    preset: u64,
    /// The emitter manager's vfunc92 (`sub_824A2438`) on the attribute: its preset key is one of
    /// the 24 reverb presets (the image table `0x8302E298` = the exported `aud_reverb` keys).
    enabled: bool,
}

#[derive(Default)]
pub(super) struct ZoneState {
    /// Map name, map generation, audio content generation.
    map: Option<(String, u64, u64)>,
    records: Vec<ZoneRecord>,
    /// Reached records in discovery order.
    active: Vec<usize>,
    /// The map's records (the first `map_len`); published `ReverbZoneVolume` zones after them.
    map_len: usize,
}

/// A published reverb zone's id bit (map records are `file << 32 | index`).
const PUBLISHED_ZONE: u64 = 1 << 63;

/// Per frame, before `native::reverb_frame`: which reverb zones hold the listener (the camera, as
/// the emitter query's `0x820CFDD4`), with their normalised distance after the inner core
/// (`sub_828EA918`'s sphere / ellipsoid test, as for the sound emitters) and their attribute's
/// preset (`99FD793BC30CF0FA`). Native runtime only.
pub(super) fn reverb_zones(
    mut state: Local<ZoneState>,
    map: Res<crate::map_transition::CurrentMap>,
    library: Option<Res<Library>>,
    native: Option<Res<Native>>,
    listener: Query<&GlobalTransform, With<super::GameAudioListener>>,
    mut out: ResMut<ReverbZones>,
    content: Res<super::AudioContent>,
    audio: Res<super::map_audio::MapAudio>,
    published: Query<(Entity, &GlobalTransform, &crate::world_audio::ReverbZoneVolume)>,
    mut stats: ResMut<crate::world_audio::WorldEmitterStats>,
) {
    let (Some(library), Some(_)) = (library, native) else { return };
    let state = &mut *state;
    let identity = (map.name.clone(), map.generation, content.world_generation);
    if state.map.as_ref() != Some(&identity) {
        state.records = zone_records(&library, &audio);
        state.active.clear();
        if !state.records.is_empty() {
            info!("Reverb zones: {} records on {}", state.records.len(), audio.stem);
        }
        state.map_len = state.records.len();
        state.map = Some(identity);
    }
    // Published zones (mods, engine systems): after the map's records, rebuilt every frame while
    // any exists; the reached ones keep their place in the node list.
    let any = !published.is_empty() || state.records.len() > state.map_len;
    if any {
        let held: Vec<u64> = state.active.iter().map(|&i| state.records[i].id).collect();
        state.records.truncate(state.map_len);
        let (presets, _) = library.bus_tuning();
        let mut list: Vec<ZoneRecord> = published
            .iter()
            .map(|(e, t, z)| {
                let (_, rotation, position) = t.to_scale_rotation_translation();
                let id = PUBLISHED_ZONE | e.to_bits();
                ZoneRecord {
                    shape: Shape { position, extent: z.extent, forward: (rotation * z.forward).normalize_or(Vec3::X), core: z.core },
                    id,
                    attribute: id,
                    preset: z.preset,
                    enabled: zone_enabled(&presets, z.preset),
                }
            })
            .collect();
        list.sort_by_key(|z| z.id);
        state.records.extend(list);
        let records = &state.records;
        state.active = held.iter().filter_map(|id| records.iter().position(|r| r.id == *id)).collect();
    }
    out.zones.clear();
    let Ok(listener) = listener.single() else { return };
    zones_at(&state.records, &mut state.active, listener.translation(), &mut out.zones);
    if any || !stats.zones.is_empty() {
        let inside: Vec<Entity> = state.active.iter().filter(|&&i| i >= state.map_len).map(|&i| Entity::from_bits(state.records[i].id & !PUBLISHED_ZONE)).collect();
        if stats.zones != inside {
            stats.zones = inside;
        }
    }
}

/// The map's reverb-zone records (eVolumeType 5, flags 0) from its `.ems` files. A record whose
/// attribute names no known reverb preset stays in the list, disabled: retail's zone query
/// (`sub_82488278`) stops at the first zone node whose vfunc92 check fails instead of skipping it.
/// On the disc every zone attribute names one of the 24 presets, so all are enabled.
fn zone_records(library: &Library, audio: &super::map_audio::MapAudio) -> Vec<ZoneRecord> {
    let (presets, _) = library.bus_tuning();
    let mut records = Vec::new();
    {
        for (f, r) in audio.records(library) {
            if r.kind != 5 || r.flags != 0 {
                continue;
            }
            let preset = r.reverb.as_deref().and_then(|k| u64::from_str_radix(k, 16).ok()).unwrap_or(0);
            let s = r.scalars;
            records.push(ZoneRecord {
                shape: Shape { position: Vec3::from(r.position), extent: Vec3::from(r.extent), forward: Vec3::new(s[1], s[2], s[3]), core: s[0] },
                id: ((f as u64) << 32) | u64::from(r.index),
                attribute: u64::from_str_radix(&r.sound_id, 16).unwrap_or(0),
                preset,
                enabled: zone_enabled(&presets, preset),
            });
        }
    }
    records
}

/// `sub_824A2438` (the emitter manager's vfunc92): the attribute's reverb RefSpec key is in the
/// image's table of the 24 reverb preset keys (`0x8302E298`; the same 24 keys as the exported
/// `aud_reverb` presets). A missing field reads the default RefSpec (key 0): not in the table.
fn zone_enabled<V>(presets: &std::collections::HashMap<u64, V>, preset: u64) -> bool {
    preset != 0 && presets.contains_key(&preset)
}

/// One frame of the zone node list: drop the records the listener left, append new hits in
/// record order, and list the active ones (in node order) with their distance.
fn zones_at(records: &[ZoneRecord], active: &mut Vec<usize>, ear: Vec3, out: &mut Vec<skate_audio::bus::zones::Zone>) {
    let reached: Vec<Option<f32>> = records.iter().map(|z| reach(&z.shape, ear)).collect();
    active.retain(|&i| reached[i].is_some());
    for (i, hit) in reached.iter().enumerate() {
        if hit.is_some() && !active.contains(&i) {
            active.push(i);
        }
    }
    out.clear();
    for &i in active.iter() {
        let (z, d) = (&records[i], reached[i].unwrap_or(1.0));
        out.push(skate_audio::bus::zones::Zone {
            id: z.id,
            attribute: z.attribute,
            preset: z.preset,
            d,
            position: [z.shape.position.x, z.shape.position.z],
            enabled: z.enabled,
        });
    }
}

/// A record's shape, in world space.
#[derive(Clone, Copy, Debug)]
struct Shape {
    position: Vec3,
    extent: Vec3,
    forward: Vec3,
    core: f32,
}

/// Normalised distance of `listener` in the shape (0 at the core, 1 at the
/// edge), or None outside.
fn reach(shape: &Shape, listener: Vec3) -> Option<f32> {
    let delta = listener - shape.position;
    let e = shape.extent;
    let d = if e.x == e.y && e.y == e.z {
        delta.length() / e.x
    } else {
        let side = Vec3::Y.cross(shape.forward).normalize_or_zero();
        let up = shape.forward.cross(side).normalize_or_zero();
        let (f, u, s) = (delta.dot(shape.forward) / e.x, delta.dot(up) / e.y, delta.dot(side) / e.z);
        (f * f + u * u + s * s).sqrt()
    };
    if !d.is_finite() || d >= 1.0 {
        return None;
    }
    let core = shape.core;
    Some(if core > 0.0 && d < core { 0.0 } else if core >= 1.0 { 0.0 } else { (d - core) / (1.0 - core) })
}

/// The record test for a shape (a sphere when the three extents are equal, else an ellipsoid with
/// semi-axes `extent` along `forward`, up and side; inner `core`) and the falloff curve: the level
/// factor at `ear`, None outside (the native mod voices and rule sounds, `mod_voices::Reach`; the
/// same functions as the records').
pub(super) fn shape_level(position: Vec3, extent: Vec3, forward: Vec3, core: f32, curve: i32, ear: Vec3) -> Option<f32> {
    let d = reach(&Shape { position, extent, forward, core }, ear)?;
    Some(falloff(curve, d))
}

/// Retail falloff curve by `eVolumeFalloffType`.
fn falloff(kind: i32, d: f32) -> f32 {
    match kind {
        0 => (1.0 - d) * (1.0 - d),
        1 => 1.0 - d,
        _ => 1.0,
    }
}

struct Emitter {
    shape: Shape,
    volume: f32,
    falloff: i32,
    bank: String,
    patch: i32,
}

/// A reached emitter: its native post and emitter state once started.
struct Node {
    record: usize,
    started: bool,
    post: Option<NodeId>,
    state: Option<usize>,
    /// A published emitter's entity (`WorldEmitter`; None for the map's records).
    entity: Option<Entity>,
    /// A published emitter's private-MixMap instance and its build (the "extra" setting, the default).
    extra: Option<(usize, u64)>,
}

impl Node {
    /// The event rows' owner: the record index, or a published emitter's entity.
    fn owner(&self) -> u64 {
        self.entity.map_or(self.record as u64, Entity::to_bits)
    }
}

#[derive(Default)]
pub(super) struct State {
    /// Map name, map generation, audio content generation.
    map: Option<(String, u64, u64)>,
    emitters: Vec<Emitter>,
    /// Reached records in discovery order (retail's node list).
    nodes: Vec<Node>,
    /// The map's native emitter banks (prefetch, `native::prefetch`), each emitter's index into
    /// them and each bank's prefetch status.
    banks: Vec<String>,
    emitter_bank: Vec<usize>,
    bank_status: Vec<BankStatus>,
    /// Per bank: the listener's distance to its nearest emitter's bounding sphere (scratch).
    near: Vec<f32>,
    /// Banks to request this frame, nearest first (scratch).
    order: Vec<(f32, usize)>,
    /// The map's records (the first `map_len` emitters); the published `WorldEmitter` entities
    /// after them, in `dynamic` order.
    map_len: usize,
    dynamic: Vec<Entity>,
    /// `AudioContent::runtime_generation` the nodes belong to.
    runtime: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BankStatus {
    Idle,
    Requested,
    Loaded,
}

/// Ask the prefetch worker for the native banks of the emitters the listener is getting close to
/// (nearest first) and drop the unused ones it moved away from (`native::prefetch`). Touches no
/// runtime, random or node state: what plays and when stays as without it.
fn prefetch_near(state: &mut State, native: &mut Native, library: &Library, ear: Vec3) {
    use super::native::prefetch::{AHEAD, EVICT};
    state.near.clear();
    state.near.resize(state.banks.len(), f32::INFINITY);
    for (e, &b) in state.emitters.iter().zip(&state.emitter_bank) {
        if let Some(near) = state.near.get_mut(b) {
            *near = near.min(e.shape.position.distance(ear) - e.shape.extent.max_element());
        }
    }
    state.order.clear();
    for (b, &d) in state.near.iter().enumerate() {
        match state.bank_status[b] {
            BankStatus::Idle if d <= AHEAD => state.order.push((d, b)),
            BankStatus::Requested if d > EVICT => {
                native.prefetch.drop_bank(&state.banks[b]);
                state.bank_status[b] = BankStatus::Idle;
            }
            _ => {}
        }
    }
    state.order.sort_by(|a, b| a.0.total_cmp(&b.0));
    for &(_, b) in &state.order {
        let stem = &state.banks[b];
        if native.bank_loaded(stem) {
            state.bank_status[b] = BankStatus::Loaded;
            continue;
        }
        if let Ok(source) = library.bank_source(stem) {
            native.prefetch.request(source);
            state.bank_status[b] = BankStatus::Requested;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn update(
    mut state: Local<State>,
    map: Res<crate::map_transition::CurrentMap>,
    library: Option<Res<Library>>,
    listener: Query<&GlobalTransform, With<super::GameAudioListener>>,
    menu: Option<Res<crate::graphics_menu::Menu>>,
    replay: Res<crate::replay::Replay>,
    native: Option<ResMut<Native>>,
    cues: Res<super::skate_events::Cues>,
    content: Res<super::AudioContent>,
    audio: Res<super::map_audio::MapAudio>,
    mut api: ResMut<super::mod_audio::AudioApi>,
    dynamic: Query<(Entity, &GlobalTransform, &crate::world_audio::WorldEmitter)>,
    mut mix: ResMut<super::mod_voices::ModMix>,
    settings: Option<Res<super::AudioSettings>>,
    mut stats: ResMut<crate::world_audio::WorldEmitterStats>,
    rules: Res<super::mod_rules::AudioRules>,
) {
    let _timing = super::timing::scope(&super::timing::EMITTERS);
    // No native runtime: the emitters are silent (its start logged why).
    let (Some(library), Some(mut native)) = (library, native) else { return };
    let native = &mut *native;
    let state = &mut *state;
    let identity = (map.name.clone(), map.generation, content.world_generation);
    let runtime_changed = state.runtime != content.runtime_generation;
    if runtime_changed {
        // The runtime restarted (`content::restart`): its posts and emitter states are the old
        // runtime's. Forget them; never release old ids into the new runtime.
        state.nodes.clear();
        state.runtime = content.runtime_generation;
    }
    if state.map.as_ref() != Some(&identity) {
        // A new map or a restart unloads the map's banks (the map-change path); a hot swap that
        // changed the world layer (`content::swap_or_restart`) only rebuilds the records: the
        // runtime and its banks stay (doc 16 L1).
        let map_changed = runtime_changed || state.map.as_ref().is_none_or(|m| m.0 != map.name || m.1 != map.generation);
        for node in state.nodes.drain(..) {
            if let Some(post) = node.post {
                native.release(post);
            }
            if let Some(g) = node.state {
                native.release_emitter_state(g);
            }
            if let Some((g, build)) = node.extra {
                if build == mix.build {
                    mix.release(g);
                }
            }
        }
        if map_changed {
            native.unload_map_banks();
        }
        let stem = audio.stem.as_str();
        // Retail's emitter system loads every file of the map's database entry and dispatches by
        // the attribute's eVolumeType: 1 = looping emitter (here), 5 = reverb zone
        // (`reverb_zones`), 4 = the single-winner music zone (`sub_828EB410`: a playlist of the
        // music system, not ported). 6 / 7 (speakers / crowds) are not dispatched by the
        // emitter system at all. On the disc only the `sfx_` / `skateschool` files hold type 1.
        // The map's own definition and mods add records after the files' (`map_audio`).
        let records: Vec<&super::library::EmitterRecord> = audio.records(&library).map(|(_, r)| r).collect();
        state.emitters = records.iter().filter(|r| r.kind == 1 && r.flags == 0).filter_map(|r| {
            let bank = r.bank.clone().filter(|b| native.has_bank(&library, b))?;
            let s = r.scalars;
            Some(Emitter {
                shape: Shape { position: Vec3::from(r.position), extent: Vec3::from(r.extent), forward: Vec3::new(s[1], s[2], s[3]), core: s[0] },
                volume: r.volume, falloff: r.falloff, bank, patch: r.patch,
            })
        }).collect();
        info!("World emitters: {} of {} records on {stem} have a played sound", state.emitters.len(), records.len());
        state.banks.clear();
        state.emitter_bank.clear();
        for e in &state.emitters {
            let b = if let Some(b) = state.banks.iter().position(|s| *s == e.bank) {
                b
            } else {
                state.banks.push(e.bank.clone());
                state.banks.len() - 1
            };
            state.emitter_bank.push(b);
        }
        state.bank_status = vec![BankStatus::Idle; state.banks.len()];
        state.map_len = state.emitters.len();
        state.dynamic.clear();
        state.map = Some(identity);
    }
    // Emitters published as `WorldEmitter` components (mods, engine systems): a tail after the
    // map's records, rebuilt every frame while any exists (nothing runs without them).
    let extra = settings.as_deref().is_some_and(super::AudioSettings::extra_mod_emitter_slots);
    if !dynamic.is_empty() || !state.dynamic.is_empty() {
        sync_dynamic(state, native, &library, &mut mix, &dynamic, &mut api);
        if extra {
            mix.ensure(&library, native, content.runtime_generation);
        }
    }
    let Ok(listener) = listener.single() else { return };
    let ear = listener.translation();
    let skater = cues.riding.board;
    prefetch_near(state, native, &library, ear);
    let silent = super::silenced(menu.as_deref(), &replay);

    // Release nodes the listener left (or everything while silenced).
    let reached: Vec<Option<f32>> = state.emitters.iter().map(|e| if silent { None } else { reach(&e.shape, ear) }).collect();
    state.nodes.retain(|node| {
        let keep = reached[node.record].is_some();
        if !keep {
            if node.started {
                info!("AUDIO_EMITTER stop {} #{}", state.emitters[node.record].bank, node.record);
                if api.events.on() {
                    let e = &state.emitters[node.record];
                    api.events.push(super::mod_audio::EventRow { kind: super::mod_audio::EventKind::EmitterStop, source: super::mod_audio::Source::Emitter, class: super::mod_audio::intern(&e.bank), slot: "", id: e.patch, owner: node.owner(), hit: None });
                }
            }
            if let Some(post) = node.post {
                native.release(post);
            }
            if let Some(g) = node.state {
                native.release_emitter_state(g);
            }
            if let Some((g, build)) = node.extra {
                if build == mix.build {
                    mix.release(g);
                }
            }
        }
        keep
    });
    // New hits join the node list in discovery order.
    for (index, hit) in reached.iter().enumerate() {
        if hit.is_some() && !state.nodes.iter().any(|n| n.record == index) {
            let entity = index.checked_sub(state.map_len).and_then(|i| state.dynamic.get(i).copied());
            state.nodes.push(Node { record: index, started: false, post: None, state: None, entity, extra: None });
        }
    }
    // Waiting nodes take free states in list order: retail's 5 (the map's emitters, and published
    // ones with the "shared" setting), or, with the "extra" setting (the default), a published
    // emitter takes an instance of the private MixMap instead.
    let mut active = state.nodes.iter().filter(|n| n.started && n.extra.is_none()).count();
    for node in state.nodes.iter_mut().filter(|n| !n.started) {
        let own = extra && node.entity.is_some();
        if own {
            let Some(g) = mix.claim() else { continue };
            node.extra = Some((g, mix.build));
        } else if active >= MAX_ACTIVE {
            continue;
        } else {
            active += 1;
        }
        node.started = true;
        let e = &state.emitters[node.record];
        info!("AUDIO_EMITTER start {} #{} at {:.1?} volume {:.2} (native{})", e.bank, node.record, e.shape.position.to_array(), e.volume, if own { ", own instance" } else { "" });
        match native.ensure_bank(&library, &e.bank) {
            Ok(_) => {
                if let Some(status) = state.emitter_bank.get(node.record).and_then(|&b| state.bank_status.get_mut(b)) {
                    *status = BankStatus::Loaded;
                }
                let level = e.volume * falloff(e.falloff, reached[node.record].unwrap_or(1.0));
                let payload = match node.extra {
                    Some((g, _)) => {
                        mix.set_position(g, listener, skater, e.shape.position);
                        mix.words(g, level, e.patch).unwrap_or_else(|| native.emitter_payload(None, level, super::native::azimuth(listener, e.shape.position), e.patch))
                    }
                    None => {
                        node.state = native.claim_emitter_state();
                        if let Some(g) = node.state {
                            native.set_emitter_position(g, listener, skater, e.shape.position);
                        }
                        native.emitter_payload(node.state, level, super::native::azimuth(listener, e.shape.position), e.patch)
                    }
                };
                // A mod rule may mute the start: the node keeps its state and posts nothing. A rule
                // sound placed at the owner plays at the record, with the record's reach.
                let muted = rules.set.as_deref().is_some_and(|r| {
                    let site = (e.shape.position, super::mod_voices::Reach { extent: e.shape.extent, forward: e.shape.forward, core: e.shape.core, curve: e.falloff });
                    r.mutes_at_published(&super::mod_audio::EventRow { kind: super::mod_audio::EventKind::EmitterStart, source: super::mod_audio::Source::Emitter, class: super::mod_audio::intern(&e.bank), slot: "", id: e.patch, owner: node.owner(), hit: None }, Some(site), node.entity.map(Entity::to_bits))
                });
                node.post = if muted { None } else { native.post_emitter(&payload) };
                if api.events.on() && (node.post.is_some() || muted) {
                    api.events.push(super::mod_audio::EventRow { kind: super::mod_audio::EventKind::EmitterStart, source: super::mod_audio::Source::Emitter, class: super::mod_audio::intern(&e.bank), slot: "", id: e.patch, owner: node.owner(), hit: None });
                }
            }
            Err(error) => warn!("AUDIO_EMITTER {}: {error}", e.bank),
        }
    }

    // The bank's program does the rest; only the game-side words change.
    for node in state.nodes.iter_mut().filter(|n| n.started) {
        let emitter = &state.emitters[node.record];
        let (Some(d), Some(post)) = (reached[node.record], node.post) else { continue };
        let level = emitter.volume * falloff(emitter.falloff, d);
        let payload = match node.extra {
            Some((g, _)) => {
                mix.set_position(g, listener, skater, emitter.shape.position);
                mix.words(g, level, emitter.patch).unwrap_or_else(|| native.emitter_payload(None, level, super::native::azimuth(listener, emitter.shape.position), emitter.patch))
            }
            None => {
                if let Some(g) = node.state {
                    native.set_emitter_position(g, listener, skater, emitter.shape.position);
                }
                native.emitter_payload(node.state, level, super::native::azimuth(listener, emitter.shape.position), emitter.patch)
            }
        };
        native.redeliver(post, &payload);
    }
    // Read back which published emitters play (only while any exists).
    if !state.dynamic.is_empty() || !stats.playing.is_empty() {
        let playing: Vec<Entity> = state.nodes.iter().filter(|n| n.started && n.post.is_some()).filter_map(|n| n.entity).collect();
        if stats.playing != playing {
            stats.playing = playing;
        }
    }
}

/// Rebuild the published emitters' tail of the list (sorted by entity, so the order is stable)
/// and follow the reached nodes to their new indices; a node whose entity is gone (or whose bank
/// the install lacks now) is released as when the listener leaves.
fn sync_dynamic(
    state: &mut State,
    native: &mut Native,
    library: &Library,
    mix: &mut super::mod_voices::ModMix,
    q: &Query<(Entity, &GlobalTransform, &crate::world_audio::WorldEmitter)>,
    api: &mut super::mod_audio::AudioApi,
) {
    let mut list: Vec<(Entity, Emitter)> = q
        .iter()
        .filter(|(_, _, e)| native.has_bank(library, &e.bank))
        .map(|(entity, t, e)| {
            let (_, rotation, position) = t.to_scale_rotation_translation();
            let forward = (rotation * e.forward).normalize_or(Vec3::X);
            (entity, Emitter { shape: Shape { position, extent: e.extent, forward, core: e.core }, volume: e.volume, falloff: e.falloff, bank: e.bank.clone(), patch: e.patch })
        })
        .collect();
    list.sort_by_key(|(e, _)| e.to_bits());
    state.emitters.truncate(state.map_len);
    state.dynamic.clear();
    for (entity, e) in list {
        state.dynamic.push(entity);
        state.emitters.push(e);
    }
    let (map_len, dynamic) = (state.map_len, &state.dynamic);
    state.nodes.retain_mut(|node| {
        let Some(entity) = node.entity else { return true };
        if let Some(i) = dynamic.iter().position(|d| *d == entity) {
            node.record = map_len + i;
            return true;
        }
        if node.started && api.events.on() {
            api.events.push(super::mod_audio::EventRow { kind: super::mod_audio::EventKind::EmitterStop, source: super::mod_audio::Source::Emitter, class: "", slot: "", id: 0, owner: entity.to_bits(), hit: None });
        }
        if let Some(post) = node.post {
            native.release(post);
        }
        if let Some(g) = node.state {
            native.release_emitter_state(g);
        }
        if let Some((g, build)) = node.extra {
            if build == mix.build {
                mix.release(g);
            }
        }
        false
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The distance prefetch over DownTown's emitters (data-gated): the listener visits every
    /// emitter in turn. Each frame's `prefetch_near` only queues / drops decodes: the runtime's
    /// banks, evaluator random state and blocks stay untouched; requested = the banks within
    /// `AHEAD` m that are not loaded; far away everything is dropped. Prints its cost per call.
    #[test]
    #[ignore = "needs the private install data"]
    fn the_prefetch_follows_the_listener_and_touches_no_runtime_state() {
        use super::super::native::prefetch::{AHEAD, EVICT};
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
        let Ok(library) = Library::load(root) else { panic!("missing private data: no audio install") };
        let Ok(mut native) = Native::start(&library) else { panic!("missing private data: no AEMS install") };
        let mut state = State::default();
        for r in super::super::map_audio::tests::old_ems_files("DownTown").iter().flat_map(|f| library.emitters(f)) {
            let Some(bank) = r.bank.clone().filter(|b| r.kind == 1 && r.flags == 0 && native.has_bank(&library, b)) else { continue };
            let s = r.scalars;
            state.emitters.push(Emitter {
                shape: Shape { position: Vec3::from(r.position), extent: Vec3::from(r.extent), forward: Vec3::new(s[1], s[2], s[3]), core: s[0] },
                volume: r.volume, falloff: r.falloff, bank: bank.clone(), patch: r.patch,
            });
            let b = state.banks.iter().position(|x| *x == bank).unwrap_or_else(|| {
                state.banks.push(bank);
                state.banks.len() - 1
            });
            state.emitter_bank.push(b);
        }
        if state.emitters.is_empty() {
            panic!("missing private data: no DownTown emitters");
        }
        state.bank_status = vec![BankStatus::Idle; state.banks.len()];
        let snapshot = |n: &Native| {
            let rt = n.shared.lock().unwrap();
            (rt.eval.rng, rt.blocks)
        };
        let before = snapshot(&native);
        let mut worst = std::time::Duration::ZERO;
        let mut total = std::time::Duration::ZERO;
        let mut calls = 0u32;
        let mut first = std::time::Duration::ZERO;
        let positions: Vec<Vec3> = state.emitters.iter().map(|e| e.shape.position).collect();
        for (i, &at) in positions.iter().enumerate() {
            for step in 0..20 {
                let ear = at + Vec3::new(step as f32 * 5.0, 0.0, 0.0);
                let t = std::time::Instant::now();
                prefetch_near(&mut state, &mut native, &library, ear);
                let dt = t.elapsed();
                if calls == 0 {
                    first = dt;
                } else {
                    worst = worst.max(dt);
                }
                (total, calls) = (total + dt, calls + 1);
                for (b, stem) in state.banks.iter().enumerate() {
                    let near = state.emitters.iter().zip(&state.emitter_bank).filter(|(_, x)| **x == b)
                        .map(|(e, _)| e.shape.position.distance(ear) - e.shape.extent.max_element()).fold(f32::INFINITY, f32::min);
                    let requested = native.prefetch.contains(stem);
                    assert_eq!(requested, state.bank_status[b] == BankStatus::Requested, "{stem}");
                    if near <= AHEAD {
                        assert!(requested || native.bank_loaded(stem), "{stem} within {near} m");
                    }
                    if near > EVICT {
                        assert!(!requested, "{stem} kept at {near} m");
                    }
                }
            }
            // Every tenth emitter starts: its bank comes from the prefetch, nothing decodes here.
            // (Waiting for the worker first: in play the listener spends seconds inside AHEAD.)
            let stem = state.emitters[i].bank.clone();
            if i % 10 == 0 && !native.bank_loaded(&stem) {
                assert!(native.prefetch.contains(&stem), "{stem}: the start's bank is prefetched");
                native.prefetch.wait(&stem);
                let decodes = super::super::library::WAV_DECODES.with(|n| n.get());
                native.ensure_bank(&library, &stem).unwrap();
                if let Some(s) = state.bank_status.get_mut(state.emitter_bank[i]) {
                    *s = BankStatus::Loaded;
                }
                assert_eq!(super::super::library::WAV_DECODES.with(|n| n.get()), decodes, "{stem}: decoded on the game thread");
            }
        }
        prefetch_near(&mut state, &mut native, &library, Vec3::splat(1.0e5));
        assert_eq!(native.prefetch.stems().count(), 0, "far away nothing is held");
        assert_eq!(snapshot(&native), before, "the prefetch never touches the runtime");
        println!("prefetch_near over {} emitters / {} banks: {} calls, mean {:?}, first {:?} (starts the worker), max of the rest {:?}", state.emitters.len(), state.banks.len(), calls, total / calls, first, worst);
    }

    fn shape(extent: Vec3, forward: Vec3, core: f32) -> Shape {
        Shape { position: Vec3::ZERO, extent, forward, core }
    }

    #[test]
    fn sphere_uses_the_first_extent_as_radius() {
        let s = shape(Vec3::splat(10.0), Vec3::X, 0.0);
        assert_eq!(reach(&s, Vec3::new(5.0, 0.0, 0.0)), Some(0.5));
        assert_eq!(reach(&s, Vec3::new(0.0, 0.0, 10.0)), None);
    }

    #[test]
    fn ellipsoid_axes_follow_forward_up_and_side() {
        // University fountain: 13 along forward (x), 7 up, 61 along the side (z).
        let s = shape(Vec3::new(13.0, 7.0, 61.0), Vec3::X, 0.0);
        assert!((reach(&s, Vec3::new(0.0, 0.0, 30.5)).unwrap() - 0.5).abs() < 1e-5);
        assert!((reach(&s, Vec3::new(6.5, 0.0, 0.0)).unwrap() - 0.5).abs() < 1e-5);
        assert!((reach(&s, Vec3::new(0.0, 3.5, 0.0)).unwrap() - 0.5).abs() < 1e-5);
        assert_eq!(reach(&s, Vec3::new(14.0, 0.0, 0.0)), None);
        // Rotated a quarter turn, the long axis lies along x instead.
        let turned = shape(Vec3::new(13.0, 7.0, 61.0), Vec3::Z, 0.0);
        assert!(reach(&turned, Vec3::new(30.0, 0.0, 0.0)).is_some());
    }

    #[test]
    fn inner_core_is_full_level_and_rescales_the_rest() {
        let s = shape(Vec3::splat(10.0), Vec3::X, 0.2);
        assert_eq!(reach(&s, Vec3::new(1.0, 0.0, 0.0)), Some(0.0));
        assert!((reach(&s, Vec3::new(6.0, 0.0, 0.0)).unwrap() - 0.5).abs() < 1e-5);
    }

    #[test]
    fn falloff_curves() {
        assert_eq!(falloff(0, 0.5), 0.25);
        assert_eq!(falloff(1, 0.5), 0.5);
        assert_eq!(falloff(7, 0.5), 1.0);
    }

    /// A DownTown reverb zone (attribute `1F94F2F815C00368` → reverb11) through the retail selector:
    /// standing in its core the zone's preset fades in and commits, Reverb.in5 rises, and the
    /// MixMap's Reverb out4 (the global env scale) drops by F213's −400 mB.
    #[test]
    #[ignore = "needs the private install data"]
    fn a_downtown_reverb_zone_selects_its_preset_and_raises_reverb_in5() {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
        let Ok(library) = Library::load(root) else { panic!("missing private data: no audio install") };
        let audio = super::super::map_audio::build("DownTown", None, &library, Some(&retail_downtown()));
        let records = zone_records(&library, &audio);
        if records.is_empty() {
            panic!("missing private data: the install has no reverb-zone presets (stage_reverb_zones.py)");
        }
        let (presets, _) = library.bus_tuning();
        let Some(zone) = records.iter().find(|z| z.attribute == 0x1F94_F2F8_15C0_0368) else { panic!("no 1F94F2F815C00368 zone") };
        assert_eq!(zone.preset, 0xBEEF_C8E3_DE04_FBAE);
        let (mut active, mut zones) = (Vec::new(), Vec::new());
        zones_at(&records, &mut active, zone.shape.position, &mut zones);
        let first = zones.iter().find(|z| z.id == zone.id).expect("the listener at its centre is inside");
        assert_eq!(first.d, 0.0, "inside the inner core");
        zones_at(&records, &mut active, Vec3::new(1.0e5, 0.0, 1.0e5), &mut zones);
        assert!(zones.is_empty() && active.is_empty(), "far away: no zone");
        let mut env = skate_audio::bus::env::EnvNetwork::default();
        env.presets = presets;
        let at = zone.shape.position + Vec3::new(0.0, 0.0, 0.0);
        let camera = skate_audio::bus::zones::Camera { position: at.to_array(), forward: [0.0, 0.0, -1.0] };
        let mut zones = Vec::new();
        // Only this zone, so the test does not depend on overlapping records.
        let only = [ZoneRecord { shape: zone.shape, id: zone.id, attribute: zone.attribute, preset: zone.preset, enabled: zone.enabled }];
        for _ in 0..90 {
            zones_at(&only, &mut active, at, &mut zones);
            env.update(1.0 / 60.0, 0, &zones, Some(&camera));
        }
        assert_eq!(env.target_key(), Some(0xBEEF_C8E3_DE04_FBAE));
        assert_eq!(env.reverb_inputs(), [0, 0, 0, 0, 0, 32767, 0], "reverb11 → Reverb.in5");
        let Some(mxb) = library.aems().mixmap.clone() else { panic!("missing private data: no MixMap") };
        let mut m = skate_audio::mixmap::MixMap::from_bytes(&library.read(&mxb).unwrap()).unwrap();
        let out4 = |m: &mut skate_audio::mixmap::MixMap, inputs: [i32; 7]| {
            for id in 1..=4 {
                m.set_input(skate_audio::mixmap::keys::MASTER, id, 32767);
            }
            for (id, x) in inputs.into_iter().enumerate() {
                m.set_input(skate_audio::mixmap::keys::REVERB, id, x);
            }
            for _ in 0..120 {
                m.tick(1.0 / 60.0);
            }
            m.level(skate_audio::mixmap::keys::REVERB, 4)
        };
        let outside = out4(&mut m, [0, 0, 0, 0, 32767, 0, 0]);
        let inside = out4(&mut m, env.reverb_inputs());
        println!("Reverb out4: reverb01 {outside}, reverb11 zone {inside}");
        assert_eq!(outside, 32730, "0 mB with in4 (reverb01)");
        assert!((20000..21200).contains(&inside), "−400 mB with in5: {inside}");
    }

    /// DownTown's retail entry as the old table listed it (the lookup itself is proven by
    /// `map_audio::tests::retail_maps_reproduce_the_old_tables`).
    fn retail_downtown() -> std::collections::HashMap<String, super::super::map_audio::RetailMap> {
        let ems = super::super::map_audio::tests::old_ems_files("DownTown").iter().map(|s| s.to_string()).collect();
        std::collections::HashMap::from([("downtown".to_owned(), super::super::map_audio::RetailMap { ems, crossfade_bank: None })])
    }

    /// The zone records keep retail's ids (`file << 32 | index`) and order: the map's audio lists
    /// the same files as the old table for every retail map (data-gated).
    #[test]
    #[ignore = "needs the private install data"]
    fn zone_records_through_map_audio_match_the_old_table() {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
        let Ok(library) = Library::load(root) else { panic!("missing private data: no audio install") };
        let Ok(c) = skate_data::collections::Collections::load(root) else { panic!("missing private data: no stock collections") };
        let table = super::super::map_audio::retail_table(&c);
        let mut total = 0;
        for stem in super::super::map_audio::tests::MAPS {
            let audio = super::super::map_audio::build(stem, None, &library, Some(&table));
            let new: Vec<(u64, u64)> = zone_records(&library, &audio).iter().map(|z| (z.id, z.attribute)).collect();
            let mut old = Vec::new();
            for (f, file) in super::super::map_audio::tests::old_ems_files(stem).iter().enumerate() {
                for r in library.emitters(file).iter().filter(|r| r.kind == 5 && r.flags == 0) {
                    old.push((((f as u64) << 32) | u64::from(r.index), u64::from_str_radix(&r.sound_id, 16).unwrap_or(0)));
                }
            }
            assert_eq!(new, old, "{stem}");
            total += new.len();
        }
        assert!(total > 0, "some reverb zones");
    }


    /// A world for the emitter / zone systems with the install's runtime, no map records (the
    /// default map audio) and the listener at the origin.
    fn published_world(extra: bool) -> (World, Entity) {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
        let Ok(library) = Library::load(root) else { panic!("missing private data: no audio install") };
        let Ok(native) = Native::start(&library) else { panic!("missing private data: no AEMS install") };
        let mut world = World::new();
        world.insert_resource(native);
        world.insert_resource(library);
        world.insert_resource(crate::map_transition::CurrentMap { path: None, name: "Test world".into(), spawn: [0.0; 3], heading: 0.0, generation: 0, audio_tag: None });
        world.init_resource::<super::super::map_audio::MapAudio>();
        world.init_resource::<super::super::AudioContent>();
        world.init_resource::<super::super::mod_audio::AudioApi>();
        world.init_resource::<super::super::mod_voices::ModMix>();
        world.init_resource::<crate::world_audio::WorldEmitterStats>();
        world.init_resource::<super::super::skate_events::Cues>();
        world.init_resource::<crate::replay::Replay>();
        world.init_resource::<ReverbZones>();
        world.init_resource::<super::super::mod_rules::AudioRules>();
        // `extra` = the default settings (own instances since 2026-10-04); else the "shared" option.
        let saved = if extra { super::super::SavedSettings::default() } else { super::super::SavedSettings { mod_emitter_slots: super::super::ModEmitterSlots::Shared, ..Default::default() } };
        world.insert_resource(super::super::AudioSettings { saved, path: std::env::temp_dir().join("skate-emitter-test-audio.json"), muted: true, volume_overrides: Default::default() });
        let listener = world.spawn((super::super::GameAudioListener, Transform::default(), GlobalTransform::default())).id();
        (world, listener)
    }

    fn downtown_emitter(world: &World) -> (String, i32) {
        let library = world.resource::<Library>();
        let r = library.emitters("sfx_downtown").iter().find(|r| r.kind == 1 && r.bank.as_ref().is_some_and(|b| library.aems().banks.contains_key(b))).unwrap_or_else(|| panic!("missing private data: no DownTown emitter"));
        (r.bank.clone().unwrap(), r.patch)
    }

    fn publish(world: &mut World, bank: &str, patch: i32, at: Vec3) -> Entity {
        let t = Transform::from_translation(at);
        world.spawn((t, GlobalTransform::from(t), crate::world_audio::WorldEmitter { bank: bank.into(), patch, extent: Vec3::splat(20.0), forward: Vec3::X, core: 0.0, volume: 1.0, falloff: 0 })).id()
    }

    /// Published emitters (data-gated), the "shared" setting: they join the live list after the map's
    /// records and share retail's 5 emitter states (7 reached at once: the first 5 in list order
    /// play, the others wait), post `c_emitter` so the bank's program plays, take a freed state
    /// when one leaves, stop when the listener leaves their reach, and a despawned one is
    /// released; with none left the list is the map's again and nothing is held.
    #[test]
    #[ignore = "needs the private install data"]
    fn published_emitters_share_retail_slots_play_and_release() {
        let (mut world, listener) = published_world(false);
        // One system instance throughout (its `Local` state is the live node list).
        let update = world.register_system(update);
        let (bank, patch) = downtown_emitter(&world);
        let mut entities: Vec<Entity> = (0..7).map(|i| publish(&mut world, &bank, patch, Vec3::new(2.0 + i as f32, 0.0, -3.0))).collect();
        entities.sort_by_key(|e| e.to_bits());
        world.run_system(update).unwrap();
        let playing = world.resource::<crate::world_audio::WorldEmitterStats>().playing.clone();
        assert_eq!(playing, entities[..5], "retail's 5 states, first in list order");
        assert!(world.resource_mut::<Native>().claim_emitter_state().is_none(), "all 5 states taken");
        // The program plays: voices of the bank after some blocks.
        let id = world.resource::<Native>().bank_id(&bank).expect("loaded at the start");
        let mut out = vec![0.0f32; 2 * skate_audio::BLOCK];
        for _ in 0..60 {
            world.resource::<Native>().shared.lock().unwrap().fill_stereo(&mut out);
        }
        assert!(world.resource::<Native>().shared.lock().unwrap().mixer.snapshot().iter().any(|v| v.bank == id), "the bank's program opened voices");
        // One leaves (despawned): the 6th takes its state.
        world.despawn(entities[0]);
        world.run_system(update).unwrap();
        assert_eq!(world.resource::<crate::world_audio::WorldEmitterStats>().playing, entities[1..6]);
        // The listener walks away: everything stops, the states are free.
        world.entity_mut(listener).insert(GlobalTransform::from(Transform::from_xyz(500.0, 0.0, 0.0)));
        world.run_system(update).unwrap();
        assert!(world.resource::<crate::world_audio::WorldEmitterStats>().playing.is_empty());
        for e in &entities[1..] {
            world.despawn(*e);
        }
        world.run_system(update).unwrap();
        let mut native = world.resource_mut::<Native>();
        let free: Vec<_> = (0..5).filter_map(|_| native.claim_emitter_state()).collect();
        assert_eq!(free, [0, 1, 2, 3, 4], "nothing held");
    }

    /// The default settings ("extra", user decision 2026-10-04; data-gated): published emitters
    /// take private-MixMap instances, so all 7 play and retail's 5 states stay free for the map's
    /// emitters; their words come from the private MixMap's Emitter instances; despawning frees
    /// the instances.
    #[test]
    #[ignore = "needs the private install data"]
    fn the_default_gives_published_emitters_their_own_instances() {
        let (mut world, _) = published_world(true);
        let update = world.register_system(update);
        let (bank, patch) = downtown_emitter(&world);
        let entities: Vec<Entity> = (0..7).map(|i| publish(&mut world, &bank, patch, Vec3::new(2.0 + i as f32, 0.0, -3.0))).collect();
        world.run_system(update).unwrap();
        assert_eq!(world.resource::<crate::world_audio::WorldEmitterStats>().playing.len(), 7);
        assert!(world.resource::<super::super::mod_voices::ModMix>().in_use());
        let mut native = world.resource_mut::<Native>();
        let free: Vec<_> = (0..5).filter_map(|_| native.claim_emitter_state()).collect();
        assert_eq!(free, [0, 1, 2, 3, 4], "retail's states untouched");
        for g in free {
            native.release_emitter_state(g);
        }
        for e in entities {
            world.despawn(e);
        }
        world.run_system(update).unwrap();
        assert!(!world.resource::<super::super::mod_voices::ModMix>().in_use(), "instances freed");
        assert!(world.resource::<crate::world_audio::WorldEmitterStats>().playing.is_empty());
    }

    /// A published emitter that moves after its sound started (data-gated, doc 16 "moving
    /// emitters"): the same post keeps playing and its instance's 3-D input follows the entity
    /// (the camera distance it writes), and leaving the reach as it moves away releases it.
    #[test]
    #[ignore = "needs the private install data"]
    fn a_moving_published_emitter_is_followed() {
        let (mut world, _) = published_world(true);
        let update = world.register_system(update);
        let (bank, patch) = downtown_emitter(&world);
        let e = publish(&mut world, &bank, patch, Vec3::new(3.0, 0.0, 0.0));
        world.run_system(update).unwrap();
        assert_eq!(world.resource::<crate::world_audio::WorldEmitterStats>().playing, [e]);
        let dist = |w: &mut World| {
            let mut mix = w.resource_mut::<super::super::mod_voices::ModMix>();
            let m = mix.mixmap().unwrap();
            (0..super::super::mod_voices::MIX_INSTANCES as u32).map(|g| f32::from_bits(m.input(skate_audio::mixmap::keys::emitter_pos(g), skate_audio::mixmap::keys::pos::DIST_CAMERA) as u32)).find(|d| *d > 0.0)
        };
        let near = dist(&mut world).expect("an instance with a position");
        let posts = world.resource::<Native>().next_node();
        let t = Transform::from_xyz(8.0, 0.0, 0.0);
        world.entity_mut(e).insert((t, GlobalTransform::from(t)));
        world.run_system(update).unwrap();
        let far = dist(&mut world).unwrap();
        assert!((near - 3.0).abs() < 1e-3 && (far - 8.0).abs() < 1e-3, "followed: {near} → {far}");
        assert_eq!(world.resource::<Native>().next_node(), posts, "the same post, not a new one");
        let t = Transform::from_xyz(500.0, 0.0, 0.0);
        world.entity_mut(e).insert((t, GlobalTransform::from(t)));
        world.run_system(update).unwrap();
        assert!(world.resource::<crate::world_audio::WorldEmitterStats>().playing.is_empty(), "out of reach");
    }

    /// A published reverb zone (data-gated): the listener inside it lists the zone (after the
    /// map's, with its preset and a published id), the retail selector fades to its preset and
    /// raises its Reverb input; outside or despawned it is gone.
    #[test]
    #[ignore = "needs the private install data"]
    fn a_published_reverb_zone_selects_its_preset() {
        let (mut world, listener) = published_world(false);
        let reverb_zones = world.register_system(reverb_zones);
        let (presets, _) = world.resource::<Library>().bus_tuning();
        let preset = presets.keys().copied().find(|k| *k != skate_audio::bus::env::DEFAULT_PRESET).unwrap_or_else(|| panic!("missing private data: no reverb presets"));
        let t = Transform::from_xyz(0.0, 0.0, 0.0);
        let zone = world.spawn((t, GlobalTransform::from(t), crate::world_audio::ReverbZoneVolume { preset, extent: Vec3::new(30.0, 10.0, 15.0), forward: Vec3::X, core: 0.5 })).id();
        world.run_system(reverb_zones).unwrap();
        let zones = world.resource::<ReverbZones>().zones.clone();
        assert_eq!(zones.len(), 1);
        assert!(zones[0].id & PUBLISHED_ZONE != 0 && zones[0].preset == preset && zones[0].enabled && zones[0].d == 0.0);
        assert_eq!(world.resource::<crate::world_audio::WorldEmitterStats>().zones, [zone]);
        let mut env = skate_audio::bus::env::EnvNetwork::default();
        env.presets = presets;
        let camera = skate_audio::bus::zones::Camera { position: [0.0; 3], forward: [0.0, 0.0, -1.0] };
        for _ in 0..90 {
            env.update(1.0 / 60.0, 0, &zones, Some(&camera));
        }
        assert_eq!(env.target_key(), Some(preset), "the selector fades to the zone's preset");
        world.entity_mut(listener).insert(GlobalTransform::from(Transform::from_xyz(0.0, 0.0, 40.0)));
        world.run_system(reverb_zones).unwrap();
        assert!(world.resource::<ReverbZones>().zones.is_empty(), "outside its 15 m side axis");
        world.entity_mut(listener).insert(GlobalTransform::default());
        world.despawn(zone);
        world.run_system(reverb_zones).unwrap();
        assert!(world.resource::<ReverbZones>().zones.is_empty() && world.resource::<crate::world_audio::WorldEmitterStats>().zones.is_empty());
    }

    /// A rule muting an emitter's start (data-gated): the published emitter takes its state (the
    /// slot use is retail's) but posts nothing; without the rule it plays.
    #[test]
    #[ignore = "needs the private install data"]
    fn a_rule_mutes_an_emitter_start() {
        let (mut world, _) = published_world(false);
        let update = world.register_system(update);
        let (bank, patch) = downtown_emitter(&world);
        let rule: skate_mods::audio_rules::Rule = serde_json::from_value(serde_json::json!({"match": {"kind": "emitter_start", "class": bank}, "action": "mute"})).unwrap();
        world.resource_mut::<super::super::mod_rules::AudioRules>().set = Some(super::super::mod_rules::RuleSet::for_test(&[("dev.a", "quiet", rule)], Default::default()));
        let e = publish(&mut world, &bank, patch, Vec3::new(2.0, 0.0, -3.0));
        world.run_system(update).unwrap();
        assert!(world.resource::<crate::world_audio::WorldEmitterStats>().playing.is_empty(), "muted: no post");
        let mut native = world.resource_mut::<Native>();
        let free: Vec<_> = (0..5).filter_map(|_| native.claim_emitter_state()).collect();
        assert_eq!(free.len(), 4, "its state is taken");
        for g in free {
            native.release_emitter_state(g);
        }
        // Without the rule a fresh start plays.
        world.resource_mut::<super::super::mod_rules::AudioRules>().set = None;
        world.despawn(e);
        world.run_system(update).unwrap();
        let e2 = publish(&mut world, &bank, patch, Vec3::new(2.0, 0.0, -3.0));
        world.run_system(update).unwrap();
        assert_eq!(world.resource::<crate::world_audio::WorldEmitterStats>().playing, [e2]);
    }
}
