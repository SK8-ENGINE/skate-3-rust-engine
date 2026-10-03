//! Retail world sound emitters: the map's `.ems` records (positions, shapes) with
//! their sound attributes from the skatercollections database, exported by
//! setup into the audio manifest (tools/asset_pipeline/audio_export.py).
//!
//! The game side follows TU3 (.claude/notes/ems-emitters-re.md; docs 11):
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

/// The `.ems` file of a map (by its `.skate` file stem).
fn ems_file(map_stem: &str) -> Option<&'static str> {
    Some(match map_stem {
        "University" => "sfx_university",
        "DownTown" => "sfx_downtown",
        "Industrial" => "sfx_industrial",
        "DownTownSkatePark" => "sfx_dt_skatepark",
        "IndustrialSkatePark" => "sfx_ind_skatepark",
        "MegaPark" => "sfx_mega_skatepark",
        "MaloofMoneyCup" => "sfx_maloof_money_cup",
        "StartPark" => "sfx_startpark",
        "BlackBoxPark" => "sfx_blackbox_park",
        "SkateSchool" => "skateschool",
        _ => return None,
    })
}

/// The `.ems` files a map's database entry lists (`F4917ACACAFAF913` field `65FA976EF23A314E`, in
/// that order): the districts load five, the parks one. The emitter system (`sub_824A24F8`) loads
/// them all and dispatches every record by its attribute's eVolumeType (sound emitters 1, reverb
/// zones 5, music zones 4); `music_` holds music zones, `speakers_` / `crowds_` types 6 / 7.
fn ems_files(map_stem: &str) -> &'static [&'static str] {
    match map_stem {
        "University" => &["music_university", "sfx_university", "reverb_university", "speakers_university", "crowds_university"],
        "DownTown" => &["music_downtown", "sfx_downtown", "reverb_downtown", "speakers_downtown", "crowds_downtown"],
        "Industrial" => &["music_industrial", "sfx_industrial", "reverb_industrial", "speakers_industrial", "crowds_industrial"],
        _ => match ems_file(map_stem) {
            Some("sfx_dt_skatepark") => &["sfx_dt_skatepark"],
            Some("sfx_ind_skatepark") => &["sfx_ind_skatepark"],
            Some("sfx_mega_skatepark") => &["sfx_mega_skatepark"],
            Some("sfx_maloof_money_cup") => &["sfx_maloof_money_cup"],
            Some("sfx_startpark") => &["sfx_startpark"],
            Some("sfx_blackbox_park") => &["sfx_blackbox_park"],
            Some("skateschool") => &["skateschool"],
            _ => &[],
        },
    }
}

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
    map: Option<(String, u64)>,
    records: Vec<ZoneRecord>,
    /// Reached records in discovery order.
    active: Vec<usize>,
}

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
) {
    let (Some(library), Some(_)) = (library, native) else { return };
    let state = &mut *state;
    let identity = (map.name.clone(), map.generation);
    if state.map.as_ref() != Some(&identity) {
        let stem = map.path.as_deref().and_then(|p| p.file_stem()).and_then(|s| s.to_str()).unwrap_or("");
        state.records = zone_records(&library, stem);
        state.active.clear();
        if !state.records.is_empty() {
            info!("Reverb zones: {} records on {stem}", state.records.len());
        }
        state.map = Some(identity);
    }
    out.zones.clear();
    let Ok(listener) = listener.single() else { return };
    zones_at(&state.records, &mut state.active, listener.translation(), &mut out.zones);
}

/// The map's reverb-zone records (eVolumeType 5, flags 0) from its `.ems` files. A record whose
/// attribute names no known reverb preset stays in the list, disabled: retail's zone query
/// (`sub_82488278`) stops at the first zone node whose vfunc92 check fails instead of skipping it.
/// On the disc every zone attribute names one of the 24 presets, so all are enabled.
fn zone_records(library: &Library, map_stem: &str) -> Vec<ZoneRecord> {
    let (presets, _) = library.bus_tuning();
    let mut records = Vec::new();
    for (f, file) in ems_files(map_stem).iter().enumerate() {
        for r in library.emitters(file) {
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
}

#[derive(Default)]
pub(super) struct State {
    map: Option<(String, u64)>,
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
) {
    let _timing = super::timing::scope(&super::timing::EMITTERS);
    // No native runtime: the emitters are silent (its start logged why).
    let (Some(library), Some(mut native)) = (library, native) else { return };
    let native = &mut *native;
    let state = &mut *state;
    let identity = (map.name.clone(), map.generation);
    if state.map.as_ref() != Some(&identity) {
        for node in state.nodes.drain(..) {
            if let Some(post) = node.post {
                native.release(post);
            }
            if let Some(g) = node.state {
                native.release_emitter_state(g);
            }
        }
        native.unload_map_banks();
        let stem = map.path.as_deref().and_then(|p| p.file_stem()).and_then(|s| s.to_str()).unwrap_or("");
        // Retail's emitter system loads every file of the map's database entry and dispatches by
        // the attribute's eVolumeType: 1 = looping emitter (here), 5 = reverb zone
        // (`reverb_zones`), 4 = the single-winner music zone (`sub_828EB410`: a playlist of the
        // music system, not ported). 6 / 7 (speakers / crowds) are not dispatched by the
        // emitter system at all. On the disc only the `sfx_` / `skateschool` files hold type 1.
        let records: Vec<&super::library::EmitterRecord> = ems_files(stem).iter().flat_map(|file| library.emitters(file)).collect();
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
        state.map = Some(identity);
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
            }
            if let Some(post) = node.post {
                native.release(post);
            }
            if let Some(g) = node.state {
                native.release_emitter_state(g);
            }
        }
        keep
    });
    // New hits join the node list in discovery order.
    for (index, hit) in reached.iter().enumerate() {
        if hit.is_some() && !state.nodes.iter().any(|n| n.record == index) {
            state.nodes.push(Node { record: index, started: false, post: None, state: None });
        }
    }
    // Waiting nodes take free states in list order.
    let mut active = state.nodes.iter().filter(|n| n.started).count();
    for node in state.nodes.iter_mut().filter(|n| !n.started) {
        if active >= MAX_ACTIVE {
            break;
        }
        node.started = true;
        active += 1;
        let e = &state.emitters[node.record];
        info!("AUDIO_EMITTER start {} #{} at {:.1?} volume {:.2} (native)", e.bank, node.record, e.shape.position.to_array(), e.volume);
        match native.ensure_bank(&library, &e.bank) {
            Ok(_) => {
                if let Some(status) = state.emitter_bank.get(node.record).and_then(|&b| state.bank_status.get_mut(b)) {
                    *status = BankStatus::Loaded;
                }
                let level = e.volume * falloff(e.falloff, reached[node.record].unwrap_or(1.0));
                node.state = native.claim_emitter_state();
                if let Some(g) = node.state {
                    native.set_emitter_position(g, listener, skater, e.shape.position);
                }
                let payload = native.emitter_payload(node.state, level, super::native::azimuth(listener, e.shape.position), e.patch);
                node.post = native.post_emitter(&payload);
            }
            Err(error) => warn!("AUDIO_EMITTER {}: {error}", e.bank),
        }
    }

    // The bank's program does the rest; only the game-side words change.
    for node in state.nodes.iter_mut().filter(|n| n.started) {
        let emitter = &state.emitters[node.record];
        let (Some(d), Some(post)) = (reached[node.record], node.post) else { continue };
        let level = emitter.volume * falloff(emitter.falloff, d);
        if let Some(g) = node.state {
            native.set_emitter_position(g, listener, skater, emitter.shape.position);
        }
        let payload = native.emitter_payload(node.state, level, super::native::azimuth(listener, emitter.shape.position), emitter.patch);
        native.redeliver(post, &payload);
    }
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
        for r in ems_files("DownTown").iter().flat_map(|f| library.emitters(f)) {
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
        let records = zone_records(&library, "DownTown");
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

    #[test]
    fn every_map_has_its_emitter_file() {
        for map in ["University", "DownTown", "Industrial", "SkateSchool", "MegaPark"] {
            assert!(ems_file(map).is_some(), "{map}");
        }
    }

}
