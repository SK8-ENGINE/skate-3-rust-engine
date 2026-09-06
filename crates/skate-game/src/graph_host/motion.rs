#[path = "motion_air_execute.rs"]
mod air_execution;
#[path = "motion_execute.rs"]
mod execution;
#[path = "motion_filter_execute.rs"]
mod filter_execution;
#[path = "motion_sliding_execute.rs"]
mod slide_execution;
#[path = "motion_wipeout_execute.rs"]
mod wipeout_execution;
// Persistent host called by the production stock MotionGraph controller.
pub use super::motion_animation::MotionAnimation;
use super::{
    motion_nodes::{MotionFactory, MotionOperation},
    pushing::{PushContext, PushInstance, PushOperation},
    pushing_settings::PushingSettings,
};
use crate::graph_runtime::{LoadedGraph, OperationRemap};
use skate_core::graph::intents::IntentMap;
use skate_core::{
    animation::{
        playback::{PlayAnimationInstance, PlaybackContext},
        playback_parameters::{AttributeSink, ParameterInputs, SettableAttribute},
        skeleton_input::name::encode,
    },
    graph::{
        activation::ConditionHost,
        conditions::ConditionInputs,
        controller::{BehaviorId, Frame, HookId, Host},
    },
    input::set_turning,
    riding::push_behaviors::{PushFootFrame, PushState},
};
use skate_data::collections::Collections;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug)]
pub struct MotionPhysical {
    pub turning: set_turning::Physical,
    pub stance: (bool, bool),
    pub forward_speed: f32,
    pub time_since_teleport: f32,
    pub is_switch: bool,
    pub foot_frame: Option<PushFootFrame>,
}
enum Instance {
    OffboardAir(super::motion_offboard_air::State),
    ToggleBoard(super::motion_toggle_board::ToggleBoard),
    Cadence(super::motion_cadence::MatchCadence),
    Runout(Option<super::motion_runout::Parameters>),
    Trick(i32),
    Stateless,
    IntentFilter(skate_core::animation::intent_filter::State),
    Landing(super::motion_landing::State),
    Wipeout(super::motion_wipeout::State),
    TwistLean(skate_core::player::offboard::twist_lean::State),
    Play(PlayAnimationInstance),
    Push(PushInstance),
    Turning(set_turning::State),
    Crouching(Option<skate_core::animation::crouching::State>),
    BodyTilt(skate_core::animation::body_tilt::State),
    RidingFakie(skate_core::animation::riding_fakie::State),
    Pumping(skate_core::animation::pumping_channel::State),
    FakieHead(super::motion_channels::FakieHead),
    LandingData(super::motion_native::LandingData),
    Sliding(super::motion_sliding::State),
    SlideDeceleration(f32),
    KickTurn(skate_core::animation::kickturn::State),
    BodySpin(super::motion_spin::State),
    AirLeg(skate_core::animation::air_leg_extension::State),
    CharacterGesture(super::motion_character_gesture::CharacterGesture),
    Shove(super::motion_shove::ShoveState),
}
impl Instance {
    fn new(operation: &MotionOperation) -> Self {
        match operation {
            MotionOperation::OffboardAir(_) => Self::OffboardAir(Default::default()),
            MotionOperation::Cadence(_) => Self::Cadence(Default::default()),
            MotionOperation::AddRunoutAttribs => Self::Runout(None),
            MotionOperation::Trick(_) => Self::Trick(0),
            MotionOperation::IntentFilter(_) => Self::IntentFilter(Default::default()),
            MotionOperation::Landing(_) => Self::Landing(Default::default()),
            MotionOperation::Wipeout(_) => Self::Wipeout(Default::default()),
            MotionOperation::TwistLean(_) => Self::TwistLean(Default::default()),
            MotionOperation::Play(_) => Self::Play(PlayAnimationInstance::default()),
            MotionOperation::Push(operation) => Self::Push(PushInstance::new(operation.clone())),
            MotionOperation::SetTurning(_) => Self::Turning(set_turning::State {
                elapsed: 0.0,
                smoothed: 0.0,
                mode: 2,
            }),
            MotionOperation::Crouching(_) => Self::Crouching(None),
            MotionOperation::SettingBodyTilt(_) => {
                Self::BodyTilt(skate_core::animation::body_tilt::State::default())
            }
            MotionOperation::UpdateRidingFakie(_) => {
                Self::RidingFakie(skate_core::animation::riding_fakie::State::default())
            }
            MotionOperation::Pumping => {
                Self::Pumping(skate_core::animation::pumping_channel::State::default())
            }
            MotionOperation::FakieHeadChannel => {
                Self::FakieHead(super::motion_channels::FakieHead::default())
            }
            MotionOperation::Native(super::motion_native::Operation::StoreLandingData) => {
                Self::LandingData(super::motion_native::LandingData::default())
            }
            MotionOperation::Slide(super::motion_sliding::Operation::Update) => {
                Self::Sliding(super::motion_sliding::State::default())
            }
            MotionOperation::Slide(super::motion_sliding::Operation::Deceleration) => {
                Self::SlideDeceleration(0.0)
            }
            MotionOperation::KickTurn(super::motion_kickturn::Operation::Steering(_)) => {
                Self::KickTurn(skate_core::animation::kickturn::State::new())
            }
            MotionOperation::BodySpin => Self::BodySpin(super::motion_spin::State::default()),
            MotionOperation::AirLeg(_) => Self::AirLeg(Default::default()),
            MotionOperation::CharacterGesture => {
                Self::CharacterGesture(super::motion_character_gesture::CharacterGesture::new())
            }
            MotionOperation::Shove(_) => Self::Shove(super::motion_shove::ShoveState::new()),
            MotionOperation::ToggleBoard => Self::ToggleBoard(Default::default()),
            MotionOperation::ResetAnimation(_) => Self::Stateless,
            _ => Self::Stateless,
        }
    }
}
pub struct MotionHost {
    pub offboard_output: skate_core::player::input_phase::OffBoardOutputFields,
    pub toggle_board_physical: super::motion_toggle_board::Physical,
    pub disable_dismount: bool,
    pub offboard_phase: f32,
    pub offboard_locomotion: u32,
    pub offboard_slope: u32,
    pub offboard_support: [f32; 3],
    pub offboard_ground_thin: bool,
    pub phase_write: Option<f32>,
    pub animation: MotionAnimation,
    pub playback_context: PlaybackContext,
    pub condition_inputs: ConditionInputs,
    pub gameplay_conditions: Option<super::motion_gameplay_conditions::GameplayConditions>,
    pub riding_conditions: Option<super::motion_riding_conditions::RidingConditionInputs>,
    pub condition_random: super::motion_riding_conditions::MotionRandom,
    pub native_physical: Option<super::motion_native::Physical>,
    pub runout_physical: Option<super::motion_runout::Physical>,
    pub prelanding_physical: Option<super::motion_spin::PrelandingPhysical>,
    pub air_leg_physical: Option<skate_core::animation::air_leg_extension::Physical>,
    pub gesture_physical: Option<super::motion_native::GesturePhysical>,
    pub gesture_publication: Option<super::motion_character_gesture::GesturePublication>,
    pub shove_physical: Option<super::motion_shove::ShovePhysical>,
    pub hand_services: super::motion_hand_services::HandServices,
    pub slide_latch: set_turning::SlideLatch,
    ///SpecificCA4 bit26, getter8258F900/setter8258F910; reset clears it.
    pub is_power_sliding: bool,
    pub score_packet: super::motion_native::ScorePacket,
    pub(super) trick_requests: super::motion_tricks::Requests,
    pub(super) trick_height_settings: (bool, bool),
    pub action_intents: IntentMap,
    pub time_tags: Option<BTreeMap<String, f32>>,
    pub physical: Option<MotionPhysical>,
    pub push_state: Option<PushState>,
    pub applying_body_tilt: bool,
    pub crouching_physical: Option<skate_core::animation::crouching::Physical>,
    pub body_tilt_physical: Option<skate_core::animation::body_tilt::Physical>,
    pub fakie_physical: Option<skate_core::animation::riding_fakie::Physical>,
    /// PhysOutAnimation158/157, used directly by IsRidingGoofy82BA5AA8.
    pub physical_stance: Option<(bool, bool)>,
    pub flags: super::motion_landing::Flags,
    pub landing_physical: Option<super::motion_landing::Physical>,
    pub wipeout_physical: Option<super::motion_wipeout::Physical>,
    pub wipeout_controls: super::motion_wipeout::Controls,
    wipeout_settings: super::motion_wipeout::Settings,
    ///Native PhysOutGround pumping acceleration268.
    pub pumping_acceleration: Option<f32>,
    pub bump_acceleration: Option<[f32; 4]>,
    bump_settings: skate_core::animation::bump::Settings,
    pub allow_pumping: bool,
    pub riding: super::motion_riding::RidingState,
    pub errors: Vec<String>,
    pub(super) state_parents: Vec<Option<usize>>,
    operations: Vec<MotionOperation>,
    instances: Vec<Instance>,
    remap: OperationRemap,
    pushing: PushingSettings,
    turning: set_turning::Settings,
    crouching: skate_core::animation::crouching::Settings,
    body_tilt: skate_core::animation::body_tilt::Settings,
    pumping: skate_core::animation::pumping_channel::Settings,
    sliding: super::motion_sliding::Settings,
    pub(super) spin: super::motion_spin::Settings,
    air_leg: skate_core::animation::air_leg_extension::Settings,
    kickturn: skate_core::animation::kickturn::Settings,
    next_instance: u32,
}
impl MotionHost {
    pub fn from_graph(
        graph: &LoadedGraph,
        data: &Collections,
        mut metadata: skate_data::animation_metadata::AnimationMetadata,
        playback_context: PlaybackContext,
    ) -> Result<Self, String> {
        let mut operations = graph
            .binding
            .instantiate_operations(&graph.source, &mut MotionFactory)
            .map_err(|e| e.to_string())?
            .operations;
        super::motion_conditions::bind_states(graph, &mut operations);
        let instances = graph
            .runtime
            .operations
            .behaviors
            .iter()
            .map(|&id| Instance::new(&operations[id]))
            .collect();
        //8258F488 and final reset825953B0 explicitly zero the complete push
        //state; reset clears manualing and body-tilt flag bits too.
        let zero = skate_core::riding::push_animation::PushBlendParameters {
            hstr_vel_b: 0.0,
            lstr_vel_b: 0.0,
            vel_e: 0.0,
        };
        let push_state = Some(PushState {
            out_factor: 0.0,
            current_push_dv: 0.0,
            current: zero,
            target: zero,
            continue_push: false,
        });
        let pushing = PushingSettings::load(data, &mut metadata)?;
        Ok(Self {
            toggle_board_physical: Default::default(),
            disable_dismount: false,
            offboard_output: Default::default(),
            offboard_phase: 0.,
            offboard_locomotion: 0,
            offboard_slope: 0,
            offboard_support: [0.; 3],
            offboard_ground_thin: false,
            phase_write: None,
            animation: MotionAnimation::from_metadata(metadata),
            playback_context,
            condition_inputs: ConditionInputs::default(),
            gameplay_conditions: None,
            riding_conditions: None,
            condition_random: super::motion_riding_conditions::MotionRandom::new(),
            native_physical: None,
            runout_physical: None,
            prelanding_physical: None,
            air_leg_physical: None,
            gesture_physical: None,
            gesture_publication: None,
            shove_physical: None,
            hand_services: Default::default(),
            slide_latch: set_turning::SlideLatch::default(),
            is_power_sliding: false,
            score_packet: super::motion_native::ScorePacket::default(),
            trick_requests: Default::default(),
            trick_height_settings: (
                data.boolean("anim_motion", "jumping", "use_gesture_speed")?,
                data.boolean("anim_motion", "jumping", "clamp_gesture_to_antic")?,
            ),
            action_intents: IntentMap::new(),
            time_tags: None,
            physical: None,
            push_state,
            applying_body_tilt: false,
            crouching_physical: None,
            body_tilt_physical: None,
            fakie_physical: None,
            physical_stance: None,
            flags: Default::default(),
            landing_physical: None,
            wipeout_physical: None,
            wipeout_controls: Default::default(),
            wipeout_settings: super::motion_wipeout::Settings::load(data)?,
            pumping_acceleration: None,
            bump_acceleration: None,
            bump_settings: skate_core::animation::bump::Settings {
                scale_x_acc: data.float("anim_motion", "bumps", "scale_x_acc")?,
                min_bump_mag: data.float("anim_motion", "bumps", "min_bump_mag")?,
                min_bump_blend_value: data.float("anim_motion", "bumps", "min_bump_blend_value")?,
                max_bump_mag: data.float("anim_motion", "bumps", "max_bump_mag")?,
            },
            //8258F488 seeds bit24, and reset825953B0 preserves that bit.
            allow_pumping: true,
            riding: super::motion_riding::RidingState::new(),
            errors: Vec::new(),
            state_parents: graph.binding.states.iter().map(|s| s.parent).collect(),
            operations,
            instances,
            remap: graph.runtime.operations.clone(),
            pushing,
            turning: super::turning_settings::load(data)?,
            crouching: super::crouching_settings::load(data)?,
            body_tilt: super::motion_riding::body_tilt_settings(data)?,
            pumping: super::pumping_settings::load(data)?,
            sliding: super::motion_sliding::Settings::load(data)?,
            spin: super::motion_spin::Settings::load(data)?,
            air_leg: super::motion_air_leg::load(data)?,
            kickturn: super::motion_kickturn::load_settings(data)?,
            next_instance: 1,
        })
    }
    fn run(&mut self, id: BehaviorId, frame: &Frame, phase: u8) {
        if let Err(error) = self.execute(id, frame, phase) {
            self.errors
                .push(format!("MotionGraph behavior {id}: {error}"));
        }
    }
}
impl ConditionHost for MotionHost {
    fn condition_activation(&mut self, condition: usize, frame: &Frame) -> u32 {
        let result = self
            .remap
            .conditions
            .get(condition)
            .and_then(|&id| self.operations.get(id))
            .ok_or_else(|| "Unbound MotionGraph condition".to_owned())
            .and_then(|op| match op {
                MotionOperation::Condition(condition) => condition.evaluate(self, frame),
                other => Err(format!("Unsupported MotionGraph condition {other:?}")),
            });
        match result {
            Ok(value) => u32::from(value),
            Err(error) => {
                self.errors.push(error);
                0
            }
        }
    }
}
impl Host for MotionHost {
    fn context(&self) -> [u32; 6] {
        [0; 6]
    }
    fn allocate(&mut self, behavior: BehaviorId, _frame: &Frame) -> u32 {
        if let Some(&id) = self.remap.behaviors.get(behavior) {
            self.instances[behavior] = Instance::new(&self.operations[id]);
        }
        let handle = self.next_instance;
        self.next_instance = self.next_instance.wrapping_add(1).max(1);
        handle
    }
    fn begin(&mut self, id: BehaviorId, _context: [u32; 6], frame: &Frame) {
        self.run(id, frame, 0);
    }
    fn update(&mut self, id: BehaviorId, _context: [u32; 6], frame: &Frame) {
        self.run(id, frame, 1);
    }
    fn end(&mut self, id: BehaviorId, _context: [u32; 6], frame: &Frame) {
        self.run(id, frame, 2);
    }
    fn hook(&mut self, hook: HookId, _frame: &Frame) {
        if let Some(MotionOperation::Hook(operation)) = self
            .remap
            .hooks
            .get(hook)
            .and_then(|&id| self.operations.get(id))
            .cloned()
        {
            match operation {
                super::motion_hooks::MotionHook::GrabSlide { right } => {
                    self.slide_latch.grab(right)
                }
                super::motion_hooks::MotionHook::Override(settings) => {
                    self.playback_context.transition_override = Some(settings)
                }
                super::motion_hooks::MotionHook::MongoPushToAntic { animation } => {
                    //82BBBBB8 passes the authored name for both channel/tree.
                    let settings = skate_core::animation::channel_playback::ChannelSettings {
                        priority: 0,
                        keep_alive: false,
                        mirrored: false,
                        speed: 1.0,
                        blend_in: f32::from_bits(0x3dcccccd),
                        hold_during_blend_in: false,
                        blend_out: f32::from_bits(0x3d8f5c29),
                        hold_during_blend_out: false,
                        use_attributes: false,
                    };
                    if let Err(error) = self.animation.new_channel(&animation, &animation, settings)
                    {
                        self.errors.push(error);
                    }
                }
            }
            return;
        }
        self.errors.push(format!(
            "Unsupported MotionGraph hook {:?}",
            self.remap
                .hooks
                .get(hook)
                .and_then(|&id| self.operations.get(id))
        ));
    }
    fn release(&mut self, _instance: u32) {}
}

#[cfg(test)]
#[path = "tests/motion_stock.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/motion_slide.rs"]
mod slide_tests;

#[cfg(test)]
#[path = "tests/motion_bump.rs"]
mod bump_tests;

#[cfg(test)]
#[path = "tests/motion_tricks.rs"]
mod trick_tests;
