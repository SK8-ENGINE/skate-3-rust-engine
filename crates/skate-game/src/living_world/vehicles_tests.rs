//! Headless, seeded tests of the V3 traffic cars: spawn records become car entities, the cars
//! drive their lanes through a signalled junction, publish `TrafficAudio`, leave on despawn and
//! move the same at any engine rate. No window, no assets (synthetic road, synthetic models).

use super::vehicles::*;
use crate::world_audio::TrafficAudio;
use super::*;
use skate_core::living_world::traffic::follow::FollowParams;
use skate_core::living_world::traffic::graph::{ConnectorInput, Curve, EndInput, JunctionId, JunctionInput, PieceInput, RoadInput, SegmentInput};
use skate_core::living_world::traffic::{Entry, Light, RoadNetwork, SegmentId, SignalClock, SignalTimings};
use skate_core::living_world::{DespawnRecord, SpawnRecord};

const J: u64 = 0xA;

fn seg(id: u64, from: (u64, u8), to: (u64, u8), a: [f32; 3], b: [f32; 3]) -> SegmentInput {
    let length = ((b[2] - a[2]).powi(2) + (b[0] - a[0]).powi(2)).sqrt();
    let piece = |s: [f32; 3], e: [f32; 3], d: f32| PieceInput {
        end_distance: d,
        centre: Curve::straight(s, e),
        left_start: [s[0] - 2.0, s[1], s[2]],
        right_start: [s[0] + 2.0, s[1], s[2]],
        left_end: [e[0] - 2.0, e[1], e[2]],
        right_end: [e[0] + 2.0, e[1], e[2]],
    };
    let mid = [(a[0] + b[0]) * 0.5, 0.0, (a[2] + b[2]) * 0.5];
    SegmentInput {
        id: SegmentId(id),
        from_node: from.0,
        from_end: from.1,
        to_node: to.0,
        to_end: to.1,
        length,
        speed_limit: 14.166_667,
        lanes: 1,
        manoeuvres: 2,
        district: 0,
        pieces: vec![piece(a, mid, length * 0.5), piece(mid, b, length)],
    }
}

/// A straight road along +z through a signalled junction at the origin: approach 0x1 (end 0)
/// -> connector 0 -> exit 0x2 (end 2).
fn road() -> RoadNetwork {
    let mut approaches: [Option<EndInput>; 4] = Default::default();
    let mut exits: [Option<EndInput>; 4] = Default::default();
    approaches[0] = Some(EndInput { lanes: 1, lane_connectors: vec![vec![0]] });
    exits[2] = Some(EndInput { lanes: 1, lane_connectors: vec![vec![0]] });
    let input = RoadInput {
        segments: vec![seg(0x1, (0x10, 0), (J, 0), [0.0, 0.0, -200.0], [0.0, 0.0, -8.0]), seg(0x2, (J, 2), (0x20, 0), [0.0, 0.0, 8.0], [0.0, 0.0, 400.0])],
        junctions: vec![JunctionInput {
            id: JunctionId(J),
            signalled: true,
            speed: 14.166_667,
            approaches,
            exits,
            connectors: vec![ConnectorInput { index: 0, entry_speed: 14.166_667, from_end: 0, from_lane: 0, to_end: 2, to_lane: 0, curve: Curve::straight([0.0, 0.0, -8.0], [0.0, 0.0, 8.0]) }],
        }],
    };
    RoadNetwork::build(&input).unwrap()
}

fn data() -> VehicleData {
    let vehicles = br#"{"models": {"vehicle_taxi01": {"glb": "vehicles/taxi_sedan_01.glb",
        "chassis_colours": [[0.86, 0.79, 0.08, 1.0], [0.55, 0.1, 0.1, 1.0]], "secondary_colours": [[0.22, 0.22, 0.22, 1.0]],
        "palette_ids": {"chassis": ["vehicle_taxi01/chassis/0", "vehicle_taxi01/chassis/1"], "secondary": ["vehicle_taxi01/secondary/0"]},
        "wheel_hint": 0.3, "mesh_bounds": [[-0.9, 0.0, -2.2], [0.9, 1.5, 2.1]],
        "grab_splines": [{"points": [[0.78, 0.93, -2.39], [0.3, 0.93, -2.39], [-0.3, 0.93, -2.39], [-0.78, 0.93, -2.39]], "direction": [0.0, 0.0, -1.0]},
            {"points": [[0.0, 0.0, 0.0]], "direction": [0.0, 0.0, -1.0]}]}},
        "entities": {"taxi01": {"model": "vehicle_taxi01", "spec": "vehicle_spec_taxi01", "driver": "driver_taxi"}}}"#;
    let tables = br#"{"classes": {"livingworld": {"trafficlights": {"fields": {"signal_green": 7.0, "signal_amber": 1.0, "signal_all_red": 0.5, "Hash_5E41C959D17527CC": 0.4}}},
        "livingworld_vehicle_characteristics": {"vehicle_spec_taxi01": {"fields": {"Hash_328B9F4685A14018": 3.0, "Hash_758229215579C6D1": 2.5, "Hash_2ACE3FEE16D6F7A2": 0.25, "Hash_73A1989BB90C93BA": 0.5,
        "follow_min_speed_kmh": 20.0, "follow_speed_margin_kmh": 20.0, "engine_audio": {"class": "aud_traffic_engine", "key": "c04_taxi01"}}}},
        "livingworld_vehicle_drivers": {"driver_taxi": {"fields": {"honk_blocked_time": 1.0, "honk_obstacle_time": 2.0, "honk_approach_speed_kmh": 10.0,
        "Hash_50E084076390A573": 0.2, "Hash_20E9C6487FDDBDE8": 1.0}}}}}"#;
    parse_vehicle_data(vehicles, Some(tables)).unwrap()
}

fn record(serial: u32, tick: u64, distance: f32, chassis: u32) -> SpawnRecord {
    SpawnRecord {
        id: LivingWorldId { kind: Kind::Vehicle, serial },
        tick,
        position: [0.0, 0.0, -200.0 + distance],
        heading: 0.0,
        seed: serial as u64,
        initial: false,
        choice: SpawnChoice::Vehicle {
            record: "dwntwn".into(),
            category: "taxis".into(),
            entity: "taxi01".into(),
            model: "vehicle_taxi01".into(),
            chassis,
            secondary: 0,
            segment: 0x1,
            lane: 0,
            distance,
        },
    }
}

#[derive(Resource, Default)]
struct Seen(Vec<TrafficEvent>);

fn collect(mut ev: MessageReader<TrafficEvent>, mut seen: ResMut<Seen>) {
    seen.0.extend(ev.read().cloned());
}

fn app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let settings = LivingWorldSettings { seed: 5, ..LivingWorldSettings::default() };
    let mut state = PopulationState::default();
    let loaded = LoadedData { config: PopulationConfig::retail(), census: None, skaters: None, roads: Some(road()), vehicles: None, npc: Default::default(), status: "traffic test".into() };
    state.install("Test", 1, &settings, loaded);
    let d = data();
    let traffic = TrafficState { clock: d.timings.map(SignalClock::new), data: d, ..Default::default() };
    app.insert_resource(settings).insert_resource(state).insert_resource(traffic).init_resource::<LivingWorldObservers>().init_resource::<VehicleOverrides>().init_resource::<Seen>();
    app.add_message::<LivingWorldSpawn>().add_message::<LivingWorldDespawn>().add_message::<TrafficEvent>();
    app.add_systems(Update, (step_population, apply_vehicle_records, drive_traffic, collect).chain());
    app
}

fn run(app: &mut App, seconds: f32, hz: f32) {
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f32(1.0 / hz)));
    for _ in 0..(seconds * hz) as u32 {
        app.update();
    }
}

fn spawn(app: &mut App, r: SpawnRecord) {
    app.world_mut().resource_mut::<PopulationState>().world.apply(&skate_core::living_world::Decision::Spawn(r.clone()));
    app.world_mut().write_message(LivingWorldSpawn(r));
}

fn cars(app: &mut App) -> Vec<(TrafficCar, Transform, TrafficAudio, Vec3)> {
    let mut q = app.world_mut().query::<(&TrafficCar, &CarMotion, &TrafficAudio, &crate::world_audio::AudioVelocity)>();
    let mut v: Vec<_> = q.iter(app.world()).map(|(c, m, a, vel)| (c.clone(), m.curr, a.clone(), vel.0)).collect();
    v.sort_by_key(|x| x.0.id);
    v
}

#[test]
fn living_world_traffic_spawns_a_car_from_the_record() {
    let mut a = app();
    run(&mut a, 0.1, 60.0);
    spawn(&mut a, record(1, 0, 40.0, 1));
    run(&mut a, 0.05, 60.0);
    let list = cars(&mut a);
    assert_eq!(list.len(), 1);
    let (car, t, audio, _) = &list[0];
    assert_eq!(car.entity, "taxi01");
    assert_eq!(car.chassis_id, "vehicle_taxi01/chassis/1");
    assert_eq!(car.chassis, [0.55, 0.1, 0.1, 1.0]);
    assert_eq!(car.secondary_id, "vehicle_taxi01/secondary/0");
    assert_eq!(car.glb, "private/living_world/vehicles/taxi_sedan_01.glb");
    assert_eq!(car.wheel_radius, 0.3);
    assert_eq!(car.paint_gain, PAINT_GAIN, "retail: no gain");
    assert_eq!(PAINT_GAIN, 1.0);
    assert_eq!(audio.engine, "c04_taxi01", "engine record from the entity's spec");
    // On the lane, facing +z (the lane direction).
    assert!(t.translation.x.abs() < 1e-3 && (t.translation.z - (-160.0)).abs() < 1.0, "{:?}", t.translation);
    assert!((t.rotation * Vec3::Z).z > 0.999);
    let spec = &a.world().resource::<TrafficState>().cars[0].params;
    // 328B9F46 / 75822921 are the passage and approach factors (b71), not the follower's accel / decel (b75).
    assert_eq!((spec.passage_factor, spec.approach_factor), (3.0, 2.5), "manoeuvre numbers from the spec record");
    // The junction / lead standoff factors (2ACE3FEE / 73A1989B; stock taxi values 0.25 / 0.5 in the fixture, b163).
    assert_eq!((spec.stop_standoff_factor, spec.lead_standoff_factor), (0.25, 0.5), "standoff factors from the spec record");
    let car0 = &a.world().resource::<TrafficState>().cars[0];
    assert!((spec.accel_lo..=spec.accel_hi).contains(&car0.accel_cap), "the drawn max acceleration {}", car0.accel_cap);
    assert!(a.world().resource::<Seen>().0.iter().any(|e| matches!(e, TrafficEvent::Spawned { entity, .. } if entity == "taxi01")));
}

#[test]
fn living_world_traffic_drives_obeys_the_light_and_publishes_audio() {
    let mut a = app();
    spawn(&mut a, record(1, 0, 170.0, 0));
    let mut saw_stop_on_red = false;
    let mut accel_seen = false;
    for _ in 0..60 * 30 {
        run(&mut a, 1.0 / 60.0, 60.0);
        let light = a.world().resource::<TrafficState>().clock.as_ref().unwrap().controller_for_end(0).car().light;
        let list = cars(&mut a);
        let Some((_, t, audio, vel)) = list.first() else { break };
        let c = a.world().resource::<TrafficState>().cars[0];
        // TrafficAudio carries the follower's speed and acceleration; the velocity is along +z.
        assert_eq!(audio.speed, Some(c.speed));
        assert_eq!(audio.load, Some(c.accel));
        assert!((vel.z - c.speed).abs() < 1e-3);
        accel_seen |= c.accel > 0.5;
        if light == Light::Red && c.speed < 0.05 && t.translation.z < -8.0 && t.translation.z > -14.0 {
            saw_stop_on_red = true;
        }
        let lane = a.world().resource::<PopulationState>().world.live(Kind::Vehicle).next().unwrap().position;
        assert_eq!(lane, t.translation.to_array(), "position goes back to the population");
    }
    assert!(accel_seen);
    assert!(saw_stop_on_red, "the car waited at the red light: {:?}", a.world().resource::<Seen>().0.iter().take(6).collect::<Vec<_>>());
    let seen = &a.world().resource::<Seen>().0;
    let red_at = seen.iter().position(|e| matches!(e, TrafficEvent::Junction { entry: Entry::Signal, .. })).expect("held by the light");
    let entered = seen.iter().position(|e| matches!(e, TrafficEvent::EnteredJunction { junction: J, connector: 0, .. })).expect("crossed the junction");
    let on_exit = seen.iter().position(|e| matches!(e, TrafficEvent::EnteredLane { segment: 0x2, .. })).expect("on the exit lane");
    assert!(red_at < entered && entered < on_exit);
}

#[test]
fn living_world_traffic_despawn_removes_the_car() {
    let mut a = app();
    spawn(&mut a, record(1, 0, 40.0, 0));
    spawn(&mut a, record(2, 0, 80.0, 0));
    run(&mut a, 1.0, 60.0);
    assert_eq!(cars(&mut a).len(), 2);
    let tick = a.world().resource::<PopulationState>().world.tick();
    let r = DespawnRecord { id: LivingWorldId { kind: Kind::Vehicle, serial: 1 }, tick, reason: DespawnReason::Distance };
    a.world_mut().write_message(LivingWorldDespawn(r));
    run(&mut a, 0.1, 60.0);
    let list = cars(&mut a);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].0.id.serial, 2);
    let t = a.world().resource::<TrafficState>();
    assert_eq!((t.cars.len(), t.index.len()), (1, 1));
    assert!(a.world().resource::<Seen>().0.iter().any(|e| matches!(e, TrafficEvent::Despawned { id, reason: DespawnReason::Distance } if id.serial == 1)));
}

#[test]
fn living_world_traffic_keeps_the_gap_and_is_the_same_at_any_engine_rate() {
    let positions = |hz: f32| {
        let mut a = app();
        spawn(&mut a, record(1, 0, 100.0, 0));
        spawn(&mut a, record(2, 0, 60.0, 0));
        spawn(&mut a, record(3, 0, 20.0, 0));
        let mut out = Vec::new();
        for _ in 0..20 {
            run(&mut a, 1.0, hz);
            let tick = a.world().resource::<PopulationState>().world.tick();
            let c: Vec<(u32, f32, f32)> = a.world().resource::<TrafficState>().cars.iter().map(|c| (c.key, c.cursor.distance, c.speed)).collect();
            let list = cars(&mut a);
            for w in list.windows(2) {
                let d = w[0].1.translation.distance(w[1].1.translation);
                assert!(d >= 4.0 + FollowParams::default().min_gap * 0.5 - 1e-3, "cars {} / {} {d} m apart", w[0].0.id.serial, w[1].0.id.serial);
            }
            out.push((tick, c));
        }
        out
    };
    let x = positions(60.0);
    let y = positions(144.0);
    // Same world ticks give the same cars (compare where the tick counts agree).
    let mut compared = 0;
    for (tx, cx) in &x {
        if let Some((_, cy)) = y.iter().find(|(ty, _)| ty == tx) {
            assert_eq!(cx, cy, "tick {tx}");
            compared += 1;
        }
    }
    assert!(compared >= 5, "compared {compared}");
}

#[test]
fn living_world_traffic_overrides_by_key() {
    let mut a = app();
    {
        let mut o = a.world_mut().resource_mut::<VehicleOverrides>();
        o.colours.insert("vehicle_taxi01/chassis/0".into(), [0.0, 1.0, 0.0, 1.0]);
        o.glbs.insert("vehicle_taxi01".into(), "mods/x/car.glb".into());
        o.params.insert("taxi01".into(), FollowParams { accel_max: 1.0, ..FollowParams::default() });
        o.paint_gain = Some(1.0);
    }
    spawn(&mut a, record(1, 0, 40.0, 0));
    run(&mut a, 0.1, 60.0);
    let list = cars(&mut a);
    assert_eq!(list[0].0.chassis, [0.0, 1.0, 0.0, 1.0]);
    assert_eq!(list[0].0.glb, "mods/x/car.glb");
    assert_eq!(list[0].0.paint_gain, 1.0);
    assert_eq!(a.world().resource::<TrafficState>().cars[0].params.accel_max, 1.0);
}

#[test]
fn living_world_traffic_material_rows_resolve_by_name_kind_and_mod() {
    use super::vehicle_material::Program;
    let doc = br#"{"models": {}, "entities": {},
        "materials": {"character.livingworld_vehicles": {"params": [[0.05, 1.0, 12.0, 3.0], [1.5, 1.4, 1.2, 5.0]]},
                      "character.livingworld_vehicles_glass": {"params": [[0.05, 1.0, 12.0, 3.0], [1.5, 1.4, 1.2, 9.0], [8.0, 7.0, 5.0, 10.0], [0.0, 0.0, 0.0, 60.0], [0.0, 0.0, 0.0, 0.75]]}},
        "material_kinds": {"vehicle_chassis": "character.livingworld_vehicles", "vehicle_glass": "character.livingworld_vehicles_glass"},
        "environments": {"2c7f38180017266e": "vehicles/env_2c7f38180017266e.png"}}"#;
    let d = parse_vehicle_data(doc, None).unwrap();
    let mut o = VehicleOverrides::default();
    // The GLB's own name wins; a material without one takes its kind's retail row.
    let x = |s: &str| serde_json::from_str::<serde_json::Value>(s).unwrap();
    let named = x(r#"{"kind": "vehicle_chassis", "attrib_material": "character.livingworld_vehicles_glass"}"#);
    assert_eq!(material_rows(&d, &o, Program::Chassis, Some(&named)).unwrap()[4], [0.0, 0.0, 0.0, 0.75]);
    assert_eq!(material_rows(&d, &o, Program::Glass, None).unwrap()[1][3], 9.0);
    assert_eq!(material_rows(&d, &o, Program::Chassis, Some(&x("{}"))).unwrap()[1][3], 5.0);
    // A mod row replaces the export's; restoring the default undoes it.
    o.material_params.insert("character.livingworld_vehicles_glass".into(), vec![[0.0; 4], [0.0; 4], [0.0; 4], [0.0; 4], [0.2, 0.3, 0.4, 0.5]]);
    assert_eq!(material_rows(&d, &o, Program::Glass, None).unwrap()[4], [0.2, 0.3, 0.4, 0.5]);
    assert_eq!(material_rows(&d, &VehicleOverrides::default(), Program::Glass, None).unwrap()[4][3], 0.75);
    // Unknown name: no row, the car keeps its glTF material.
    assert!(material_rows(&d, &o, Program::Glass, Some(&x(r#"{"attrib_material": "character.none"}"#))).is_none());
    assert_eq!(Program::of("vehicle_glass_000030f203e38817"), Some(Program::Glass));
    assert_eq!(Program::of("vehicle_chassis_000030f103e38817"), Some(Program::Chassis));
    assert_eq!(Program::of("Wheel_x"), None);
    // Environment cube: the material's id through the export's strip, a mod's path, or none.
    let env = x(r#"{"environment": "2c7f38180017266e"}"#);
    assert_eq!(environment_path(&d, &o, Some(&env)).as_deref(), Some("private/living_world/vehicles/env_2c7f38180017266e.png"));
    assert_eq!(environment_path(&d, &o, Some(&x("{}"))), None);
    o.environments.insert("2c7f38180017266e".into(), String::new());
    assert_eq!(environment_path(&d, &o, Some(&env)), None, "a mod switches reflections off");
}

#[test]
fn living_world_traffic_environment_strip_becomes_a_cube() {
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureViewDimension};
    let strip = |w: u32, h: u32| Image::new_fill(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, &[0, 0, 0, 255], TextureFormat::Rgba8UnormSrgb, default());
    let mut image = strip(4, 24);
    assert!(make_cube(&mut image));
    assert_eq!(image.texture_descriptor.size.depth_or_array_layers, 6);
    assert_eq!((image.width(), image.height()), (4, 4));
    assert_eq!(image.texture_view_descriptor.as_ref().and_then(|d| d.dimension), Some(TextureViewDimension::Cube));
    assert!(make_cube(&mut image), "already a cube");
    assert!(!make_cube(&mut strip(4, 20)), "not six square faces");
}

#[test]
fn living_world_traffic_material_uniform_is_the_retail_constant_set() {
    use super::vehicle_material::{alpha_mode, params, Program, AMBIENT};
    let rows = [[0.05, 1.0, 12.0, 3.0], [1.5, 1.4, 1.2, 9.0], [8.0, 7.0, 5.0, 10.0], [0.0, 0.0, 0.0, 60.0], [0.0, 0.0, 0.0, 0.75], [0.0, 0.0, 0.0, 0.5]];
    let light = Vec4::new(0.0, 1.0, 0.0, 2.5);
    let p = params(Program::Glass, &rows, [0.9, 0.8, 0.1, 1.0], [0.2, 0.2, 0.2, 1.0], 1.0, light, true, false);
    // i_colorize_red = secondary, i_colorize_blue = chassis (vehicle_defaultPS c21 / c22).
    assert_eq!(p.red, Vec4::new(0.2, 0.2, 0.2, 1.0));
    assert_eq!(p.blue, Vec4::new(0.9, 0.8, 0.1, 1.0));
    // Glass program, normal map, i_params.x = 1 (no fade), no cube.
    assert_eq!(p.options, Vec4::new(1.0, 1.0, 1.0, 0.0));
    assert_eq!(p.literals, Vec4::new(AMBIENT[0], AMBIENT[1], AMBIENT[2], skate_core::living_world::peds::colorize::MASK_GREEN_SQ_MAX));
    assert_eq!(p.rows[4], Vec4::new(0.0, 0.0, 0.0, 0.75));
    assert_eq!(p.rows[8], Vec4::ZERO, "rows past the export's are zero");
    assert_eq!(p.light, light);
    // A mod gain scales both colours, not their alpha.
    let g = params(Program::Chassis, &rows, [0.5, 0.25, 0.0, 1.0], [0.1, 0.1, 0.1, 1.0], 2.0, light, false, false);
    assert_eq!(g.blue, Vec4::new(1.0, 0.5, 0.0, 1.0));
    assert_eq!(g.options.x, 0.0);
    // Retail render state: opaque body, alpha-blended glass (no depth write).
    assert_eq!(alpha_mode(Program::Chassis), AlphaMode::Opaque);
    assert_eq!(alpha_mode(Program::Glass), AlphaMode::Blend);
}

#[test]
fn living_world_traffic_proxy_is_the_model_box() {
    let d = data();
    let net = road();
    let (car, ..) = car_from_record(&net, &d, &VehicleOverrides::default(), &std::collections::BTreeMap::new(), &record(7, 0, 50.0, 0), 0.0).unwrap();
    let t = Transform::from_xyz(1.0, 0.0, 2.0);
    let p = proxy(&car, &t, Vec3::new(0.0, 0.0, 5.0));
    assert_eq!(p.id, PROXY_ID_TAG | car.id.to_u64());
    assert_eq!(p.inverse_mass, 0.0);
    // The retail vehicle contact group: the skater's car-hit bail reads contacts against it.
    assert_eq!(p.contact_group, crate::physics::VEHICLE_GROUP);
    assert_eq!((p.linvel.x, p.linvel.y, p.linvel.z), (0.0, 0.0, 5.0));
    let c = p.colliders[0].shape.as_cuboid().unwrap().half_extents;
    assert!((c.x - 0.9).abs() < 1e-5 && (c.y - 0.75).abs() < 1e-5 && (c.z - 2.15).abs() < 1e-5);
    let at = p.colliders[0].pose.translation;
    assert!((at.x - 1.0).abs() < 1e-5 && (at.y - 0.75).abs() < 1e-5 && (at.z - (2.0 - 0.05)).abs() < 1e-5);
    let _ = SignalTimings { green: 7.0, amber: 1.0, all_red: 0.5, walk_split: 0.4 };
}

#[test]
fn grab_splines_and_driver_horn_values_load() {
    let d = data();
    let m = &d.models["vehicle_taxi01"];
    assert_eq!(m.grab_splines.len(), 1, "a spline that is not whole Bezier segments is dropped");
    assert_eq!((m.grab_splines[0].points.len(), m.grab_splines[0].direction), (4, [0.0, 0.0, -1.0]));
    let spec = &d.specs["taxi01"];
    assert_eq!(spec.driver, "driver_taxi");
    let h = spec.params.horn;
    assert_eq!((h.blocked_time, h.enabled_chance), (1.0, 0.2));
    assert!((h.approach_speed - 10.0 / 3.6).abs() < 1e-6);
}

#[test]
fn a_car_enters_the_grab_scene_with_its_rear_spline_in_world_space() {
    let d = data();
    let net = road();
    let (car, ..) = car_from_record(&net, &d, &VehicleOverrides::default(), &std::collections::BTreeMap::new(), &record(7, 0, 50.0, 0), 0.0).unwrap();
    let t = Transform::from_xyz(1.0, 0.0, 2.0).with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
    let motion = CarMotion { drawn_prev: t, prev: t, curr: t, wheel_drawn_prev: 0.0, wheel_prev: 0.0, wheel: 0.0, velocity: Vec3::new(5.0, 0.0, 0.0) };
    let o = car_grab_object(&car, &motion, &d).unwrap();
    assert_eq!(o.id, CAR_GRAB_TAG | 7);
    assert!(matches!(o.provider, skate_core::player::offboard::grab_scene::Provider::Vehicle));
    let s = &o.splines[0];
    assert_eq!((s.descriptor.kind, s.descriptor.id), (1, CAR_GRAB_TAG | 7 << 3));
    // The record puts the rear-edge end (model (0.78, 0.93, -2.39)) behind the car: yaw 90 deg maps -z to -x and +x to -z.
    let r = o.record(s).unwrap();
    let [start, _] = r.endpoints();
    assert!((start[0] - (1.0 - 2.39)).abs() < 1e-4 && (start[1] - 0.93).abs() < 1e-4 && (start[2] - (2.0 - 0.78)).abs() < 1e-4, "{start:?}");
    assert!(skate_core::player::offboard::grab_scene::Registry::new(vec![o]).is_ok());
}

/// StayingParked (`82C39120` / `82C391F0`): a parked car carries `VehicleParked` (only then do contacts set its
/// alarm off), loses it when it leaves the parked state, and with no alarm rule present it never alarms.
#[test]
fn a_parked_car_is_marked_for_the_car_alarm() {
    use skate_core::living_world::traffic::manoeuvre::Manoeuvre;
    let has = |a: &mut App| {
        let mut q = a.world_mut().query_filtered::<(), (With<TrafficCar>, With<crate::world_audio::VehicleParked>)>();
        q.iter(a.world()).count()
    };
    let mut a = app();
    spawn(&mut a, record(1, 0, 40.0, 0));
    run(&mut a, 0.1, 60.0);
    assert_eq!(has(&mut a), 0, "a driving car is not parked");
    {
        let mut t = a.world_mut().resource_mut::<TrafficState>();
        let d = t.cars[0].cursor.distance;
        t.cars[0].manoeuvre = Manoeuvre::Parked { spot: d, time: 0.0 };
    }
    run(&mut a, 0.1, 60.0);
    assert_eq!(has(&mut a), 1);
    let c = a.world().resource::<TrafficState>().cars[0];
    assert!(!c.alarming && matches!(c.manoeuvre, Manoeuvre::Parked { time, .. } if time > 0.0), "{:?}", c.manoeuvre);
    a.world_mut().resource_mut::<TrafficState>().cars[0].manoeuvre = Manoeuvre::Following;
    run(&mut a, 0.1, 60.0);
    assert_eq!(has(&mut a), 0);
}

/// Car rails (second rail provider, 82C4C4E0 with the car matrix): model-space rails placed by the car's current
/// transform, owner (car serial, rail), no moving / parked test; a model without rails provides none.
#[test]
fn a_car_provides_its_grind_rails_at_its_current_transform() {
    use crate::grind_world::{props::{car_owner, PropRail, PropSegment}, RailRules, RailSource};
    let mut d = data();
    let net = road();
    let (car, ..) = car_from_record(&net, &d, &VehicleOverrides::default(), &std::collections::BTreeMap::new(), &record(7, 0, 50.0, 0), 0.0).unwrap();
    let rail = PropRail { guids: [3, 4], segments: vec![PropSegment { start: [0.0, 1.5, -1.0, 1.0], end: [0.0, 1.5, 1.0, 1.0], min: [0.0, 1.5, -1.0], max: [0.0, 1.5, 1.0] }] };
    let t = Transform::from_xyz(1.0, 0.0, 2.0).with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
    let motion = CarMotion { drawn_prev: t, prev: t, curr: t, wheel_drawn_prev: 0.0, wheel_prev: 0.0, wheel: 0.0, velocity: Vec3::new(5.0, 0.0, 0.0) };
    assert!(car_rails(&[(&car, &motion)], &d).0.is_empty(), "no rails exported for the model: none provided");
    d.models.get_mut(&car.model).unwrap().grind_rails = vec![rail].into();
    let poses = car_rails(&[(&car, &motion)], &d);
    let mut hits = Vec::new();
    poses.append_rails([-1.0, 0.0, 0.0], [3.0, 3.0, 4.0], None, &RailRules::default(), &mut hits);
    assert_eq!(hits.len(), 1);
    let h = hits[0].primitive;
    // Yaw 90 deg maps model -z to -x: the roof rail runs along world x at height 1.5.
    assert!((h.start[0] - 0.0).abs() < 1e-4 && (h.start[1] - 1.5).abs() < 1e-4 && (h.start[2] - 2.0).abs() < 1e-4, "{:?}", h.start);
    assert!((h.end[0] - 2.0).abs() < 1e-4 && (h.end[2] - 2.0).abs() < 1e-4, "{:?}", h.end);
    assert_eq!(h.owner, car_owner(7, 0));
}

#[test]
fn a_model_without_its_own_collision_takes_the_recipe_mesh() {
    // Installs exported before 2026-10-16 keep the chassis mesh only on the recipe; without it the car collided as its
    // bounds box.
    let doc = br#"{"models": {
        "a": {"glb": "vehicles/a.glb", "recipe": "r"},
        "b": {"glb": "vehicles/b.glb", "recipe": "none"},
        "c": {"glb": "vehicles/c.glb", "recipe": "r", "collision": {"triangles": [[[0,0,0],[0,1,0],[1,0,0]]]}}},
        "recipes": {"r": {"glb": {"collision": {"triangles": [[[0,0,0],[0,1,0],[0,0,1]], [[0,0,0],[1,0,0],[0,0,1]]]}}}}}"#;
    let d = parse_vehicle_data(doc, None).unwrap();
    assert_eq!(d.models["a"].collision.as_ref().map(|c| c.triangles.len()), Some(2));
    assert!(d.models["b"].collision.is_none(), "no mesh anywhere: the bounds box");
    assert_eq!(d.models["c"].collision.as_ref().map(|c| c.triangles.len()), Some(1), "the model's own mesh wins");
}

/// b285: the skater's solve of a tick meets the car where it stood the tick before (traffic moves after Physics), so
/// the car is drawn one tick behind `curr`, between the same ticks as the skater holding it.
#[test]
fn the_car_is_drawn_on_the_ticks_the_skater_met_it() {
    let at = |x: f32| Transform::from_xyz(x, 0.0, 0.0);
    let motion = CarMotion { drawn_prev: at(0.0), prev: at(0.5), curr: at(1.0), wheel_drawn_prev: 6.0, wheel_prev: 0.2, wheel: 0.4, velocity: Vec3::new(30.0, 0.0, 0.0) };
    let (p, _, wheel) = drawn_pose(&motion, 0.5);
    assert!((p.x - 0.25).abs() < 1e-6, "halfway from drawn_prev to prev, never past prev: {p:?}");
    assert!((drawn_pose(&motion, 1.0).0.x - 0.5).abs() < 1e-6);
    // The wheel wraps forward through 2 pi like before.
    let d = 0.2 + std::f32::consts::TAU - 6.0;
    assert!((wheel - (6.0 + d * 0.5)).abs() < 1e-5, "{wheel}");
}
