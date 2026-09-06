//! Original PostInput82DB5588 with the live ballistic trajectory selector.
//! The authored world has no grind edges or grab-spline descriptors.
use super::*;
use skate_core::player::post_input::*;

pub(crate) struct PostInputState {
    pub jump_reference: [u32; 4],
    pub jump_fix_frames: u32,
    pub latch_frames: u32,
    pub state_frames: u32,
    pub heading_adjust: f32,
    pub complete: bool,
    pub trajectory_pending: bool,
    pub trajectory_valid: bool,
    pub trajectory_available: bool,
    pub trajectory_new_candidate: bool,
    pub candidates: CandidatePublicationFields,
    pub grind: EmptyEdgePost,
}
impl PostInputState {
    pub fn new() -> Self {
        Self {
            //Original Player ctor82DB1AAC=1000,1ACC=0,3028 clears1264.
            jump_reference: [0; 4],
            jump_fix_frames: 1000,
            latch_frames: 0,
            state_frames: 100,
            heading_adjust: 0.0,
            complete: false,
            trajectory_pending: false,
            trajectory_valid: false,
            trajectory_available: false,
            trajectory_new_candidate: false,
            candidates: CandidatePublicationFields {
                first_object_present_196: false,
                first_pending_288: false,
                second_object_present_500: false,
                second_pending_592: false,
                staged_word_12768: 0,
                staged_valid_12772: 0,
                staged_latched_12776: 0,
                staged_pending_12780: false,
            },
            grind: EmptyEdgePost::new(),
        }
    }
}
///Scorer/decision state that still advances in the genuine no-candidate path
///82D8AB08 ->82D86808/82D86C88/82D86DE8/82D89F58/82D739D8.
pub(crate) struct EmptyEdgePost {
    free_frames: u32,
    duration: u32,
    engage_state: u32,
    expiry: f32,
    pop_frames: u32,
    lean: [f32; 2],
    reset_frames: u32,
    previous_grind: u32,
    blend: f32,
}
impl EmptyEdgePost {
    fn new() -> Self {
        Self {
            free_frames: 0,
            duration: 0,
            engage_state: 0,
            expiry: 0.0,
            pop_frames: 0,
            lean: [0.0; 2],
            reset_frames: 0,
            previous_grind: 0,
            blend: 0.0,
        }
    }
    fn advance(&mut self, p: &mut skate_core::player::input_phase::ProcessedPhysicsInput) {
        //No candidate means82D8ACF0 skips wallassist,82D86318 skips geometry.
        self.expiry = (self.expiry - p.timestep_2604).max(0.0);
        self.free_frames = self.free_frames.wrapping_add(1);
        if self.free_frames > 20 {
            self.duration = 0;
            self.engage_state = 2;
        }
        self.pop_frames = decrement(self.pop_frames);
        for value in &mut self.lean {
            *value *= f32::from_bits(0x3f68f5c3);
            if value.abs() < 0.0001 {
                *value = 0.0;
            }
        }
        self.reset_frames = decrement(self.reset_frames);
        if self.reset_frames > 0 {
            p.flags_2476 |= 0x02000000;
        }
        if p.grind_words_2532_2536[0] != u32::MAX {
            self.previous_grind = p.grind_words_2532_2536[0];
        }
        self.blend = (self.blend + 0.0035).min(1.0);
    }
}
fn decrement(v: u32) -> u32 {
    let n = v.wrapping_sub(1);
    if n & 0x80000000 != 0 { 0 } else { n }
}
struct Services<'a> {
    heading: f32,
    grind: &'a mut EmptyEdgePost,
    processed: &'a mut skate_core::player::input_phase::ProcessedPhysicsInput,
    trajectory: &'a mut super::super::air_trajectory::AirTrajectoryRuntime,
    trajectory_input: skate_core::air::trajectory::SelectorInput,
    world: &'a skate_core::physics::board_world::BoardWorld,
    error: Option<String>,
}
impl PostInputServices for Services<'_> {
    fn update_grind_manager_82d8ab08(&mut self) {
        self.grind.advance(self.processed);
    }
    fn update_trajectory_selector_82d68800(&mut self) -> u8 {
        //82DB56B0 runs the real selector after GrindManager. That manager
        //can change2476, so use its current output rather than a prior snapshot.
        let mut input = self.trajectory_input;
        input.flags_2476 = self.processed.flags_2476;
        match self.trajectory.update(input, self.world) {
            Ok(valid) => u8::from(valid),
            Err(error) => {
                self.error = Some(error);
                0
            }
        }
    }
    fn calculate_scalar_2740_82db5e10(&mut self) -> f32 {
        self.heading
    }
    fn register_candidate_82762ab0(&mut self, _: CandidateRegistration) {
        //82762AB0 copies a grab-spline record; the inherited trait name does
        //not imply engine registration or ballistic trajectory publication.
        unreachable!(
            "grab-spline publication requires an authored descriptor checked before this phase"
        )
    }
}
pub(super) fn advance(physics: &mut GamePhysics, skater: &mut SkaterRuntime) -> Result<(), String> {
    let trajectory_input = super::super::air_phase::selector_input(physics, skater)?;
    let post = &mut skater.player_state.post;
    let p = &mut skater.player_input.processed;
    if skater.ground_lifecycle.edge.is_some() || p.category_2512 == 400 || p.state_2508 == 701 {
        return Err("PostInput active grind requires its actual candidate/scorer output".into());
    }
    let requests = &skater.ground_lifecycle.trajectory;
    if requests.primary_valid_288
        || requests.secondary_valid_592
        || post.candidates.staged_pending_12780
    {
        return Err(
            "PostInput grab-spline publication requires its retained authored descriptor".into(),
        );
    }
    let mut player = PostInputPlayerFields {
        jump_reference_1264: post.jump_reference,
        flags_1296: skater.player_input.player.flags_1296,
        state_frames_1304: skater.player_input.player.ground_history_frames_1304 as u32,
        jump_fix_frames_1308: post.jump_fix_frames,
        latch_frames_1320: post.latch_frames,
    };
    let mut processed = PostInputProcessedFields {
        jump_reference_848: post.jump_reference,
        word_2464: 0,
        flags_2468: p.flags_2468,
        flags_2472: p.flags_2472,
        flags_2480: p.flags_2480,
        flags_2484: p.flags_2484,
        current_state_2508: p.state_2508,
        state_frames_2572: post.state_frames,
        jump_fix_frames_2576: post.jump_fix_frames,
        scalar_2740: post.heading_adjust,
    };
    let mut output = PostInputPhysOutFields {
        reset_state_frames_316: skater
            .player_state
            .ground_output
            .as_ref()
            .is_some_and(|g| g.ground_32.wall_ride_exit),
        //82DB5588 consumes the preceding state's actual jump publication.
        capture_jump_reference_442: skater.player_input.physical.air.launched_442 != 0,
        jump_reference_128: skater.player_input.physical.air.launch_velocity_128,
        complete_76: post.complete,
    };
    let heading = physics.riding.update_input_heading(
        f32::from_bits(p.vectors_464_480_496_512_528[0][1]),
        p.scalar_2612,
    );
    let mut services = Services {
        heading,
        grind: &mut post.grind,
        processed: p,
        trajectory: &mut skater.trajectory,
        trajectory_input,
        world: &physics.world,
        error: None,
    };
    run_post_input(
        PostInputContext {
            player: &mut player,
            processed: &mut processed,
            phys_out: &mut output,
            candidates: &mut post.candidates,
        },
        &mut services,
    );
    if let Some(error) = services.error.take() {
        return Err(error);
    }
    drop(services);
    //These are observations of the actual owner, never inputs to selection.
    post.trajectory_pending = skater.trajectory.selector.pending();
    post.trajectory_valid = skater.trajectory.selector.valid();
    post.trajectory_available = skater.trajectory.selector.valid();
    post.trajectory_new_candidate = skater.trajectory.selector.just_changed();
    skater.player_input.player.flags_1296 = player.flags_1296;
    skater.player_input.player.ground_history_frames_1304 = player.state_frames_1304 as i32;
    p.flags_2468 = processed.flags_2468;
    p.flags_2472 = processed.flags_2472;
    p.flags_2480 = processed.flags_2480;
    p.flags_2484 = processed.flags_2484;
    post.jump_reference = player.jump_reference_1264;
    post.jump_fix_frames = player.jump_fix_frames_1308;
    post.latch_frames = player.latch_frames_1320;
    post.state_frames = processed.state_frames_2572;
    post.heading_adjust = processed.scalar_2740;
    post.complete = output.complete_76;
    Ok(())
}
