//! Concrete original Skate3 player input phase for the authored riding world.
mod dynamic_normal;
pub(crate) mod grind;
mod grind_output;
mod ground_position;
mod initial;
mod output_reset;
mod pre_input;
mod reset;
mod services;
use super::{ground_runtime::GroundRuntime, riding_outputs::RidingOutputs};
pub(crate) use output_reset::reset_outputs;
pub(crate) use services::{InputHostFrame, PlayerInputCallbacks};
use skate_core::{
    math::Vector3,
    physics::{
        board_dynamic_normal::{BoardDynamicNormal, DynamicNormalSettings},
        board_input_output::{BoardInputFrame, publish_board_input},
        board_runtime::BoardRuntime,
        board_toolkit::BoardToolkit,
        skeleton_animation_record::AnimationPartTransform,
    },
    player::input_phase::{
        self, AnimationInputPacket, PhysicalPlayerInput, PlayerInputState, ProcessedPhysicsInput,
    },
};
use skate_data::collections::Collections;
pub(crate) enum InputStage {
    ThroughTeleport,
    AfterTeleport(input_phase::InputContinuation),
}
pub(crate) struct PlayerInputRuntime {
    pub player: PlayerInputState,
    pub physical: PhysicalPlayerInput,
    pub processed: ProcessedPhysicsInput,
    pub toolkit: Option<BoardToolkit>,
    pub dynamic_normal: BoardDynamicNormal,
    normal_settings: DynamicNormalSettings,
    pub pre_input: pre_input::PreInputManager,
    pub grind: grind::GrindInputState,
    pending_teleport: Option<AnimationPartTransform>,
    pending_velocity: Option<[f32; 3]>,
    pub pending_grind: Option<grind::Pending>,
    pub grind_observation: Option<super::grind::ManagerObservation>,
}
impl PlayerInputRuntime {
    pub fn pending_teleport(&self) -> Option<AnimationPartTransform> {
        self.pending_teleport
    }

    pub fn request_teleport(&mut self, target: AnimationPartTransform) -> Result<(), String> {
        self.request_teleport_ex(target, None)
    }

    pub fn request_teleport_ex(
        &mut self,
        target: AnimationPartTransform,
        velocity: Option<[f32; 3]>,
    ) -> Result<(), String> {
        if self.pending_teleport.is_some() {
            return Err("A pending teleport must complete before replacement".into());
        }
        if let Some(v) = velocity {
            if v.iter().any(|x| !x.is_finite() || x.abs() > 200.0) {
                return Err("Teleport velocity out of range".into());
            }
        }
        self.pending_teleport = Some(target);
        self.pending_velocity = velocity;
        Ok(())
    }

    pub fn load(data: &Collections) -> Result<Self, String> {
        let mut physical = PhysicalPlayerInput::default();
        reset_outputs(&mut physical);
        let mut processed = ProcessedPhysicsInput::default();
        reset::reset_processed(&mut processed);
        Ok(Self {
            player: initial::player(data)?,
            physical,
            processed,
            toolkit: None,
            dynamic_normal: BoardDynamicNormal::new(),
            normal_settings: dynamic_normal::settings(data)?,
            pre_input: pre_input::PreInputManager::new(),
            grind: grind::GrindInputState::load(data)?,
            pending_teleport: None,
            pending_velocity: None,
            pending_grind: None,
            grind_observation: None,
        })
    }
    pub fn processed_snapshot(
        &self,
        tick: u64,
    ) -> skate_core::player::input_phase::ProcessedPhysicsSnapshot {
        skate_core::player::input_phase::ProcessedPhysicsSnapshot::new(tick, self.processed)
    }
    ///Run after the current board/contact solve, before ProcessOutput publishes.
    pub fn update_dynamic_normal(&mut self, riding: &RidingOutputs, gravity: Vector3) {
        self.dynamic_normal.update(
            &riding.ground,
            gravity,
            self.processed.scalar_2656,
            &self.normal_settings,
        );
    }
    ///Reset and all other component publications are coordinator-owned phases.
    pub fn publish_board(&mut self, riding: &RidingOutputs) -> Result<(), String> {
        let t = self
            .toolkit
            .as_ref()
            .ok_or("Board output requires the current input toolkit")?;
        publish_board_input(
            &mut self.physical,
            &riding.motion,
            &riding.ground,
            BoardInputFrame {
                processed_forward: xyz(t.deck[2]),
                reckoning_normal_1216: riding.reckoning.ground_normal,
                reckoning_ground_up: xyz(riding.reckoning_frames.ground[1]),
                retained_ground_normal_112: self.dynamic_normal.normal,
                processed_flags_2476: self.processed.flags_2476,
            },
        );
        // Board Fill82C03318 and common ProcessOutput82DB7598.
        let normal = riding.ground.wheel_normal;
        self.physical.ground.vector_80 = [normal.x, normal.y, normal.z, 0.0].map(f32::to_bits);
        self.physical.ground.flag_273 = u8::from(self.processed.flags_2468 & 0x0010_0000 != 0);
        Ok(())
    }
    /// Water (surface type 12) contact for Collision+3481/+28, which reach the
    /// wipeout special-surface path as Processed2488 bit30 and Processed2924.
    /// Project choice, see docs/hails-additions/09-water.md: the retail writer
    /// is unconfirmed. The skater body's contact (SkeletonCollision4081/height,
    /// the same layout as 4079/4080 -> Collision3479/3480) wins over the
    /// board's (Body872 bit25 / Body864).
    pub fn publish_water(
        &mut self,
        board: &skate_core::physics::board_ground::BoardGroundState,
        body: &skate_core::physics::skeleton_body::SkeletonCollisionFeedback,
    ) {
        publish_water(
            &mut self.physical,
            body.flags.material_12.then_some(body.material_12_height),
            (board.collision_flags & (1 << 25) != 0).then_some(board.surface_twelve_height),
        );
    }
    pub fn process_stage<C: PlayerInputCallbacks>(
        &mut self,
        board: &mut BoardRuntime,
        ground: &mut GroundRuntime,
        ground_frame: AnimationPartTransform,
        packet: &AnimationInputPacket<'_>,
        host: InputHostFrame,
        callbacks: &mut C,
        stage: InputStage,
        world: &skate_core::physics::board_world::BoardWorld,
        world_edges: &crate::grind_world::StaticProvider,
    ) -> Result<Option<input_phase::InputContinuation>, String> {
        let mut service = services::Services {
            board,
            ground,
            ground_frame,
            host,
            callbacks,
            pre_input: &mut self.pre_input,
            grind: &mut self.grind,
            world,
            world_edges,
            pending_grind: &mut self.pending_grind,
            toolkit: &mut self.toolkit,
            pending_teleport: &mut self.pending_teleport,
            pending_velocity: &mut self.pending_velocity,
        };
        match stage {
            InputStage::ThroughTeleport => input_phase::start_input(
                &mut self.player,
                &mut self.physical,
                packet,
                &mut self.processed,
                &mut service,
            )
            .map(Some),
            InputStage::AfterTeleport(continuation) => input_phase::finish_input(
                continuation,
                &mut self.player,
                &mut self.physical,
                packet,
                &mut self.processed,
                &mut service,
            )
            .map(|()| None),
        }
        .map_err(|e| format!("Player input: {e:?}"))
    }
}
fn xyz(v: [f32; 4]) -> Vector3 {
    Vector3::new(v[0], v[1], v[2])
}

/// The packet reset leaves both fields clear when neither owner touched water.
fn publish_water(out: &mut PhysicalPlayerInput, body: Option<f32>, board: Option<f32>) {
    if let Some(height) = body.or(board) {
        out.collision.flag_3481 = 1;
        out.collision.scalar_28 = height;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn water_contact_prefers_the_skater_body_height() {
        let mut out = PhysicalPlayerInput::default();
        publish_water(&mut out, None, None);
        assert_eq!((out.collision.flag_3481, out.collision.scalar_28), (0, 0.0));
        publish_water(&mut out, None, Some(217.87));
        assert_eq!((out.collision.flag_3481, out.collision.scalar_28), (1, 217.87));
        publish_water(&mut out, Some(10.9), Some(217.87));
        assert_eq!((out.collision.flag_3481, out.collision.scalar_28), (1, 10.9));
    }
    #[test]
    #[ignore = "requires private stock collections"]
    fn original_player_input_settings_and_reset_load() {
        let assets = std::path::PathBuf::from(
            std::env::var_os("SKATE3_ASSET_ROOT").expect("SKATE3_ASSET_ROOT"),
        );
        let data = Collections::load(&assets).unwrap();
        let state = PlayerInputRuntime::load(&data).unwrap();
        assert_eq!(state.player.flags_1296, 0xe00c0000);
        assert!(state.pending_teleport().is_none());
        assert_eq!(state.physical.ground.scalar_276, -1.0);
        assert_eq!(state.dynamic_normal.normal, Vector3::new(0.0, 1.0, 0.0));
    }
}
