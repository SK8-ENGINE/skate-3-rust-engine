//! TU3 82D86F00 airborne entry and 82D872B8 impact admission.
//! The caller retains this result until Grind::PreUpdate82D40AF8 consumes it.
use super::{V, dot3};

pub fn airborne_velocity(kind: u32, direction: V, normal: V, velocity: V) -> V {
    let across = cross(normal, direction);
    let removed = across.map(|v| v * dot3(velocity, across));
    let amount = match kind {
        0 | 3 => 0.4,
        1 | 5 => 0.2,
        2 | 4 => 0.8,
        _ => 0.0,
    };
    core::array::from_fn(|i| {
        (velocity[i] - removed[i]).mul_add(amount, velocity[i] * (1.0 - amount))
    })
}

/// Native reason indices: byte33 steep entry,29 excessive correction,34
/// excessive horizontal impact. Retain both impact requests when both fire.
pub fn airborne_rejections(
    direction: V, up: V, board_velocity: V, air_velocity: V, corrected: V,
    surface_kind: u32, surface_side: V, max_delta: f32,
) -> Vec<usize> {
    if dot3(up, direction).abs() > 0.5 { return vec![13]; }
    let delta = core::array::from_fn(|i| corrected[i] - air_velocity[i]);
    let mut reasons = Vec::new();
    if dot3(delta, delta).sqrt() > max_delta { reasons.push(9); }
    let along = dot3(board_velocity, direction);
    let mut transverse = core::array::from_fn(|i| direction[i] * along - board_velocity[i]);
    transverse[1] = 0.0;
    if dot3(transverse, transverse) > 49.0
        && (surface_kind == 0 || dot3(board_velocity, surface_side) > 0.0)
    { reasons.push(14); }
    reasons
}
fn cross(a: V, b: V) -> V {
    [(-a[2]).mul_add(b[1],a[1]*b[2]),(-a[0]).mul_add(b[2],a[2]*b[0]),
     (-a[1]).mul_add(b[0],a[0]*b[1]),0.0]
}
