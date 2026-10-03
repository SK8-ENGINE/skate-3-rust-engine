//! NPC (AI) skaters' board sounds on the native runtime (`skate_audio::world::skaters`): the
//! local player's components run for the MixMap Player slot's second instance, for the NPC skater
//! retail's `CSTATEMGR_Player` would give it to (the first one, in the skater list's order, within
//! 30 m of the camera; spec `.claude/notes/world-npc-skater-audio.md`). **Inert until an AI-skater
//! system publishes skaters**: [`NpcSkaters`] stays empty and [`frame`] returns at once.
//! `SKATE_AEMS_NPC_SKATERS=0` turns it off even with skaters.
//!
//! The hook for a future AI-skater system: each frame, fill [`NpcSkaters::skaters`] with every
//! live NPC skater in its list order (stable `id`, an `AudioState` filled like the local player's
//! `skate_events::audio_state`); drop the ones that despawn. Everything else happens here, per
//! MixMap evaluation, after `native::mixmap_frame` (like `world_sources.rs`, so the inputs an
//! evaluation sees are one console frame old — the seam to move into `mixmap_frame` once a system
//! exists):
//! - the instance assignment (`skaters::Slots`);
//! - the held skater's components `update` from this evaluation's outputs, then its inputs and
//!   `process` for the next one, with the local player's tuning and banks (the components post
//!   into the banks the local player's host loaded);
//! - its collision messages go to the local player's collision manager (retail's one
//!   `CSTATEMGR_Collision`).
//!
//! - its granular rolling bed (2026-10-03): retail's SkateBoard update runs per instance, so the
//!   held skater's routing binds its own grain players (`grain_bed::Bed::for_instance`, the
//!   runtime's `npc_grains`) on its SkateBoard instance's MixMap outputs; the local-only parts stay
//!   off (`grain_bed.rs` module docs). Its picks draw from the local bed's generator.
//!
//! Not yet: wheels, tricks, footsteps, clothing (module docs of `skaters`).
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
    slots: Slots,
    objects: HashMap<u64, NpcSkater>,
    nodes: HashMap<(u64, Slot), NodeId>,
    classes: HashMap<&'static str, usize>,
    last_tick: u64,
    /// The camera at the last evaluation and the host's cut count then (`Native::cuts`: no
    /// velocity across a teleport / map change).
    last_camera: Option<([f32; 3], u64)>,
    announced: bool,
    /// The held skater's grain bed (instance 1) and its push-plant count (`+335` rises; the bed's
    /// push envelopes and SkateBoard input 4 follow it), with the native rolling layers on.
    beds: HashMap<u64, (super::grain_bed::Bed, u32)>,
    /// `Native::map_epoch` this host last ran in (None: never ran; [`NpcHost::reset`]).
    epoch: Option<u64>,
}

pub(crate) fn register(app: &mut App) {
    app.init_resource::<NpcSkaters>().init_resource::<NpcHost>().add_systems(Update, frame.after(super::native::mixmap_frame));
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
                    self.nodes.insert((owner, slot), rt.post(id, &words));
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
            for (_, mut npc) in self.objects.drain() {
                npc.deactivate(m, &l);
            }
            self.last_tick = self.last_tick.min(m.ticks);
        }
        self.objects.clear();
        if !self.nodes.is_empty() || !self.beds.is_empty() {
            if let Ok(mut runtime) = super::timing::lock(shared, &super::timing::GAME_LOCK) {
                for (_, node) in self.nodes.drain() {
                    runtime.release(node);
                }
                if !self.beds.is_empty() {
                    super::grain_bed::stop_npc(&mut runtime);
                }
            }
        }
        self.nodes.clear();
        self.beds.clear();
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

#[allow(clippy::too_many_arguments)]
pub(super) fn frame(
    library: Option<Res<super::Library>>,
    native: Option<ResMut<Native>>,
    published: Res<NpcSkaters>,
    mut host: ResMut<NpcHost>,
    mut held: ResMut<super::world_sources::WorldHeld>,
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
    run(&mut host, &published, &mut native, library.as_deref(), (camera.translation().to_array(), camera.forward().as_vec3().to_array()), &cues.riding.audio);
    let skaters: Vec<(u64, u32)> = host.slots.holders().map(|(g, id)| (id, g)).collect();
    if held.skaters != skaters {
        held.skaters = skaters;
    }
}

/// One frame of the host after the inert checks (see the module docs). `camera` = the listener
/// (position, forward), `local` = the local player's audio state.
pub(crate) fn run(host: &mut NpcHost, published: &NpcSkaters, native: &mut Native, library: Option<&super::Library>, camera: ([f32; 3], [f32; 3]), local: &skate_audio::player::AudioState) {
    if host.epoch != Some(native.map_epoch) {
        host.reset(native);
    }
    let Native { mixmap, player, shared, bed: local_bed, cuts, .. } = native;
    let (Some(m), Some(player)) = (mixmap.as_mut(), player.as_mut()) else { return };
    if !player.components {
        return;
    }
    if m.ticks == host.last_tick {
        return;
    }
    let evaluations = m.ticks.saturating_sub(host.last_tick).max(1);
    host.last_tick = m.ticks;
    let dt = super::world_sources::evaluation_dt() * evaluations.min(4) as f32;
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
        }
        host.release_all(rt, id);
        if host.beds.remove(&id).is_some() {
            super::grain_bed::stop_npc(rt);
        }
        info!("AUDIO_NPC release skater {id} (instance {g})");
    }
    let parts = Parts { rolling: player.rolling_on, rattle: player.rattle_on, contacts: player.contacts_on };
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

    let tuning = Tuning { player: &player.tuning, contacts: &player.contact_tuning };
    let mut collisions = Vec::new();
    let held: Vec<(u32, u64)> = host.slots.holders().collect();
    for (_, id) in held {
        let Some(p) = published.skaters.iter().find(|p| p.id == id) else { continue };
        let Some(mut npc) = host.objects.remove(&id) else { continue };
        let mut s = skaters::component_state(&p.state, local.soft_wheels);
        s.dt = dt;
        let cmds = npc.update(m, &s, tuning, &mut rt.splice_host());
        host.apply(rt, id, cmds);
        // The instance's grain bed after this evaluation (SkateBoard update `sub_824C6BD8`: records
        // from its outputs), with the binds / stops of the routing's last process; then its owner
        // inputs 2 / 3 / 4 for the next evaluation.
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
            bed.slew_calls = Some(evaluations as usize);
            super::grain_bed::step_with(bed, library, m, &r, dt, tuning.player, routed, |apply| apply(&mut *rt));
            bed.write_inputs(m, &s, false);
        }
        npc.write_inputs(m, &s, &l, local.com_velocity, tuning.player);
        // The body / deck posters once per console evaluation, as the local player's.
        npc.set_body_calls(Some(evaluations as usize));
        npc.set_deck_calls(Some(evaluations as usize));
        let cmds = npc.process(m, &s, tuning, &mut rt.splice_host());
        host.apply(rt, id, cmds);
        // The routing's binds wait for the bed's next step (dropped without a bed).
        if !host.beds.contains_key(&id) {
            npc.routed.grains.clear();
        }
        collisions.extend(npc.take_collisions());
        host.objects.insert(id, npc);
    }
    player.post_collisions(collisions, rt);
}

#[cfg(test)]
mod tests;
