//! Headless, seeded tests of the living-world plugin: a fake player drives along a path, the
//! population systems run in a Bevy app with manual time, and the published messages are checked.

use super::*;
use skate_core::living_world::census::CensusCategory;
use skate_core::living_world::{CensusCircle, CensusGrid, CensusRange, CensusRecord, SkaterCharacter, SkaterLine};
use std::collections::BTreeMap;

fn census() -> CensusMap {
    let (w, h) = (600u32, 600u32);
    let mut layers = BTreeMap::new();
    layers.insert("livingworld_npc_census".to_string(), vec![1u16; (w * h) as usize]);
    layers.insert("livingworld_vehicle_census".to_string(), vec![2u16; (w * h) as usize]);
    let grid = CensusGrid { cell: 4.0, origin: [-1200.0, -1200.0], width: w, height: h, names: vec!["aletown".into(), "dwntwn".into()], layers };
    let rec = |max, cats: &[(&str, f32)]| CensusRecord { max_population: max, categories: cats.iter().map(|(n, w)| CensusCategory { name: n.to_string(), weight: *w }).collect() };
    let mut records = BTreeMap::new();
    records.insert("aletown".into(), rec(15, &[("adult", 0.5), ("teen", 0.5)]));
    records.insert("dwntwn".into(), rec(30, &[("sedans", 0.5), ("taxis", 0.5)]));
    CensusMap { grids: vec![grid], records }
}

/// Synthetic roads (milestone V2 places cars on lanes): east-west road pairs every 100 m from
/// z = -1100 to 1100, 2 lanes per direction, 8 m wide, 2.4 km long in 8 m pieces; and one
/// entity per census category.
fn roads_and_vehicles() -> (skate_core::living_world::traffic::RoadNetwork, skate_core::living_world::VehicleCatalog) {
    use skate_core::living_world::traffic::{Curve, PieceInput, RoadInput, RoadNetwork, SegmentId, SegmentInput};
    let mut segments = Vec::new();
    for k in 0..23u64 {
        let z = -1100.0 + k as f32 * 100.0;
        for (dir, zc) in [(1.0f32, z + 5.0), (-1.0f32, z - 5.0)] {
            let x0 = -1200.0 * dir;
            let pieces = (0..300)
                .map(|i| {
                    let a = [x0 + dir * 8.0 * i as f32, 0.0, zc];
                    let b = [x0 + dir * 8.0 * (i + 1) as f32, 0.0, zc];
                    let side = |p: [f32; 3], s: f32| [p[0], 0.0, p[2] + s * dir * 4.0];
                    PieceInput { end_distance: 8.0 * (i + 1) as f32, centre: Curve::straight(a, b), left_start: side(a, -1.0), right_start: side(a, 1.0), left_end: side(b, -1.0), right_end: side(b, 1.0) }
                })
                .collect();
            let id = 1000 + k * 2 + (dir < 0.0) as u64;
            segments.push(SegmentInput { id: SegmentId(id), from_node: id * 10, from_end: 0, to_node: id * 10 + 1, to_end: 0, length: 2400.0, speed_limit: 14.167, lanes: 2, manoeuvres: 2, district: 0, pieces });
        }
    }
    let mut cat = skate_core::living_world::VehicleCatalog::default();
    for c in ["sedans", "taxis"] {
        cat.categories.insert(c.into(), vec![format!("{c}01")]);
        cat.entities.insert(format!("{c}01"), skate_core::living_world::VehicleEntity { model: format!("vehicle_{c}01"), chassis_colours: 4, secondary_colours: 1, length: 4.5, width: 1.9 });
    }
    (RoadNetwork::build(&RoadInput { segments, junctions: Vec::new() }).unwrap(), cat)
}

fn data() -> LoadedData {
    let mut config = PopulationConfig::retail();
    let c = |i, o, cull, f, s| CensusCircle { spawn_inner: i, spawn_outer: o, cull, forward_offset: f, speed_kmh: s };
    config.pedestrians.range = Some(CensusRange { slow: c(50.0, 60.0, 70.0, 0.0, 45.0), fast: c(50.0, 80.0, 90.0, 20.0, 80.0) });
    config.vehicles.range = Some(CensusRange { slow: c(80.0, 100.0, 110.0, 0.0, 0.0), fast: c(80.0, 100.0, 110.0, 0.0, 0.0) });
    // Lines every 25 m along the path's side streets.
    let lines = (0..80)
        .map(|i| {
            let mut id = [0u8; 16];
            id[0] = i as u8;
            SkaterLine { id, start: [-1000.0 + i as f32 * 25.0, 0.0, 70.0], heading: 0.0, valid: true, allowed_skaters: u64::MAX, flags: 0 }
        })
        .collect();
    let characters = (0..6).map(|i| SkaterCharacter { key: format!("pro_{i}"), pro_index: Some(i), capabilities: [false; 3], community: false }).collect();
    let (roads, vehicles) = roads_and_vehicles();
    LoadedData { config, census: Some(census()), skaters: Some(SkaterData { lines, characters }), roads: Some(roads), vehicles: Some(vehicles), npc: Default::default(), status: "test".into() }
}

#[derive(Resource, Default)]
struct Collected(Vec<WireRecord>);

#[derive(Resource)]
struct Path {
    t: f32,
    speed: f32,
}

/// The fake player: along +x at `speed` m/s from x = -900.
fn drive(time: Res<Time>, mut path: ResMut<Path>, mut obs: ResMut<LivingWorldObservers>) {
    path.t += time.delta_secs();
    let x = -900.0 + path.t * path.speed;
    obs.observers = vec![Observer { position: [x, 0.0, 0.0], velocity: [path.speed, 0.0, 0.0] }];
    obs.player_slots = 1;
}

fn collect(mut s: MessageReader<LivingWorldSpawn>, mut d: MessageReader<LivingWorldDespawn>, mut out: ResMut<Collected>) {
    for m in s.read() {
        out.0.push(WireRecord::from_decision(&Decision::Spawn(m.0.clone())));
    }
    for m in d.read() {
        out.0.push(WireRecord::from_decision(&Decision::Despawn(m.0.clone())));
    }
}

fn app(seed: u64, speed: f32) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let settings = LivingWorldSettings { seed, ..LivingWorldSettings::default() };
    let mut state = PopulationState::default();
    state.install("Test", 1, &settings, data());
    app.insert_resource(settings).insert_resource(state).init_resource::<LivingWorldObservers>().init_resource::<Collected>();
    app.insert_resource(Path { t: 0.0, speed });
    app.add_message::<LivingWorldSpawn>().add_message::<LivingWorldDespawn>();
    app.add_systems(Update, (drive, step_population, collect).chain());
    app
}

/// Run `seconds` of game time at an engine rate of `hz`.
fn run(app: &mut App, seconds: f32, hz: f32) {
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f32(1.0 / hz)));
    app.update(); // the first update has no delta
    for _ in 0..(seconds * hz) as u32 {
        app.update();
    }
}

fn spawned(c: &Collected) -> Vec<(&str, [f32; 3], bool)> {
    c.0.iter().filter_map(|r| match r { WireRecord::Spawn { kind, position, initial, .. } => Some((kind.as_str(), *position, *initial)), _ => None }).collect()
}

#[test]
fn living_world_population_follows_a_driving_player() {
    let mut app = app(7, 8.0);
    let mut max = (0, 0, 0);
    for _ in 0..60 {
        run(&mut app, 2.0, 60.0);
        let st = app.world().resource::<PopulationState>();
        let obs = app.world().resource::<LivingWorldObservers>().observers[0].position;
        let w = &st.world;
        max = (max.0.max(w.count(Kind::Skater)), max.1.max(w.count(Kind::Pedestrian)), max.2.max(w.count(Kind::Vehicle)));
        assert!(w.count(Kind::Skater) <= 3 && w.count(Kind::Pedestrian) <= 15 && w.count(Kind::Vehicle) <= 15);
        // Census entities never outlive their cull radius by more than one rotation (4 ticks at 8 m/s).
        for l in w.live(Kind::Pedestrian) {
            let d = ((l.position[0] - obs[0]).powi(2) + (l.position[2] - obs[2]).powi(2)).sqrt();
            assert!(d <= 90.0 + 2.0, "ped at {d}");
        }
    }
    // Cars: the vehicle limit 15 under the census cap of 30 (milestone V2).
    assert_eq!(max, (3, 15, 15));
    let c = app.world().resource::<Collected>();
    for (kind, p, initial) in spawned(c) {
        assert!(p[1] == 0.0);
        if kind == "skater" {
            assert!(!initial);
        }
    }
    assert!(c.0.iter().any(|r| matches!(r, WireRecord::Despawn { reason, .. } if reason == "distance")));
}

#[test]
fn living_world_same_seed_same_stream_at_any_engine_rate() {
    let mut a = app(99, 8.0);
    run(&mut a, 60.0, 60.0);
    let mut b = app(99, 8.0);
    run(&mut b, 60.0, 60.0);
    let (ra, rb) = (&a.world().resource::<Collected>().0, &b.world().resource::<Collected>().0);
    assert!(ra.len() > 40);
    assert_eq!(ra, rb);
    // 144 Hz: the same console ticks, so the same decisions while the player stands still.
    let mut c = app(5, 0.0);
    run(&mut c, 30.0, 60.0);
    let mut d = app(5, 0.0);
    run(&mut d, 30.0, 144.0);
    let (rc, rd) = (&c.world().resource::<Collected>().0, &d.world().resource::<Collected>().0);
    let n = rc.len().min(rd.len());
    assert!(n > 20);
    assert_eq!(rc[..n], rd[..n]);
}

#[test]
fn living_world_online_spawns_nothing_and_client_mirrors_records() {
    let mut a = app(3, 8.0);
    run(&mut a, 20.0, 60.0);
    let records = a.world().resource::<Collected>().0.clone();
    // Wire round trip and a client that only mirrors.
    let json = serde_json::to_string(&records).unwrap();
    let back: Vec<WireRecord> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, records);
    let mut client = PopulationState::default();
    client.apply_records(&back);
    for k in Kind::ALL {
        let host: Vec<_> = a.world().resource::<PopulationState>().world.live(k).map(|l| l.id).collect();
        let mirror: Vec<_> = client.world.live(k).map(|l| l.id).collect();
        assert_eq!(host, mirror);
    }
    // Online: nothing new spawns; a Client role runs no rules at all.
    a.world_mut().resource_mut::<Collected>().0.clear();
    a.add_systems(Update, (|mut o: ResMut<LivingWorldObservers>| o.online = true).after(drive).before(step_population));
    run(&mut a, 20.0, 60.0);
    assert!(a.world().resource::<Collected>().0.iter().all(|r| matches!(r, WireRecord::Despawn { .. })));
    let mut c = app(3, 8.0);
    c.world_mut().resource_mut::<LivingWorldSettings>().net_role = NetRole::Client;
    run(&mut c, 10.0, 60.0);
    assert!(c.world().resource::<Collected>().0.is_empty());
}

#[test]
fn living_world_settings_disable_and_density() {
    let mut a = app(4, 0.0);
    {
        let mut s = a.world_mut().resource_mut::<LivingWorldSettings>();
        s.vehicles.density = 0.5;
        s.skaters.enabled = false;
    }
    run(&mut a, 20.0, 60.0);
    let w = &a.world().resource::<PopulationState>().world;
    assert_eq!(w.count(Kind::Vehicle), 15);
    assert_eq!(w.count(Kind::Skater), 0);
    assert_eq!(w.count(Kind::Pedestrian), 15);
    assert!(readout(a.world().resource::<PopulationState>()).contains("vehicles 15"));
}

#[test]
fn vehicle_records_round_trip_through_the_wire_form() {
    use skate_core::living_world::{LivingWorldId, SpawnChoice, SpawnRecord};
    let record = Decision::Spawn(SpawnRecord {
        id: LivingWorldId { kind: Kind::Vehicle, serial: 4 },
        tick: 120,
        position: [1.0, 2.0, 3.0],
        heading: 0.5,
        seed: 77,
        initial: true,
        choice: SpawnChoice::Vehicle {
            record: "dwntwn".into(),
            category: "taxis".into(),
            entity: "taxi01".into(),
            model: "vehicle_taxi01".into(),
            chassis: 1,
            secondary: 0,
            segment: 0x1234_5678_9ABC,
            lane: 1,
            distance: 42.0,
        },
    });
    let wire = WireRecord::from_decision(&record);
    let json = serde_json::to_string(&wire).unwrap();
    let back: WireRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(back.to_decision(), Some(record));
}

#[test]
fn living_world_npc_draw_distance_setting_scales_population_and_mods_reset_to_the_players_choice() {
    // Retail by default; the settings write the multiplier into the authority's config.
    let s = LivingWorldSettings::default();
    assert_eq!(s.npc_draw_distance, 1.0);
    let mut config = PopulationConfig::retail();
    s.apply(&mut config);
    assert_eq!(config.draw_distance, 1.0);

    // 1x explicitly set through the menu path: the same decision stream as the default.
    let mut a = app(11, 8.0);
    run(&mut a, 30.0, 60.0);
    let mut b = app(11, 8.0);
    b.world_mut().resource_mut::<LivingWorldSettings>().set_user_draw_distance(1.0);
    run(&mut b, 30.0, 60.0);
    assert_eq!(a.world().resource::<Collected>().0, b.world().resource::<Collected>().0);

    // 2x: a still player gets 4x the peds (cap 15 x 4) and up to 4x the cars (limit 15 x 4).
    let mut c = app(4, 0.0);
    c.world_mut().resource_mut::<LivingWorldSettings>().set_user_draw_distance(2.0);
    run(&mut c, 20.0, 60.0);
    let w = &c.world().resource::<PopulationState>().world;
    assert_eq!(w.count(Kind::Pedestrian), 60);
    assert!(w.count(Kind::Vehicle) > 15 && w.count(Kind::Vehicle) <= 60, "cars {}", w.count(Kind::Vehicle));
    // The retail data stays as loaded (the multiplier sits on top).
    assert_eq!(w.config.draw_distance, 2.0);
    assert_eq!(w.config.pedestrians.range, c.world().resource::<PopulationState>().data_config.pedestrians.range);

    // A mod overrides it; disabling the mod restores the player's menu choice, not the mod's.
    let mut s = LivingWorldSettings::default();
    s.set_user_draw_distance(1.5);
    s.npc_draw_distance = 3.0;
    s.ped_fade.enabled = false;
    s.reset_mod_overrides();
    assert_eq!((s.npc_draw_distance, s.user_npc_draw_distance), (1.5, 1.5));
    assert_eq!(s.ped_fade, skate_core::living_world::peds::PedFadeConfig::default());
    // Bad values never reach the rules.
    s.npc_draw_distance = f32::NAN;
    assert!(s.draw_distance().is_retail());

    // "None": every kind off (live NPCs despawn), ranges retail, kept across a mod reset, undone
    // by picking a step again.
    let mut s = LivingWorldSettings::default();
    s.set_user_draw_distance(skate_core::living_world::DrawDistance::NONE);
    let mut cfg = c.world().resource::<PopulationState>().data_config.clone();
    s.apply(&mut cfg);
    assert!(!cfg.skaters.enabled && !cfg.pedestrians.enabled && !cfg.vehicles.enabled);
    assert!(s.draw_distance().is_retail());
    s.reset_mod_overrides();
    assert!(s.user_npcs_off);
    s.set_user_draw_distance(1.0);
    s.apply(&mut cfg);
    assert!(cfg.skaters.enabled && cfg.pedestrians.enabled && cfg.vehicles.enabled);
    // In a running population nothing spawns.
    let mut d = app(4, 0.0);
    d.world_mut().resource_mut::<LivingWorldSettings>().set_user_draw_distance(skate_core::living_world::DrawDistance::NONE);
    run(&mut d, 20.0, 60.0);
    let w = &d.world().resource::<PopulationState>().world;
    assert_eq!((w.count(Kind::Pedestrian), w.count(Kind::Vehicle), w.count(Kind::Skater)), (0, 0, 0));
}

#[test]
fn living_world_ped_fade_scales_with_the_draw_distance() {
    let mut s = LivingWorldSettings::default();
    let a = |s: &LivingWorldSettings, d: f32| super::peds::ped_draw_alpha(s, Some([45.0, 55.0]), d, 10.0);
    assert_eq!(a(&s, 50.0), skate_core::living_world::peds::draw_alpha(&s.ped_fade, Some([45.0, 55.0]), 50.0, 10.0));
    assert_eq!(a(&s, 55.0), 0.0);
    s.set_user_draw_distance(2.0);
    assert_eq!(a(&s, 90.0), 1.0);
    assert!((a(&s, 100.0) - 0.5).abs() < 1e-6);
    assert_eq!(a(&s, 110.0), 0.0);
    // Without a model pair the configured default (45 / 55) scales the same way.
    assert!((super::peds::ped_draw_alpha(&s, None, 100.0, 10.0) - 0.5).abs() < 1e-6);
}

/// The walking player and the board left behind (session 2026-10-05, 17:45:02 to 17:45:15): the
/// player walks along +x at `speed`, the deck lies 500 m away.
const LEFT_DECK: [f32; 3] = [-400.0, 0.0, 300.0];

fn walk_away_from_board(time: Res<Time>, mut path: ResMut<Path>, mut obs: ResMut<LivingWorldObservers>) {
    path.t += time.delta_secs();
    let player = ([-900.0 + path.t * path.speed, 0.0, 0.0], [path.speed, 0.0, 0.0]);
    obs.observers = vec![local_focus(Some(player), (LEFT_DECK, [0.0; 3]))];
    obs.player_slots = 1;
}

#[test]
fn living_world_off_board_player_keeps_the_population_around_the_player_not_the_board() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let settings = LivingWorldSettings { seed: 11, ..LivingWorldSettings::default() };
    let mut state = PopulationState::default();
    state.install("Test", 1, &settings, data());
    app.insert_resource(settings).insert_resource(state).init_resource::<LivingWorldObservers>().init_resource::<Collected>();
    app.insert_resource(Path { t: 0.0, speed: 1.5 });
    app.add_message::<LivingWorldSpawn>().add_message::<LivingWorldDespawn>();
    app.add_systems(Update, (walk_away_from_board, step_population, collect).chain());
    run(&mut app, 20.0, 60.0);
    let player = app.world().resource::<LivingWorldObservers>().observers[0].position;
    let w = &app.world().resource::<PopulationState>().world;
    assert!(w.count(Kind::Pedestrian) > 0 && w.count(Kind::Vehicle) > 0, "population around the player");
    let flat = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
    for k in [Kind::Pedestrian, Kind::Vehicle] {
        for l in w.live(k) {
            assert!(flat(l.position, player) <= 110.0 + 1.0, "{k:?} {} m from the player", flat(l.position, player));
            assert!(flat(l.position, LEFT_DECK) > 200.0, "{k:?} spawned around the left board");
        }
    }
    // Nothing was ever spawned around the board.
    assert!(spawned(app.world().resource::<Collected>()).iter().all(|(_, p, _)| flat(*p, LEFT_DECK) > 200.0));
}

#[test]
fn living_world_focus_is_the_character_and_still_velocity_is_zero() {
    let deck = ([5.0, 0.0, 5.0], [9.0, 0.0, 0.0]);
    let walking = local_focus(Some(([1.0, 2.0, 3.0], [1.2, 0.0, 0.0])), deck);
    assert_eq!((walking.position, walking.velocity), ([1.0, 2.0, 3.0], [1.2, 0.0, 0.0]));
    // No skater loaded: the board.
    assert_eq!(local_focus(None, deck).position, [5.0, 0.0, 5.0]);
    // |v|^2 <= 1e-4 counts as standing (retail `sub_826BE870`).
    assert_eq!(local_focus(Some(([0.0; 3], [0.005, 0.0, 0.005])), deck).velocity, [0.0; 3]);
    // A broken focus stays broken, so the census guard holds the population.
    assert!(local_focus(Some(([f32::NAN, 0.0, 0.0], [0.0; 3])), deck).position[0].is_nan());
}

#[test]
fn living_world_debug_summaries_restart_with_a_new_world() {
    let mut last = 0;
    assert!(!report_due(299, &mut last, 300));
    assert!(report_due(300, &mut last, 300));
    assert!(report_due(9_000, &mut last, 300));
    // Map reload / respawn into a new generation: the world tick restarts at 0.
    assert!(!report_due(10, &mut last, 300));
    assert!(report_due(300, &mut last, 300), "the summary logs again 5 s into the new world");
    assert_eq!(last, 300);
}
