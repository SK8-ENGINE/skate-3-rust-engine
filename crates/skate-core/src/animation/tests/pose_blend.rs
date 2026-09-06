use super::*;

fn pose(rotation: [f32; 4]) -> Sqt {
    Sqt {
        scale: [1.0; 4],
        rotation,
        translation: [0.0; 4],
    }
}

#[test]
fn blend_retains_native_antipodal_and_zero_dot_decisions() {
    let identity = pose([0.0, 0.0, 0.0, 1.0]);
    let antipode = pose([0.0, 0.0, 0.0, -1.0]);
    let same = blend_sample(identity, antipode, 0.5);
    assert!((same.rotation[3] - 1.0).abs() < 1e-6);
    let half_turn = pose([1.0, 0.0, 0.0, 0.0]);
    let quarter = blend_sample(identity, half_turn, 0.5);
    assert!(quarter.rotation[0] < 0.0); //Native sign gate is >0, not >=0.
    assert!((quarter.rotation[0].abs() - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    assert!((quarter.rotation[3] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
}

#[test]
fn nested_blends_preserve_tree_order_and_per_blend_normalization() {
    let a = pose([0.0, 0.0, 0.0, 1.0]);
    let b = pose([0.6, 0.0, 0.0, 0.8]);
    let mut c = pose([0.0, 0.6, 0.0, 0.8]);
    c.translation = [4.0, 8.0, 12.0, 0.0];
    let left = blend_sample(blend_sample(a, b, 0.5), c, 0.25);
    let right = blend_sample(a, blend_sample(b, c, 0.25), 0.5);
    assert_eq!(left.translation, [1.0, 2.0, 3.0, 0.0]);
    assert!(
        left.rotation
            .iter()
            .zip(right.rotation)
            .any(|(a, b)| (a - b).abs() > 0.01)
    );
}
