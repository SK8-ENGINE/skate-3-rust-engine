//! Bounded numeric history for user-reproduced airborne bails. No per-frame IO.
use crate::physics::SkaterRuntime;
use std::collections::VecDeque;

#[derive(Debug)]
#[allow(dead_code)] // Fields are serialized together only when a bail is requested.
struct Sample {
    tick: i32,
    flags: [u32; 3],
    packet: skate_core::player::offboard::air_prediction::Packet,
    root: [[f32; 4]; 4],
    root_velocity: [f32; 4],
    body: [f32; 4],
    lift: f32,
    extra: [[f32; 4]; 2],
    maximum_error: Option<f32>,
    reasons: [bool; 34],
}

pub(crate) struct History {
    samples: VecDeque<Sample>,
    reports: usize,
}
impl Default for History {
    fn default() -> Self {
        Self { samples: VecDeque::with_capacity(32), reports: 0 }
    }
}
impl History {
    pub(crate) fn clear(&mut self) { self.samples.clear(); }
}

pub(crate) fn record(skater: &mut SkaterRuntime) {
    let history = &mut skater.offboard.air_diagnostics;
    if history.reports >= 3 { return; }
    let state = &skater.offboard.air_state;
    let p = &skater.player_input.processed;
    if history.samples.len() == 32 { history.samples.pop_front(); }
    history.samples.push_back(Sample {
        tick: state.tick,
        flags: [p.flags_2476, p.flags_2480, p.flags_2484],
        packet: state.packet,
        root: skater.animated_skeleton.roots.animation_to_world,
        root_velocity: skater.skeleton_input.root_velocity,
        body: state.body_position,
        lift: state.lift,
        extra: skater.collision_extra_displacements,
        maximum_error: skater.collision_maximum_error,
        reasons: skater.wipeout.state.reasons,
    });
    if skater.wipeout.state.reasons.iter().any(|&requested| requested) {
        // Format in memory, then one write; never send Debug's individual
        // formatting fragments to the Windows redirected stderr handle.
        let report = format!("SKATE_BIPED_AIR_BAIL history={:#?}", history.samples);
        eprintln!("{report}");
        history.samples.clear();
        history.reports += 1;
    }
}
