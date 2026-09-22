//! The stock held-flip ladder and the air late flips.
//!
//! `T_Kickflip.xml` turns one authored flip into `<Trick>2`/`3`/`4` by cycling
//! `B_<TRICK>_CYC1..3`, but only while the scoop coordinate is still held *and*
//! the takeoff trajectory has the authored time left: `End.Out.Out1` claims the
//! exit at `TimeToLand < 0.525` and `Out2` at `< 0.8`, and those exits are
//! listed before the cycle transitions, so a short pop ends the ladder early.
//! `KnownAir` takes its prediction once, on entering the air, so the pop alone
//! decides how far the ladder can run. That is why these replays pop hard
//! instead of editing the world: the canonical course carries authored query
//! metadata that a rebuilt `BoardWorld` would drop.
//!
//! Every input here is a real controller sample fed through the production
//! gesture recognizer, action graph, motion graph and physics.
use super::*;
use skate_core::animation::{output::attributes::AttributeName, skeleton_input::name::encode};

/// Pop, hold the scoop, and the graph must walk the whole authored ladder.
/// The heelflip's authored clips are longer than the kickflip's, so it needs a
/// taller pop to reach the same count -- that is the authored gate, not a bug.
#[test]
#[ignore = "requires private stock graphs and animation assets"]
fn held_flips_cycle_through_the_authored_double_triple_and_quad() {
    for (trick, boost) in [("Kickflip", 8.0), ("Heelflip", 10.0)] {
        let run = replay(Input::Held { trick, boost }, 260);
        for count in ["", "2", "3", "4"] {
            assert!(
                run.tricks.contains(&format!("{trick}{count}")),
                "{trick}: held ladder never published {trick}{count}; saw {:?} over {:?}",
                run.tricks,
                run.animations
            );
        }
        let cycles = run
            .animations
            .iter()
            .filter(|a| a.ends_with("_CYC1") || a.ends_with("_CYC2") || a.ends_with("_CYC3"))
            .count();
        assert_eq!(
            cycles, 3,
            "{trick}: expected the three authored cycle clips, saw {:?}",
            run.animations
        );
    }
}

/// The ladder, landed: the quad has to reach the scorer as the quad's own
/// scorable and be named by the quad's authored label, not the single's.
#[test]
#[ignore = "requires private stock graphs and animation assets"]
fn landed_quads_bank_and_name_the_quad_scorable() {
    for (trick, boost, label) in [
        ("Kickflip", 8.0, "#ID_TRICK_FLIP_QUADRUPLE_KICKFLIP"),
        ("Heelflip", 10.0, "#ID_TRICK_FLIP_QUADRUPLE_HEELFLIP"),
    ] {
        let run = replay(Input::Held { trick, boost }, 300);
        assert!(
            run.tricks.contains(&format!("{trick}4")),
            "{trick}: the ladder no longer reaches the quad: {:?} over {:?}",
            run.tricks,
            run.animations
        );
        assert_eq!(run.name, label, "{trick}: the landed quad published");
        // The ladder converts rather than accumulating, so only the quad's own
        // authored 250 is banked -- never the single's 100 as well.
        assert!(
            run.reward > 250.,
            "{trick}: the landed quad banked {}",
            run.reward
        );
    }
}

/// The single flip is the baseline the ladder must not fall below. The ladder
/// *converts* its carrier (`LINKS`), so each count replaces the previous one's
/// reward instead of adding to it -- a regression there would silently make a
/// quad worth less than a single.
#[test]
#[ignore = "requires private stock graphs and animation assets"]
fn a_single_flip_is_the_ladder_baseline() {
    let single = replay(
        Input::Single {
            trick: "Kickflip",
            boost: 8.0,
        },
        300,
    );
    let quad = replay(
        Input::Held {
            trick: "Kickflip",
            boost: 8.0,
        },
        300,
    );
    eprintln!("single={} quad={}", single.reward, quad.reward);
    assert_eq!(
        single.tricks,
        ["Kickflip"],
        "the released flick still cycled"
    );
    assert!(
        quad.reward >= single.reward,
        "the quad banked {} but the single banked {}",
        quad.reward,
        single.reward
    );
}

/// An ollie, then the air scoop: `air.xml`'s `Lateflip` state, whose leaves name
/// `latekickflip`/`lateheelflip` and friends. These gestures live in their own
/// recognizer (`skater_air.pat`) and never enter the 270-row trick mapping, so
/// they reach the motion graph as bare intents.
#[test]
#[ignore = "requires private stock graphs and animation assets"]
fn air_scoops_reach_the_authored_late_flips() {
    for (gesture, trick, label) in [
        (
            "L_F_Kickflip",
            "LateKickflip",
            "#ID_TRICK_FLIP_LATE_KICKFLIP",
        ),
        (
            "L_F_Heelflip",
            "LateHeelflip",
            "#ID_TRICK_FLIP_LATE_HEELFLIP",
        ),
    ] {
        let run = replay(Input::Late { gesture, trick }, 260);
        assert!(
            run.tricks.contains(&trick.to_owned()),
            "{gesture}: the air scoop never published {trick}; saw {:?} over {:?}",
            run.tricks,
            run.animations
        );
        assert_eq!(run.name, label, "{gesture}: the landed late flip published");
    }
}

enum Input<'a> {
    /// Hold the ground scoop's final coordinate through the whole air.
    Held { trick: &'a str, boost: f32 },
    /// Flick and release, so the ladder ends at the single flip.
    Single { trick: &'a str, boost: f32 },
    /// Ollie, then run a `skater_air.pat` scoop once airborne.
    Late { gesture: &'a str, trick: &'a str },
}

struct Outcome {
    animations: Vec<String>,
    tricks: Vec<String>,
    reward: f32,
    name: String,
}

fn replay(input: Input<'_>, ticks: u32) -> Outcome {
    let root = std::path::PathBuf::from(
        std::env::var_os("SKATE3_ASSET_ROOT").expect("set SKATE3_ASSET_ROOT"),
    );
    let assets = skate_data::GameAssets::load(&root).unwrap();
    let graphs = crate::graph_runtime::StockGraphs::load(&root, &assets).unwrap();
    let mut physics = GamePhysics::load_with_terrain(&root, ground::Terrain::Course).unwrap();
    let mut skater = SkaterRuntime::load(&root, &graphs, &physics, "normal").unwrap();
    let mut camera = crate::camera::CameraRuntime::load(&root).unwrap();
    let mut controller = crate::input::ControllerInput::default();
    let mut controls = PlayerControls::load(&root).unwrap();

    let hold = !matches!(input, Input::Single { .. });
    // The authored scoops, straight out of the PATs the recognizer itself loads.
    let (ground_scoop, air_scoop, boost, watched) = match input {
        Input::Held { trick, boost } | Input::Single { trick, boost } => (
            authored("skater.pat", &root, trick),
            Vec::new(),
            boost,
            ["", "2", "3", "4"]
                .iter()
                .map(|count| format!("{trick}{count}"))
                .collect::<Vec<_>>(),
        ),
        Input::Late { gesture, trick } => (
            authored("skater.pat", &root, "Ollie"),
            authored("skater_air.pat", &root, gesture),
            9.0,
            vec![trick.to_owned()],
        ),
    };
    let watched: Vec<(String, AttributeName)> = watched
        .into_iter()
        .map(|name| {
            let encoded = encode(name.as_bytes());
            (name, encoded)
        })
        .collect();
    // GameInputManager 82696030 negates the mapped Y before matching.
    let sample = |p: [f32; 2]| [(p[0] * 32767.) as i16, (-p[1] * 32767.) as i16];

    let mut outcome = Outcome {
        animations: Vec::new(),
        tricks: Vec::new(),
        reward: 0.,
        name: String::new(),
    };
    let mut air_ticks = 0u32;
    for tick in 0..ticks {
        if tick == 60 {
            // A rolling approach, without a steering input that would also spin.
            for body in physics.board.bodies_mut() {
                body.rates.linear_velocity = Vector3::new(0., 0., 8.);
            }
            for body in skater.skeleton.bodies_mut() {
                body.rates.linear_velocity = Vector3::new(0., 0., 8.);
            }
        }
        if tick == 123 {
            for body in physics.board.bodies_mut() {
                body.rates.linear_velocity.y += boost;
            }
            for body in skater.skeleton.bodies_mut() {
                body.rates.linear_velocity.y += boost;
            }
        }
        let right = if air_scoop.is_empty() {
            // Crouch on the first coordinate, then hold the flick: holding the
            // final coordinate is what keeps `<Trick>Hold` published.
            if (100..112).contains(&tick) {
                sample(ground_scoop[0])
            } else if tick >= 112 && (hold || tick < 118) {
                sample(ground_scoop[ground_scoop.len() - 1])
            } else {
                [0; 2]
            }
        } else if (100..112).contains(&tick) {
            sample(ground_scoop[0])
        } else if (112..118).contains(&tick) {
            sample(ground_scoop[ground_scoop.len() - 1])
        } else if air_ticks >= 10 {
            // One coordinate every four ticks, comfortably inside the authored
            // culling window, and then release so the trick can end.
            let step = (air_ticks as usize - 10) / 4;
            air_scoop.get(step).copied().map(sample).unwrap_or([0; 2])
        } else {
            [0; 2]
        };
        controller.sample_raw_for_test(skate_core::input::xbox::XboxState {
            buttons: 0,
            triggers: [0; 2],
            left: [0; 2],
            right,
        });
        let mut actions = controller.player_actions();
        controls.update(
            &mut actions,
            physics.settings.step.simulation.time_step,
            physics.settings.input_magnitude_threshold,
            skater.player_input.physical.scoring.capabilities_204,
        );
        controls.publish_gestures(
            physics.animation_profile.physics_mode,
            skater.player_input.physical.state.state_16,
        );
        frame::advance(
            &mut physics,
            &mut skater,
            &mut controls,
            &graphs,
            &mut actions,
            true,
            &mut camera,
        )
        .unwrap();
        if skater.player_state.current().category() == 200 {
            air_ticks += 1;
        }

        let motion = &skater.animation.motion;
        if let Some(name) = motion.animation.current_name.as_deref() {
            if outcome.animations.last().map(String::as_str) != Some(name) {
                outcome.animations.push(name.to_owned());
            }
        }
        if let Some(published) = motion.score_packet.trick_names.first {
            if let Some((name, _)) = watched.iter().find(|(_, e)| *e == published) {
                if outcome.tricks.last() != Some(name) {
                    outcome.tricks.push(name.clone());
                }
            }
        }
        outcome.reward = skater.scoring.session.holder.snapshot.last_reward;
        let published = skater.scoring.hud_input().trick_name;
        if !published.is_empty() && published != "#ID_TRICK_AIR" {
            outcome.name = published;
        }
    }
    eprintln!(
        "{:?}: tricks={:?} reward={} name={:?}\n  animations={:?}",
        watched.iter().map(|(n, _)| n).collect::<Vec<_>>(),
        outcome.tricks,
        outcome.reward,
        outcome.name,
        outcome.animations
    );
    outcome
}

fn authored(file: &str, root: &std::path::Path, name: &str) -> Vec<[f32; 2]> {
    skate_data::gesture_patterns::load(&root.join("private/stock/data/joystick").join(file))
        .unwrap()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("authored {name} pattern in {file}"))
        .points
}
