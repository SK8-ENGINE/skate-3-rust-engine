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
        "livingworld_vehicle_characteristics": {"vehicle_spec_taxi01": {"fields": {"Hash_328B9F4685A14018": 3.0, "Hash_758229215579C6D1": 2.5,
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
    assert_eq!(car.paint_gain, PAINT_GAIN, "estimate default, not retail");
    assert_eq!(audio.engine, "c04_taxi01", "engine record from the entity's spec");
    // On the lane, facing +z (the lane direction).
    assert!(t.translation.x.abs() < 1e-3 && (t.translation.z - (-160.0)).abs() < 1.0, "{:?}", t.translation);
    assert!((t.rotation * Vec3::Z).z > 0.999);
    let spec = &a.world().resource::<TrafficState>().cars[0].params;
    assert_eq!((spec.accel_max, spec.plan_decel), (3.0, 2.5), "follower numbers from the spec record");
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
fn living_world_traffic_tint_rule() {
    // Base palette = identity.
    let mut px = vec![0, 0, 140, 255, 200, 20, 20, 255, 90, 90, 90, 128, 10, 30, 200, 7];
    let orig = px.clone();
    tint_rgba8(&mut px, [0.0, 0.0, 1.0, 1.0], [1.0, 0.0, 0.0, 1.0], 1.0);
    for (a, b) in px.iter().zip(&orig) {
        assert!((*a as i32 - *b as i32).abs() <= 1, "{px:?}");
    }
    // Pure paint blue takes the chassis colour times its shading; grey stays; alpha kept.
    let mut px = vec![0, 0, 128, 255, 90, 90, 90, 128];
    tint_rgba8(&mut px, [1.0, 0.5, 0.0, 1.0], [0.3, 0.3, 0.3, 1.0], 1.0);
    assert_eq!(&px[..4], &[128, 64, 0, 255]);
    assert_eq!(&px[4..], &[90, 90, 90, 128]);
    // Pure red (tail-light mask) takes the secondary colour.
    let mut px = vec![200, 0, 0, 255];
    tint_rgba8(&mut px, [1.0, 0.5, 0.0, 1.0], [0.5, 0.5, 0.5, 1.0], 1.0);
    assert_eq!(&px[..3], &[100, 100, 100]);
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
    let motion = CarMotion { prev: t, curr: t, wheel_prev: 0.0, wheel: 0.0, velocity: Vec3::new(5.0, 0.0, 0.0) };
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
