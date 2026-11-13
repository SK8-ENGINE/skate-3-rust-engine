//! Real Xbox bail chord through input, stock graphs, physical state and pose.
use super::*;
use skate_core::{input::xbox::XboxState, player::state::PhysicalStateId};
#[path = "bail_geometry.rs"]
mod geometry;

#[test]
#[ignore = "requires private stock animation banks and collections"]
fn raw_controller_bail_runs_stock_wipeout_for_900_ticks() {
    let root = std::env::var_os("SKATE3_ASSET_ROOT").expect("set SKATE3_ASSET_ROOT");
    let root = std::path::Path::new(&root);
    let assets = skate_data::GameAssets::load(root).unwrap();
    let graphs = crate::graph_runtime::StockGraphs::load(root, &assets).unwrap();
    let mut physics = GamePhysics::load(root).unwrap();
    let mut skater = SkaterRuntime::load(root, &graphs, &physics, "normal").unwrap();
    let mut controls = PlayerControls::default();
    let mut input = crate::input::ControllerInput::default();
    let mut camera = crate::camera::CameraRuntime::load(root).unwrap();
    let mut settled_ticks = 0;
    let mut requested_at = None;
    let mut entered_at = None;
    let mut saw_action_request = false;
    let mut geometry = geometry::Geometry::default();
    for tick in 0..1200 {
        let request = settled_ticks >= 60 && requested_at.is_none();
        if request {
            requested_at = Some(tick);
        }
        //8259BDCC..BEF0 requires exact full trigger endpoints, both stick
        //buttons and a fresh edge. Supply one raw packet, then release it.
        input.sample_raw_for_test(XboxState {
            buttons: if request { 0x00C0 } else { 0 },
            triggers: if request { [255; 2] } else { [0; 2] },
            left: [0; 2],
            right: [0; 2],
        });
        let mut actions = input.player_actions();
        controls.update(
            &mut actions,
            physics.settings.step.simulation.time_step,
            physics.settings.input_magnitude_threshold,
            skater.player_input.physical.scoring.capabilities_204,
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
        .unwrap_or_else(|error| {
            panic!(
                "Raw bail tick{tick}: {error}; state={:?}; requested={requested_at:?}; entered={entered_at:?}",
                skater.player_state.current()
            )
        });
        let action_request = skater.animation.action.action_intents.get("WipeOutRequest");
        if action_request == Some(&1.0) {
            assert!(
                requested_at.is_some(),
                "Bail intent appeared before the raw chord"
            );
            saw_action_request = true;
        }
        let state = skater.player_state.current();
        if state == PhysicalStateId::WipeoutGround {
            geometry.observe(&physics, &skater, tick);
            // WipeoutEnter82D3B5E8 releases both board drive channels;
            // SetPhysicsState82DB8540/LetGo82D75440 releases possession.
            // Finite transforms alone cannot detect a stale attachment.
            assert_eq!(
                physics.board.hook().drive.dynamics,
                [0, 0, 0, 2, 0, 0, 0, 2],
                "Deck still driven at bail tick{tick}"
            );
            assert_eq!(
                skater.board_possession.state.selected_hand_424, 2,
                "Hand still owns released board at tick{tick}"
            );
            for hand in &skater.board_possession.state.hands {
                assert_eq!(
                    hand.dynamics,
                    [[0, 0, 0, 2]; 2],
                    "Hand/deck constraint survives bail tick{tick}"
                );
            }
            assert_eq!(physics.board.collision_group(), 7);
            assert!(skater.skeleton_collision.is_ragdoll);
        }
        if requested_at.is_none() {
            assert_ne!(
                state,
                PhysicalStateId::WipeoutGround,
                "Bailed before controller request"
            );
            if state == PhysicalStateId::PhysicsGround
                && physics.riding.ground.wheel_contact_count > 0
            {
                settled_ticks += 1;
            } else {
                settled_ticks = 0;
            }
        } else if state == PhysicalStateId::WipeoutGround && entered_at.is_none() {
            assert!(
                saw_action_request,
                "State300 entered without stock ActionGraph bail intent"
            );
            entered_at = Some(tick);
        }
        assert_finite(&physics, &skater, tick);
        assert_eq!(
            skater.pose_generation, physics.ticks,
            "Stale pose at tick{tick}"
        );
        assert!(
            camera.frame.is_some(),
            "Missing gameplay camera at tick{tick}"
        );
        if let Some(entered) = entered_at {
            if tick - entered >= 900 {
                eprintln!(
                    "Raw bail completed: request={requested_at:?}, state300={entered}, final_tick={tick}"
                );
                eprintln!(
                    "Post-solve bail geometry (meters/tick/joint; no parity tolerance): {geometry:?}"
                );
                return;
            }
        }
    }
    assert!(
        requested_at.is_some(),
        "Never settled for60 consecutive grounded ticks"
    );
    assert!(
        saw_action_request,
        "Raw chord never reached stock ActionGraph WipeOutRequest"
    );
    panic!("Did not complete900 ticks after state300: entered={entered_at:?}");
}

fn assert_finite(physics: &GamePhysics, skater: &SkaterRuntime, tick: usize) {
    for body in physics
        .board
        .bodies()
        .iter()
        .chain(skater.skeleton.bodies())
    {
        for vector in [
            body.rates.position,
            body.rates.linear_velocity,
            body.rates.angular_velocity,
        ] {
            assert!(
                [vector.x, vector.y, vector.z]
                    .into_iter()
                    .all(f32::is_finite),
                "Nonfinite physical body at tick{tick}: {body:?}"
            );
        }
    }
    assert!(
        physics.board.part_transforms().iter().all(|part| part
            .basis
            .columns
            .iter()
            .flatten()
            .all(|v| v.is_finite())),
        "Nonfinite board orientation at tick{tick}"
    );
    assert!(
        skater
            .skeleton
            .part_transforms()
            .iter()
            .flatten()
            .flatten()
            .all(|v| v.is_finite()),
        "Nonfinite physical skeleton pose at tick{tick}"
    );
    assert!(
        !skater.animation.pose.is_empty(),
        "Missing animation pose at tick{tick}"
    );
    assert!(
        skater.animation.pose.iter().all(|sample| sample
            .scale
            .iter()
            .chain(&sample.rotation)
            .chain(&sample.translation)
            .all(|v| v.is_finite())),
        "Nonfinite animation at tick{tick}"
    );
}

/// Exercise the actual actor checkpoint reply, not a forced physical state.
#[test]
#[ignore = "requires private stock animation banks and collections"]
fn authored_checkpoint_reply_runs_teleport_state_and_restores_riding() {
    checkpoint_reply(false);
}

#[test]
#[ignore = "requires private stock animation banks and collections"]
fn marker_reply_restores_on_foot() {
    checkpoint_reply(true);
}

fn checkpoint_reply(on_foot: bool) {
    let root = std::env::var_os("SKATE3_ASSET_ROOT").expect("set SKATE3_ASSET_ROOT");
    let root = std::path::Path::new(&root);
    let assets = skate_data::GameAssets::load(root).unwrap();
    let graphs = crate::graph_runtime::StockGraphs::load(root, &assets).unwrap();
    let mut physics = GamePhysics::load(root).unwrap();
    let mut skater = SkaterRuntime::load(root, &graphs, &physics, "normal").unwrap();
    let mut controls = PlayerControls::default();
    let mut input = crate::input::ControllerInput::default();
    let mut camera = crate::camera::CameraRuntime::load(root).unwrap();
    let mut checkpoint = physics.board.bodies()[6].rates.position;
    let destination = if on_foot { PhysicalStateId::BipedGround } else { PhysicalStateId::PhysicsGround };
    let mut saw_ready = false;
    let mut restored_ticks = 0;
    for tick in 0..160 {
        if tick == 60 {
            assert!((physics.board.bodies()[6].rates.position.z - checkpoint.z).abs() > 0.1);
            if on_foot {
                let transform = skater.animated_skeleton.roots.animation_to_world;
                checkpoint = skate_core::math::Vector3::new(transform[3][0], transform[3][1], transform[3][2]);
                skater.teleport_state.request_manual(transform, false);
            }
            if !on_foot { skater.teleport_state.request_checkpoint(); }
        }
        input.sample_raw_for_test(XboxState {
            buttons: if (12..60).contains(&tick) { 0x1000 } else { 0 },
            triggers: [0; 2],
            left: [0; 2],
            right: [0; 2],
        });
        let mut actions = input.player_actions();
        controls.update(
            &mut actions,
            physics.settings.step.simulation.time_step,
            physics.settings.input_magnitude_threshold,
            skater.player_input.physical.scoring.capabilities_204,
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
        .unwrap_or_else(|error| panic!("Checkpoint tick{tick}: {error}"));
        if tick == 60 {
            assert_eq!(skater.player_state.current(), PhysicalStateId::Teleporting);
            let output = skater
                .player_input
                .physical
                .teleport_output
                .expect("702 must publish the captured reply");
            assert_eq!(
                (output.next_state, output.state_61, output.board_272),
                (if on_foot { 500 } else { 100 }, 1, 1)
            );
            assert_eq!(f32::from_bits(output.transform[3][2]), checkpoint.z);
            saw_ready = true;
        } else if tick == 61 {
            assert!(saw_ready);
            assert_eq!(
                skater.player_state.current(),
                PhysicalStateId::PhysicsGround
            );
            let position = physics.board.bodies()[6].rates.position;
            assert!((position.x - checkpoint.x).abs() < 0.05);
            assert!((position.z - checkpoint.z).abs() < 0.05);
            assert!(!skater.skeleton_collision.is_ragdoll);
        }
        if tick > 61 && skater.player_state.current() == destination {
            restored_ticks += 1;
        }
        assert_finite(&physics, &skater, tick);
    }
    assert!(saw_ready && restored_ticks > 60);
}

/// b471 T1 / T5: a teleport is ONE placement (retail's actor reply), and a second request while it is in flight is
/// refused. Before the fix the host teleport added a second reset a tick earlier (placements rose by 2).
#[test]
#[ignore = "requires private stock animation banks and collections"]
fn travel_is_one_placement_and_refuses_a_second_request() {
    travel_once(None);
}

/// b471 T4: a mod's arrival velocity survives the single reset.
#[test]
#[ignore = "requires private stock animation banks and collections"]
fn travel_keeps_the_mods_arrival_velocity() {
    travel_once(Some([2.0, 0.0, 0.0]));
}

/// Laptop session 2026-10-21 21:38: an on-foot arrival (Challenge Map, a marker set on foot) went Teleporting ->
/// BipedGround -> PhysicsAir in one tick and flung the skater tens of metres. It must stay at the target.
#[test]
#[ignore = "requires private stock animation banks and collections"]
fn travel_on_foot_stays_at_the_target() {
    let root = std::env::var_os("SKATE3_ASSET_ROOT").expect("set SKATE3_ASSET_ROOT");
    let root = std::path::Path::new(&root);
    let assets = skate_data::GameAssets::load(root).unwrap();
    let graphs = crate::graph_runtime::StockGraphs::load(root, &assets).unwrap();
    let mut physics = GamePhysics::load(root).unwrap();
    let mut skater = SkaterRuntime::load(root, &graphs, &physics, "normal").unwrap();
    let mut controls = PlayerControls::default();
    let mut input = crate::input::ControllerInput::default();
    let mut camera = crate::camera::CameraRuntime::load(root).unwrap();
    let mut target = [[0.0f32; 4]; 4];
    for tick in 0..180 {
        if tick == 60 {
            target = skater.animated_skeleton.roots.animation_to_world;
            target[3][2] += 5.0;
            skater.travel_ex(target, None, false).unwrap();
        }
        input.sample_raw_for_test(XboxState { buttons: 0, triggers: [0; 2], left: [0; 2], right: [0; 2] });
        let mut actions = input.player_actions();
        controls.update(
            &mut actions,
            physics.settings.step.simulation.time_step,
            physics.settings.input_magnitude_threshold,
            skater.player_input.physical.scoring.capabilities_204,
        );
        frame::advance(&mut physics, &mut skater, &mut controls, &graphs, &mut actions, true, &mut camera)
            .unwrap_or_else(|error| panic!("On-foot travel tick{tick}: {error}"));
        if tick > 61 {
            let at = skater.animated_skeleton.roots.animation_to_world[3];
            let off = ((at[0] - target[3][0]).powi(2) + (at[2] - target[3][2]).powi(2)).sqrt();
            assert!(off < 2.0, "tick {tick}: {:?} is {off:.1} m from the target, state {:?}", at,
                skater.player_state.current());
        }
    }
    assert_eq!(skater.player_state.current(), PhysicalStateId::BipedGround, "an on-foot arrival ends on foot");
}

fn travel_once(velocity: Option<[f32; 3]>) {
    let root = std::env::var_os("SKATE3_ASSET_ROOT").expect("set SKATE3_ASSET_ROOT");
    let root = std::path::Path::new(&root);
    let assets = skate_data::GameAssets::load(root).unwrap();
    let graphs = crate::graph_runtime::StockGraphs::load(root, &assets).unwrap();
    let mut physics = GamePhysics::load(root).unwrap();
    let mut skater = SkaterRuntime::load(root, &graphs, &physics, "normal").unwrap();
    let mut controls = PlayerControls::default();
    let mut input = crate::input::ControllerInput::default();
    let mut camera = crate::camera::CameraRuntime::load(root).unwrap();
    let mut target = [[0.0f32; 4]; 4];
    let mut before = 0;
    for tick in 0..130 {
        if tick == 60 {
            target = skater.animated_skeleton.roots.animation_to_world;
            target[3][2] += 5.0;
            before = skater.placements();
            skater.travel(target, velocity).unwrap();
            assert!(skater.teleport_pending());
            assert!(skater.travel(target, None).is_err(), "a second request while one is in flight");
        }
        input.sample_raw_for_test(XboxState { buttons: 0, triggers: [0; 2], left: [0; 2], right: [0; 2] });
        let mut actions = input.player_actions();
        controls.update(
            &mut actions,
            physics.settings.step.simulation.time_step,
            physics.settings.input_magnitude_threshold,
            skater.player_input.physical.scoring.capabilities_204,
        );
        frame::advance(&mut physics, &mut skater, &mut controls, &graphs, &mut actions, true, &mut camera)
            .unwrap_or_else(|error| panic!("Travel tick{tick}: {error}"));
        if tick > 60 && !skater.teleport_pending() && skater.placements() == before + 1 {
            if let Some(v) = velocity {
                let got = physics.board.bodies()[6].rates.linear_velocity;
                assert!((got.x - v[0]).abs() < 0.3, "arrival velocity {got:?}");
                return;
            }
        }
        assert_finite(&physics, &skater, tick);
    }
    assert_eq!(skater.placements(), before + 1, "one placement per teleport");
    let position = physics.board.bodies()[6].rates.position;
    assert!((position.z - target[3][2]).abs() < 0.5, "landed at the target: {position:?}");
}

/// b475 G1: a teleport target 3 m above the ground lands on it (ground + 0.15, plus the reset's 0.1) instead of
/// falling; retail `82598428` fits the arrival height with one ray down.
#[test]
#[ignore = "requires private stock animation banks and collections"]
fn travel_fits_the_arrival_onto_the_ground() {
    let root = std::env::var_os("SKATE3_ASSET_ROOT").expect("set SKATE3_ASSET_ROOT");
    let root = std::path::Path::new(&root);
    let assets = skate_data::GameAssets::load(root).unwrap();
    let graphs = crate::graph_runtime::StockGraphs::load(root, &assets).unwrap();
    let mut physics = GamePhysics::load(root).unwrap();
    let mut skater = SkaterRuntime::load(root, &graphs, &physics, "normal").unwrap();
    let mut controls = PlayerControls::default();
    let mut input = crate::input::ControllerInput::default();
    let mut camera = crate::camera::CameraRuntime::load(root).unwrap();
    let mut ground = None;
    for tick in 0..62 {
        if tick == 60 {
            let mut target = skater.animated_skeleton.roots.animation_to_world;
            let start = skate_core::math::Vector3::new(target[3][0], target[3][1] + 0.75, target[3][2]);
            let end = skate_core::math::Vector3::new(start.x, start.y - 500.0, start.z);
            ground = physics.world.query_thin_line(start, end).unwrap().map(|h| h.geometry.position.y);
            target[3][1] += 3.0;
            skater.travel(target, None).unwrap();
        }
        input.sample_raw_for_test(XboxState { buttons: 0, triggers: [0; 2], left: [0; 2], right: [0; 2] });
        let mut actions = input.player_actions();
        controls.update(
            &mut actions,
            physics.settings.step.simulation.time_step,
            physics.settings.input_magnitude_threshold,
            skater.player_input.physical.scoring.capabilities_204,
        );
        frame::advance(&mut physics, &mut skater, &mut controls, &graphs, &mut actions, true, &mut camera)
            .unwrap_or_else(|error| panic!("Fit tick{tick}: {error}"));
    }
    let ground = ground.expect("the test spawn stands on ground");
    let deck = physics.board.bodies()[6].rates.position.y;
    assert!(deck - ground < 1.0, "landed near the ground ({ground}), not 3 m up: deck {deck}");
    assert!(physics.board.bodies()[6].rates.linear_velocity.y.abs() < 0.5, "no fall after the arrival");
}

/// b457 / b469 row 14: a Challenge Map teleport to another map waits for the map transition and places once on the
/// new map; when the transition left the old map current, it is dropped (never another map's coordinates).
#[test]
#[ignore = "requires private stock animation banks and collections"]
fn a_far_challenge_map_teleport_places_after_its_map_loads() {
    use crate::challenge_map::{finish_travel, ChallengeMapTravel, TeleportRequest};
    use crate::map_transition::{CurrentMap, MapTransition};
    use bevy::ecs::system::RunSystemOnce;
    use std::path::PathBuf;
    let root = std::env::var_os("SKATE3_ASSET_ROOT").expect("set SKATE3_ASSET_ROOT");
    let root = std::path::Path::new(&root);
    let assets = skate_data::GameAssets::load(root).unwrap();
    let graphs = crate::graph_runtime::StockGraphs::load(root, &assets).unwrap();
    let physics = GamePhysics::load(root).unwrap();
    let skater = SkaterRuntime::load(root, &graphs, &physics, "normal").unwrap();
    let (downtown, university) = (PathBuf::from("maps/DownTown.skate"), PathBuf::from("maps/University.skate"));
    let current = |path: &PathBuf| CurrentMap {
        path: Some(path.clone()), name: String::new(), spawn: [0.0; 3], heading: 0.0, generation: 0, audio_tag: None, layout: None,
    };
    let mut target = skater.animated_skeleton.roots.animation_to_world;
    target[3][2] += 5.0;
    let request = TeleportRequest { destination: "dist_university_x".into(), map: "University".into(), transform: target, on_board: false, session_marker: false };
    let entry = crate::map_library::Entry { label: "University".into(), path: Some(university.clone()), world: None };
    let start = |world: &mut World| {
        let mut transition = MapTransition::default();
        let mut travel = ChallengeMapTravel::default();
        travel.start((request.clone(), Some(entry.clone())), Some(downtown.clone()), &mut transition);
        assert!(transition.busy(), "another map: the transition is requested");
        world.insert_resource(transition);
        world.insert_resource(travel);
        world.insert_resource(current(&downtown));
    };
    let mut world = World::new();
    world.insert_resource(skater);
    start(&mut world);
    world.run_system_once(finish_travel).unwrap();
    assert!(!world.resource::<SkaterRuntime>().teleport_pending(), "nothing while the map loads");
    assert!(world.resource::<ChallengeMapTravel>().pending.is_some());
    world.insert_resource(MapTransition::default());
    world.insert_resource(current(&university));
    world.run_system_once(finish_travel).unwrap();
    assert!(world.resource::<SkaterRuntime>().teleport_pending(), "placed once the new map is current");
    assert!(world.resource::<ChallengeMapTravel>().pending.is_none());
    // A transition that ended with the old map still current drops the teleport.
    let mut world = World::new();
    world.insert_resource(SkaterRuntime::load(root, &graphs, &physics, "normal").unwrap());
    start(&mut world);
    world.insert_resource(MapTransition::default());
    world.run_system_once(finish_travel).unwrap();
    assert!(!world.resource::<SkaterRuntime>().teleport_pending(), "dropped on the old map");
    assert!(world.resource::<ChallengeMapTravel>().pending.is_none());
}