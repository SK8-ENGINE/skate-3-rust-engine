//! BipedToolkit refresh82D81610. Query observations are owned by the scene;
//! this owner retains native contact history and executes the classifier chain.
use super::{
    contact_intersections::plane_segment,
    contact_output::History,
    contact_packet::{Packet, SupportHit},
    contact_queries::{Input, Layout, V},
    contact_records::{Direction, Records, Source},
    contact_segments::Segments,
    ground_query::Edge,
};
use crate::{math::Vector3, physics::native_arithmetic::dot3};
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub position: V,
    pub normal: V,
}
pub struct Observations {
    pub input: Input,
    pub support: [Option<SupportHit>; 3],
    /// Provider IDs from Layout, including the reverse horizontal queries.
    pub obstacles: [Option<Hit>; 44],
    /// Actual82C1EAD8 results in native provider order and capped at40.
    pub edges: Vec<Edge>,
}
#[derive(Default)]
pub struct Toolkit {
    pub packet: Packet,
    pub history: History,
    pub contact_age: i32,
    pub pending: Option<Observations>,
}
fn vector(v: Vector3) -> V {
    [v.x, v.y, v.z, 0.]
}
fn xyz(v: V) -> Vector3 {
    Vector3::new(v[0], v[1], v[2])
}
fn sub(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] - b[i])
}
impl Toolkit {
    /// Player Update82DB4048 completes the preceding Sync's submission before
    /// any physical-state PreUpdate. The source decrements320, then refreshes
    /// unconditionally (refresh itself publishes30 even with no pending query).
    pub fn begin_player_update(&mut self, layout: &Layout) {
        self.contact_age = self.contact_age.saturating_sub(1).max(0);
        self.refresh(layout);
    }
    /// Reset82D30BD0 / Exit82D30CC0 must finish outstanding providers before
    /// clearing the completion latch and classifier history.
    pub fn reset_contacts(&mut self, layout: &Layout) {
        self.refresh(layout);
        self.contact_age = 0;
        self.history = History::default();
    }
    pub fn submit(&mut self, observations: Observations) {
        self.pending = Some(observations);
        self.contact_age = 0;
    }
    pub fn refresh(&mut self, layout: &Layout) {
        self.packet = Packet::default();
        self.contact_age = 30;
        let Some(observations) = self.pending.take() else {
            return;
        };
        let input = observations.input;
        let previous_flags = self.history.retained_candidate.map_or(0, |c| c.flags);
        let mut records = Records::default();
        self.packet
            .consume_support(input, observations.support, previous_flags, &mut records);
        gather(layout, &observations, &mut records);
        super::contact_sort::sort(&mut records.surface);
        super::contact_sort::sort(&mut records.obstacle);
        super::contact_promotion::promote(input, &mut records);
        super::contact_intersections::constrain(input, &mut records);
        let reduction = super::contact_simplify::simplify(input, &mut records.surface);
        self.packet.distance_172 = reduction.forward_limit;
        let mut segments = Segments::default();
        segments.rebuild(input, &records.surface);
        let mut candidates = super::contact_classify::candidates(
            input,
            &self.packet,
            &segments,
            reduction.forward_limit,
            previous_flags,
        );
        super::contact_output::publish(
            input,
            &mut self.packet,
            &records,
            &segments,
            &reduction,
            &mut candidates,
            &mut self.history,
        );
    }
}

/// Bounds at19056/19072, used by the scene to obtain real query edges.
pub fn edge_bounds(input: Input) -> (V, V) {
    let center: V = std::array::from_fn(|i| {
        input.surface_up[i].mul_add(
            f32::from_bits(0x3e99_9999),
            input.surface_forward[i].mul_add(f32::from_bits(0x3f66_6666), input.position[i]),
        )
    });
    let extent: V = std::array::from_fn(|i| {
        input.surface_forward[i].abs().mul_add(
            f32::from_bits(0x3f66_6666),
            input.surface_up[i].abs().mul_add(
                f32::from_bits(0x3f8c_cccd),
                input.surface_right[i].abs() * f32::from_bits(0x3d19_999a),
            ),
        )
    });
    (
        std::array::from_fn(|i| center[i] - extent[i]),
        std::array::from_fn(|i| center[i] + extent[i]),
    )
}
fn edge_angle(delta: V, forward: V) -> f32 {
    let angle = crate::physics::board_ground::angle_between(xyz(delta), xyz(forward));
    let turns = angle * f32::from_bits(0x3e22_f983);
    let fraction = turns - turns.floor();
    let wrapped = (fraction - if fraction > 0.5 { 1. } else { 0. }) * f32::from_bits(0x40c9_0fdb);
    let sign = if wrapped > 0. { 1. } else { -1. };
    let absolute = wrapped * sign;
    sign * if absolute > f32::from_bits(0x3fc9_0fdb) {
        absolute - f32::from_bits(0x4049_0fdb)
    } else {
        absolute
    }
}
fn gather(layout: &Layout, observations: &Observations, records: &mut Records) {
    let input = observations.input;
    let (_, world) = layout.world_probes(input);
    let mut overrides: Vec<Option<V>> = vec![None; layout.vertical.len()];
    for edge in observations.edges.iter().take(40) {
        let start = vector(edge.start);
        let end = vector(edge.end);
        let delta = sub(end, start);
        if edge_angle(delta, input.surface_forward).abs() > 30. * f32::from_bits(0x3c8e_fa35) {
            if let Some(position) = plane_segment(input.position, input.surface_right, start, end) {
                let mut normal = input.surface_up;
                records.insert(
                    input,
                    position,
                    &mut normal,
                    Source::Intersection,
                    Direction::None,
                    -1.,
                );
            }
            continue;
        }
        let a = dot3(sub(start, input.position), input.surface_forward);
        let b = dot3(sub(end, input.position), input.surface_forward);
        let crossing: V = std::array::from_fn(|i| delta[i].mul_add(a / (a - b), start[i]));
        if dot3(input.surface_right, sub(crossing, input.position)).abs() > 0.5 {
            continue;
        }
        let (low, high) = if a - b >= 0. { (b, a) } else { (a, b) };
        for (i, probe) in layout.vertical.iter().enumerate() {
            let forward = probe.start[2];
            if !(forward >= low && high >= forward) {
                continue;
            }
            let t = (forward - a) / (b - a);
            let edge_point: V = std::array::from_fn(|k| delta[k].mul_add(t, start[k]));
            let base = world[i].start;
            let h = dot3(input.surface_up, sub(edge_point, base));
            let point: V = std::array::from_fn(|k| input.surface_up[k].mul_add(h, base[k]));
            if dot3(input.surface_right, sub(point, edge_point)).abs()
                >= f32::from_bits(0x3d4c_cccd)
            {
                continue;
            }
            if overrides[i].is_none_or(|previous| point[1] > previous[1]) {
                overrides[i] = Some(point);
            }
        }
    }
    for (i, probe) in layout.vertical.iter().enumerate() {
        let hit = observations.obstacles[probe.id];
        let chosen = match (hit, overrides[i]) {
            (Some(hit), Some(position)) if position[1] > hit.position[1] => Some(Hit {
                position,
                normal: input.surface_up,
            }),
            (Some(hit), _) => Some(hit),
            (None, Some(position)) => Some(Hit {
                position,
                normal: input.surface_up,
            }),
            (None, None) => None,
        };
        if let Some(hit) = chosen {
            let mut normal = hit.normal;
            records.insert(
                input,
                hit.position,
                &mut normal,
                Source::Support,
                Direction::None,
                -1.,
            );
        }
    }
    for probe in &layout.horizontal {
        for id in std::iter::once(probe.id).chain(probe.reverse_id) {
            if let Some(hit) = observations.obstacles[id] {
                let mut normal = hit.normal;
                records.insert(
                    input,
                    hit.position,
                    &mut normal,
                    Source::Probe,
                    Direction::None,
                    -1.,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reduced_forward_limit_reaches_the_retained_ground_packet() {
        let input = Input {
            position: [0.; 4],
            surface_right: [1., 0., 0., 0.],
            surface_up: [0., 1., 0., 0.],
            surface_forward: [0., 0., 1., 0.],
            animation_right: [1., 0., 0., 0.],
            animation_up: [0., 1., 0., 0.],
            velocity: [0., 0., 2., 0.],
        };
        let layout = Layout::default();
        let mut obstacles = [None; 44];
        for i in 0..3 {
            obstacles[layout.vertical[i].id] = Some(Hit {
                position: [
                    0.,
                    if i == 0 { 0. } else { 1. },
                    layout.vertical[i].start[2],
                    0.,
                ],
                normal: input.surface_up,
            });
        }
        let support = SupportHit {
            position: [0.; 4],
            normal: input.surface_up,
            frame: Packet::default().support_frame,
            support_id: 1,
        };
        let mut toolkit = Toolkit::default();
        toolkit.submit(Observations {
            input,
            support: [Some(support), None, None],
            obstacles,
            edges: Vec::new(),
        });
        toolkit.refresh(&layout);
        assert_eq!(toolkit.packet.distance_172, layout.vertical[1].start[2]);
        assert!(toolkit.packet.position.iter().all(|x| x.is_finite()));
    }
}
