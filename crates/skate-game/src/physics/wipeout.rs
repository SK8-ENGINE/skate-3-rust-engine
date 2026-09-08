//! Shared Ground/Air wipeout requests from original TU3 82D8F9E0/82D90358.
mod observations;
mod settings;
pub(crate) use observations::Observations;
use skate_core::player::{
    input_phase::ProcessedPhysicsInput,
    wipeout::{self, Mode, Requests, Settings},
};
use skate_data::collections::Collections;
pub(crate) struct Wipeout {
    pub state: Requests,
    settings: Settings,
    offboard: skate_core::player::offboard::ground_lifecycle::CollisionSettings,
    offboard_air: skate_core::player::offboard::air_collision::Settings,
    modes: [Mode; 5],
}
impl Wipeout {
    pub fn load(data: &Collections) -> Result<Self, String> {
        let (settings, modes) = settings::load(data)?;
        let mut state = Requests::new();
        state.initialize_player(); //Player82DB3024, after the component ctor.
        Ok(Self {
            state,
            settings,
            offboard: settings::offboard(data)?,
            offboard_air: settings::offboard_air(data)?,
            modes,
        })
    }
    pub fn check_ground(&mut self, input: &Observations<'_>) -> Result<(), String> {
        let mode = self.mode(input.processed)?;
        let frame = input.frame()?;
        wipeout::check_ground(&mut self.state, &self.settings, &mode, &frame);
        Ok(())
    }
    pub fn check_ground_animation(
        &mut self,
        input: &Observations<'_>,
        scale: f32,
    ) -> Result<(), String> {
        let mode = self.mode(input.processed)?;
        let frame = input.frame()?;
        wipeout::check_ground_animation(&mut self.state, &self.settings, &mode, &frame, scale);
        Ok(())
    }
    pub fn check_air(&mut self, input: &Observations<'_>, use_com: bool) -> Result<(), String> {
        let mode = self.mode(input.processed)?;
        let frame = input.frame()?;
        wipeout::check_air(&mut self.state, &self.settings, &mode, &frame, use_com);
        Ok(())
    }
    fn mode(&self, p: &ProcessedPhysicsInput) -> Result<Mode, String> {
        self.modes
            .get(p.state_variant_index_2528 as usize)
            .copied()
            .ok_or_else(|| {
                format!(
                    "Undefined wipeout physics mode {}",
                    p.state_variant_index_2528
                )
            })
    }
    ///Consumed by state selection and by the same tick's postphysics IK branch.
    pub fn requests_runout(&self, p: &ProcessedPhysicsInput) -> bool {
        self.state.requests_runout(&observations::request_input(p))
    }
    pub fn requests_wipeout(&self, p: &ProcessedPhysicsInput) -> bool {
        self.state.requests_wipeout(&observations::request_input(p))
    }
}

///Player postphysics82BD83E0 follows contact/error publication with the
///selected state's check, before FootIK sees that tick's wipeout request.
pub(super) fn check_after_physics(
    physics: &super::GamePhysics,
    skater: &mut super::SkaterRuntime,
) -> Result<(), String> {
    use skate_core::player::state::PhysicalStateId;
    let observations = Observations {
        processed: &skater.player_input.processed,
        board: &physics.riding.ground,
        collision: &skater.collision_feedback,
        deck: super::solve::deck_frame(&physics.board),
        //Ground82BDF694..6A4 stores this target into Processed0..48;
        //Animated/KnownAir publish the same unblended target before board drive.
        input_board: skater.animated_skeleton.board_frames.animation_target,
        world_to_animation: skater.animated_skeleton.roots.world_to_animation,
        pose_error: skater.collision_pose_error,
        maximum_pose_error: skater.collision_maximum_error,
        jump_fix_frames: skater.player_state.post.jump_fix_frames,
        air: &skater.air_reckoning.state,
        system_up_y: physics.riding.reckoning_frames.system[1][1],
        grind_locked_to_middle: skater.trajectory.selector.grind_locked_to_middle(),
        grind_normal: skater.trajectory.selector.grind_normal(),
    };
    match skater.player_state.current() {
        PhysicalStateId::FootPlant | PhysicalStateId::HandPlant => {
            wipeout::check_plant(&mut skater.wipeout.state, &skater.wipeout.settings, &observations.frame()?);
            if skater.player_state.current() == PhysicalStateId::FootPlant {
                super::footplant::ground::post_physics(skater);
            }
            Ok(())
        }
        PhysicalStateId::BipedAir => {
            let mode = skater.wipeout.mode(&skater.player_input.processed)?;
            let frame = observations.frame()?;
            let p = &skater.player_input.processed;
            skate_core::player::offboard::air_collision::post(
                &skater.offboard.air_state,
                &mut skater.wipeout.state,
                &skater.wipeout.offboard_air,
                &skate_core::player::offboard::air_collision::PostInput {
                    shared: &frame,
                    root_velocity: skater.skeleton_input.root_velocity,
                    check_squash: mode.check_squash,
                    air_settings: &skater.wipeout.settings.air,
                    elapsed: p.state_timer_2664,
                    flags_2484: p.flags_2484,
                    forward: p.effective_anim_transform_192[2].map(f32::from_bits),
                    right: p.effective_anim_transform_192[0].map(f32::from_bits),
                    right_stick: [
                        skater.animation_input.extra.look_x,
                        skater.animation_input.extra.look_y,
                    ],
                },
            );
            super::offboard::air_diagnostics::record(skater);
            Ok(())
        }
        PhysicalStateId::PhysicsGround | PhysicalStateId::SlideGround => {
            skater.wipeout.check_ground(&observations)
        }
        PhysicalStateId::GroundAnimation => {
            skater.wipeout.check_ground_animation(&observations, 1.0)
        }
        PhysicalStateId::BipedGround => {
            use skate_core::player::offboard::ground_lifecycle::{
                CollisionInput, PostInput, post_physics,
            };
            let frame = observations.frame()?;
            post_physics(
                &mut skater.offboard.ground,
                &mut skater.wipeout.state,
                &skater.wipeout.offboard,
                &PostInput {
                    collision: CollisionInput {
                        shared: &frame,
                        skeleton_velocity_16336: skater.skeleton_input.root_velocity,
                        contact_flag_4072: skater.collision_feedback.flags.group_8,
                        contact_force_4056: skater.collision_feedback.maximum_group_8_force,
                    },
                    processed_flags_2484: skater.player_input.processed.flags_2484,
                    processed_velocity_608: skater.player_input.processed.vectors_544_560_592_608
                        [3]
                    .map(f32::from_bits),
                    skeleton_displacement_16288: skater.collision_extra_displacements[0],
                    skeleton_displacement_16304: skater.collision_extra_displacements[1],
                    ground_kind_356: skater.offboard.retained_contact.kind_164,
                },
            );
            super::offboard::air_diagnostics::record(skater);
            Ok(())
        }
        PhysicalStateId::PhysicsAir => skater.wipeout.check_air(&observations, false),
        _ => Ok(()), //Other concrete states dispatch their own postphysics check.
    }
}
