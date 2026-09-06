//! Original trick leaves. Registration strings, not community symbol guesses,
//! identify SetTrickAttr82BC7848 and ScoringTrick82BCC030.
use super::motion::MotionHost;
use skate_core::animation::{
    output::attributes::AttributeName,
    playback_parameters::{AttributeSink, ParameterInputs, SettableAttribute},
    skeleton_input::name::encode,
};
use skate_data::state_graph::attributes::Attributes;

#[derive(Clone, Debug, PartialEq)]
pub enum Operation {
    Height {
        from: AttributeName,
        rename: AttributeName,
        value: Option<f32>,
        manual: bool,
        grind: bool,
    },
    Attribute(AttributeName),
    Scoring(AttributeName),
    MonitorUnderflip,
    Dark,
    WeightOnNose,
}
impl Operation {
    pub fn parse(a: &Attributes<'_>) -> Option<Self> {
        Some(match a.text("name")? {
            "SetTrickHeight" => Self::Height {
                from: encode(a.text("from").unwrap_or("").as_bytes()),
                rename: encode(a.text("rename").unwrap_or("").as_bytes()),
                value: a.get("setToValue").map(|v| f32::from_bits(v.float_bits)),
                manual: a.get("trickFromManual").is_some(),
                grind: a.get("trickFromGrind").is_some(),
            },
            "SetTrickAttr" => Self::Attribute(encode(a.text("trick").unwrap_or("").as_bytes())),
            "ScoringTrick" => Self::Scoring(encode(a.text("trick").unwrap_or("").as_bytes())),
            "MonitorUnderflip" => Self::MonitorUnderflip,
            "SetDark" => Self::Dark,
            "UpdateIsWeightOnNose" => Self::WeightOnNose,
            _ => return None,
        })
    }
}

#[derive(Default)]
pub struct Requests {
    pub underflip: bool,
    pub dark_catch: bool,
}

pub fn execute(
    host: &mut MotionHost,
    operation: Operation,
    updates: &mut i32,
    phase: u8,
) -> Result<(), String> {
    match operation {
        Operation::Height {
            from,
            rename,
            value,
            manual,
            grind,
        } if phase == 0 => {
            // 82BAEE90: graph intents override animation only on the authored paths.
            let gesture = host
                .animation
                .motion_intents
                .get("GestureSpeed")
                .copied()
                .unwrap_or(0.0);
            let height = if let Some(value) = value {
                value
            } else if let Some(value) = host.animation.motion_intents.get("TrickHeight") {
                *value
            } else {
                let mut height = host
                    .animation
                    .last_attribute(from)?
                    .and_then(|a| a.payload.0[0])
                    .map(f32::from_bits)
                    .unwrap_or(0.0);
                if host.trick_height_settings.0 {
                    height = if host.trick_height_settings.1 {
                        height.min(gesture.max(0.0))
                    } else {
                        gesture
                    };
                }
                height
            };
            // Independent packet output carries gesture speed, not selected height.
            host.animation.emit_packet(
                encode(b"JumpHeightOverride"),
                gesture
                    * if manual {
                        0.27
                    } else if grind {
                        1.0
                    } else {
                        0.2
                    },
            );
            host.animation.set_attribute(SettableAttribute {
                name: rename,
                value: height,
                normalized: true,
                sequence_id: -1,
            });
        }
        Operation::Attribute(value) => {
            let name = encode(b"Trick");
            if phase == 0 {
                if let Some(entry) = host
                    .animation
                    .construction_values
                    .iter_mut()
                    .find(|e| e.0 == name)
                {
                    entry.1 = value;
                } else {
                    host.animation.construction_values.push((name, value));
                }
            } else if phase == 2
                && host
                    .animation
                    .last_attribute(encode(b"defaultcyc"))?
                    .is_some()
            {
                host.animation.construction_values.retain(|e| e.0 != name);
            }
        }
        Operation::Scoring(name) if phase == 1 => {
            // ISkaterMotionGraph236 ->8258FA20 stores both equal names and bit24.
            host.score_packet.trick = Some(name);
            host.score_packet.flags |= 0x0100_0000;
        }
        Operation::MonitorUnderflip => {
            if phase == 0 {
                *updates = 0;
            } else if phase == 2 {
                host.trick_requests = Requests::default();
            } else {
                *updates = updates.wrapping_add(1);
                if *updates > 1 {
                    let intents = &host.animation.motion_intents;
                    if [
                        "U_L_F_Kickflip",
                        "U_L_F_Heelflip",
                        "U_L_B_Kickflip",
                        "U_L_B_Heelflip",
                        "U_Nollie",
                        "U_Fingerflip",
                        "U_Kickflip",
                        "U_Heelflip",
                        "U_N_Kickflip",
                        "U_N_Heelflip",
                    ]
                    .iter()
                    .any(|n| intents.contains_key(n))
                    {
                        host.trick_requests.underflip = true;
                    } else if intents.contains_key("DarkCatch") {
                        host.trick_requests.dark_catch = true;
                    }
                }
            }
        }
        Operation::Dark => {
            host.riding.dark = phase != 2;
            if phase == 1 {
                host.animation.emit_packet(encode(b"IsDark"), 1.0);
            }
        }
        Operation::WeightOnNose if phase != 1 => {
            //82BB2670/26D0 -> ISkaterAnim20 ->82B97110, bit28.
            let flags = host
                .animation
                .skater_animation_flags
                .as_mut()
                .ok_or("UpdateIsWeightOnNose requires live ISkaterAnim")?;
            *flags = (*flags & !0x1000_0000) | if phase == 0 { 0x1000_0000 } else { 0 };
        }
        _ => {}
    }
    Ok(())
}
