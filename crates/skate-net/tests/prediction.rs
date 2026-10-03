use skate_net::{Body, Pose, prediction::CollisionPrediction};

fn body() -> Body {
    Body {
        pose: Pose {
            p: [1., 2., 3.],
            q: [0., 0., 0., 1.],
        },
        velocity: [12., -2., 4.],
        angular: [0., 2., 0.],
    }
}
fn predicted(age: f32) -> Body {
    CollisionPrediction::at(age).unwrap().body(&body())
}
fn near(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.002, "{actual} != {expected}");
}

#[test]
fn fresh_snapshot_coasts_and_rotates_at_published_velocity() {
    let b = predicted(0.025);
    near(b.pose.p[0], 1.3);
    near(b.pose.p[1], 1.95);
    near(b.pose.q[1], 0.025_f32.sin());
    near(b.pose.q[3], 0.025_f32.cos());
    assert_eq!(b.velocity, body().velocity);
    assert_eq!(b.angular, body().angular);
}

#[test]
fn contact_velocity_matches_collider_motion_through_packet_loss() {
    // Includes both sides of the coasting/fading boundary and two lost packets.
    for age in [0.01, 0.049, 0.05, 0.051, 0.075, 0.1, 0.125, 0.149] {
        let dt = 0.0005;
        let before = predicted(age - dt);
        let after = predicted(age + dt);
        let current = predicted(age);
        for i in 0..3 {
            // A centered difference across the fade boundary averages its two accelerations.
            let measured = (after.pose.p[i] - before.pose.p[i]) / (2. * dt);
            assert!((measured - current.velocity[i]).abs() < 0.02);
        }
        let angle = |b: &Body| 2. * b.pose.q[1].atan2(b.pose.q[3]);
        assert!(((angle(&after) - angle(&before)) / (2. * dt) - current.angular[1]).abs() < 0.005);
    }
}

#[test]
fn prolonged_loss_expires_instead_of_leaving_a_moving_ghost() {
    let last = predicted(0.14999);
    near(last.pose.p[0], 2.2); // Bounded to 100 ms of full-speed travel.
    near(last.velocity[0], 0.);
    near(last.angular[1], 0.);
    for age in [0.15, 0.5, 10., f32::INFINITY, f32::NAN, -0.01] {
        assert!(CollisionPrediction::at(age).is_none());
    }
}

#[test]
fn resumed_or_teleported_snapshot_starts_from_its_own_state() {
    assert!(CollisionPrediction::at(2.).is_none());
    let mut resumed = body();
    resumed.pose.p = [100., 4., -200.];
    resumed.velocity = [0.; 3];
    resumed.angular = [0.; 3];
    let b = CollisionPrediction::at(0.).unwrap().body(&resumed);
    assert_eq!(b.pose, resumed.pose);
    assert_eq!(b.velocity, resumed.velocity);
}

#[test]
fn angular_prediction_uses_world_axes_and_normalizes_orientation() {
    let mut b = body();
    // Start rotated 90 degrees about X, then rotate about world Y.
    let s = std::f32::consts::FRAC_1_SQRT_2;
    b.pose.q = [s, 0., 0., s];
    let result = CollisionPrediction::at(0.05).unwrap().body(&b);
    near(result.pose.q[0], s * 0.05_f32.cos());
    near(result.pose.q[1], s * 0.05_f32.sin());
    near(result.pose.q[2], -s * 0.05_f32.sin());
    near(result.pose.q[3], s * 0.05_f32.cos());
    assert!(result.pose.valid());
    b.angular = [500., -500., 500.];
    for age in [0., 0.05, 0.1, 0.149] {
        assert!(CollisionPrediction::at(age).unwrap().body(&b).pose.valid());
    }
}
