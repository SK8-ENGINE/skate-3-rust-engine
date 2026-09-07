//! Native grind contact leaf82C1E2C8. This is distinct from the tolerant wheel
//! triangle query: exact parallel rejection and closed segment/barycentric bounds.
use super::native_arithmetic::dot3;
type V = [f32; 4];

/// Intersect a grind primitive segment with a board/truck probe triangle.
/// Returns the native hit position; acquisition/scoring belongs to the caller.
/// Both windings are accepted. Failed branches do not synthesize a contact.
pub fn segment_triangle(start: V, end: V, triangle: [V; 3]) -> Option<V> {
    let [a, b, c] = triangle;
    let ac = sub(c, a);
    let ab = sub(b, a);
    let direction = sub(end, start);
    let normal = cross(ab, ac);
    let denominator = -dot3(direction, normal);
    if denominator == 0.0 { return None; }
    let from = sub(start, a);
    let inverse = 1.0 / denominator;
    let t = inverse * dot3(from, normal);
    if t < 0.0 || t > 1.0 { return None; }
    let side = cross(from, direction);
    let u = inverse * dot3(ac, side);
    let v = (1.0 / -denominator) * dot3(ab, side);
    if u + v > 1.0 || u < 0.0 || v < 0.0 { return None; }
    Some(core::array::from_fn(|i| direction[i].mul_add(t, start[i])))
}

fn sub(a: V, b: V) -> V { core::array::from_fn(|i| a[i] - b[i]) }
fn cross(a: V, b: V) -> V {
    [(-a[2]).mul_add(b[1], a[1] * b[2]),
     (-a[0]).mul_add(b[2], a[2] * b[0]),
     (-a[1]).mul_add(b[0], a[0] * b[1]), 0.0]
}
