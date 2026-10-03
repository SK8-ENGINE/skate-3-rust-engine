//! The local player's audio state through the production frame (headless, data-gated): a scripted
//! controller (push, carves, ollies, a powerslide, a long roll) drives `frame::advance`, and the
//! audio bridge `game_audio::skate_events::observe` runs after every tick as in the game. Every
//! published sample (`Cues::riding`, the whole `AudioState` included) is written as one Debug line.
//!
//!   SKATE3_ASSET_ROOT=<repo>/assets/private AUDIO_STATE_CAPTURE=<file>
//!   cargo test -p skate-game --release --bin skate3rust --locked -- --ignored audio_state_capture --nocapture
//!
//! A missing file is written (the baseline); an existing one is compared line by line, so a refactor
//! of the bridge is proved identical by running this once before and once after it (doc 11 "World
//! audio hook-in"). The test also checks that the per-skater builder
//! (`skate_events::skater_audio_state`) on its own memory gives the same state as `observe`.
use super::*;
use bevy::prelude::{Time, World};

#[test]
#[ignore = "needs the private install data"]
fn audio_state_capture() {
    let root = std::env::var_os("SKATE3_ASSET_ROOT").expect("missing private data: set SKATE3_ASSET_ROOT");
    let root = std::path::Path::new(&root);
    let assets = skate_data::GameAssets::load(root).expect("missing private data: assets");
    let graphs = crate::graph_runtime::StockGraphs::load(root, &assets).unwrap();
    let physics = GamePhysics::load(root).unwrap();
    let skater = SkaterRuntime::load(root, &graphs, &physics, "normal").unwrap();
    let mut controls = PlayerControls::default();
    let mut input = crate::input::ControllerInput::default();
    let mut camera = crate::camera::CameraRuntime::load(root).unwrap();
    let dt = physics.settings.step.simulation.time_step;
    let mut world = World::new();
    world.insert_resource(physics);
    world.insert_resource(skater);
    world.insert_resource(crate::game_audio::skate_events::Cues::default());
    world.insert_resource(Time::<()>::default());
    let observe = world.register_system(crate::game_audio::skate_events::observe);
    let mut lines = Vec::new();
    // The per-skater builder on its own memory: what an AI-skater system would call.
    let mut builder = crate::game_audio::skate_events::SkaterAudioMemory::default();
    // No host takes the steps here, so `Cues::publish` keeps OR-latching the one-step pulses
    // (`latch_pulses`): the builder's own sample gets the same latch for the comparison.
    let mut latched = false;
    for tick in 0..1500u32 {
        // Push (A) from 12 to 300, carve right / left, two ollies (right stick down → up), a
        // powerslide, then roll out.
        let (buttons, left, right) = match tick {
            12..=300 => (0x1000, [0, 0], [0, 0]),
            301..=420 => (0, [16000, 0], [0, 0]),
            421..=540 => (0, [-16000, 0], [0, 0]),
            560..=566 | 760..=766 => (0, [0, 0], [0, -32767]),
            567..=572 | 767..=772 => (0, [0, 0], [0, 32767]),
            900..=1000 => (0x1000, [0, 0], [0, 0]),
            1012..=1023 => (0, [32767, 0], [0, 0]),
            1036..=1095 => (0, [22000, 32767], [0, 0]),
            _ => (0, [0, 0], [0, 0]),
        };
        input.sample_raw_for_test(skate_core::input::xbox::XboxState { buttons, triggers: [0; 2], left, right });
        let mut actions = input.player_actions();
        world.resource_scope(|world, mut physics: bevy::prelude::Mut<GamePhysics>| {
            let mut skater = world.resource_mut::<SkaterRuntime>();
            controls.update(&mut actions, dt, physics.settings.input_magnitude_threshold, skater.player_input.physical.scoring.capabilities_204);
            frame::advance(&mut physics, &mut skater, &mut controls, &graphs, &mut actions, true, &mut camera)
                .unwrap_or_else(|e| panic!("tick {tick}: {e}"));
        });
        world.resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(dt));
        let mut built = {
            let dt = world.resource::<Time>().delta_secs();
            crate::game_audio::skate_events::skater_audio_state(world.resource::<GamePhysics>(), world.resource::<SkaterRuntime>(), &mut builder, dt)
        };
        latched |= built.push_trigger;
        built.push_trigger = latched;
        world.run_system(observe).unwrap();
        let cues = world.resource::<crate::game_audio::skate_events::Cues>();
        assert_eq!(format!("{built:?}"), format!("{:?}", cues.riding.audio), "tick {tick}: the builder differs from observe");
        lines.push(format!("{tick}\t{:?}", cues.riding));
    }
    let text = lines.join("\n");
    let Some(path) = std::env::var_os("AUDIO_STATE_CAPTURE") else { return };
    let path = std::path::Path::new(&path);
    match std::fs::read_to_string(path) {
        Ok(base) => {
            let (a, b): (Vec<&str>, Vec<&str>) = (base.lines().collect(), text.lines().collect());
            assert_eq!(a.len(), b.len(), "sample count");
            for (x, y) in a.iter().zip(&b) {
                assert_eq!(x, y, "the published audio state changed");
            }
            eprintln!("AUDIO_STATE_CAPTURE identical: {} samples", b.len());
        }
        Err(_) => {
            std::fs::write(path, text).unwrap();
            eprintln!("AUDIO_STATE_CAPTURE written: {} samples", lines.len());
        }
    }
}
