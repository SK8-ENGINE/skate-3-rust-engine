//! Stock grind graph behaviors82BAF208/82BB0A30/82BB0D40/82BB10F8.
use skate_core::animation::{
    grind_control::{Fade, FadeSettings, crouch},
    playback_parameters::{AttributeSink, SettableAttribute},
    skeleton_input::name::encode,
};
use skate_data::{collections::Collections, state_graph::attributes::Attributes};
#[derive(Clone, Debug, PartialEq)]
pub enum Operation {
    Attributes,
    Crouch,
    Fade {
        height: String,
        twist: String,
        intent: String,
    },
}
impl Operation {
    pub fn parse(a: &Attributes<'_>) -> Option<Self> {
        Some(match a.text("name")? {
            "CreateGrindAttributes" => Self::Attributes,
            "ControlGrindCrouch" => Self::Crouch,
            "GrindControlFade" => Self::Fade {
                height: a
                    .text("distBoardToCogAnimAttribute")
                    .unwrap_or("DistToCog")
                    .into(),
                twist: a.text("twistAnimAttribute").unwrap_or("twist").into(),
                intent: a.text("twistMGIntent").unwrap_or("GrindBalanceX").into(),
            },
            _ => return None,
        })
    }
}
pub struct Settings {
    pub fade: FadeSettings,
    pub height: [f32; 3],
}
impl Settings {
    pub fn load(d: &Collections) -> Result<Self, String> {
        let f = |n| d.float("anim_motion", "grind_twist", n);
        Ok(Self {
            fade: FadeSettings {
                response: f("twist_smoothing")?,
                input_scale: f("twist_sensitivity")?,
                acceleration: f("twist_max_delta_delta")?,
                maximum_step: f("twist_max_delta")?,
            },
            height: [
                d.float("anim_motion", "grind_height", "min_grind_disttocog")?,
                d.float("anim_motion", "grind_height", "max_grind_disttocog")?,
                d.float("anim_motion", "grind_height", "grind_disttocog_speed")?,
            ],
        })
    }
}
#[derive(Default)]
pub struct State {
    pub fade: Fade,
    pub height: f32,
}
#[derive(Clone, Debug, Default)]
pub struct Physical {
    pub name: String,
    pub grinding: bool,
    pub dropping_in: bool,
    pub crouch: f32,
    pub twist: f32,
    pub facing_backwards: bool,
}
pub fn execute(
    state: &mut State,
    op: &Operation,
    phase: u8,
    dt: f32,
    settings: &Settings,
    physical: &Physical,
    height: f32,
    animation: &mut super::motion_animation::MotionAnimation,
) -> Result<(), String> {
    let set = |animation: &mut super::motion_animation::MotionAnimation, name: &str, value: f32| {
        animation.set_attribute(SettableAttribute {
            name: encode(name.as_bytes()),
            value,
            normalized: false,
            sequence_id: -1,
        });
    };
    match op {
        Operation::Attributes if phase == 1 && physical.grinding => {
            animation.motion_intents.insert(&physical.name, 1.0);
            animation.motion_intents.insert(
                if physical.facing_backwards {
                    "GrindFacingBackwards"
                } else {
                    "GrindFacingForwards"
                },
                1.0,
            );
        }
        Operation::Crouch if phase == 0 => state.height = height,
        Operation::Crouch if phase == 1 => {
            let intent = animation
                .motion_intents
                .get("Crouch")
                .copied()
                .unwrap_or(0.0);
            let [min, max, rate] = settings.height;
            state.height = crouch(state.height, intent, physical.crouch, min, max, rate, dt);
            set(animation, "DistToCog", state.height);
        }
        Operation::Fade {
            height: name,
            twist,
            intent,
        } => {
            if phase == 0 {
                set(animation, name, height);
                let bounds = animation.attribute_endpoints(encode(twist.as_bytes()))?;
                let value = state.fade.begin(bounds[0], bounds[1], physical.twist);
                set(animation, twist, value);
            } else if phase == 1 {
                let input = animation.motion_intents.get(intent).copied().unwrap_or(0.0);
                if let Some(value) = state.fade.update(dt, input, settings.fade) {
                    set(animation, twist, value);
                }
            }
        }
        _ => {}
    }
    Ok(())
}
