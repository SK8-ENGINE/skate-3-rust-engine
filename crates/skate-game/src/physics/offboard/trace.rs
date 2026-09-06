//! Opt-in observations of user-controlled dismounts; never modifies simulation.
use crate::physics::{GamePhysics, SkaterRuntime};

pub(crate) struct Trace {
    enabled: bool,
    was_offboard: bool,
    remaining: u32,
    captures: u32,
}
impl Default for Trace {
    fn default() -> Self {
        Self {
            enabled: std::env::var_os("SKATE_OFFBOARD_TRACE").is_some(),
            was_offboard: false,
            remaining: 0,
            captures: 0,
        }
    }
}
pub(crate) fn record(physics: &GamePhysics, skater: &mut SkaterRuntime) {
    let p = &skater.player_input.processed;
    let trace = &mut skater.offboard.trace;
    if !trace.enabled { return; }
    let offboard = p.category_2512 == 500;
    if offboard && !trace.was_offboard && trace.remaining == 0 && trace.captures < 3 {
        trace.remaining = 600;
        trace.captures += 1;
    }
    trace.was_offboard = offboard;
    if trace.remaining == 0 { return; }
    trace.remaining -= 1;
    let motion = skater.offboard.controller.output();
    let s = &skater.animated_skeleton;
    let a = &skater.animation_input.fields;
    eprintln!(
        "OFFBOARD_TRACE tick={} state={} clip={:?} flags={:08x}/{:08x}/{:08x}/{:08x}/{:08x} root={:?} root_velocity={:?} controller={:?} velocity={:?} up={:?} com_target={:?} com={:?} pose_error={:?} extra_error={:?} contact={:x} contact_pos={:?} contact_normal={:?} cadence={} anim_velocity={:?} anim_motion={:?} anim_time={} override_time={} hand={} board_target={:?} board={:?} bail={:?}",
        physics.ticks, p.state_2508, skater.animation.motion.animation.current_name,
        p.flags_2468, p.flags_2472, p.flags_2476, p.flags_2480, p.flags_2484,
        s.roots.animation_to_world[3], skater.skeleton_input.root_velocity,
        motion.physical_frame[3], motion.velocity, motion.animation_frame[1],
        s.board_frames.com_frame[3], skater.skeleton.record.centre_of_mass,
        skater.collision_maximum_error, skater.collision_extra_displacements,
        skater.offboard.retained_contact.flags, skater.offboard.retained_contact.position,
        skater.offboard.retained_contact.normal, a.cadence_end_percent,
        s.motion.velocity_world, a.animation_translation, a.animation_time,
        a.animation_physics_blend_seconds, skater.offboard.possession.selected_hand_424,
        s.board_frames.animation_target[3], crate::physics::solve::deck_frame(&physics.board)[3],
        skater.wipeout.state.reasons,
    );
}
