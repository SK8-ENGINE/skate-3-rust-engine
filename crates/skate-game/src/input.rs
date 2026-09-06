//! Platform input adapter; no animation or physics state mutation here.
use crate::app::SimulationSet;
use bevy::prelude::*;

mod controllers;
pub(crate) mod gesture_catalog;
mod platform;
pub(crate) use controllers::{ControllerInput, ControllerStatus};

pub(crate) struct InputPlugin;
impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ControllerInput>()
            .add_systems(PreUpdate, (poll_controllers, exit_requested))
            .add_systems(FixedUpdate, publish_actions.in_set(SimulationSet::Input));
    }
}

fn poll_controllers(mut input: ResMut<ControllerInput>) {
    let previous = input.status;
    input.collect(std::array::from_fn(platform::poll));
    for (index, (&before, &after)) in previous.iter().zip(&input.status).enumerate() {
        if before != after {
            match after {
                ControllerStatus::Ready => info!("Controller {index}: raw XInput ready"),
                ControllerStatus::Unavailable(platform::DeviceError::Disconnected) => {
                    info!("Controller {index}: disconnected");
                }
                _ => warn!("Controller {index}: {after:?}"),
            }
        }
    }
}

fn publish_actions(mut input: ResMut<ControllerInput>) {
    input.publish_actions();
}
fn exit_requested(keys: Res<ButtonInput<KeyCode>>, mut exit: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}
