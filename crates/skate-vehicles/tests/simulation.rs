use skate_vehicles::*;
fn definition() -> VehicleDefinition {
    serde_json::from_str(include_str!("../../../sdk/examples/mario-kart/vehicle.json")).unwrap()
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
    assert!(s.vehicles[&id].controller.current_vehicle_speed.abs() < speed * 0.25, "brake speed {} initial {speed}, pose {:?}", s.vehicles[&id].controller.current_vehicle_speed, s.pose(id));
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
    d.animations.file = None;
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
fn steering_turns_toward_the_drivers_requested_side() {
    for heading in [0., std::f32::consts::FRAC_PI_2] {
        for steering in [-0.5, 0.5] {
            let mut s = simulation();
            let id = s.spawn(definition(), [0., 1., 0.], heading).unwrap();
            for _ in 0..120 {
                s.step(1. / 120.);
            }
            s.vehicles.get_mut(&id).unwrap().controls = Controls {
                throttle: 1.,
                steering,
                ..Default::default()
            };
            for _ in 0..180 {
                s.step(1. / 120.);
            }
            // Driver/camera left is up cross forward: +X at heading zero (+Z forward).
            let p = s.pose(id).unwrap().0;
            let left_displacement = p[0] * heading.cos() - p[2] * heading.sin();
            assert!(
                left_displacement * steering > 0.1,
                "heading={heading}, steering={steering}, left displacement={left_displacement}"
            );
        }
    }
}
#[test]
fn kart_climbs_ramps_without_nose_catching() {
    for degrees in [20_f32, 30., 40.] {
        let slope = degrees.to_radians().tan();
        let mut s = Simulation::default();
        let y = 20. * slope;
        s.ground([
            [[-20.,0.,-30.],[20.,0.,3.],[20.,0.,-30.]],
            [[-20.,0.,-30.],[-20.,0.,3.],[20.,0.,3.]],
            [[-20.,0.,3.],[20.,y,23.],[20.,0.,3.]],
            [[-20.,0.,3.],[-20.,y,23.],[20.,y,23.]],
        ].into_iter()).unwrap();
        let id = s.spawn(definition(), [0.,1.,-2.], 0.).unwrap();
        for _ in 0..240 { s.step(1./120.); }
        s.vehicles.get_mut(&id).unwrap().controls = Controls { throttle: 1., ..Default::default() };
        let mut highest = 0_f32;
        let mut forward = 0_f32;
        for _ in 0..960 {
            s.step(1./120.);
            let p=s.pose(id).unwrap().0;
            highest=highest.max(p[1]); forward=forward.max(p[2]);
        }
        println!("Ramp {degrees}: height={highest}, forward={forward}");
        assert!(highest > 2. && forward > 7., "failed {degrees} degree ramp: {highest}, {forward}");
    }
}
#[test]
fn collision_mass_and_audio_tuning_are_validated() {
    let base=definition();
    let mut d=base.clone(); d.collider_rounding=0.5; assert!(d.validate().is_err());
    let mut d=base.clone(); d.center_of_mass[1]=f32::NAN; assert!(d.validate().is_err());
    let mut d=base.clone(); d.inertia_half_extents=Some([0.,1.,1.]); assert!(d.validate().is_err());
    let mut d=base.clone(); d.engine_audio.max_pitch=0.1; assert!(d.validate().is_err());
    let mute=VehicleTuning { engine_volume:Some(0.), ..Default::default() };
    assert!(mute.valid()); assert_eq!(mute.apply(&base).unwrap().engine_audio.volume,0.);
    let invalid=VehicleTuning { engine_volume:Some(f32::NAN), ..Default::default() };
    assert!(!invalid.valid()); assert!(invalid.apply(&base).is_err());
}
