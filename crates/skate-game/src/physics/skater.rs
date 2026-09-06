//! The game's single skater: stock graph animation and its physical skeleton.
//! Loading creates the real bodies from the animator's initial evaluated pose.
use super::{
    GamePhysics,
    animated_skeleton::AnimatedSkeleton,
    animation_feedback::AnimationFeedback,
    animation_input::AnimationInput,
    foot_ik::FootIk,
    foot_physical_output::FootPhysicalOutputs,
    ground_runtime::{GroundRuntime, GroundSettings, GroundState},
    player_input::{NoGrindEdges, PlayerInputRuntime},
    skeleton_body,
    skeleton_input_runtime::SkeletonInputRuntime,
    skeleton_output::SkeletonOutput,
};
use crate::{graph_runtime::StockGraphs, skater_animation::SkaterAnimation};
use bevy::prelude::Resource;
use skate_core::physics::{
    board::BodyId,
    skeleton_animation_record::{IDENTITY, map_animation_parts},
    skeleton_body::{
        SkeletonBody, SkeletonCollisionMode, SkeletonDriveBatch, SkeletonDrives, SkeletonJoints,
    },
};
use skate_data::collections::Collections;
use std::path::Path;

#[derive(Resource)]
pub(crate) struct SkaterRuntime {
    /// Completed physical pose in native animation space, read by rendering.
    pub render_pose: Vec<skate_core::animation::output::NativeMatrix>,
    pub pose_generation: u64,
    pub centre_of_mass_filter: skate_core::physics::centre_of_mass_filter::CentreOfMassFilter,
    pub centre_of_mass_output: skate_core::physics::centre_of_mass_filter::CentreOfMassOutput,
    pub animation: SkaterAnimation,
    pub animated_skeleton: AnimatedSkeleton,
    pub skeleton_air: super::skeleton_air::SkeletonAir,
    pub air_reckoning: super::air_reckoning::AirReckoning,
    pub air_state: skate_core::air::state::PhysicsAirState,
    pub air_settings: super::air_phase::AirSettings,
    pub known_air: super::known_air::KnownAir,
    pub ground_animation: super::ground_animation::GroundAnimationRuntime,
    pub ground_animation_settings: super::ground_animation::GroundAnimationSettings,
    pub slide_state: super::slide_state::SlideState,
    pub trajectory: super::air_trajectory::AirTrajectoryRuntime,
    pub footplant: super::footplant::Footplant,
    pub wipeout: super::wipeout::Wipeout,
    pub wipeout_state: super::wipeout_states::WipeoutState,
    pub teleport_state: super::teleport_state::Runtime,
    pub offboard: super::offboard::runtime::Runtime,
    pub skeleton: SkeletonBody,
    pub skeleton_joints: SkeletonJoints,
    pub skeleton_drives: SkeletonDrives,
    pub skeleton_collision: SkeletonCollisionMode,
    pub collision_feedback: skate_core::physics::skeleton_body::SkeletonCollisionFeedback,
    pub pose_errors: skate_core::physics::skeleton_body::SkeletonPoseErrors,
    pub collision_pose_error: [f32; 4],
    ///Skeleton16288/16304, published by the completed collision response.
    pub collision_extra_displacements: [[f32; 4]; 2],
    ///Skeleton16384 is published by the completed pose-error response. Wipeout
    ///runs after that publication; no fabricated pre-solve measurement exists.
    pub collision_maximum_error: Option<f32>,
    pub solved_drives: Option<SkeletonDriveBatch>,
    pub foot_ik: FootIk,
    pub foot_physical: FootPhysicalOutputs,
    pub player_input: PlayerInputRuntime,
    pub player_state: super::player_state::PlayerState,
    pub skeleton_input: SkeletonInputRuntime,
    pub ground: GroundState,
    pub ground_runtime: GroundRuntime,
    pub ground_settings: std::sync::Arc<GroundSettings>,
    pub ground_profiles: super::ground_runtime::GroundProfiles,
    pub ground_lifecycle: super::ground_phase::GroundLifecycle,
    pub animation_input: AnimationInput,
    pub animation_feedback: AnimationFeedback,
    pub physical_feedback: skate_core::animation::physical_feedback::PhysicalFeedback,
    pub landing_quality: skate_core::animation::landing_quality::Output,
    pub landing_quality_settings: skate_core::animation::landing_quality::Settings,
    pub skeleton_output: SkeletonOutput,
    pub skateboard_controller: super::skateboard_controller::SkateboardController,
}

impl SkaterRuntime {
    pub fn load(
        asset_root: &Path,
        graphs: &StockGraphs,
        physics: &GamePhysics,
        mode: &str,
    ) -> Result<Self, String> {
        let data = Collections::load(asset_root)?;
        let ground_profiles = super::ground_runtime::GroundProfiles::load(&data)?;
        let mode_index = crate::difficulty::NATIVE_MODES.iter().position(|m| *m == mode)
            .ok_or_else(|| format!("Invalid skater mode {mode}"))? as u32;
        // The host's current character is a custom skater with no pro selector
        // or equipped physical hat. These are profile choices, not force values.
        let mut animation = SkaterAnimation::load(asset_root, &data, graphs, b"")?;
        let initial_hierarchy = animation.evaluate_initial_pose()?;
        let offboard = super::offboard::runtime::Runtime::load(&data, animation.motion.animation.metadata())?;
        let mut animated_skeleton =
            AnimatedSkeleton::load(asset_root, &data, &animation.evaluator.frames, false)?;
        let initial_parts = map_animation_parts(
            &initial_hierarchy,
            &animated_skeleton.bone_indices,
            &animated_skeleton.physics_frames,
        )?;
        let deck = physics.board.part_transforms()[BodyId::Deck.index()];
        let mut spawn = IDENTITY;
        for (column, axis) in deck.basis.columns.iter().enumerate() {
            spawn[column][..3].copy_from_slice(axis);
        }
        spawn[3] = [
            deck.translation.x,
            deck.translation.y,
            deck.translation.z,
            0.0,
        ];
        let mut skeleton = skeleton_body::load(
            asset_root,
            &data,
            &animation.evaluator.frames.source_sha256,
            &initial_parts,
            spawn,
            physics.settings.step.simulation,
            None,
        )?;
        animated_skeleton
            .roots
            .reset_initial_alignment(skeleton.animation_to_world);
        animated_skeleton.board_frames.reset(spawn);
        for _ in 0..2 {
            animated_skeleton.board_frames.publish_centre_of_mass(
                skeleton.record.centre_of_mass,
                physics.settings.step.simulation.time_step,
                0,
            );
        }
        animated_skeleton
            .board_frames
            .publish_local_observations(&animated_skeleton.roots, &spawn);
        let animation_input = AnimationInput::load(&data, &animation.evaluator.frames, mode)?;
        let skeleton_joints = skeleton_body::load_joints(
            asset_root,
            &data,
            &animation.evaluator.frames.source_sha256,
            &initial_hierarchy,
            &animation.evaluator.frames.parents,
            &animated_skeleton.bone_indices,
        )?;
        let mut skeleton_drives = skeleton_body::load_drives(
            asset_root,
            &data,
            &animation.evaluator.frames.source_sha256,
            &initial_hierarchy,
            &animated_skeleton.bone_indices,
            &initial_parts,
            &skeleton_joints,
            skeleton.animation_to_world,
            spawn,
            physics.settings.step.simulation,
        )?;
        // Reset82BD9990 finishes by publishing both COM targets to the
        // actual extra bodies. These bodies subsequently share the solver.
        let initial_targets = skeleton_drives.targets.update_extra_targets(
            &mut skeleton,
            &animated_skeleton.board_frames.com_frame,
            &animated_skeleton.board_frames.lifted_com_frame,
        );
        let foot_ik = FootIk::load(&data, &animation.evaluator.frames, &animated_skeleton)?;
        let skeleton_output =
            SkeletonOutput::load(&data, &animation.evaluator.frames, &animated_skeleton)?;
        // Actor82591448 passes network || ghost for self-pair suppression.
        // This local game has neither networking nor a ghost actor.
        let skeleton_collision = skeleton_body::load_collision(
            asset_root,
            &data,
            &animation.evaluator.frames.source_sha256,
            false,
        )?;
        let wipeout_state = super::wipeout_states::WipeoutState::load(
            &data, asset_root, &animation.evaluator.frames.source_sha256,
        )?;
        Ok(Self {
            offboard,
            render_pose: initial_hierarchy,
            pose_generation: 0,
            centre_of_mass_filter: Default::default(),
            // PhysOut reset82DE53F0 clears these observations before first output.
            centre_of_mass_output: skate_core::physics::centre_of_mass_filter::CentreOfMassOutput {
                velocity: [0.0; 4],
                acceleration: [0.0; 4],
                position: [0.0; 4],
            },
            animation,
            landing_quality: Default::default(),
            landing_quality_settings: super::landing_quality::load(&data)?,
            animated_skeleton,
            skeleton_air: super::skeleton_air::SkeletonAir::load(&data)?,
            air_reckoning: super::air_reckoning::AirReckoning::load(&data)?,
            air_state: Default::default(),
            air_settings: super::air_phase::AirSettings::load(&data)?,
            known_air: super::known_air::KnownAir::load(&data)?,
            ground_animation: Default::default(),
            ground_animation_settings: super::ground_animation::GroundAnimationSettings::load(&data)?,
            slide_state: super::slide_state::SlideState::load(&data)?,
            trajectory: super::air_trajectory::AirTrajectoryRuntime::load(&data)?,
            footplant: super::footplant::Footplant::load(&data)?,
            wipeout: super::wipeout::Wipeout::load(&data)?,
            wipeout_state,
            teleport_state: super::teleport_state::Runtime::new(super::teleport_state::Checkpoint {
                transform: spawn, on_board: true,
            }),
            skeleton,
            skeleton_joints,
            skeleton_drives,
            collision_feedback: skeleton_body::load_feedback(&data, skeleton_collision.settings)?,
            skeleton_collision,
            pose_errors: skate_core::physics::skeleton_body::SkeletonPoseErrors {
                targets: [
                    initial_targets.com,
                    initial_targets.lifted_com,
                    initial_targets.following_com,
                ],
                ..Default::default()
            },
            collision_pose_error: [0.0; 4],
            collision_extra_displacements: [[0.0; 4]; 2],
            collision_maximum_error: None,
            solved_drives: None,
            foot_ik,
            foot_physical: FootPhysicalOutputs::load(&data)?,
            player_input: PlayerInputRuntime::load(&data, NoGrindEdges)?,
            player_state: super::player_state::PlayerState::load(&data, mode)?,
            skeleton_input: SkeletonInputRuntime {
                drive_frames: initial_parts,
                extra_target_positions: [
                    initial_targets.com,
                    initial_targets.lifted_com,
                    initial_targets.following_com,
                ],
                ..Default::default()
            },
            ground: GroundState::load(&data, mode, true)?,
            ground_runtime: GroundRuntime::load(&data)?,
            ground_settings: ground_profiles.select(mode_index, 1)?,
            ground_profiles,
            ground_lifecycle: super::ground_phase::GroundLifecycle::new(),
            animation_input,
            animation_feedback: AnimationFeedback::load(&data)?,
            physical_feedback: super::animation_phase::initial_feedback(),
            skeleton_output,
            skateboard_controller: super::skateboard_controller::SkateboardController::new(),
        })
    }
}
