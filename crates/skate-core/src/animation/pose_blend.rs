//! Standard SQT blend in TU3 ACS828CB418..4A4 and worker828D6BA8..6C2C.
//! The caller owns tree order and coefficients; interpolation does not flatten
//! nested blends or use a renderer's quaternion blend implementation.
use super::output::{PoseBufferError, Sqt};
use crate::physics::native_arithmetic;

pub fn blend(
    first: &[Sqt],
    second: &[Sqt],
    weight: f32,
    output: &mut [Sqt],
) -> Result<(), PoseBufferError> {
    if first.len() != second.len() {
        return Err(PoseBufferError::ShortInput);
    }
    if output.len() < first.len() {
        return Err(PoseBufferError::ShortOutput);
    }
    for ((&a, &b), destination) in first.iter().zip(second).zip(output) {
        *destination = blend_sample(a, b, weight);
    }
    Ok(())
}

pub fn blend_sample(first: Sqt, second: Sqt, weight: f32) -> Sqt {
    let positive = native_arithmetic::dot4(first.rotation, second.rotation) > 0.0;
    let rotation = core::array::from_fn(|lane| {
        if positive {
            (second.rotation[lane] - first.rotation[lane]).mul_add(weight, first.rotation[lane])
        } else {
            (-(second.rotation[lane] + first.rotation[lane])).mul_add(weight, first.rotation[lane])
        }
    });
    let squared = native_arithmetic::dot4(rotation, rotation);
    let mut inverse = native_arithmetic::reciprocal_square_root_estimate(squared);
    for _ in 0..2 {
        inverse = (inverse * 0.5).mul_add((-squared).mul_add(inverse * inverse, 1.0), inverse);
    }
    Sqt {
        scale: interpolate(first.scale, second.scale, weight),
        rotation: rotation.map(|lane| lane * inverse),
        translation: interpolate(first.translation, second.translation, weight),
    }
}

/// ACSChannelBlend828CC91C..CA64 selects the first subtree's translation.W
/// when requested, otherwise the second's, and clamps the weighted coefficient.
pub fn channel_blend_sample(first: Sqt, second: Sqt, weight: f32, use_first_weights: bool) -> Sqt {
    let channel = if use_first_weights { first.translation[3] } else { second.translation[3] };
    let coefficient = weight * channel;
    let coefficient = if coefficient > 1.0 { 1.0 } else { coefficient };
    let coefficient = if 0.0 > coefficient { 0.0 } else { coefficient };
    blend_sample(first, second, coefficient)
}

fn interpolate(first: [f32; 4], second: [f32; 4], weight: f32) -> [f32; 4] {
    core::array::from_fn(|lane| (second[lane] - first[lane]).mul_add(weight, first[lane]))
}

#[cfg(test)]
#[path = "tests/pose_blend.rs"]
mod tests;
