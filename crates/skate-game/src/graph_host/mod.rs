//! Production graph host building blocks. Root owns schedule and registration.
pub mod action;
mod action_conditions;
pub(crate) mod action_nodes;
mod condition_nodes;
pub(crate) use condition_nodes::numeric as parse_numeric_condition;
mod crouching_settings;
pub mod motion;
pub(crate) mod motion_grind;
mod motion_air_leg;
mod motion_animation;
#[path = "motion_offboard/cadence.rs"]
pub(crate) mod motion_cadence;
mod motion_channels;
pub(crate) mod motion_character_gesture;
mod motion_conditions;
pub(crate) mod motion_gameplay_conditions;
mod motion_ground_slope;
pub(crate) mod motion_hand_services;
mod motion_hooks;
pub(crate) mod motion_intent_filter;
mod motion_kickturn;
pub(crate) mod motion_landing;
mod motion_landing_execute;
pub(crate) mod motion_native;
mod motion_nodes;
#[path = "motion_offboard/air.rs"]
pub(crate) mod motion_offboard_air;
#[path = "motion_offboard/push_off.rs"]
pub(crate) mod motion_push_off;
#[path = "motion_offboard/reset.rs"]
pub(crate) mod motion_reset;
mod motion_riding;
mod motion_manual;
pub(crate) mod motion_riding_conditions;
pub(crate) mod motion_runout;
pub(crate) mod motion_shove;
mod motion_sliding;
pub(crate) mod motion_spin;
pub(crate) mod motion_toggle_board;
mod motion_tricks;
#[path = "motion_offboard/twist_lean.rs"]
pub(crate) mod motion_twist_lean;
pub(crate) mod motion_wipeout;
mod pumping_settings;
pub mod pushing;
pub mod pushing_settings;
mod turning_settings;

pub(crate) mod outputs;
pub(crate) mod motion_stock_gameplay;
