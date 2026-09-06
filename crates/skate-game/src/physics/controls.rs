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
    gestures: Option<crate::input::gesture_input::GestureInput>,
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
            gestures: None,
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
    player.publish_gestures(physics.animation_profile.physics_mode, skater.player_input.physical.state.state_16);
}

impl PlayerControls {
    pub fn load(root: &std::path::Path) -> Result<Self, String> {
        Ok(Self { gestures: Some(crate::input::gesture_input::GestureInput::load(root)?), ..Self::default() })
    }

    pub fn publish_gestures(&mut self, difficulty: u32, physical_state: u32) {
        if let Some(gestures) = &mut self.gestures {
            let words = self.controller.words();
            let axes = [[words[7],words[8]], [words[9],words[10]]].map(|p|p.map(f32::from_bits));
            gestures.publish(axes,difficulty,self.actor_flags,physical_state,&mut self.action_intents);
        }
    }

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
        self.intents.extend(skate_core::input::anticipation_intentions::produce(&self.controller));
        //GenerateActionGraphIntents82594310 clears the AG map through82BC1B68
        //before Listener::Fill. MG lifecycle intents use a different persistent map.
        self.action_intents.clear();
        for intent in &self.intents {
            self.action_intents.insert(intent.name, intent.value);
        }
        self.ticks += 1;
    }
}
