use skate_vehicles::*;
fn definition() -> VehicleDefinition {
    serde_json::from_str(include_str!("../../../mods/mario-kart/vehicle.json")).unwrap()
}
fn simulation() -> Simulation {
    let mut s = Simulation::default();
    s.ground(
        [
            [[-100., 0., -100.], [100., 0., 100.], [100., 0., -100.]],
            [[-100., 0., -100.], [-100., 0., 100.], [100., 0., 100.]],
        ]
        .into_iter(),
    )
    .unwrap();
    s
}
#[test]
fn kart_suspension_acceleration_braking_reset_and_cleanup() {
    let mut s = simulation();
    let id = s.spawn(definition(), [0., 2., 0.], 0.).unwrap();
    for _ in 0..240 {
        s.step(1. / 120.);
    }
    let height = s.pose(id).unwrap().0[1];
    assert!((0.2..1.).contains(&height), "settled chassis {height}");
    assert!(
        s.vehicles[&id]
            .controller
            .wheels()
            .iter()
            .all(|w| w.raycast_info().is_in_contact)
    );
    s.vehicles.get_mut(&id).unwrap().controls = Controls {
        throttle: 1.,
        ..Default::default()
    };
    for _ in 0..360 {
        s.step(1. / 120.);
    }
    let speed = s.vehicles[&id].controller.current_vehicle_speed;
    assert!(speed > 3., "forward speed {speed}");
    assert!(s.pose(id).unwrap().0[2] > 3.);
    s.vehicles.get_mut(&id).unwrap().controls = Controls {
        brake: 1.,
        ..Default::default()
    };
    for _ in 0..240 {
        s.step(1. / 120.);
    }
    assert!(s.vehicles[&id].controller.current_vehicle_speed.abs() < speed * 0.25);
    s.reset(id, [5., 2., 5.], 1.).unwrap();
    assert_eq!(s.pose(id).unwrap().0, [5., 2., 5.]);
    s.remove(id);
    assert!(s.vehicles.is_empty());
    assert_eq!(s.world.bodies.len(), 1);
}
#[test]
fn definitions_and_controls_reject_invalid_inputs() {
    let mut d = definition();
    d.mass = f32::NAN;
    assert!(d.validate().is_err());
    let mut d = definition();
    d.model = "../outside.glb".into();
    assert!(d.validate().is_err());
    let mut d = definition();
    d.animations.drive = Some("drive".into());
    assert!(d.validate().is_err());
    let mut d = definition();
    d.wheels.clear();
    assert!(d.validate().is_err());
    assert!(
        !Controls {
            throttle: 2.,
            ..Default::default()
        }
        .valid()
    );
    assert!(
        !Controls {
            brake: f32::NAN,
            ..Default::default()
        }
        .valid()
    );
    assert!(!package_path("C:/model.glb"));
    assert!(!package_path("../model.glb"));
    assert!(!package_path("model.glb#Scene1"));
    assert!(
        VehicleTuning {
            max_speed: Some(-1.),
            ..Default::default()
        }
        .apply(&definition())
        .is_err()
    );
}
#[test]
fn independent_vehicles_collide_and_remain_finite() {
    let mut s = simulation();
    let a = s.spawn(definition(), [0., 1., -4.], 0.).unwrap();
    let b = s
        .spawn(definition(), [0., 1., 4.], std::f32::consts::PI)
        .unwrap();
    for id in [a, b] {
        s.vehicles.get_mut(&id).unwrap().controls = Controls {
            throttle: 1.,
            ..Default::default()
        };
    }
    for _ in 0..400 {
        s.step(1. / 120.);
    }
    for id in [a, b] {
        let (p, q) = s.pose(id).unwrap();
        assert!(p.iter().chain(q.iter()).all(|x| x.is_finite()));
    }
}

#[test]
fn positive_steering_turns_left() {
    let mut s = simulation();
    let id = s.spawn(definition(), [0., 1., 0.], 0.).unwrap();
    for _ in 0..120 {
        s.step(1. / 120.);
    }
    s.vehicles.get_mut(&id).unwrap().controls = Controls {
        throttle: 1.,
        steering: 0.5,
        ..Default::default()
    };
    for _ in 0..180 {
        s.step(1. / 120.);
    }
    let x = s.pose(id).unwrap().0[0];
    assert!(x < -0.1, "left steering x={x}");
}
