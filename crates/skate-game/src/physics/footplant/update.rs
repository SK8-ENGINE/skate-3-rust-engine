//! Original82D6FD60 ->82D6F5F0 ->82D6F910, invoked by82D36728.
use super::{Footplant, V, math::*};
use crate::physics::{
    air_trajectory::AirTrajectoryRuntime, animated_skeleton::AnimatedSkeleton, foot_ik::FootIk,
};
use skate_core::{
    air::{
        known::KnownAirFootplantInput,
        trajectory::{QueryRequest, Trajectory},
    },
    physics::{
        board_toolkit::BoardToolkit,
        board_world::BoardWorld,
        skeleton_body::{SkeletonBody, SkeletonCollisionMode},
    },
    player::input_phase::ProcessedPhysicsInput,
};
pub(crate) struct FootplantFrame<'a> {
    pub processed: &'a ProcessedPhysicsInput,
    pub toolkit: &'a BoardToolkit,
    pub animated: &'a AnimatedSkeleton,
    pub body: &'a SkeletonBody,
    pub gravity: V,
    pub world: &'a BoardWorld,
}
impl Footplant {
    pub fn update_candidate(&mut self, input: &KnownAirFootplantInput, frame: &FootplantFrame<'_>) {
        self.candidate = false;
        let offset = [0.0, self.settings.deck_bounds_y_offset, 0.0, 0.0];
        let upper = add(self.settings.deck_bounds, offset);
        let lower = sub(offset, self.settings.deck_bounds);
        let physical = frame.body.record.pose;
        let toes = [physical[15][3], physical[19][3]];
        let board = toes.map(|p| point(&frame.toolkit.inverse_effective, p));
        let outside = |p: V| (0..3).any(|i| lower[i] > p[i] || p[i] > upper[i]);
        self.physical_com = madd(
            input.trajectory.velocity,
            STEP,
            frame.animated.board_frames.centre_of_mass,
        );
        self.animation_com = madd(
            input.trajectory.velocity,
            STEP,
            point(
                &frame.animated.roots.animation_to_world,
                frame.animated.record.centre_of_mass,
            ),
        );
        let side = if outside(board[0]) {
            0
        } else if outside(board[1]) {
            1
        } else {
            return;
        };
        let toe = [15, 19][side];
        self.selected_toe = Some(toe);
        self.candidate = true;
        self.selected_world = point(
            &frame.animated.roots.animation_to_world,
            frame.animated.record.pose[toe][3],
        );
        self.selected_record = toes[side];
        self.leg_direction = normalize(sub(self.selected_record, self.physical_com));
    }
    pub fn update_launch(&mut self, input: &KnownAirFootplantInput) {
        use skate_core::air::reckoning::clamp_vector_within_max_angle as limit;
        if !self.hit {
            self.contact_time = input.remaining_collision_time;
        }
        let future = madd(
            input.trajectory.acceleration,
            self.contact_time,
            input.trajectory.velocity,
        );
        let candidate = sub(normalize(future), input.landing_normal);
        let from = scale(self.current_up, -1.0);
        let angle = f32::from_bits(0x420c0000) * f32::from_bits(0x3c8efa35);
        let direction = normalize(limit(candidate, from, angle));
        //82D6F7A8's second pure limiter result is discarded; v125 retains direction.
        let _ = limit(sub(self.adjusted_contact, self.physical_com), from, angle);
        self.launch_direction = normalize(direction);
        self.launch_valid = true;
        self.request = Trajectory {
            position: madd(
                self.launch_direction,
                self.settings.leg_length_on_landing,
                self.physical_com,
            ),
            velocity: input.trajectory.velocity,
            //830BD330 initializer82F826B0 uses822F8B40, independently read original bytes.
            acceleration: [0.0, f32::from_bits(0xc11ccccd), 0.0, 0.0],
            duration: -1.0,
        };
    }
    pub fn consume_and_submit(
        &mut self,
        input: &KnownAirFootplantInput,
        frame: &FootplantFrame<'_>,
        ik: &mut FootIk,
        collision: &mut SkeletonCollisionMode,
    ) -> Result<(), String> {
        self.hit = false;
        if self.enabled && self.result.contact_time >= 0.0 {
            self.contact_time = clamp01(self.result.contact_time);
            self.contact = self.result.contact_position;
            self.surface = self.result.surface;
            self.hit = true;
            //82D6FBB0's nearby-edge scan is empty in our explicitly edge-free BoardWorld.
            self.adjusted_contact = madd(
                input.landing_normal,
                self.settings.foot_volume_y_offset,
                self.contact,
            );
            if !self.contact_active {
                if self.candidate
                    && self.contact_time > f32::from_bits(0x3de147ae)
                    && self.contact_time < 0.3
                {
                    self.contact_active = true;
                    self.active_elapsed = 0.0;
                } else {
                    self.clear_contact();
                }
            } else if self.active_elapsed + self.contact_time < 0.0 {
                self.contact_active = false;
                self.clear_contact();
                self.active_elapsed = 0.0;
            }
            if self.contact_active {
                self.requested |= frame.processed.flags_2480 & (1 << 23) != 0;
                self.active_elapsed += STEP;
                self.update_pose(input, frame.animated, ik, collision);
            }
        }
        self.current_up = frame.processed.vectors_544_560_592_608[0].map(f32::from_bits);
        let next = AirTrajectoryRuntime::query(
            frame.world,
            QueryRequest {
                trajectory: Trajectory {
                    position: self.request.position,
                    velocity: self.request.velocity,
                    acceleration: frame.gravity,
                    duration: 1.0,
                },
                radius: f32::from_bits(0x3d23d70a),
                start_error: 1.0,
                end_error: 1.0,
            },
        )?;
        self.result = next;
        self.enabled = true;
        Ok(())
    }
    fn clear_contact(&mut self) {
        self.surface = 0;
        self.hit = false;
        self.contact_time = -1.0;
        self.contact = [0.0; 4];
    }
}
