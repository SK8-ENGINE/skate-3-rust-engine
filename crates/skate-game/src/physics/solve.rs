//! One physical solve for the board, skater and original animation targets.
//! The caller publishes forces and drive targets before entering this phase.
mod assembly_contacts;
mod diagnostics;
use super::{GamePhysics, SkaterRuntime, colliders, skeleton_colliders};
use skate_core::physics::{
    board::BodyId,
    board_step::{ATTACHED_REACTION_BASE, AttachedStep},
    skeleton_animation_record::{AnimationPartTransform, IDENTITY},
    skeleton_body::PART_COUNT,
};

pub(super) fn advance(physics:&mut GamePhysics, skater:&mut SkaterRuntime,truck_targets:[f32;2],carry_tick:super::prop_carry::Tick)->Result<(),String> {
    let restore=crate::modding::player_physics::apply_parts(skater);
    let result=advance_inner(physics,skater,truck_targets,carry_tick);
    restore.restore(skater);
    result
}
fn advance_inner(
    physics: &mut GamePhysics,
    skater: &mut SkaterRuntime,
    truck_targets: [f32; 2],
    carry_tick: super::prop_carry::Tick,
) -> Result<(), String> {
    let mod_before = crate::modding::player_physics::before_solve(physics,skater);
    let before = diagnostics::snapshot(physics, skater);
    diagnostics::validate(&before, "before shared solve").map_err(|error| format!(
        "{error}; com_frame={:?}; lifted_com_frame={:?}; animation_root={:?}; biped_position={:?}; biped_surface={:?}",
        skater.animated_skeleton.board_frames.com_frame,
        skater.animated_skeleton.board_frames.lifted_com_frame,
        skater.animated_skeleton.roots.animation_to_world,
        skater.biped_ground.controller.state.position_368,
        skater.biped_ground.controller.state.surface,
    ))?;
    let mut board_volumes = colliders::world_volumes(&physics.board, &physics.settings);
    board_volumes.retain(|volume| skater.board_possession_live.volume_enabled(volume.body));
    let skeleton_volumes =
        skeleton_colliders::enabled_volumes(&skater.skeleton, &skater.skeleton_collision)?;
    // Each native assembly has its own query record and retention buffer.
    // Skeleton82BE5094 passes false to82768728: its edge threshold is -1,
    // whereas the board requests .999. GroundPipeline supplies the remaining
    // shared values. Do not let the second query overwrite the first's rows.
    let mut contacts = physics
        .world
        .query_primitives(&board_volumes, physics.query, physics.retention)
        .to_vec();
    let mut skeleton_query = physics.query;
    skeleton_query.edge_cos_bend_normal_threshold = -1.0;
    let mut skeleton_world_volumes = skeleton_volumes.clone();
    skeleton_colliders::retain_world_volumes(&mut skeleton_world_volumes, &skater.skeleton_collision);
    // Prop carry (Phase 3) steers the held body before pushes/integration so
    // the follow velocity participates in this tick's contacts and rebake.
    {
        let root = skater.animated_skeleton.roots.animation_to_world;
        let flat = skate_core::math::Vector3::new(root[2][0], 0.0, root[2][2]);
        let length = (flat.x * flat.x + flat.z * flat.z).sqrt();
        let forward = if length > 1e-3 {
            skate_core::math::Vector3::new(flat.x / length, 0.0, flat.z / length)
        } else {
            skate_core::math::Vector3::new(0.0, 0.0, 1.0)
        };
        // OB_ObjectMvX / Z / Rot (8259C4B0) drive the held prop's command.
        let extra = &skater.animation_input.extra;
        let carry_tick = super::prop_carry::Tick {
            object_move: [extra.object_move_x, extra.object_move_z, extra.object_move_rotation],
            ..carry_tick
        };
        physics.update_prop_carry(
            carry_tick,
            super::prop_carry::Carrier {
                state: skater.player_state.current(),
                position: skate_core::math::Vector3::new(root[3][0], root[3][1], root[3][2]),
                forward,
                time_step: physics.settings.step.simulation.time_step,
                // Retail Move Object inputs: Player+192, bone 23 (+272),
                // Skeleton+15872 (+416 at the grab).
                skeleton: Some(super::prop_carry::CarrierSkeleton {
                    frame: skater.player_input.processed.effective_anim_transform_192.map(|v| v.map(f32::from_bits)),
                    reference: {
                        let b = skate_core::physics::skeleton_animation_record::compose_affine(
                            &skater.animated_skeleton.roots.animation_to_world,
                            &skater.animated_skeleton.record.pose[23],
                        )[3];
                        skate_core::math::Vector3::new(b[0], b[1], b[2])
                    },
                    body: {
                        let c = skater.animated_skeleton.board_frames.com_frame[3];
                        skate_core::math::Vector3::new(c[0], c[1], c[2])
                    },
                    hand_span: {
                        let pose = &skater.animated_skeleton.record.pose;
                        let (a, b) = (pose[3][3], pose[7][3]);
                        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
                    },
                    collision_flag: skater.player_input.processed.flags_2484 & 0x0400_0000 != 0,
                    state_time: skater.player_input.processed.state_timer_2664,
                }),
            },
        );
        // 82D45008 -> 82BD9728 / 82BD97D0: the held prop's hand points as hand IK targets (limbs 2 / 3, the
        // handplant hand slots), clamped to the reach around the animated hand targets, at the hand IK weight.
        if let Some((hands, weight)) = physics.prop_carry.hand_ik_targets() {
            let reach = physics.prop_carry.locomotion().move_object.hand_ik_reach;
            let animated = &skater.animated_skeleton;
            for (h, target) in hands.iter().enumerate() {
                let limb = 2 + h;
                let o = super::footplant::math::point(&animated.roots.animation_to_world, animated.targets[limb][3]);
                let d = [target[0] - o[0], target[1] - o[1], target[2] - o[2]];
                let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                let k = if l > reach { reach / l } else { 1.0 };
                let p = [o[0] + d[0] * k, o[1] + d[1] * k, o[2] + d[2] * k, 1.0];
                skater.foot_ik.state.external_targets[limb].world_position = p;
                skater.foot_ik.state.limbs[limb].external_target_set = true;
                skater.foot_ik.state.limbs[limb].target_blend = weight;
            }
        }
    }
    // Dynamic props push back on the skater's live volumes before the queries
    // below see the freshly re-baked prop triangles.
    let push_volumes: Vec<_> = board_volumes
        .iter()
        .chain(&skeleton_world_volumes)
        .copied()
        .collect();
    physics.step_props(&push_volumes);
    contacts.extend_from_slice(physics.world.query_primitives(
        &skeleton_world_volumes,
        skeleton_query,
        physics.retention,
    ));
    // Static prop instances live in their own world; query the same volumes
    // against it without disturbing the two native query records above.
    let query = physics.query;
    let retention = physics.retention;
    if let Some(props) = physics.prop_world_mut() {
        contacts.extend_from_slice(props.query_primitives(&board_volumes, query, retention));
        contacts.extend_from_slice(props.query_primitives(
            &skeleton_world_volumes,
            skeleton_query,
            retention,
        ));
    }
    assembly_contacts::append(
        &mut contacts,
        &board_volumes,
        &skeleton_volumes,
        physics.board.collision_group(),
        &skater.skeleton_collision,
    )?;
    // Remote actors use separate reaction indices after skeleton and targets.
    // Never apply the single-skater self-culling bitmap to another player.
    let before_remote = contacts.len();
    for a in board_volumes.iter().chain(&skeleton_volumes) {
        for b in &physics.network_proxies.volumes {
            let (ac, ar) = super::network::bounds(a.primitive);
            let (bc, br) = super::network::bounds(b.primitive);
            if ac.distance_squared(bc) <= (ar + br + 0.05).powi(2) {
                assembly_contacts::append_pair(&mut contacts, a, b);
            }
        }
    }
    super::solid_contacts::append(&mut contacts, &board_volumes,
        &physics.network_proxies.solids, physics, skater);
    super::solid_contacts::append(&mut contacts, &skeleton_volumes,
        &physics.network_proxies.solids, physics, skater);
    physics.network_contacts = contacts.len() - before_remote;
    physics.contact_count = contacts.len();
    let dt = physics.settings.step.simulation.time_step;
    let mut joints = crate::modding::player_physics::joints(skater)
        .build(skater.skeleton.bodies(), ATTACHED_REACTION_BASE, dt);
    crate::modding::player_physics::filter_joints(skater, &mut joints);
    let mut drives = skater.skeleton_drives.build(
        skater.skeleton.bodies(),
        ATTACHED_REACTION_BASE,
        ATTACHED_REACTION_BASE + PART_COUNT,
        dt,
    );
    crate::modding::player_physics::filter_drives(skater,&mut drives);
    let skeleton_drive_count = drives.rows.len();
    //82D74FD8: persistent hand drives share the deck and skeleton reactions.
    skater.board_possession.append_drives(
        physics.board.bodies()[BodyId::Deck.index()],
        [skater.skeleton.bodies()[3], skater.skeleton.bodies()[7]],
        BodyId::Deck.index(),
        [ATTACHED_REACTION_BASE + 3, ATTACHED_REACTION_BASE + 7],
        dt,
        &mut drives.rows,
    );
    crate::modding::player_physics::filter_possession(skater, &mut drives.rows, skeleton_drive_count);
    let bodies = skater
        .skeleton
        .bodies_mut()
        .iter_mut()
        .chain(skater.skeleton_drives.targets.bodies.iter_mut())
        .chain(physics.network_proxies.bodies.iter_mut())
        .collect();
    physics.board.advance_attached(
        &contacts,
        truck_targets,
        physics.settings.step,
        AttachedStep {
            bodies,
            contacts: &mut [],
            joints: &mut joints,
            drives: &mut drives.rows,
        },
    );
    skater.mod_contact_frame = crate::modding::player_physics::after_solve(physics,skater,&mod_before);
    skater.mod_contact_frame.joint_loads=crate::modding::player_physics::joint_loads(skater,&joints,dt);
    physics.network_proxies.capture_dynamics_reactions(physics.board.solved_reactions(), dt);
    if let Err(error) = diagnostics::validate(
        &diagnostics::snapshot(physics, skater), "after shared solve",
    ) {
        return Err(format!("{error}; input_bodies={before:?}; contacts={contacts:?}; joints={joints:?}; drives={:?}", drives.rows));
    }
    skater
        .skeleton
        .publish_physical_record(deck_frame(&physics.board));
    // These solved rows are consumed by the actual collision/drive feedback
    // phase; keep their identity and impulses after the shared solve.
    // Possession drives have no skeleton spy identity. They participate in the
    // same solve above, but must not enter the skeleton-only feedback batch.
    drives.rows.truncate(skeleton_drive_count);
    skater.solved_drives = Some(drives);
    Ok(())
}

pub(super) fn deck_frame(
    board: &skate_core::physics::board_runtime::BoardRuntime,
) -> AnimationPartTransform {
    let deck = board.part_transforms()[BodyId::Deck.index()];
    let mut frame = IDENTITY;
    for (axis, column) in deck.basis.columns.iter().enumerate() {
        frame[axis][..3].copy_from_slice(column);
    }
    frame[3] = [
        deck.translation.x,
        deck.translation.y,
        deck.translation.z,
        0.0,
    ];
    frame
}
