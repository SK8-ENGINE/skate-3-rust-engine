//! Physical-output publications consumed by gameplay camera82DF69C0.
//! Reset82DE53F0 runs before the actual producers82DB6EC0/82D3A388.
use super::{GamePhysics, SkaterRuntime};
use crate::camera::{
    CameraAirOutput, CameraAnimationOutput, CameraEventsOutput,
    CameraOffboardOutput, CameraPreferences, CameraPublicationInputs, CameraStateOutput,
};
use skate_core::{
    animation::physical_feedback::PhysicalFeedback,
    camera::{MovingObstacleProvider, PathObstacle},
    physics::centre_of_mass_filter::CentreOfMassOutput,
};

/// The actual game camera call, after completed physical output conditioning.
/// The user's selected normal High camera is graph type1. This custom world
/// has no road/ledge/camera-volume annotations or other moving actors.
pub(crate) fn advance(
    physics: &mut GamePhysics,
    skater: &SkaterRuntime,
    feedback: &PhysicalFeedback,
    camera: &mut crate::camera::CameraRuntime,
) -> Result<(), String> {
    //Once per physical publication, preserving conditioner history across
    //direct grind-type changes and invalidating it on the first non-grind tick.
    physics.grind.advance_camera(
        skater.player_input.processed.vectors_400_416[0].map(f32::from_bits),
    )?;
    let inputs = publish(
        physics,
        skater,
        feedback,
        skater.centre_of_mass_output,
        CameraPreferences {
            // User preferences are owned here: ordinary uninverted controls.
            invert_look: [false; 2],
            // Native preferences constructor82DF6294/629C initializes both
            // fields to zero.82DF8E80 only updates its air-side bytes37..39.
            shake_variant: 0,
            value_32: 0.0,
        },
        // SkaterAnim vtable8231E170+28=82B97140: full15180 bit30.
        u8::from(skater.animation.stance().1),
        1, // Stable host player identity replaces the original actor pointer.
    )?;
    let snapshot = crate::camera::publish_camera_subject(physics, skater, &inputs)?;
    let environment = crate::camera::CameraGraphEnvironment {
        camera_type: 1,
        on_road: false,
        ledge_left: false,
        ledge_right: false,
        volumes: Vec::new(),
    };
    let gravity = physics.settings.step.simulation.gravity_acceleration;
    camera
        .advance(
            physics.settings.step.simulation.time_step,
            snapshot,
            physics.world(),
            [gravity.x, gravity.y, gravity.z, 0.0],
            &environment,
            &mut StaticWorld,
        )
        .map(|_| ())
}

struct StaticWorld;
impl MovingObstacleProvider for StaticWorld {
    fn collect(&mut self, _: [f32; 4], _: [f32; 4], _: f32, _: &mut [PathObstacle; 50]) -> usize {
        // The subject actor is excluded by the native provider. Our current
        // world contains this subject and static triangles, with no other actors.
        0
    }
}

/// Call after current state, board/Skeleton and animation conditioning have
/// published, before clearing AnimationControlOutput's per-frame intent bits.
///Selected state owners publish their output before this common consumer.
pub(crate) fn publish(
    physics: &GamePhysics,
    skater: &SkaterRuntime,
    feedback: &PhysicalFeedback,
    com: CentreOfMassOutput,
    preferences: CameraPreferences,
    skater_animation_stance: u8,
    context: u32,
) -> Result<CameraPublicationInputs, String> {
    let p = &skater.player_input.processed;
    let physical = &skater.player_input.physical;
    let fields = &skater.animation_input.fields;
    let intents = skater.animation_input.output.flags;
    let packet = &skater.animation.packet;
    let normal = physics.riding.ground.wheel_normal;
    Ok(CameraPublicationInputs {
        state: CameraStateOutput {
            height_32: physical.state.surface_height_32,
            physically_pushing_55: bit(p.flags_2468, 25),
            wiping_out_59: bit(p.flags_2468, 18),
            manual_60: u8::from(fields.balance != 0.0),
            reset_62: bit(p.flags_2472, 10),
            use_skeleton_root_75: u8::from(physical.state.category_12 == 500),
            flag_79: u8::from(p.state_variant_index_2528 == 3),
            flag_81: u8::from(skater.player_state.state_flags[81 - 52]),
        },
        animation: CameraAnimationOutput {
            conditioned_turn: feedback.conditioned_turn,
            input_turn_64: fields.turn,
            input_kickturn_68: p.spin_input_2672,
            time_since_input_128: p.time_since_last_input_2748,
            wipeout_tweak_148: physical.animation.profile_148,
            stance_155: u8::from(packet.riding_fakie),
            running_out_160: bit(p.flags_2480, 2),
            skater_animation_stance,
        },
        //Consume current output, including KnownAir82D36880's predicted
        //landing/apex. Other states retain their actual template fields.
        air: CameraAirOutput {
            apex_0: physical.air.trajectory_apex_0.map(f32::from_bits),
            landing_position_16: physical.air.collision_position_16.map(f32::from_bits),
            landing_normal_32: physical.air.landing_normal_32.map(f32::from_bits),
            launch_position_48: physical.air.selector_vector_48.map(f32::from_bits),
            heading_80: physical.air.landing_heading_80.map(f32::from_bits),
            time_176: physical.air.time_in_state_176,
            duration_180: physical.air.collision_time_180,
            apex_time_196: physical.air.time_to_apex_196,
            flag_440: bit(p.flags_2468, 22),
        },
        //Offboard82DE4010 has a distinct board-present327=1. None of that
        //record's alternate trajectory fields becomes valid while riding.
        offboard: CameraOffboardOutput {
            duration_92: 0.0,
            time_152: 0.0,
            apex_time_156: 0.0,
            launch_normal_160: [0.0; 4],
            launch_position_176: [0.0; 4],
            landing_normal_192: [0.0; 4],
            landing_position_208: [0.0; 4],
            heading_224: [0.0; 4],
            apex_240: [0.0; 4],
            //82DB76E0 overwrites the Ground Fill's earlier304 publication.
            object_held_304: bit(p.flags_2480, 19),
            hurdle_317: physical.off_board.hippy_hurdling_317,
            use_trajectory_331: 0,
            dropping_in_334: 0,
        },
        //Shared grind Fill82D40EA0 publishes the directed rail tangent;
        //conditioner82DF0640 supplies the continuous contact anchor.
        grinds: physics.grind.camera_output(),
        events: CameraEventsOutput {
            intent_51: bit(intents, 28),
            preparing_52: bit(intents, 27),
            dropping_in_63: bit(intents, 22),
            trick_125: bit(p.flags_2480, 11),
            //Ground322 resets in82DE3728. Air/trick states own active writes.
            hippy_jump_322: 0,
            //Scoring2 reset82DE4468; HoM duration stays inactive in this host.
            broken_bone_duration_200: 0.0,
            //AirCollector82DA78D0 publishes the world conditioner capability mask.
            capabilities_204: physical.scoring.capabilities_204,
        },
        damped_com_80: com.position,
        //82C03304..3340 copies actual BoardBody80 into Ground80 and96.
        ground_up_80: [normal.x, normal.y, normal.z, 0.0],
        //Ground288 resets82DE3728; selected ordinary Ground Fill leaves it.
        ground_scalar_288: 0.0,
        look_552_556: [
            skater.animation_input.extra.look_x,
            skater.animation_input.extra.look_y,
        ],
        collision_look_target_64: physical.collision.predicted_position_64.map(f32::from_bits),
        preferences,
        context,
    })
}

fn bit(value: u32, shift: u32) -> u8 {
    ((value >> shift) & 1) as u8
}

#[cfg(test)]
#[path = "camera_tricks.rs"]
mod tests;
