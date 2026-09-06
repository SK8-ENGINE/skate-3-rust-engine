//! Live Skeleton BipedAir82BDDD70 over the shared solve owners.
use crate::physics::{GamePhysics, SkaterRuntime, skeleton_input_runtime::SkeletonOwners};
use skate_core::physics::skeleton_animation_record::{
    AnimationPartTransform as Frame, compose_affine,
};
pub(crate) fn update(
    physics: &mut GamePhysics,
    skater: &mut SkaterRuntime,
    frame: Frame,
    trajectory_position: [f32; 4],
    body_position: [f32; 4],
    height: f32,
) -> Result<(), String> {
    let collision = crate::physics::input_phase::collision(skater);
    let p = &mut skater.player_input.processed;
    let s = &mut skater.animated_skeleton;
    s.board_frames.skate_root = compose_affine(&s.roots.animation_to_world, &s.record.pose[0]);
    s.board_frames
        .update_com_lift(&s.roots.animation_to_world, body_position, height);
    let target = skate_core::physics::skeleton_biped_air::prepare(
        &mut s.roots,
        frame,
        trajectory_position,
        s.record.centre_of_mass,
        &skater.skeleton_input.drive_frames[0],
        p.flags_2476,
        &mut p.flags_2468,
    );
    s.board_frames.animation_target = target;
    if p.flags_2480 & 0x8000 != 0 {
        s.board_frames.physical_board =
            skater
                .skeleton_air
                .apply_board(&mut physics.board, &target, true);
    }
    let mut owners = SkeletonOwners {
        animated: s,
        body: &mut skater.skeleton,
        drives: &mut skater.skeleton_drives,
        ik: &mut skater.foot_ik,
        animation_input: &mut skater.animation_input,
        correction: &mut skater.skeleton_output.correction,
        pose_errors: &mut skater.pose_errors,
    };
    skater.skeleton_input.general_update(
        p,
        &mut owners,
        &skater.animation.packet.hierarchy,
        &collision,
        physics.settings.step.simulation,
    )?;
    let root = owners.animated.roots.animation_to_world;
    //82D8E3E0's forward is the processed board forward96 in this Air caller.
    physics.riding.update_biped_reckoning(
        &mut skater.air_reckoning.state,
        super::skeleton_ground::ReckoningUpdate {
            up: root[1],
            forward: skater
                .player_input
                .toolkit
                .as_ref()
                .ok_or("BipedAir Skeleton has no board toolkit")?
                .deck[2],
            blend: 0.5,
        },
        p.flags_2468,
        owners.animation_input.extra.physical_body_spin,
    );
    owners.animated.finish_ground();
    owners.animation_input.fields.flags2468 = p.flags_2468;
    Ok(())
}
