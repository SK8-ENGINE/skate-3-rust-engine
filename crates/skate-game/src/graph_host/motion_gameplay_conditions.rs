//! Original TU3 physical-state condition leaves used by the stock MotionGraph.
use super::motion::MotionHost;
use skate_data::state_graph::attributes::Attributes;

/// The preceding completed physical publication. These are native output
/// fields, separate from controller intents and presentation transforms.
#[derive(Clone, Copy, Debug)]
pub struct GameplayConditions {
    pub state: u32,
    pub wants_runout: bool,
    pub physics_wiping: bool,
    pub body_flipping: bool,
    pub wants_wipeout: bool,
    pub bumped: bool,
    pub grabbing_object: bool,
    pub retrieving_board: bool,
    pub dropping_board: bool,
    pub in_biped_air: bool,
    pub hippy_hurdling: bool,
    pub handplant_flags: u32,
    pub time_to_skitch: f32,
    pub skitch_transition_time: f32,
    /// PhysOutAnimation+144 collision time used by TimeToLand.
    pub time_to_land: f32,
    /// PhysOutOffBoard+32, shared by OBTimeToLand and OBTrajTime.
    pub offboard_time_to_land: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GameplayCondition {
    RetrievingBoard,
    DroppingBoard,
    InBipedAir,
    HippyHurdling,
    WantsRunout,
    PhysicsWiping,
    BodyFlipping,
    WantsWipeout,
    Bumped,
    CanEnterSlide { right: bool },
    GrabbingObject,
    Dark,
    UnderflipRequested,
    DarkCatchRequested,
    HandPlanting { state: u8, direction: u8 },
    EnteringSkitch,
    Skitching,
}
impl GameplayCondition {
    pub fn recognizes(name: &str) -> bool {
        matches!(
            name,
            "IsRetrievingSkateboard"
                | "IsDroppingSkateboard"
                | "IsInBipedAir"
                | "IsHippyHurdling"
                | "PhysicsWantsRunout"
                | "IsPhysicsWiping"
                | "IsBodyFlipping"
                | "PhysicsWantsWipeOut"
                | "IsBumped"
                | "CanEnterSlide"
                | "IsGrabbingObject"
                | "IsDark"
                | "IsUnderflipRequested"
                | "IsDarkCatchRequested"
                | "IsHandPlanting"
                | "IsEnteringSkitch"
                | "IsSkitching"
        )
    }
    pub fn parse(a: &Attributes<'_>) -> Result<Self, String> {
        Ok(match a.text("name").unwrap_or("") {
            "IsRetrievingSkateboard" => Self::RetrievingBoard,
            "IsDroppingSkateboard" => Self::DroppingBoard,
            "IsInBipedAir" => Self::InBipedAir,
            "IsHippyHurdling" => Self::HippyHurdling,
            "PhysicsWantsRunout" => Self::WantsRunout,
            "IsPhysicsWiping" => Self::PhysicsWiping,
            "IsBodyFlipping" => Self::BodyFlipping,
            "PhysicsWantsWipeOut" => Self::WantsWipeout,
            "IsBumped" => Self::Bumped,
            "CanEnterSlide" => Self::CanEnterSlide {
                right: a.boolean_byte("right", 1) != 0, //82BC53F0
            },
            "IsGrabbingObject" => Self::GrabbingObject,
            "IsDark" => Self::Dark,
            "IsUnderflipRequested" => Self::UnderflipRequested,
            "IsDarkCatchRequested" => Self::DarkCatchRequested,
            "IsEnteringSkitch" => Self::EnteringSkitch,
            "IsSkitching" => Self::Skitching,
            "IsHandPlanting" => Self::HandPlanting {
                //82BA54A0 compares these authored strings in this order.
                state: match a.text("state").unwrap_or("") {
                    "air" => 0,
                    "ground" => 1,
                    value => return Err(format!("IsHandPlanting has unauthored state {value:?}")),
                },
                direction: match a.text("dir").unwrap_or("") {
                    "FS" => 1,
                    "BS" => 0,
                    _ => 2,
                },
            },
            name => return Err(format!("Unknown gameplay condition {name}")),
        })
    }
    /// Both stock graphs register these same native leaves. Return None for
    /// conditions that also require the MotionGraph/channel owner.
    pub fn evaluate_physical(&self, p: &GameplayConditions) -> Option<bool> {
        Some(match self {
            Self::RetrievingBoard => p.retrieving_board, //82BA5EF0:Offboard323
            Self::InBipedAir => p.in_biped_air,          //82BA8030:Offboard328
            Self::HippyHurdling => p.hippy_hurdling,     //82BA5DA0:Offboard317
            Self::WantsRunout => p.wants_runout,         //82BA44B8:State78
            Self::PhysicsWiping => p.physics_wiping,     //82BA4390:State59
            Self::BodyFlipping => p.body_flipping,       //82BA71E0:Air441
            Self::WantsWipeout => p.wants_wipeout,       //82BA4400:State63 || State65
            Self::Bumped => p.bumped, //82BA7310: published acceleration and anim_motion/bumps
            Self::GrabbingObject => p.grabbing_object, //82BA5700:Offboard304
            Self::Skitching => p.state == 104, //82BBBC88:State16
            Self::EnteringSkitch => {
                //82BBBDE0:Ground276,Globals400/layout96
                !(p.time_to_skitch < 0.0) && p.time_to_skitch <= p.skitch_transition_time
            }
            Self::HandPlanting { state, direction } => {
                //82BA55A8:Air324,State16
                let active = if *state == 0 {
                    p.state == 600
                } else {
                    p.handplant_flags & 0x8000_0000 != 0
                };
                active
                    && match direction {
                        2 => true,
                        1 => p.handplant_flags & 0x2000_0000 != 0,
                        0 => p.handplant_flags & 0x2000_0000 == 0,
                        _ => false,
                    }
            }
            Self::DroppingBoard | Self::Dark | Self::UnderflipRequested | Self::DarkCatchRequested | Self::CanEnterSlide { .. } => return None,
        })
    }

    pub fn evaluate(&self, host: &MotionHost) -> Result<bool, String> {
        let p = host
            .gameplay_conditions
            .as_ref()
            .ok_or("MotionGraph requires the actual physical condition publication")?;
        if let Some(result) = self.evaluate_physical(p) {
            return Ok(result);
        }
        Ok(match self {
            //82BA5F60: Offboard322 or the actual RetrieveBoard channel.
            Self::DroppingBoard => p.dropping_board || host.animation.channels.has("RetrieveBoard"),
            //82BA79A0 calls the specific MotionGraph getter8258FB68.
            Self::Dark => host.riding.dark,
            Self::UnderflipRequested => host.trick_requests.underflip,
            Self::DarkCatchRequested => host.trick_requests.dark_catch,
            Self::CanEnterSlide { right } => {
                //82BA6FE0 rejects RevertGround102, reverses the side for fakie,
                //then reads its start bit in the retained C84 slide packet.
                if p.state == 102 {
                    false
                } else {
                    //ISkaterAnim virtual12=82B970D8 reads bit29=fakie.
                    let fakie = host
                        .animation
                        .skater_animation_flags
                        .ok_or("CanEnterSlide requires actual SkaterAnim stance flags")?
                        & 0x2000_0000
                        != 0;
                    if *right ^ fakie {
                        host.slide_latch.start(true)
                    } else {
                        host.slide_latch.start(false)
                    }
                }
            }
            _ => unreachable!("Physical condition handled above"),
        })
    }
}
