//! Diagnostic (data-gated): the cost of each physics tick through ollies and flip tricks, on a
//! normal and on a large air, the first time and repeated. Looks for the user's report "lag spike
//! on every flip on a large jump": a tick that costs much more than the others, which phase it is
//! in (`frame_timing::hitch` phases) and whether it recurs or only happens on first use.
//!
//!   SKATE3_ASSET_ROOT=<repo>/assets/private
//!   cargo test -p skate-game --release --bin skate3rust --locked -- --ignored flip_hitch_timing --nocapture
//!
//! A large air is made by adding upward speed to every body on the first airborne tick (the
//! gesture and takeoff are the game's own). Measuring only: no assertion on the times.
use super::*;


#[test]
#[ignore = "needs the private install data; timing diagnostic"]
fn flip_hitch_timing() {
    let root = std::env::var_os("SKATE3_ASSET_ROOT").expect("missing private data: set SKATE3_ASSET_ROOT");
    let root = std::path::Path::new(&root);
    let assets = skate_data::GameAssets::load(root).expect("missing private data: assets");
    let graphs = crate::graph_runtime::StockGraphs::load(root, &assets).unwrap();
    let mut physics = GamePhysics::load(root).unwrap();
    let mut skater = SkaterRuntime::load(root, &graphs, &physics, "normal").unwrap();
    let mut controls = PlayerControls::load(root).unwrap();
    let mut input = crate::input::ControllerInput::default();
    let mut camera = crate::camera::CameraRuntime::load(root).unwrap();
    let dt = physics.settings.step.simulation.time_step;
    let mut step = |physics: &mut GamePhysics, skater: &mut SkaterRuntime, right: [i16; 2]| -> (f64, [u64; 6]) {
        input.sample_raw_for_test(skate_core::input::xbox::XboxState { buttons: 0, triggers: [0; 2], left: [0; 2], right });
        let mut actions = input.player_actions();
        controls.update(&mut actions, dt, physics.settings.input_magnitude_threshold, skater.player_input.physical.scoring.capabilities_204);
        controls.publish_gestures(physics.animation_profile.physics_mode, skater.player_input.physical.state.state_16);
        let _ = crate::frame_timing::hitch::take_phases();
        let t = std::time::Instant::now();
        frame::advance(physics, skater, &mut controls, &graphs, &mut actions, true, &mut camera).unwrap();
        (t.elapsed().as_secs_f64() * 1000.0, crate::frame_timing::hitch::take_phases())
    };
    for _ in 0..90 {
        step(&mut physics, &mut skater, [0; 2]);
    }
    let set_velocity = |physics: &mut GamePhysics, skater: &mut SkaterRuntime, v: Vector3, add: bool| {
        for body in physics.board.bodies_mut().iter_mut().chain(skater.skeleton.bodies_mut()) {
            body.rates.linear_velocity = if add {
                let o = body.rates.linear_velocity;
                Vector3::new(o.x + v.x, o.y + v.y, o.z + v.z)
            } else {
                v
            };
        }
    };
    // (name, flick, extra upward speed on the first airborne tick)
    let ollie = [0, 32767];
    let kickflip = [-23170, 23170];
    let heelflip = [23170, 23170];
    let runs: [(&str, [i16; 2], f32); 10] = [
        ("ollie", ollie, 0.0),
        ("kickflip", kickflip, 0.0),
        ("kickflip again", kickflip, 0.0),
        ("ollie big", ollie, 7.0),
        ("kickflip big", kickflip, 7.0),
        ("kickflip big again", kickflip, 7.0),
        ("heelflip big", heelflip, 7.0),
        ("heelflip big again", heelflip, 7.0),
        ("kickflip big 3rd", kickflip, 7.0),
        ("ollie big again", ollie, 7.0),
    ];
    for (name, flick, boost) in runs {
        set_velocity(&mut physics, &mut skater, Vector3::new(0., 0., 5.), false);
        let mut ticks: Vec<(f64, u32, u32, String, [u64; 6])> = Vec::new();
        let (mut boosted, mut was_air, mut air_ticks) = (false, false, 0);
        for tick in 0..400u32 {
            let right = match tick {
                20..=29 => [0, -32767],
                30..=34 => flick,
                _ => [0; 2],
            };
            let (ms, phases) = step(&mut physics, &mut skater, right);
            let state = skater.player_state.current() as u32;
            let air = (200..300).contains(&state);
            if air {
                air_ticks += 1;
                if !boosted && boost > 0.0 {
                    set_velocity(&mut physics, &mut skater, Vector3::new(0., boost, 0.), true);
                    boosted = true;
                }
            }
            ticks.push((ms, tick, state, skater.scoring.trick_name().to_owned(), phases));
            if was_air && !air && tick > 40 {
                for _ in 0..30 {
                    let (ms, phases) = step(&mut physics, &mut skater, [0; 2]);
                    ticks.push((ms, 999, skater.player_state.current() as u32, String::new(), phases));
                }
                break;
            }
            was_air = air;
        }
        let mut sorted: Vec<f64> = ticks.iter().map(|t| t.0).collect();
        sorted.sort_by(f64::total_cmp);
        let median = sorted[sorted.len() / 2];
        let trick = ticks.iter().rev().map(|t| t.3.as_str()).find(|s| !s.is_empty()).unwrap_or("");
        eprintln!("{name}: {} ticks, air {air_ticks} ticks, trick {trick:?}, median {median:.2} ms", ticks.len());
        let mut worst: Vec<&(f64, u32, u32, String, [u64; 6])> = ticks.iter().collect();
        worst.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (ms, tick, state, _, p) in worst.iter().take(3) {
            eprintln!("   tick {tick:3} state {state} {ms:7.2} ms  anim_graphs {:.2} solve {:.2} finish {:.2} scoring {:.2}",
                p[0] as f64 / 1000.0, p[1] as f64 / 1000.0, p[2] as f64 / 1000.0, p[3] as f64 / 1000.0);
        }
    }
}
