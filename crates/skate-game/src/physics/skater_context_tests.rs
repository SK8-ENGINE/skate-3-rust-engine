//! Simulated NPC skaters share `GamePhysics` through the per-skater context swap: the local
//! player's simulation is bit-identical with or without an NPC skater ticking in between.
use super::*;
use skate_core::physics::board::BodyId;
use skate_core::player::state::PhysicalStateId;

fn pad() -> skate_core::input::xbox::XboxState {
    skate_core::input::xbox::XboxState::default()
}

struct Rig {
    graphs: crate::graph_runtime::StockGraphs,
    root_dir: std::path::PathBuf,
}

struct Skater {
    runtime: SkaterRuntime,
    camera: crate::camera::CameraRuntime,
    controls: PlayerControls,
    input: crate::input::ControllerInput,
}

impl Rig {
    fn skater(&self, physics: &GamePhysics) -> Skater {
        Skater {
            runtime: SkaterRuntime::load(&self.root_dir, &self.graphs, physics, "easy").unwrap(),
            camera: crate::camera::CameraRuntime::load(&self.root_dir).unwrap(),
            controls: PlayerControls::default(),
            input: crate::input::ControllerInput::default(),
        }
    }
    fn step(&self, physics: &mut GamePhysics, s: &mut Skater, state: skate_core::input::xbox::XboxState) {
        s.input.sample_raw_for_test(state);
        let mut actions = s.input.player_actions();
        s.controls.update_for_physics(&mut actions, physics, &mut s.runtime, &mut s.camera).unwrap();
        frame::advance(physics, &mut s.runtime, &mut s.controls, &self.graphs, &mut actions, true, &mut s.camera).unwrap();
    }
}

/// Deck position and velocity bits per tick.
fn trace(physics: &GamePhysics) -> [u32; 6] {
    let deck = &physics.board.bodies()[BodyId::Deck.index()].rates;
    let (p, v) = (deck.position, deck.linear_velocity);
    [p.x.to_bits(), p.y.to_bits(), p.z.to_bits(), v.x.to_bits(), v.y.to_bits(), v.z.to_bits()]
}

#[test]
#[ignore = "requires private stock graphs and an installed map (SKATE3_ASSET_ROOT, SKATE3_MAP=<maps/DownTown.skate>)"]
fn the_player_is_bit_identical_with_a_simulated_npc_skater_in_the_same_world() {
    let root_dir = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").unwrap());
    let map_path = std::path::PathBuf::from(std::env::var_os("SKATE3_MAP").unwrap());
    let map = skate_data::skate_map::SkateMap::load(&map_path).unwrap();
    let assets = skate_data::GameAssets::load(&root_dir).unwrap();
    let rig = Rig { graphs: crate::graph_runtime::StockGraphs::load(&root_dir, &assets).unwrap(), root_dir: root_dir.clone() };
    let load = || GamePhysics::load_with_difficulty(&root_dir, Some(&map), crate::difficulty::Difficulty::Easy).unwrap();
    // Push forward (A held) for a second, then roll.
    let input = |t: u32| skate_core::input::xbox::XboxState { buttons: if t < 60 { 0x1000 } else { 0 }, ..pad() };

    let mut alone = load();
    let mut player = rig.skater(&alone);
    let mut expected = Vec::new();
    for t in 0..300 {
        rig.step(&mut alone, &mut player, input(t));
        expected.push(trace(&alone));
    }

    let mut shared = load();
    let mut player = rig.skater(&shared);
    // The NPC skater: its own board 6 m to the player's side, its own runtime.
    let mut spawn = shared.board.part_transforms()[BodyId::Deck.index()];
    spawn.translation.x += 6.0;
    let mut context = shared.new_skater_context(spawn).unwrap();
    shared.swap_skater_context(&mut context);
    let mut npc = rig.skater(&shared);
    shared.swap_skater_context(&mut context);
    let start = context.board.part_transforms()[BodyId::Deck.index()].translation;
    for t in 0..300 {
        rig.step(&mut shared, &mut player, input(t));
        assert_eq!(trace(&shared), expected[t as usize], "player diverged at tick {t}");
        shared.swap_skater_context(&mut context);
        rig.step(&mut shared, &mut npc, input(t));
        shared.swap_skater_context(&mut context);
    }
    // The NPC skater simulated on its own board: it moved and did not fall through the world.
    let end = context.board.part_transforms()[BodyId::Deck.index()].translation;
    let moved = ((end.x - start.x).powi(2) + (end.z - start.z).powi(2)).sqrt();
    assert!(moved > 1.0 && (end.y - start.y).abs() < 2.0, "npc board {start:?} -> {end:?}");
    assert!(!context.failed && context.owns_props == false && shared.owns_props);
    assert_eq!(context.ticks, 300);
}

/// Steps a + b of the simulated tier: a skater with only the AI record (no pad) rides a recorded
/// DownTown NPC line on its own physics, steered by the ground states' board path.
#[test]
#[ignore = "requires private stock graphs, the living-world export and an installed map (SKATE3_ASSET_ROOT, SKATE3_MAP=<maps/DownTown.skate>)"]
fn a_simulated_skater_rides_a_recorded_line_from_its_ai_record() {
    use skate_core::living_world::replay::{path_frame, Decider, LineCursor, ReplayLine};
    let root_dir = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").unwrap());
    let map_path = std::path::PathBuf::from(std::env::var_os("SKATE3_MAP").unwrap());
    let map = skate_data::skate_map::SkateMap::load(&map_path).unwrap();
    let assets = skate_data::GameAssets::load(&root_dir).unwrap();
    let rig = Rig { graphs: crate::graph_runtime::StockGraphs::load(&root_dir, &assets).unwrap(), root_dir: root_dir.clone() };
    let mut physics = GamePhysics::load_with_difficulty(&root_dir, Some(&map), crate::difficulty::Difficulty::Easy).unwrap();
    let pack = std::fs::read(root_dir.join("private/living_world/skater_paths/DownTown.bin")).unwrap();
    let tiles = skate_data::aipath::parse_pack(&pack).unwrap();
    let (paths, _) = skate_data::aipath::district_paths(&tiles).unwrap();
    let lines: std::collections::BTreeMap<[u8; 16], ReplayLine> =
        paths.iter().filter(|p| p.path.id.is_ambient()).map(|p| (p.path.id.0, skate_data::living_world::replay_line(&p.path))).collect();
    let which = std::env::var("LINE_INDEX").ok().and_then(|v| v.parse().ok()).unwrap_or(0usize);
    // A line rolling on the ground for its first 300 frames.
    let line = lines.values().filter(|l| l.duration_frames() > 600 && l.nodes.iter().take(40).all(|n| n.flags & 0x0e == 0)).nth(which).expect("a ground line");
    let node = &line.nodes[0];
    let q = path_frame(node);
    let basis = skate_core::physics::rigid_body::basis_from_quaternion(skate_core::physics::rigid_body::RetailQuaternion { x: q[0], y: q[1], z: q[2], w: q[3] });
    let spawn = RetailAffineTransform { basis, translation: skate_core::math::Vector3::new(node.position[0], node.position[1], node.position[2]) };
    let mut context = physics.new_skater_context(spawn).unwrap();
    physics.swap_skater_context(&mut context);
    let mut npc = rig.skater(&physics);
    let mut cursor = LineCursor::spawn(&lines, line.id, 0);
    let mut errors = Vec::new();
    let start = physics.board.part_transforms()[BodyId::Deck.index()].translation;
    for tick in 0..300 {
        let target = cursor.line_target(&lines).unwrap();
        let deck = physics.board.part_transforms()[BodyId::Deck.index()];
        let forward = [deck.basis.columns[2][0], deck.basis.columns[2][1], deck.basis.columns[2][2]];
        let record = skate_core::living_world::ai_record::build(&target, forward, &Default::default());
        npc.runtime.ai_physics = Some(super::skater::AiPhysicsSource { record, fresh: true });
        // Retail spawn push while still on the start node (824701F8 -> 82C04168, every part).
        let node = &line.nodes[cursor.node as usize];
        let deck_at = [deck.translation.x, deck.translation.y, deck.translation.z];
        if let Some(v) = skate_core::living_world::ai_record::spawn_push(deck_at, node.position, target.step, cursor.node == 0, false) {
            for body in physics.board.bodies_mut() {
                body.rates.linear_velocity = skate_core::math::Vector3::new(v[0], v[1], v[2]);
            }
        }
        rig.step(&mut physics, &mut npc, pad());
        assert_eq!(npc.runtime.player_state.current(), PhysicalStateId::PhysicsGround, "on-board steering (bit 25) keeps the skater in ground physics, tick {tick}");
        cursor.step(&lines, &mut Decider::Stay, &mut Vec::new());
        let deck = physics.board.part_transforms()[BodyId::Deck.index()].translation;
        let e = ((deck.x - target.position[0]).powi(2) + (deck.z - target.position[2]).powi(2)).sqrt();
        errors.push(e);
        if tick % 30 == 0 {
            let v = physics.board.bodies()[BodyId::Deck.index()].rates.linear_velocity;
            let speed = (v.x * v.x + v.z * v.z).sqrt();
            let want = (target.step[0].powi(2) + target.step[2].powi(2)).sqrt() * 60.0;
            eprintln!("tick {tick} state {:?} speed {speed:.2} target speed {want:.2} error {e:.2}", npc.runtime.player_state.current());
        }
    }
    let end = physics.board.part_transforms()[BodyId::Deck.index()].translation;
    physics.swap_skater_context(&mut context);
    let mut sorted = errors.clone();
    sorted.sort_by(f32::total_cmp);
    eprintln!("error median {:.3} p90 {:.3} max {:.3}", sorted[150], sorted[270], sorted[299]);
    // With the retail spawn push it holds the recorded line: on DownTown lines 0..6 the median
    // error is 0.06..0.2 m (max under 0.6 m) except line 5, where the board loses speed on ground
    // geometry the recording rolls through (open, doc 26).
    if std::env::var_os("LINE_INDEX").is_none() {
        assert!(sorted[299] < 1.0 && sorted[150] < 0.5, "tracking: median {} max {}", sorted[150], sorted[299]);
    }
    let moved = ((end.x - start.x).powi(2) + (end.z - start.z).powi(2)).sqrt();
    assert!(moved > 5.0 && (end.y - start.y).abs() < 1.0, "{start:?} -> {end:?}");
}

/// A ped takedown on the player (`82592390`: actor `1904` bit 30 + direction) is published as the
/// packet's external impulse; retail's motion graph enters WipeOut on `IsPhysicsWiping` (b15).
/// Prints whether our skater reaches WipeoutGround (retail's switch is not proven statically).
#[test]
#[ignore = "requires private stock graphs and an installed map (SKATE3_ASSET_ROOT, SKATE3_MAP=<maps/DownTown.skate>)"]
fn a_ped_takedown_publishes_the_external_impulse_and_wipes_the_skater_out() {
    let root_dir = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").unwrap());
    let map_path = std::path::PathBuf::from(std::env::var_os("SKATE3_MAP").unwrap());
    let map = skate_data::skate_map::SkateMap::load(&map_path).unwrap();
    let assets = skate_data::GameAssets::load(&root_dir).unwrap();
    let rig = Rig { graphs: crate::graph_runtime::StockGraphs::load(&root_dir, &assets).unwrap(), root_dir: root_dir.clone() };
    let mut physics = GamePhysics::load_with_difficulty(&root_dir, Some(&map), crate::difficulty::Difficulty::Easy).unwrap();
    let mut player = rig.skater(&physics);
    let input = |t: u32| skate_core::input::xbox::XboxState { buttons: if t < 60 { 0x1000 } else { 0 }, ..pad() };
    for t in 0..120 {
        rig.step(&mut physics, &mut player, input(t));
    }
    assert_eq!(player.runtime.player_state.current(), PhysicalStateId::PhysicsGround);
    // The chaser hits from behind: 1 m against the board's motion.
    let deck = physics.board.bodies()[BodyId::Deck.index()].rates;
    let v = deck.linear_velocity;
    let speed = (v.x * v.x + v.z * v.z).sqrt().max(1e-3);
    let at = [deck.position.x, deck.position.y, deck.position.z];
    let chaser = [at[0] - v.x / speed, at[1], at[2] - v.z / speed];
    player.runtime.takedown = Some(super::skater::Takedown::from_positions(at, chaser));
    let mut states = Vec::new();
    for _ in 0..90 {
        rig.step(&mut physics, &mut player, pad());
        states.push(player.runtime.player_state.current());
    }
    eprintln!("takedown states: {:?}", states.iter().map(|s| *s as u32).collect::<Vec<_>>());
    assert!(states.contains(&PhysicalStateId::WipeoutGround), "no wipeout within 3 s of the takedown");
    assert!(player.runtime.takedown.is_none(), "the latch is used by WipeoutGround Enter");
}

/// Retail's car-hit bail (`sub_82D90C98`, b37): the skeleton's largest relative normal speed
/// against a vehicle-group body above `Wipeout_GroundVehicleContact` (9.0) wipes the skater
/// out. A traffic car (kinematic box in the vehicle group, as `living_world::vehicles::proxy`)
/// driving into the standing skater at 12 m/s knocks them down at once.
#[test]
#[ignore = "requires private stock graphs and an installed map (SKATE3_ASSET_ROOT, SKATE3_MAP=<maps/DownTown.skate>)"]
fn a_traffic_car_knocks_the_skater_down_above_the_contact_limit() {
    use skate_dynamics::rapier3d::prelude::{Pose, Rotation, SharedShape, Vector};
    let root_dir = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").unwrap());
    let map_path = std::path::PathBuf::from(std::env::var_os("SKATE3_MAP").unwrap());
    let map = skate_data::skate_map::SkateMap::load(&map_path).unwrap();
    let assets = skate_data::GameAssets::load(&root_dir).unwrap();
    let rig = Rig { graphs: crate::graph_runtime::StockGraphs::load(&root_dir, &assets).unwrap(), root_dir: root_dir.clone() };
    let run = |speed: f32, group: u32| {
        let mut physics = GamePhysics::load_with_difficulty(&root_dir, Some(&map), crate::difficulty::Difficulty::Easy).unwrap();
        let mut player = rig.skater(&physics);
        for _ in 0..30 {
            rig.step(&mut physics, &mut player, pad());
        }
        let deck = physics.board.bodies()[BodyId::Deck.index()].rates.position;
        // The car starts 4 m to the skater's +x and drives at them along -x.
        let mut x = deck.x + 4.0;
        let mut states = Vec::new();
        for _ in 0..60 {
            x -= speed / 60.0;
            let centre = Vector::new(x, deck.y + 0.75, deck.z);
            let solid = skate_dynamics::SolidBody {
                id: 0x7E57_0000_0000_0001,
                pose: Pose::from_parts(centre, Rotation::IDENTITY),
                center_of_mass: centre,
                inertia_rotation: Rotation::IDENTITY,
                inverse_mass: 0.0,
                inverse_inertia: Vector::new(0.0, 0.0, 0.0),
                linvel: Vector::new(-speed, 0.0, 0.0),
                angvel: Vector::new(0.0, 0.0, 0.0),
                contact_group: group,
                colliders: vec![skate_dynamics::SolidCollider { shape: SharedShape::cuboid(2.15, 0.75, 0.9), pose: Pose::from_parts(centre, Rotation::IDENTITY), friction: 0.5 }],
            };
            let mut proxies = network::Proxies::default();
            proxies.append_solid(solid, &physics, &player.runtime, false);
            physics.network_proxies = proxies;
            rig.step(&mut physics, &mut player, pad());
            states.push(player.runtime.player_state.current());
        }
        states
    };
    let first = |v: &[PhysicalStateId]| v.iter().position(|s| *s == PhysicalStateId::WipeoutGround);
    // 2026-10-09 (DownTown): 12 m/s group 8 -> tick 6, group 0 -> tick 7; 4 m/s group 8 -> tick 22 (this
    // box never brakes, unlike a retail car that hit an actor; the vehicle scalar 0.35 shrinks the other
    // limits while touching it), group 0 -> none.
    let fast = first(&run(12.0, VEHICLE_GROUP));
    eprintln!("12 m/s: first wipeout tick {fast:?}");
    assert!(fast.is_some_and(|t| t < 10), "a 12 m/s car hit wipes the skater out at once");
}

/// A ped's thrown can (`living_world::ped_hand_props::push_hand_prop_proxies`: a small finite-mass box in group 14)
/// hitting the standing skater's chest at the attack throw's 10 m/s is an ordinary contact (b92 Q2, b94): it reaches the
/// region forces (halved as a small object below 5.5 kg) and the usual wipeout check decides. Logs the outcome.
#[test]
#[ignore = "requires private stock graphs and an installed map (SKATE3_ASSET_ROOT, SKATE3_MAP=<maps/DownTown.skate>)"]
fn a_thrown_can_reaches_the_skater_region_forces() {
    use skate_dynamics::rapier3d::prelude::{Pose, Rotation, SharedShape, Vector};
    let root_dir = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").unwrap());
    let map_path = std::path::PathBuf::from(std::env::var_os("SKATE3_MAP").unwrap());
    let map = skate_data::skate_map::SkateMap::load(&map_path).unwrap();
    let assets = skate_data::GameAssets::load(&root_dir).unwrap();
    let rig = Rig { graphs: crate::graph_runtime::StockGraphs::load(&root_dir, &assets).unwrap(), root_dir: root_dir.clone() };
    let mut physics = GamePhysics::load_with_difficulty(&root_dir, Some(&map), crate::difficulty::Difficulty::Easy).unwrap();
    let mut player = rig.skater(&physics);
    for _ in 0..30 {
        rig.step(&mut physics, &mut player, pad());
    }
    let deck = physics.board.bodies()[BodyId::Deck.index()].rates.position;
    let (speed, mass) = (10.0_f32, 0.4_f32);
    let mut x = deck.x + 1.5;
    let (mut peak, mut wipeout) = (0.0_f32, None);
    for tick in 0..30 {
        x -= speed / 60.0;
        let centre = Vector::new(x, deck.y + 1.3, deck.z);
        let solid = skate_dynamics::SolidBody {
            id: 0x7E57_0000_0000_0002,
            pose: Pose::from_parts(centre, Rotation::IDENTITY),
            center_of_mass: centre,
            inertia_rotation: Rotation::IDENTITY,
            inverse_mass: 1.0 / mass,
            inverse_inertia: Vector::new(500.0, 500.0, 500.0),
            linvel: Vector::new(-speed, 0.0, 0.0),
            angvel: Vector::new(0.0, 0.0, 0.0),
            contact_group: 14,
            colliders: vec![skate_dynamics::SolidCollider { shape: SharedShape::cuboid(0.04, 0.07, 0.04), pose: Pose::from_parts(centre, Rotation::IDENTITY), friction: 0.5 }],
        };
        let mut proxies = network::Proxies::default();
        proxies.append_solid(solid, &physics, &player.runtime, false);
        physics.network_proxies = proxies;
        rig.step(&mut physics, &mut player, pad());
        peak = player.runtime.collision_feedback.regions.iter().map(|r| r.force).fold(peak, f32::max);
        if wipeout.is_none() && player.runtime.player_state.current() == PhysicalStateId::WipeoutGround {
            wipeout = Some(tick);
        }
    }
    eprintln!("thrown can {speed} m/s {mass} kg: peak region force {peak:.2}, wipeout tick {wipeout:?}");
    assert!(peak > 1.0, "the can's contact reaches the region forces: {peak}");
}

/// DMO streaming on DownTown's placed props (doc 27 "DMO streaming"): from the map spawn the fill pass keeps at most
/// 49 live props, all within 90 m; the others go dormant (collision parked, out of the obstacle list); walking 300 m away
/// culls the near ones and brings the props there in.
#[test]
#[ignore = "requires an installed map (SKATE3_ASSET_ROOT, SKATE3_MAP=<maps/DownTown.skate>)"]
fn dmo_streaming_keeps_49_live_props_round_the_spawn() {
    use skate_core::living_world::dmo::{DmoDecision, DmoStream, DmoStreamSettings, DmoView};
    use skate_core::living_world::Observer;
    let root_dir = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").unwrap());
    let map_path = std::path::PathBuf::from(std::env::var_os("SKATE3_MAP").unwrap());
    let map = skate_data::skate_map::SkateMap::load(&map_path).unwrap();
    let mut physics = GamePhysics::load_with_difficulty(&root_dir, Some(&map), crate::difficulty::Difficulty::Easy).unwrap();
    let placements = physics.prop_dynamics().unwrap().dmo_placements();
    let total = placements.len();
    let settings = DmoStreamSettings::default();
    let mut stream = DmoStream::new(placements, &settings);
    let deck = physics.board.bodies()[BodyId::Deck.index()].rates.position;
    let view = |x: f32, z: f32| (Observer { position: [x, deck.y, z], velocity: [0.0; 3] }, DmoView { camera: [x, deck.y + 2.0, z], forward: [0.0, 0.0, 1.0], reference: [x, deck.y, z] });
    let here = view(deck.x, deck.z);
    stream.step(&[here], &settings, true, &|_| false);
    let dormant: Vec<u32> = stream.placements.iter().filter(|p| !stream.live.contains(&p.id)).map(|p| p.id).collect();
    for &id in &dormant {
        assert!(physics.stream_prop(id, true, true));
    }
    let live = stream.live.len();
    eprintln!("DownTown: {total} placed props, {live} live after the fill, {} dormant", dormant.len());
    assert!(live <= 49 && live > 0);
    let flat = |p: [f32; 3]| ((p[0] - deck.x).powi(2) + (p[2] - deck.z).powi(2)).sqrt();
    assert!(stream.placements.iter().filter(|p| stream.live.contains(&p.id)).all(|p| flat(p.position) <= 90.0));
    let dynamics = physics.prop_dynamics().unwrap();
    assert_eq!(dynamics.obstacle_boxes().len(), live, "dormant props leave the obstacle list");
    assert!(dormant.iter().all(|&id| dynamics.is_dormant(id)));
    // Knock one live prop 2 m aside: after a cull it comes back at its authored pose (retail, b98).
    let moved = *stream.live.iter().next().unwrap();
    let (spawn_origin, spawn_basis) = physics.prop_dynamics().unwrap().spawn_pose(moved).unwrap();
    let shifted = skate_core::math::Vector3::new(spawn_origin.x + 2.0, spawn_origin.y, spawn_origin.z);
    physics.prop_dynamics_mut().unwrap().teleport(moved, shifted, spawn_basis);
    // Far away: the spawn's props cull, others come in.
    let far = view(deck.x + 300.0, deck.z);
    let mut changes = Vec::new();
    for _ in 0..20 {
        changes.extend(stream.step(&[far], &settings, false, &|_| false));
    }
    let culled = changes.iter().filter(|d| matches!(d, DmoDecision::Cull(_))).count();
    eprintln!("300 m away: {culled} culled, {} spawned, {} live", changes.iter().filter(|d| matches!(d, DmoDecision::Spawn(_))).count(), stream.live.len());
    assert!(culled > 0);
    for d in changes {
        match d {
            DmoDecision::Spawn(id) => assert!(physics.stream_prop(id, false, true)),
            DmoDecision::Cull(id) | DmoDecision::Evict(id) => assert!(physics.stream_prop(id, true, true)),
        }
    }
    assert_eq!(physics.prop_dynamics().unwrap().obstacle_boxes().len(), stream.live.len());
    // Back at the spawn: the knocked prop is in again, at its authored pose.
    for _ in 0..20 {
        for d in stream.step(&[here], &settings, false, &|_| false) {
            match d {
                DmoDecision::Spawn(id) => assert!(physics.stream_prop(id, false, true)),
                DmoDecision::Cull(id) | DmoDecision::Evict(id) => assert!(physics.stream_prop(id, true, true)),
            }
        }
    }
    assert!(stream.live.contains(&moved) && !physics.prop_dynamics().unwrap().is_dormant(moved));
    let (origin, _) = physics.prop_dynamics().unwrap().pose(moved).unwrap();
    assert!((origin.x - spawn_origin.x).abs() < 1e-4 && (origin.z - spawn_origin.z).abs() < 1e-4, "authored pose: {origin:?} vs {spawn_origin:?}");
}

/// Mode 7's controller B (NavMeshController, b78 to b80) on a simulated skater: B's intents alone (no pad, the AI
/// source present but not fresh) make it press the off-board toggle, step off, turn on foot through `OB_Steer` ->
/// `ob_Turn` (Processed +2680 = -OB_Steer) and line up with the node direction, then hand back (arrived).
#[test]
#[ignore = "requires private stock graphs and an installed map (SKATE3_ASSET_ROOT, SKATE3_MAP=<maps/DownTown.skate>)"]
fn controller_b_steps_off_and_walks_the_skater_onto_the_node() {
    use skate_core::living_world::controller_b::{BTick, ControllerB, ControllerBSettings, HandBack, PathElement, PathService};
    // The goal as the only corner: what the peds' navmesh returns on open ground.
    struct Corner;
    impl PathService for Corner {
        fn solve(&self, _from: [f32; 3], to: [f32; 3]) -> Option<Vec<PathElement>> {
            Some(vec![PathElement { entry: to, exit: to }])
        }
    }
    let root_dir = std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").unwrap());
    let map_path = std::path::PathBuf::from(std::env::var_os("SKATE3_MAP").unwrap());
    let map = skate_data::skate_map::SkateMap::load(&map_path).unwrap();
    let assets = skate_data::GameAssets::load(&root_dir).unwrap();
    let graphs = crate::graph_runtime::StockGraphs::load(&root_dir, &assets).unwrap();
    let mut physics = GamePhysics::load_with_difficulty(&root_dir, Some(&map), crate::difficulty::Difficulty::Easy).unwrap();
    let spawn = physics.board.part_transforms()[BodyId::Deck.index()];
    let mut context = physics.new_skater_context(spawn).unwrap();
    let mut runtime = physics.load_skater_in_context(&mut context, &root_dir, &graphs, "easy").unwrap();
    let mut controls = PlayerControls::default();
    let mut camera = crate::camera::CameraRuntime::load(&root_dir).unwrap();
    // Settle on the board for half a second (an empty, present AI source, as while B is active).
    let record = skate_core::living_world::ai_record::build(
        &skate_core::living_world::ai_record::LineTarget { position: [spawn.translation.x, spawn.translation.y, spawn.translation.z], frame: [0.0, 0.0, 0.0, 1.0], step: [0.0; 3] },
        [0.0, 0.0, 1.0],
        &Default::default(),
    );
    runtime.ai_physics = Some(super::skater::AiPhysicsSource { record, fresh: false });
    for _ in 0..30 {
        physics.advance_npc_skater(&mut context, &mut runtime, &mut controls, &graphs, &mut camera, &[]).unwrap();
    }
    let root = runtime.animated_skeleton.roots.animation_to_world;
    let at = [root[3][0], root[3][1], root[3][2]];
    let forward = [root[2][0], 0.0, root[2][2]];
    let right = [forward[2], 0.0, -forward[0]];
    let l = (right[0] * right[0] + right[2] * right[2]).sqrt();
    let right = right.map(|x| x / l);
    let target = [at[0] + right[0] * 3.0, at[1], at[2] + right[2] * 3.0];
    let s = ControllerBSettings::default();
    let input = crate::living_world::npc_sim::walker_input(&runtime);
    let mut b = ControllerB::activate(&s, &Corner, target, right, &input).expect("the corner plan accepts the goal");
    let (mut off_board_at, mut turn_checked, mut result) = (None, false, None);
    for tick in 0..600 {
        let input = crate::living_world::npc_sim::walker_input(&runtime);
        let at = input.position;
        if input.byte_161 && off_board_at.is_none() {
            off_board_at = Some(tick);
        }
        let intents: Vec<(String, f32)> = match b.tick(&s, &Corner, &input) {
            BTick::Controls(c) => c.intents().into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
            BTick::HandBack { reason, .. } => {
                result = Some((tick, reason));
                break;
            }
            BTick::Idle => Vec::new(),
        };
        let steer = intents.iter().find(|x| x.0 == "OB_Steer").map(|x| x.1);
        physics.advance_npc_skater(&mut context, &mut runtime, &mut controls, &graphs, &mut camera, &intents).unwrap();
        if runtime.player_state.current() == PhysicalStateId::BipedGround && tick < 40 {
            let x = &runtime.animation_input.extra;
            eprintln!("  tick {tick}: OB_Mag sent {:?} -> magnitude {:.3}, OB_Steer sent {steer:?} -> turn {:.3}", intents.iter().find(|x| x.0 == "OB_Mag").map(|x| x.1), x.offboard_magnitude, x.offboard_turn);
        }
        if let (Some(steer), PhysicalStateId::BipedGround) = (steer, runtime.player_state.current()) {
            if steer.abs() > 0.1 && !turn_checked && tick < 60 {
                let turn = runtime.animation_input.extra.offboard_turn;
                eprintln!("tick {tick}: OB_Steer {steer:.3} -> offboard_turn {turn:.3}, state {:?}", runtime.player_state.current());
                if (turn + steer).abs() < 1e-4 {
                    turn_checked = true;
                }
            }
        }
        if tick % 30 == 0 {
            eprintln!("tick {tick} state {:?} at {at:?} steer {steer:?} heading error {:.2}", runtime.player_state.current(), b.heading_error);
        }
    }
    eprintln!("off board at {off_board_at:?}, result {result:?}");
    assert!(off_board_at.is_some(), "the toggle presses never took the skater off the board");
    assert!(turn_checked, "no steer while off the board");
    assert!(matches!(result, Some((_, HandBack::Arrived))), "{result:?}");
}
