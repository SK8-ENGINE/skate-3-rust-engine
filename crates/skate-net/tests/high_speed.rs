use skate_net::{
    Body, Pose,
    packed::{self, BodyState, MAX_RATE, Packed},
    prediction::is_discontinuity,
};
fn state(x: f32, speed: f32) -> BodyState {
    let pose = Pose {
        p: [x, 0., 0.],
        q: [0., 0., 0., 1.],
    };
    BodyState {
        root: pose,
        enabled: (1u64 << 33) - 1,
        bodies: vec![
            Body {
                pose,
                velocity: [speed, 0., 0.],
                angular: [0.; 3]
            };
            33
        ],
    }
}
#[test]
fn backwards_man_speed_keeps_full_and_delta_snapshots_flowing() {
    let mut baseline = None;
    for (seq, speed) in [200., 251., 900., 2400., 80_000., 100.]
        .into_iter()
        .enumerate()
    {
        let mut source = state(seq as f32 * 50., speed);
        source.bodies[19].angular = [900., -1000., 0.];
        let packed = Packed::body(&source).expect("one fast body must not drop the player");
        let wire = packed::delta(
            1,
            2,
            packed::BODY,
            seq as u32 + 1,
            &packed,
            baseline.as_ref().map(|(s, p)| (*s, p)),
        );
        assert!(wire.len() <= packed::MTU);
        let decoded = packed::apply(&wire, baseline.as_ref().map(|(_, p)| p)).unwrap();
        let result = decoded.unpack_body().unwrap();
        assert_eq!(result.root.p, source.root.p);
        assert_eq!(result.bodies[0].velocity[0], speed.min(MAX_RATE));
        assert_eq!(result.bodies[19].angular[0], 900.);
        assert_eq!(source.bodies[0].velocity[0], speed);
        baseline = Some((seq as u32 + 1, decoded));
    }
}
#[test]
fn nonfinite_rates_are_still_rejected_on_send_and_receive() {
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut source = state(0., 10.);
        source.bodies[5].velocity[1] = value;
        assert!(Packed::body(&source).is_none());
    }
    let mut packed = Packed::body(&state(0., 10.)).unwrap();
    // Compact body pose occupies eleven bytes, then binary16 velocity starts.
    packed.rows[0][11..13].copy_from_slice(&0x7c00u16.to_le_bytes());
    assert!(packed.unpack_body().is_none());
    let wire = packed::delta(1, 2, packed::BODY, 1, &packed, None);
    assert!(packed::apply(&wire, None).is_none());
}
#[test]
fn high_speed_motion_and_lost_samples_do_not_reset_playback() {
    for speed in [250., 900., 2400.] {
        for dt in [0.05, 0.1, 0.3] {
            assert!(!is_discontinuity(
                &state(0., speed),
                &state(speed * dt, speed),
                dt
            ));
        }
    }
    assert!(is_discontinuity(&state(0., 0.), &state(100., 0.), 0.05));
    assert!(is_discontinuity(&state(0., 20.), &state(100., 20.), 0.05));
}
