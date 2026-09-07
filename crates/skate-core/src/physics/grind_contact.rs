//! Native grind contact leaf82C1E2C8. This is distinct from the tolerant wheel
//! triangle query: exact parallel rejection and closed segment/barycentric bounds.
use super::native_arithmetic::dot3;
type V = [f32; 4];

/// Native query entry: endpoints plus a spline owner, not three vectors.
#[derive(Clone, Copy, Debug)]
pub struct Primitive {
    pub start: V,
    pub end: V,
    pub owner: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct TruckContact {
    pub position: V,
    pub primitive: usize,
}

/// Truck rectangles from82C1FDC0. The geometry dimensions come from the
/// physics_grinds collection (TruckToWheel584, DeckCenterToTruck636).
/// Output order is positive board-forward truck, negative board-forward truck.
/// These are geometric contacts;82D89150 still decides whether to engage.
pub fn truck_contacts(
    board: [V; 4],
    flags_2484: u32,
    truck_to_wheel: f32,
    deck_center_to_truck: f32,
    primitives: &[Primitive],
) -> [Option<TruckContact>; 2] {
    if flags_2484 & 0x0020_0000 != 0 {
        return [None; 2];
    }
    let [right, up, forward, position] = board;
    //821659F8 /8208EA7C: the probe starts .02 below the board and extends
    //another .20 down its local up axis. Do not substitute world vertical.
    let centre = core::array::from_fn(|i| up[i].mul_add(-0.02, position[i]));
    let side = scale(right, truck_to_wheel);
    let along = scale(forward, deck_center_to_truck);
    let down = scale(up, -0.2);
    let centres = [add(centre, along), sub(centre, along)];
    let mut result = [None; 2];
    let mut distance = [1_000_000.0; 2]; //822F88D4, squared-distance ceiling.
    for (primitive, edge) in primitives.iter().enumerate() {
        for truck in 0..2 {
            let a = add(centres[truck], side);
            let b = sub(centres[truck], side);
            let c = add(b, down);
            let d = add(a, down);
            //Native calls both triangles and retains the first triangle's
            //intersection if both succeed, including a shared diagonal hit.
            let first = segment_triangle(edge.start, edge.end, [a, b, c]);
            let second = segment_triangle(edge.start, edge.end, [a, d, c]);
            if let Some(position) = first.or(second) {
                let delta = sub(position, centres[truck]);
                let squared = dot3(delta, delta);
                if distance[truck] > squared {
                    distance[truck] = squared;
                    result[truck] = Some(TruckContact { position, primitive });
                }
            }
        }
    }
    result
}

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
fn add(a: V, b: V) -> V { core::array::from_fn(|i| a[i] + b[i]) }
fn scale(a: V, s: f32) -> V { a.map(|v| v * s) }
fn cross(a: V, b: V) -> V {
    [(-a[2]).mul_add(b[1], a[1] * b[2]),
     (-a[0]).mul_add(b[2], a[2] * b[0]),
     (-a[1]).mul_add(b[0], a[0] * b[1]), 0.0]
}
