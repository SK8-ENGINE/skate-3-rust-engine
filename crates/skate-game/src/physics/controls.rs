//! Controller-derived state is evaluated on the same fixed gameplay tick.
use super::{GamePhysics, skater::SkaterRuntime};
use crate::input::ControllerInput;
use bevy::prelude::*;
use skate_core::graph::intents::IntentMap;
use skate_core::input::{
    controller::{ActionMap, DerivedControllerInput, MagnitudeHeldSettings},
    riding_intentions::{self, PushPreferences, RidingIntent},
    wipeout_intentions,
};

#[derive(Resource)]
pub(crate) struct PlayerControls {
    pub controller: DerivedControllerInput,
    pub intents: Vec<RidingIntent>,
    pub action_intents: IntentMap,
    pub ticks: u64,
    pub actor_flags: u32,
    pub bumper_state_502: bool,
    pub bumper_state_104: bool,
    pub preferences: PushPreferences,
}
impl Default for PlayerControls {
    fn default() -> Self {
        let mut controller = DerivedControllerInput::from_words([0; 26]);
        controller.initialize();
        Self {
            controller,
            intents: Vec::new(),
            action_intents: IntentMap::new(),
            ticks: 0,
            actor_flags: 0,
            bumper_state_502: false,
            bumper_state_104: false,
            preferences: PushPreferences::default(),
        }
    }
}

pub(super) fn sample(
    input: Res<ControllerInput>,
    physics: Res<GamePhysics>,
    skater: Res<SkaterRuntime>,
    mut player: ResMut<PlayerControls>,
) {
    let mut map = input.player_actions();
    player.update(
        &mut map,
        physics.settings.step.simulation.time_step,
        physics.settings.input_magnitude_threshold,
        skater.player_input.physical.scoring.capabilities_204,
    );
}

impl PlayerControls {
    pub fn update(&mut self, map: &mut impl ActionMap, dt: f32, magnitude_threshold: f32, physical_capabilities: u32) {
        self.controller.update(
            map,
            dt,
            self.bumper_state_502,
            self.bumper_state_104,
            &MagnitudeHeldSettings {
                attribute: Some(magnitude_threshold),
                // The validated field is present, so no engine missing-field path runs.
                missing_attribute_value: 0.0,
            },
        );
        self.intents =
            riding_intentions::produce(&self.controller, self.actor_flags, self.preferences);
        self.intents.extend(wipeout_intentions::produce(
            &self.controller, self.actor_flags, physical_capabilities,
        ));
        //GenerateActionGraphIntents82594310 clears the AG map through82BC1B68
        //before Listener::Fill. MG lifecycle intents use a different persistent map.
        self.action_intents.clear();
        for intent in &self.intents {
            self.action_intents.insert(intent.name, intent.value);
        }
        self.ticks += 1;
    }
}
