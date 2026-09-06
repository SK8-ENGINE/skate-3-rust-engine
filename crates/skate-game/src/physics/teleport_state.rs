//! Host checkpoint response to original actor reset callback82592518.
//! Shared scheduling preserves request -> next input reply ->702 output -> reset.
use skate_core::{
    animation::output::actor_packet::ExternalReset,
    physics::skeleton_animation_record::AnimationPartTransform,
    player::{
        input_phase::{PhysicalPlayerInput, ProcessedPhysicsInput},
        teleport_state::{Output, Target, TeleportState, Update},
    },
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Checkpoint {
    pub transform: AnimationPartTransform,
    /// Original actor82592518 inverts checkpoint byte68. Its ground validator
    ///82BFBC18 sets that byte for surface category8. Authored rideable spawn
    ///surfaces supply true; an off-board checkpoint must explicitly supply false.
    pub on_board: bool,
}

pub(crate) struct Runtime {
    state: TeleportState,
    checkpoint: Checkpoint,
    pending_reply: Option<Target>,
}
impl Runtime {
    pub fn new(checkpoint: Checkpoint) -> Self {
        Self {
            state: TeleportState::default(),
            checkpoint,
            pending_reply: None,
        }
    }
    pub fn enter(&mut self) {
        self.state.enter();
    }
    /// Update82D431F0 calls the actor only without Processed2468 bit1.
    ///825926F8 stores the reply and marks it for the next input publication.
    pub fn update(&mut self, input: &ProcessedPhysicsInput) {
        if self
            .state
            .update(input.flags_2468, input.matrix_1536, input.byte_1600)
            == Update::RequestCheckpoint
        {
            self.request_checkpoint();
        }
    }
    /// Host response for an independently verified actor-reset request.
    pub fn request_checkpoint(&mut self) {
        self.pending_reply = Some(Target {
            transform: self.checkpoint.transform.map(|row| row.map(f32::to_bits)),
            on_board: self.checkpoint.on_board,
        });
    }
    /// Consume at the next actor-reset publication boundary. Publish this as
    ///ExternalReset plus Actor1904 bit29, then use the ordinary animation-packet
    ///mapper to produce1536/1600 and2468 bit1. Never reset physical bodies here.
    pub fn take_reply(&mut self) -> Option<ExternalReset> {
        self.pending_reply.take().map(|reply| ExternalReset {
            transform: reply.transform,
            byte64: u8::from(reply.on_board),
        })
    }
    /// Call after ordinary physical publication so702 owns State8/61 and
    ///the returned board0 transform/board272 publication for this tick.
    pub fn publish_output(&self, physical: &mut PhysicalPlayerInput) -> Option<Output> {
        let output = self.state.output()?;
        physical.teleport_output = Some(output);
        physical.state.identifier_8 = output.next_state;
        physical.state.flag_61 = output.state_61;
        Some(output)
    }
}
