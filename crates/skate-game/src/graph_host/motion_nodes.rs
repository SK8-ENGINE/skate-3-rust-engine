//! Stock MotionGraph configuration. Native constructor defaults are kept here;
//! lifecycle execution and persistent state are owned by MotionHost.
use super::{motion_conditions::MotionCondition, pushing::PushOperation};
use skate_core::animation::{
    output::attributes::AttributeName,
    playback::{PlayAnimation, TransitionSettings},
    playback_parameters::{ParameterSource, PlaybackParameter},
    skeleton_input::name::encode,
};
use skate_data::state_graph::{
    attributes::Attributes,
    binding::{Node, OperationFactory, OperationKind},
};

#[derive(Clone, Debug, PartialEq)]
pub enum MotionOperation {
    Trick(super::motion_tricks::Operation),
    Play(PlayAnimation),
    ///TU3 vtable82309664: Begin/Update/End all point to the empty82B61BB8.
    PrintText2D,
    AttachIntent {
        intent: String,
        attribute: AttributeName,
        set: bool,
    },
    Push(PushOperation),
    /// Angle, direction, quickness, speed, holding, in native emission order.
    SetTurning([AttributeName; 5]),
    SetBumpCoefficients([AttributeName; 2]),
    ApplyingBodyTilt,
    SettingBodyTilt(AttributeName),
    Crouching(AttributeName),
    Pumping,
    DisallowPumping,
    FakieHeadChannel,
    Native(super::motion_native::Operation),
    Slide(super::motion_sliding::Operation),
    KickTurn(super::motion_kickturn::Operation),
    BodySpin,
    AirLeg(super::motion_air_leg::Operation),
    ClearTrickAttr,
    Landing(super::motion_landing::Operation),
    Wipeout(super::motion_wipeout::Operation),
    TwistLean(super::motion_twist_lean::Operation),
    HandService(super::motion_hand_services::Operation),
    IntentFilter(super::motion_intent_filter::Operation),
    CharacterGesture,
    Shove(super::motion_shove::ShoveOperation),
    ResetAnimation(super::motion_reset::Operation),
    UpdateRidingFakie(skate_core::animation::riding_fakie::Settings),
    SetSpeed {
        name: AttributeName,
        value: Option<f32>,
    },
    Hook(super::motion_hooks::MotionHook),
    Riding(super::motion_riding::RidingOperation),
    Condition(MotionCondition),
    Unsupported {
        kind: OperationKind,
        name: String,
    },
}

#[derive(Default)]
pub struct MotionFactory;
impl OperationFactory for MotionFactory {
    type Instance = MotionOperation;
    type Error = String;

    fn create(
        &mut self,
        kind: OperationKind,
        _parent: Node,
        a: &Attributes<'_>,
    ) -> Result<Option<Self::Instance>, String> {
        let name = a.text("name").ok_or("MotionGraph operation has no name")?;
        let operation = if kind == OperationKind::Condition {
            MotionCondition::parse(a)?.map(MotionOperation::Condition)
        } else if kind == OperationKind::Behavior {
            if let Some(operation) = super::motion_landing::Operation::parse(a) {
                Some(MotionOperation::Landing(operation))
            } else if let Some(operation) = super::motion_hand_services::Operation::parse(a) {
                Some(MotionOperation::HandService(operation))
            } else if let Some(push) = PushOperation::from_attributes(a) {
                Some(MotionOperation::Push(push))
            } else if let Some(operation) = super::motion_riding::RidingOperation::parse(a) {
                Some(MotionOperation::Riding(operation))
            } else if let Some(operation) = super::motion_native::Operation::parse(a)? {
                Some(MotionOperation::Native(operation))
            } else if let Some(operation) = super::motion_sliding::Operation::parse(a) {
                Some(MotionOperation::Slide(operation))
            } else if let Some(operation) = super::motion_kickturn::Operation::parse(a) {
                Some(MotionOperation::KickTurn(operation))
            } else if let Some(operation) = super::motion_shove::ShoveOperation::parse(a)? {
                Some(MotionOperation::Shove(operation))
            } else {
                match name {
                    "SetBumpCoefficients" => Some(MotionOperation::SetBumpCoefficients([key(a,"X",""),key(a,"Y","")])),
                    "ResetSkaterAnimation" | "ResetToGivenStance" => Some(MotionOperation::ResetAnimation(
                        super::motion_reset::Operation::parse(a).ok_or("Invalid reset operation")?,
                    )),
                    "MatchTwistAndLean" => Some(MotionOperation::TwistLean(
                        super::motion_twist_lean::Operation::parse(a),
                    )),
                    "Wipeout" => Some(MotionOperation::Wipeout(
                        super::motion_wipeout::Operation::Wipeout,
                    )),
                    "EnableWipeoutGestures" => Some(MotionOperation::Wipeout(
                        super::motion_wipeout::Operation::EnableGestures,
                    )),
                    "FilterMotionGraphIntent" => Some(MotionOperation::IntentFilter(
                        super::motion_intent_filter::Operation::parse(a),
                    )),
                    "PlayAnimation" => Some(MotionOperation::Play(play(a))),
                    "PrintText2D" => Some(MotionOperation::PrintText2D),
                    "ApplyingBodyTilt" => Some(MotionOperation::ApplyingBodyTilt),
                    "SettingBodyTilt" => {
                        Some(MotionOperation::SettingBodyTilt(key(a, "tilt_x", "tilt_x")))
                    }
                    "Crouching" => Some(MotionOperation::Crouching(key(a, "crouchName", ""))),
                    "Pumping" => Some(MotionOperation::Pumping),
                    "BodySpin" => Some(MotionOperation::BodySpin),
                    "ControlAirLegExtension" => Some(MotionOperation::AirLeg(
                        super::motion_air_leg::Operation::parse(a),
                    )),
                    "ClearTrickAttr" => Some(MotionOperation::ClearTrickAttr),
                    "SetTrickHeight" | "SetTrickAttr" | "ScoringTrick" | "MonitorUnderflip" | "SetDark" | "UpdateIsWeightOnNose" => super::motion_tricks::Operation::parse(a).map(MotionOperation::Trick),
                    "CharacterGesture" => Some(MotionOperation::CharacterGesture),
                    "DisallowPumping" => Some(MotionOperation::DisallowPumping),
                    "FakieHeadChannel" => Some(MotionOperation::FakieHeadChannel),
                    "SetSpeed" => Some(MotionOperation::SetSpeed {
                        name: key(a, "attribute", ""),
                        value: a.get("setToValue").map(|v| f32::from_bits(v.float_bits)),
                    }),
                    "UpdateRidingFakie" => Some(MotionOperation::UpdateRidingFakie(
                        skate_core::animation::riding_fakie::Settings {
                            high_speed: f32::from_bits(
                                a.float_bits("highSpeedThreshold", 1.0f32.to_bits()),
                            ),
                            low_speed: f32::from_bits(
                                a.float_bits("lowSpeedThreshold", 0.5f32.to_bits()),
                            ),
                            slowly_backwards_seconds: f32::from_bits(
                                a.float_bits("timeSlowlyRollingBackwardsThreshold", 0x3e4ccccd),
                            ),
                            after_teleport_seconds: f32::from_bits(
                                a.float_bits("timeFromTeleportThreshold", 3.0f32.to_bits()),
                            ),
                        },
                    )),
                    // Factory82BC6D40 defaults both identifiers to the string0.
                    "AttachIntent" => Some(MotionOperation::AttachIntent {
                        intent: a.text("intent").unwrap_or("0").into(),
                        attribute: encode(a.text("attr").unwrap_or("0").as_bytes()),
                        set: a.boolean_byte("set", 0) != 0,
                    }),
                    // Ctor82BB36D8; layout88 is holding and108 is speed, but
                    // Update emits speed before holding.
                    "SetTurning" => Some(MotionOperation::SetTurning([
                        key(a, "angleName", "angle"),
                        key(a, "dirName", "dir"),
                        key(a, "quicknessName", "quickness"),
                        key(a, "speedName", "speedtuck"),
                        key(a, "holdingName", "holding"),
                    ])),
                    _ => None,
                }
            }
        } else if kind == OperationKind::Hook {
            super::motion_hooks::MotionHook::parse(a).map(MotionOperation::Hook)
        } else {
            None
        };
        Ok(Some(operation.unwrap_or_else(|| {
            MotionOperation::Unsupported {
                kind,
                name: name.into(),
            }
        })))
    }
    fn add_parameter(
        &mut self,
        instance: &mut MotionOperation,
        a: &Attributes<'_>,
    ) -> Result<(), String> {
        if let MotionOperation::Play(play) = instance {
            //82BB56B8: all unrecognized from strings take filteredIntent.
            let source = match a.text("from").unwrap_or("") {
                "intent" => ParameterSource::MotionIntent(a.text("intent").unwrap_or("").into()),
                "lastAnim" => ParameterSource::LastAnimation(key(a, "attribute", "")),
                _ => ParameterSource::FilteredIntent(a.text("filteredIntent").unwrap_or("").into()),
            };
            let last_animation = matches!(source, ParameterSource::LastAnimation(_));
            play.parameters.push(PlaybackParameter {
                source,
                rename: a
                    .text("rename")
                    .map(|v| encode(v.as_bytes()))
                    .filter(|v| *v != encode(b"")),
                default_value: a.get("defaultValue").map(|v| f32::from_bits(v.float_bits)),
                normalized: !last_animation && a.boolean_byte("normalize", 0) != 0,
            });
        }
        // Other implemented nodes inherit the verified no-op AddParam.
        Ok(())
    }
}

fn key(a: &Attributes<'_>, name: &str, default: &str) -> AttributeName {
    encode(a.text(name).unwrap_or(default).as_bytes())
}
fn play(a: &Attributes<'_>) -> PlayAnimation {
    PlayAnimation {
        animation: a.text("anim").unwrap_or("").into(),
        switch_animation: a.text("switchAnim").map(str::to_owned),
        mirror_animation: a.text("mirrorAnim").map(str::to_owned),
        no_board_animation: a.text("noBoardAnim").map(str::to_owned),
        playback_speed: f32::from_bits(a.float_bits("playBackSpeed", 1.0f32.to_bits())),
        apply_posture: a.boolean_byte("applyPosture", 1) != 0,
        transition: transition(a),
        parameters: Vec::new(),
    }
}
pub(super) fn transition(a: &Attributes<'_>) -> TransitionSettings {
    TransitionSettings {
        kind: match a.text("transType").unwrap_or("") {
            "play" => 1,
            "sequence" => 4,
            "channelblend" => 3,
            _ => 2,
        },
        // Source literal82099280, not a fitted visual blend duration.
        seconds: f32::from_bits(a.float_bits("time", 0x3e4c_cccd)),
        under: u32::from(a.boolean_byte("transitionUnder", 0) != 0),
        matching: if a.boolean_byte("blendWithCurrentFrame", 0) != 0 {
            1
        } else if a.boolean_byte("blendMatchPhase", 0) != 0 {
            2
        } else if a.boolean_byte("blendMatchFrame", 0) != 0 {
            3
        } else {
            0
        },
        use_channels_from_weights: a.boolean_byte("useChannelFromWeights", 0) != 0,
    }
}
