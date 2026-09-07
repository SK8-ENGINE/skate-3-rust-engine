//! PhysicalPlayer startup, PostInput, state selection and state publication.
//! Native control decisions use original TU3 functions and stock collections;
//! host ownership replaces the original player component pointers.
mod post_input;
mod pre_state;
mod publication;
mod selection;
mod transition;
mod wipeout_output;
use super::{GamePhysics, skater::SkaterRuntime};
use skate_core::{
    physics::filtered_state::{FilteredState, FilteredStateOutput},
    player::{
        lifecycle::PhysicalPlayerStateLifecycle,
        selector::{StateSelector, conditions::TwoStageThresholds},
        state::PhysicalStateId,
    },
};
use skate_data::collections::Collections;

pub(crate) struct PlayerState {
    pub lifecycle: PhysicalPlayerStateLifecycle,
    pub selector: StateSelector,
    pub requested_state: PhysicalStateId,
    pub filtered: FilteredState,
    pub filtered_output: Option<FilteredStateOutput>,
    pub ground_output: Option<skate_core::riding::grounded::state::output::PhysicsGroundOutput>,
    pub post: post_input::PostInputState,
    pub state_flags: [bool; 36],
    pub state_count: u32,
    pub update_count: u32,
    normal_off_ground: TwoStageThresholds,
    skitching_off_ground: TwoStageThresholds,
    animated_board_threshold: f32,
    initialized: bool,
}
impl PlayerState {
    pub fn load(data: &Collections, _mode: &str) -> Result<Self, String> {
        let thresholds = |class| -> Result<TwoStageThresholds, String> {
            Ok(TwoStageThresholds {
                field_856_primary: data.float(class, "default", "NaturalAirMaxDist")?,
                field_856_secondary: data.float(class, "default", "NaturalAirMinDist")?,
                field_7692: data.float(class, "default", "NaturalAirTime")?,
            })
        };
        Ok(Self {
            //82DB3008 selects the owned Sleeping object before SetPhysicsState100.
            lifecycle: PhysicalPlayerStateLifecycle::new(PhysicalStateId::Sleeping),
            selector: StateSelector::default(),
            requested_state: PhysicalStateId::Sleeping,
            filtered: FilteredState::default(),
            filtered_output: None,
            ground_output: None,
            post: post_input::PostInputState::new(),
            state_flags: [false; 36],
            state_count: 0,
            update_count: 0,
            //82D8BD50 reads Globals260; its layout392/396/400 is physics_airstates.
            normal_off_ground: thresholds("physics_airstates")?,
            skitching_off_ground: thresholds("physics_state_skitching")?,
            //Skeleton+24 physics_animation, original82BDC6C8 layout800.
            animated_board_threshold: data.float(
                "physics_animation",
                "default",
                "MaxDeckZAxisYForAnimatedDeck",
            )?,
            initialized: false,
        })
    }
    pub fn current(&self) -> PhysicalStateId {
        self.lifecycle.active().state
    }
    ///ResetSystems82DB92D0 resets the selector and physical-output filter;
    ///the coordinator owns board/skeleton resets and the following Ground entry.
    pub fn reset_for_teleport(&mut self) {
        self.filtered.reset();
        self.filtered_output = None;
        self.ground_output = None;
        //ResetSystems82DB92D0 clears20..44, sets48=10 and52=0.
        //Current pointer16 and flags56/57 survive until Calculate writes them.
        self.selector.nonspecific_collision_free_frames = 0;
        self.selector.nonspecific_collision_frames = 0;
        self.selector.something_colliding_frames = 0;
        self.selector.two_wheel_counter = 0;
        self.selector.three_wheel_counter = 0;
        self.selector.post_grind_jump_counter = 0;
        self.selector.air_frames = 0;
        self.selector.teleport_countdown = 10;
        self.selector.skitch_exit_countdown = 0;
        //TrajectorySelector::Reset82D67228 clears9658, preserving9657.
        self.post.trajectory_available = false;
    }
}
///Call once before first input; startup constructs the toolkit from live board/reset inputs.
pub(crate) fn initialize(
    physics: &mut GamePhysics,
    skater: &mut SkaterRuntime,
) -> Result<(), String> {
    if skater.player_state.initialized {
        return Ok(());
    }
    transition::set(physics, skater, PhysicalStateId::PhysicsGround)?;
    skater.player_state.initialized = true;
    Ok(())
}
///Runs after ProcessInput, before the chosen state's pre-update/force phase.
pub(crate) fn post_input_and_select(
    physics: &mut GamePhysics,
    skater: &mut SkaterRuntime,
) -> Result<(), String> {
    super::grind::query(physics, skater);
    post_input::advance(physics, skater)?;
    selection::advance(physics, skater)
}
///The reset/board/skeleton publications precede this selected-state FillPhysOut.
pub(crate) fn publish(physics: &mut GamePhysics, skater: &mut SkaterRuntime) -> Result<(), String> {
    publication::publish(physics, skater)
}
///Reset continuation uses the same native SetPhysicsState path and actual entry.
pub(crate) fn enter_after_teleport(
    physics: &mut GamePhysics,
    skater: &mut SkaterRuntime,
) -> Result<(), String> {
    transition::set(physics, skater, PhysicalStateId::PhysicsGround)
}

///Original82DB6050 prefix; coordinator calls the selected state pre-update next.
pub(crate) fn pre_state(
    physics: &mut GamePhysics,
    skater: &mut SkaterRuntime,
) -> Result<(), String> {
    pre_state::advance(physics, skater)
}
