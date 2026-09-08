//! TU3 Biped launch preparation82D7BA78. This packet is consumed by the
//! off-board trajectory owner82D6CA58; it never substitutes a landing query.
use crate::point_graph::PointGraph;
pub type V = [f32; 4];
const UP: V = [0., 1., 0., 0.];
const ZERO: V = [0.; 4];
#[derive(Clone, Copy, Debug)]
pub struct Launch {
    pub velocity: V,
    pub secondary_velocity: V,
    pub position: V,
    pub up: V,
    pub forward: V,
    pub target: V,
    pub angles: [f32; 3],
    pub primary_count: u32,
    pub secondary_count: u32,
    pub has_target: bool,
    pub flag_117: bool,
}
impl Default for Launch {
    fn default() -> Self {
        Self {
            velocity: ZERO,
            secondary_velocity: ZERO,
            position: ZERO,
            up: UP,
            forward: ZERO,
            target: ZERO,
            angles: [0.; 3],
            primary_count: 1,
            secondary_count: 0,
            has_target: false,
            flag_117: false,
        }
    }
}
pub struct Input {
    pub previous_state: u32,
    pub previous_category: u32,
    pub current_state: u32,
    pub current_category: u32,
    pub flags_2472: u32,
    pub flags_2476: u32,
    pub flags_2480: u32,
    pub frame_forward_224: V,
    pub up_544: V,
    pub velocity_608: V,
    pub position_592: V,
    pub target_112: V,
    pub velocity_912: V,
    pub grind_position_1120: V,
    pub grind_axis_1136: V,
    pub stick: [f32; 2],
}
pub struct JumpInput<'a> {
    pub reference_up_144: V,
    pub contact_active_708: bool,
    pub height_792: f32,
    pub velocity_scalar_796: f32,
    pub speed_704: f32,
    pub steering_760: f32,
    pub angular_velocity_688: f32,
    pub turn_curve_1328: &'a PointGraph<8>,
}
pub fn prepare(p: &Input, j: &JumpInput<'_>, current: bool) -> Launch {
    let (state, category) = if current {
        (p.current_state, p.current_category)
    } else {
        (p.previous_state, p.previous_category)
    };
    let mut forward = p.frame_forward_224;
    forward[1] = 0.;
    let forward = unit(forward, ZERO);
    let mut out = Launch {
        velocity: p.velocity_608,
        secondary_velocity: p.velocity_608,
        position: p.position_592,
        up: p.up_544,
        forward,
        angles: [
            f32::from_bits(0x3db2b8c2),
            f32::from_bits(0x3f5f66f3),
            f32::from_bits(0x3f32b8c2),
        ],
        ..Launch::default()
    };
    match category {
        100 => {
            out.target = p.target_112;
            out.has_target = true;
            let y = out.velocity[1];
            let lower = select(y - 3., y, 3.);
            out.velocity[1] = select(y + 3. - lower, lower, y + 3.);
            out.position = madd(out.velocity, f32::from_bits(0x3c888889), out.position);
        }
        400 => {
            out.primary_count = 6;
            let delta = sub(p.position_592, p.grind_position_1120);
            let mut perpendicular = sub(
                delta,
                scale(p.grind_axis_1136, dot(delta, p.grind_axis_1136)),
            );
            perpendicular[1] = 0.;
            out.velocity = madd(unit(perpendicular, ZERO), 2., out.velocity);
            out.velocity = add(out.velocity, UP);
            out.position = madd(out.velocity, f32::from_bits(0x3c888889), out.position);
        }
        500 if p.flags_2480 & 0x80 != 0 || state == 503 => {
            let side = cross(UP, out.velocity);
            let n = length(side);
            if n > f32::from_bits(0x3a83126f) {
                out.velocity = madd(
                    side,
                    (if p.flags_2476 & 4 != 0 { 0.75 } else { -0.75 }) / n,
                    out.velocity,
                );
            }
        }
        500 if p.flags_2476 & 0x80000 != 0 => jump(&mut out, p, j),
        500 => {
            let mut horizontal = out.velocity;
            horizontal[1] = 0.;
            out.primary_count = 6;
            if length(horizontal) < 1.875 {
                out.angles[0] = f32::from_bits(0x3f060a92);
                out.velocity = scale(unit(horizontal, forward), 2.5);
                out.velocity[1] = select(1. - out.velocity[1], 1., out.velocity[1]);
            } else {
                out.velocity = scale(p.velocity_912, 0.75);
                out.velocity[1] = bounded(out.velocity[1], -10., 5.);
            }
        }
        _ => {}
    }
    out
}
fn jump(out: &mut Launch, p: &Input, j: &JumpInput<'_>) {
    let up = unit(j.reference_up_144, j.reference_up_144);
    let tangent = sub(p.velocity_912, scale(up, dot(up, p.velocity_912)));
    let amount = dot(tangent, out.forward);
    let mut initial = madd(out.forward, select(-amount, 0., amount) - amount, tangent);
    let mut redirected = initial;
    let speed2 = dot(initial, initial);
    if p.flags_2472 & 0x10000000 == 0 {
        let stick = [p.stick[0], 0., p.stick[1], 0.];
        if speed2 < 1. {
            initial = limit_length(add(initial, stick), 1.);
            redirected = initial;
        } else if j.contact_active_708 && dot(stick, stick) > f32::from_bits(0x3f4f5c28) {
            let angle =
                crate::player::wipeout_state::orientation::projected_angle(stick, initial, up);
            let turns = angle * f32::from_bits(0x3e22f983);
            let f = turns - turns.floor();
            let angle = (f - if f > 0.5 { 1. } else { 0. }) * f32::from_bits(0x40c90fdb);
            let projected = sub(stick, scale(unit(up, ZERO), dot(stick, unit(up, ZERO))));
            if angle < 45. * f32::from_bits(0x3c8efa35) {
                redirected = scale(unit_unchecked(projected), sqrt(speed2));
            } else if angle < 90. * f32::from_bits(0x3c8efa35) {
                redirected = scale(
                    limit_angle(
                        unit_unchecked(projected),
                        initial,
                        45. * f32::from_bits(0x3c8efa35),
                    ),
                    sqrt(speed2),
                );
            }
        }
    }
    let vertical = select(
        -(up[1] * sqrt(j.height_792 * f32::from_bits(0x419ccccd))),
        0.,
        up[1] * sqrt(j.height_792 * f32::from_bits(0x419ccccd)),
    );
    let mut horizontal = redirected;
    horizontal[1] = 0.;
    out.secondary_velocity = unit(horizontal, ZERO);
    let requested =
        j.turn_curve_1328.evaluate(j.speed_704) * j.steering_760 * f32::from_bits(0x3c8efa35);
    let angle = bounded(
        bounded(
            requested,
            j.angular_velocity_688 - f32::from_bits(0x3fdf66f3),
            j.angular_velocity_688 + f32::from_bits(0x3fdf66f3),
        ) * 0.4,
        f32::from_bits(0xbe860a92),
        f32::from_bits(0x3e860a92),
    );
    out.velocity = rotate(
        madd(initial, j.velocity_scalar_796, scale(up, vertical)),
        up,
        angle,
    );
    out.primary_count = 6;
    out.secondary_count = 3;
}
fn select(t: f32, a: f32, b: f32) -> f32 {
    if t >= 0. { a } else { b }
}
fn bounded(v: f32, lo: f32, hi: f32) -> f32 {
    let v = select(lo - v, lo, v);
    select(hi - v, v, hi)
}
pub(super) fn dot(a: V, b: V) -> f32 {
    crate::physics::native_arithmetic::dot3(a, b)
}
fn add(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] + b[i])
}
pub(super) fn sub(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] - b[i])
}
pub(super) fn scale(v: V, s: f32) -> V {
    v.map(|x| x * s)
}
pub(super) fn madd(v: V, s: f32, b: V) -> V {
    std::array::from_fn(|i| v[i].mul_add(s, b[i]))
}
fn inverse(q: f32) -> f32 {
    let mut r = crate::physics::reciprocal_sqrt::estimate(q);
    for _ in 0..2 {
        r = (r * 0.5).mul_add((-q).mul_add(r * r, 1.), r);
    }
    r
}
fn sqrt(q: f32) -> f32 {
    if q == 0. { 0. } else { q * inverse(q) }
}
pub(super) fn length(v: V) -> f32 {
    sqrt(dot(v, v))
}
pub(super) fn unit(v: V, fallback: V) -> V {
    let q = dot(v, v);
    let inv = inverse(q);
    if sqrt(q) > f32::from_bits(0x358637bd) {
        scale(v, inv)
    } else {
        fallback
    }
}
fn unit_unchecked(v: V) -> V {
    scale(v, inverse(dot(v, v)))
}
pub(super) fn cross(a: V, b: V) -> V {
    [
        (-a[2]).mul_add(b[1], a[1] * b[2]),
        (-a[0]).mul_add(b[2], a[2] * b[0]),
        (-a[1]).mul_add(b[0], a[0] * b[1]),
        0.,
    ]
}
pub(super) fn rotate(v: V, axis: V, angle: f32) -> V {
    let (s, c) = crate::trigonometry::sin_cos(angle * 0.5);
    let mut q = scale(axis, s);
    q[3] = c;
    crate::player::wipeout_state::orientation::rotate(q, v)
}
fn limit_length(v: V, limit: f32) -> V {
    let n = length(v);
    if n < f32::from_bits(0x37800000) {
        return v;
    }
    scale(v, select(limit - n, n, limit) / n)
}
pub(super) fn limit_angle(proposed: V, reference: V, limit: f32) -> V {
    let a = unit_unchecked(proposed);
    let b = unit_unchecked(reference);
    let angle = crate::trigonometry::acos(dot(a, b).max(-1.).min(1.));
    let axis = cross(a, b);
    if angle.abs() < limit || dot(axis, axis) < f32::from_bits(0x37800000) {
        proposed
    } else {
        scale(rotate(b, unit_unchecked(axis), -limit), length(proposed))
    }
}
