//! NPC (AI) skaters' board sounds on the native runtime (`skate_audio::world::skaters`): the
//! local player's components run for the MixMap Player slot's second instance, for the NPC skater
//! retail's `CSTATEMGR_Player` would give it to (the first one, in the skater list's order, within
//! 30 m of the camera; spec `audio-specs/world-npc-skater-audio.md`). **Inert until an AI-skater
//! system publishes skaters**: [`NpcSkaters`] stays empty and [`frame`] returns at once.
//! `SKATE_AEMS_NPC_SKATERS=0` turns it off even with skaters.
//!
//! The hook for a future AI-skater system: each frame, fill [`NpcSkaters::skaters`] with every
//! live NPC skater in its list order (stable `id`, an `AudioState` filled like the local player's
//! `skate_events::audio_state`); drop the ones that despawn. Everything else happens here, per
//! pass, in retail's process / tick / update order (like `world_sources.rs`):
//! - the instance assignment (`skaters::Slots`);
//! - before the MixMap ticks ([`frame_pre`] → [`pre`]): the held skater's inputs and its
//!   components' `process`; after them ([`frame`] → [`post`]): their `update` from this pass's
//!   outputs, with the local player's tuning and banks (the components post into the banks the
//!   local player's host loaded);
//! - its collision messages go to the local player's collision manager (retail's one
//!   `CSTATEMGR_Collision`).
//!
//! - its granular rolling bed (2026-10-03): retail's SkateBoard update runs per instance, so the
//!   held skater's routing binds its own grain players (`grain_bed::Bed::for_instance`, the
//!   runtime's `npc_grains`) on its SkateBoard instance's MixMap outputs; the local-only parts stay
//!   off (`grain_bed.rs` module docs). Its picks draw from the local bed's generator.
//!
//! - its Wheels spin-down streams (layers 0 / 1) and Clothing (push / plant foley, body slide,
//!   cloth falls), which retail runs per instance (recomp gap run G3); Tricks, Treatment and the
//!   OffBoard steps stay local only, as in retail (module docs of `skaters`);
//! - its bail grunt (`sub_824BF5F8`: event 8206 for the skater's voice) and the speaking skaters'
//!   positions go to the speech host (`world_speech`).
use std::collections::HashMap;

use bevy::prelude::*;
use skate_audio::eval::NodeId;
use skate_audio::player::components::{Command, Slot};
use skate_audio::player::objpos::Listener;
use skate_audio::world::skaters::{self, NpcSkater, NpcSkaterAudioState, Parts, Slots, Tuning};

use super::native::Native;

/// What an AI-skater system publishes each frame (empty: nothing plays), in its skater list's
/// order.
#[derive(Resource, Default)]
pub(crate) struct NpcSkaters {
    pub(crate) skaters: Vec<NpcSkaterAudioState>,
}

/// `SKATE_AEMS_NPC_SKATERS=0` keeps the NPC skaters' board sounds off.
fn requested() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("SKATE_AEMS_NPC_SKATERS").is_ok_and(|v| v == "0"))
}

#[derive(Resource, Default)]
pub(crate) struct NpcHost {
    /// Audio event rows while some mod subscribes (`mod_audio::events_frame`).
    pub(crate) events: super::mod_audio::EventBuf,
    /// The mods' mute / replace / layer rules (`mod_rules`); None without rules.
    pub(crate) rules: Option<std::sync::Arc<super::mod_rules::RuleSet>>,
    slots: Slots,
    objects: HashMap<u64, NpcSkater>,
    nodes: HashMap<(u64, Slot), NodeId>,
    classes: HashMap<&'static str, usize>,
    /// This frame's pass between [`pre`] and [`post`]: its seconds and console evaluations.
    pass: Option<(f32, usize)>,
    /// The camera at the last evaluation and the host's cut count then (`Native::cuts`: no
    /// velocity across a teleport / map change).
    last_camera: Option<([f32; 3], u64)>,
    announced: bool,
    /// The held skater's grain bed (instance 1) and its push-plant count (`+335` rises; the bed's
    /// push envelopes and SkateBoard input 4 follow it), with the native rolling layers on.
    beds: HashMap<u64, (super::grain_bed::Bed, u32)>,
    /// `Native::map_epoch` this host last ran in (None: never ran; [`NpcHost::reset`]).
    epoch: Option<u64>,
    /// Posts and Splice starts of released skaters (the summary log adds the held ones').
    posts: u64,
    /// This evaluation's bail grunts (skater id) and every published skater with a voice (id,
    /// voice, COM position, COM velocity): `frame` hands them to the speech host.
    pub(crate) grunts: Vec<u64>,
    pub(crate) speakers: Vec<super::world_speech::SkaterSpeaker>,
}

pub(crate) fn register(app: &mut App) {
    // `frame_pre` runs in the pass chain (`game_audio::mod`), between `native::mixmap_frame` and
    // `native::mixmap_tick`; the update after the ticks and the local bed's step (the NPC bed draws
    // from the local bed's generator after it).
    app.init_resource::<NpcSkaters>().init_resource::<NpcHost>().add_systems(Update, frame.after(super::native::mixmap_tick).after(super::grain_bed::update));
}

impl NpcHost {
    /// Apply one skater's commands to the runtime (its packets keyed by skater and slot).
    pub(crate) fn apply(&mut self, rt: &mut skate_audio::runtime::Runtime, owner: u64, cmds: Vec<Command>) {
        for cmd in cmds {
            match cmd {
                Command::Post { slot, class, words } => {
                    let id = *self.classes.entry(class).or_insert_with(|| rt.eval.class_id(class).unwrap_or(usize::MAX));
                    if id == usize::MAX {
                        continue;
                    }
                    if let Some(old) = self.nodes.remove(&(owner, slot)) {
                        rt.release(old);
                    }
                    let muted = self.rules.as_deref().is_some_and(|r| {
                        let (name, index) = super::mod_audio::player_slot(&slot);
                        r.mutes(&super::mod_audio::EventRow { kind: super::mod_audio::EventKind::Post, source: super::mod_audio::Source::Npc, class, slot: name, id: index, owner, hit: None })
                    });
                    if !muted {
                        self.nodes.insert((owner, slot), rt.post(id, &words));
                    }
                    if self.events.is_some() {
                        let (name, index) = super::mod_audio::player_slot(&slot);
                        super::mod_audio::record(&mut self.events, super::mod_audio::EventRow { kind: super::mod_audio::EventKind::Post, source: super::mod_audio::Source::Npc, class, slot: name, id: index, owner, hit: None });
                    }
                }
                Command::Redeliver { slot, words } => {
                    if let Some(&node) = self.nodes.get(&(owner, slot)) {
                        rt.redeliver(node, &words);
                    }
                }
                Command::Release { slot } => {
                    if let Some(node) = self.nodes.remove(&(owner, slot)) {
                        rt.release(node);
                    }
                }
            }
        }
    }

    /// A map change (`Native::map_epoch`) or the first run: release every held packet, forget the
    /// holders (their instances' 3DObjPos blocks go inactive), stop the NPC bed and drop the
    /// per-skater objects, so a skater id that survives the change (a reused id, a persistent
    /// publisher) is claimed afresh. The records take the MixMap's count (`Native::world.npc`).
    fn reset(&mut self, native: &mut Native) {
        self.epoch = Some(native.map_epoch);
        let Native { mixmap, shared, world, .. } = native;
        if let Some(m) = mixmap.as_mut() {
            let l = Listener::default();
            for npc in self.objects.values_mut() {
                npc.deactivate(m, &l);
            }
        }
        if !self.nodes.is_empty() || !self.beds.is_empty() || !self.objects.is_empty() {
            if let Ok(mut runtime) = super::timing::lock(shared, &super::timing::GAME_LOCK) {
                // Their Splice sounds too (a dropped skater is never updated again).
                for (_, mut npc) in self.objects.drain() {
                    npc.release(&mut runtime.splice_host());
                }
                for (_, node) in self.nodes.drain() {
                    runtime.release(node);
                }
                if !self.beds.is_empty() {
                    super::grain_bed::stop_npc(&mut runtime);
                }
            }
        }
        self.objects.clear();
        self.nodes.clear();
        self.beds.clear();
        self.grunts.clear();
        self.pass = None;
        self.slots = Slots::with_records(world.npc);
        self.last_camera = None;
    }

    /// Release every packet a skater holds (it lost its instance).
    fn release_all(&mut self, rt: &mut skate_audio::runtime::Runtime, owner: u64) {
        let slots: Vec<(u64, Slot)> = self.nodes.keys().filter(|k| k.0 == owner).copied().collect();
        for k in slots {
            if let Some(node) = self.nodes.remove(&k) {
                rt.release(node);
            }
        }
    }
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The process before the MixMap ticks ([`pre`]).
pub(super) fn frame_pre(
    native: Option<ResMut<Native>>,
    published: Res<NpcSkaters>,
    mut host: ResMut<NpcHost>,
    cues: Res<super::skate_events::Cues>,
    listener: Query<&GlobalTransform, With<super::GameAudioListener>>,
) {
    // Inert: nothing published and nothing held.
    if published.skaters.is_empty() && host.objects.is_empty() {
        return;
    }
    if !requested() {
        return;
    }
    let Some(mut native) = native else { return };
    let Ok(camera) = listener.single() else { return };
    let calls = native.pending.map_or(0, |p| p.calls);
    pre(&mut host, &published, &mut native, (camera.translation().to_array(), camera.forward().as_vec3().to_array()), &cues.riding.audio, calls);
}

/// The update after the MixMap ticks ([`post`]) and the hand-over to the speech host and the
/// read-back.
#[allow(clippy::too_many_arguments)]
pub(super) fn frame(
    library: Option<Res<super::Library>>,
    native: Option<ResMut<Native>>,
    published: Res<NpcSkaters>,
    mut host: ResMut<NpcHost>,
    mut held: ResMut<super::world_sources::WorldHeld>,
    cues: Res<super::skate_events::Cues>,
    mut speech: ResMut<super::world_speech::WorldSpeech>,
) {
    // Inert: nothing published and nothing held.
    if published.skaters.is_empty() && host.objects.is_empty() && host.grunts.is_empty() {
        return;
    }
    if !requested() {
        return;
    }
    let Some(mut native) = native else { return };
    post(&mut host, &published, &mut native, library.as_deref(), &cues.riding.audio);
    speech.grunts.append(&mut host.grunts);
    if speech.skaters != host.speakers {
        speech.skaters.clone_from(&host.speakers);
    }
    let skaters: Vec<(u64, u32)> = host.slots.holders().map(|(g, id)| (id, g)).collect();
    if held.skaters != skaters {
        held.skaters = skaters;
    }
    let posts = host.posts + host.objects.values().map(|n| n.posts).sum::<u64>();
    if held.npc_posts != posts {
        held.npc_posts = posts;
    }
}

/// The host's first half of a frame, before the MixMap ticks (retail's process; see the module
/// docs): the map-change reset, and with a pass this frame (`calls` console evaluations, from
/// `Native::pending`) the instance assignment, the held skater's inputs (PlayerPhysics, 3DObjPos,
/// Contacts / Rail / OffBoard, its bed's owner inputs) and its components' process, its collision
/// messages into the local player's manager, its bail grunt. [`post`] runs the update.
/// `camera` = the listener (position, forward), `local` = the local player's audio state.
pub(crate) fn pre(host: &mut NpcHost, published: &NpcSkaters, native: &mut Native, camera: ([f32; 3], [f32; 3]), local: &skate_audio::player::AudioState, calls: usize) {
    if host.epoch != Some(native.map_epoch) {
        host.reset(native);
    }
    let Native { mixmap, player, shared, bed: local_bed, cuts, .. } = native;
    let (Some(m), Some(player)) = (mixmap.as_mut(), player.as_mut()) else { return };
    if !player.components || calls == 0 {
        return;
    }
    let evaluations = calls;
    let dt = super::world_sources::evaluation_dt() * evaluations.min(4) as f32;
    host.pass = Some((dt, evaluations));
    if !host.announced {
        info!("AUDIO_NPC on: {} NPC skaters published", published.skaters.len());
        host.announced = true;
    }
    let (cam, view) = camera;
    let cam_velocity = host.last_camera.filter(|l| l.1 == *cuts).map_or([0.0; 3], |(last, _)| std::array::from_fn(|i| (cam[i] - last[i]) / dt));
    host.last_camera = Some((cam, *cuts));
    let local = *local;
    let l = Listener {
        camera: cam,
        view,
        camera_velocity: cam_velocity,
        followed: local.com_position,
        facing: local.com_velocity,
        followed_velocity: local.com_velocity,
    };
    let Ok(mut runtime) = super::timing::lock(shared, &super::timing::GAME_LOCK) else { return };
    let rt = &mut *runtime;

    let candidates: Vec<(u64, f32)> = published.skaters.iter().map(|p| (p.id, distance(p.state.com_position, cam))).collect();
    let assignment = host.slots.assign(&candidates);
    for (id, g) in assignment.released {
        if let Some(mut npc) = host.objects.remove(&id) {
            npc.deactivate(m, &l);
            npc.stop_wheels(&mut rt.stream_host());
            npc.release(&mut rt.splice_host());
            host.posts += npc.posts;
        }
        host.release_all(rt, id);
        if host.beds.remove(&id).is_some() {
            super::grain_bed::stop_npc(rt);
        }
        info!("AUDIO_NPC release skater {id} (instance {g})");
    }
    let parts = Parts { rolling: player.rolling_on, rattle: player.rattle_on, slide: player.slide_on, contacts: player.contacts_on, wheels: player.wheels_on, clothing: player.footsteps_on };
    for (id, g) in assignment.claimed {
        let npc = NpcSkater::new(g as u32, parts, true, true, true);
        host.objects.insert(id, npc);
        // The runtime has one NPC bed (`Runtime::npc_grains`): instance 1's. With the non-retail
        // "more audible" layout the further instances play without a bed.
        if let (true, Some(bed), 1) = (parts.rolling, local_bed.as_ref(), g) {
            host.beds.insert(id, (bed.for_instance(g as u32), 0));
        }
        info!("AUDIO_NPC claim skater {id} (instance {g})");
    }

    let tuning = Tuning { player: &player.tuning, contacts: &player.contact_tuning, wheels: &player.wheels_tuning, clothing: &player.clothing_tuning };
    host.speakers.clear();
    for p in published.skaters.iter().filter(|p| p.voice != 0) {
        host.speakers.push(super::world_speech::SkaterSpeaker { id: p.id, voice: p.voice, position: p.state.com_position, velocity: p.state.com_velocity, reactions: p.reactions });
    }
    let mut collisions = Vec::new();
    let held: Vec<(u32, u64)> = host.slots.holders().collect();
    for (_, id) in held {
        let Some(p) = published.skaters.iter().find(|p| p.id == id) else { continue };
        let Some(mut npc) = host.objects.remove(&id) else { continue };
        let mut s = skaters::component_state(&p.state, local.soft_wheels);
        s.dt = dt;
        // The bed's owner inputs 2 / 3 / 4 (from its last step) for this pass's evaluations.
        if let Some((bed, _)) = host.beds.get_mut(&id) {
            bed.write_inputs(m, &s, false);
        }
        npc.write_inputs(m, &s, &l, local.com_velocity, tuning.player);
        // The body / deck posters once per console evaluation, as the local player's.
        npc.set_body_calls(Some(evaluations));
        npc.set_deck_calls(Some(evaluations));
        npc.loose_board = p.loose_board;
        let cmds = npc.process(m, &s, tuning, &mut super::mod_audio::Observed::new(&mut rt.splice_host(), &mut host.events, super::mod_audio::Source::Npc, id).rules(host.rules.as_deref()));
        host.apply(rt, id, cmds);
        super::player_audio::body_hits(npc.body_hits(), super::mod_audio::Source::Npc, id, Some(cam), &mut host.events);
        // The routing's binds wait for the bed's step after the ticks (dropped without a bed).
        if !host.beds.contains_key(&id) {
            npc.routed.grains.clear();
        }
        collisions.extend(npc.take_collisions());
        if npc.take_bail_grunt() {
            host.grunts.push(id);
        }
        host.objects.insert(id, npc);
    }
    player.post_collisions(collisions, rt);
}

/// The host's second half, after the MixMap ticks (retail's update): the held skater's
/// components' update, its Wheels streams, and its grain bed's step (SkateBoard update
/// `sub_824C6BD8`: records from this pass's outputs, with the binds / stops of the routing's
/// process in [`pre`]).
pub(crate) fn post(host: &mut NpcHost, published: &NpcSkaters, native: &mut Native, library: Option<&super::Library>, local: &skate_audio::player::AudioState) {
    let Some((dt, evaluations)) = host.pass.take() else { return };
    let Native { mixmap, player, shared, .. } = native;
    let (Some(m), Some(player)) = (mixmap.as_mut(), player.as_mut()) else { return };
    let Ok(mut runtime) = super::timing::lock(shared, &super::timing::GAME_LOCK) else { return };
    let rt = &mut *runtime;
    let tuning = Tuning { player: &player.tuning, contacts: &player.contact_tuning, wheels: &player.wheels_tuning, clothing: &player.clothing_tuning };
    let held: Vec<(u32, u64)> = host.slots.holders().collect();
    for (_, id) in held {
        let Some(p) = published.skaters.iter().find(|p| p.id == id) else { continue };
        let Some(mut npc) = host.objects.remove(&id) else { continue };
        let mut s = skaters::component_state(&p.state, local.soft_wheels);
        s.dt = dt;
        npc.loose_board = p.loose_board;
        let cmds = npc.update(m, &s, tuning, &mut super::mod_audio::Observed::new(&mut rt.splice_host(), &mut host.events, super::mod_audio::Source::Npc, id).rules(host.rules.as_deref()));
        host.apply(rt, id, cmds);
        npc.update_wheels(m, &s, tuning.wheels, &mut rt.stream_host());
        if let (Some((bed, pushes)), Some(library)) = (host.beds.get_mut(&id), library) {
            *pushes = pushes.wrapping_add(u32::from(s.push_trigger));
            let r = super::skate_events::Riding {
                board: Vec3::from_array(s.board_position),
                speed: s.ground_speed,
                grinding: s.grinding,
                braking: s.brake,
                wheels: s.wheel_count,
                pushes: *pushes,
                audio: s,
                ..Default::default()
            };
            let routed = Some((std::mem::take(&mut npc.routed.grains), npc.routed.primary));
            // Its turn / brake slews once per console evaluation, as the local bed's.
            bed.slew_calls = Some(evaluations);
            super::grain_bed::step_with(bed, library, m, &r, dt, tuning.player, routed, |apply| apply(&mut *rt));
        }
        host.objects.insert(id, npc);
    }
}

/// One whole pass for tests and tools: [`pre`] with one console evaluation, the tick, [`post`] (the
/// caller sets the MixMap's globals first).
#[cfg(test)]
pub(crate) fn run(host: &mut NpcHost, published: &NpcSkaters, native: &mut Native, library: Option<&super::Library>, camera: ([f32; 3], [f32; 3]), local: &skate_audio::player::AudioState) {
    pre(host, published, native, camera, local, 1);
    if host.pass.is_some()
        && let Some(m) = native.mixmap.as_mut()
    {
        m.tick(skate_audio::mixmap::cadence::CONSOLE_DT);
    }
    post(host, published, native, library, local);
}

#[cfg(test)]
mod tests;
