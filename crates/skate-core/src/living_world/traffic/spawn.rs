//! Placing a census car on the road (milestone V2): what the retail vehicle factory does with a spawn point.
//! Retail mechanism (826B9B90, 82C36300): docs/hails-additions/26h-living-world-traffic.md#car-placement

use super::graph::RoadNetwork;
use crate::living_world::Vec3;

/// Retail values of the factory placement; every field is data a mod can override.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacementRules {
    /// Distance kept from both segment ends and the gap to the next car ahead (`0x820BD16C`).
    pub end_margin: f32,
    /// Extra gap ahead (`0x82165A10` = 0).
    pub extra_ahead: f32,
    /// The share of a car's length counted before its position (`0x8209975C` = 0.5).
    pub half: f32,
    /// Seconds of the follower's speed kept free behind the new car (the speed in m/s is added
    /// as metres in `sub_82E14928`, i.e. one second).
    pub behind_seconds: f32,
    /// Radius of the overlap query after placing (`0x820996EC` = 20 m).
    pub clear_radius: f32,
    /// Lane pick roll range: `trunc(u32 x 100 / 2^32) mod n` (`0x822F94B0`).
    pub lane_roll: u32,
}

impl Default for PlacementRules {
    fn default() -> Self {
        Self { end_margin: 15.0, extra_ahead: 0.0, half: 0.5, behind_seconds: 1.0, clear_radius: 20.0, lane_roll: 100 }
    }
}

/// A car already on a lane, as the placement test sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LaneCar {
    /// Distance along the segment (m).
    pub distance: f32,
    /// Length (m).
    pub length: f32,
    /// Speed (m/s).
    pub speed: f32,
}

/// Where a car goes: segment index, lane, distance along the segment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LaneSpot {
    pub segment: usize,
    pub lane: u8,
    pub distance: f32,
}

fn in_triangle(p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> bool {
    let cross = |o: [f32; 2], u: [f32; 2], v: [f32; 2]| (u[0] - o[0]) * (v[1] - o[1]) - (u[1] - o[1]) * (v[0] - o[0]);
    let (d1, d2, d3) = (cross(a, b, p), cross(b, c, p), cross(c, a, p));
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

/// Road surface under `point` (826B3B18 + 82E16410): first piece whose triangles hold it; `None` off the road.
pub fn road_under(net: &RoadNetwork, point: Vec3) -> Option<(usize, f32)> {
    let p = [point[0], point[2]];
    for (si, s) in net.segments.iter().enumerate() {
        let b = s.bounds;
        if p[0] < b[0] || p[0] > b[2] || p[1] < b[1] || p[1] > b[3] {
            continue;
        }
        for piece in &s.pieces {
            let q = [piece.left_start, piece.right_start, piece.right_end, piece.left_end].map(|v| [v[0], v[2]]);
            let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
            for c in &q {
                lo = [lo[0].min(c[0]), lo[1].min(c[1])];
                hi = [hi[0].max(c[0]), hi[1].max(c[1])];
            }
            if p[0] < lo[0] || p[0] > hi[0] || p[1] < lo[1] || p[1] > hi[1] {
                continue;
            }
            if in_triangle(p, q[0], q[1], q[2]) || in_triangle(p, q[0], q[2], q[3]) {
                return Some((si, piece.start_distance));
            }
        }
    }
    None
}

/// `sub_82E14928`: may a car be placed on `lane` of `segment` at `distance`? `cars` are the cars on
/// that lane (any order).
pub fn lane_fits(net: &RoadNetwork, segment: usize, lane: u8, distance: f32, rules: &PlacementRules, cars: &[LaneCar]) -> bool {
    let s = &net.segments[segment];
    if s.lanes == 0 || lane >= s.lanes || distance < rules.end_margin || distance > s.length - rules.end_margin {
        return false;
    }
    let mut sorted: Vec<LaneCar> = cars.to_vec();
    sorted.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    // First car with the point before it = ahead; the last one not past the point = behind.
    let ahead = sorted.iter().find(|c| distance < c.distance);
    let behind = sorted.iter().rev().find(|c| distance >= c.distance);
    if let Some(a) = ahead {
        if distance + rules.end_margin + rules.extra_ahead > a.distance - a.length * rules.half {
            return false;
        }
    }
    if let Some(b) = behind {
        if b.distance + b.speed * rules.behind_seconds + b.length > distance - rules.end_margin * rules.half {
            return false;
        }
    }
    true
}

/// Steps 1-4: the segment and distance under the point and the lanes a car fits on (ascending).
pub fn candidate_lanes(net: &RoadNetwork, point: Vec3, rules: &PlacementRules, cars_on: &dyn Fn(usize, u8) -> Vec<LaneCar>) -> Option<(usize, f32, Vec<u8>)> {
    let (segment, distance) = road_under(net, point)?;
    let s = &net.segments[segment];
    if distance < rules.end_margin || distance > s.length - rules.end_margin {
        return None;
    }
    let lanes: Vec<u8> = (0..s.lanes).filter(|&l| lane_fits(net, segment, l, distance, rules, &cars_on(segment, l))).collect();
    (!lanes.is_empty()).then_some((segment, distance, lanes))
}

/// Step 5's pick: `trunc(u32 x roll / 2^32) mod n`.
pub fn pick_lane(lanes: &[u8], draw: u32, rules: &PlacementRules) -> u8 {
    let roll = (draw as f64 * rules.lane_roll as f64 / 4_294_967_296.0) as u32;
    lanes[(roll % lanes.len() as u32) as usize]
}

/// Step 6: does a car of `radius` at `position` overlap any obstacle (centre, radius) within the
/// query radius? Horizontal distances.
pub fn overlaps(position: Vec3, radius: f32, obstacles: &[(Vec3, f32)], rules: &PlacementRules) -> bool {
    obstacles.iter().any(|(c, r)| {
        let (dx, dz) = (c[0] - position[0], c[2] - position[2]);
        let d2 = dx * dx + dz * dz;
        d2 <= rules.clear_radius * rules.clear_radius && d2 < (radius + r) * (radius + r)
    })
}
