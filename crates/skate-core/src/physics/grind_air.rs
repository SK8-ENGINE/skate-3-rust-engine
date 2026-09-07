//! TU3 GrindAirAdjust82D712E0: predicted board/contact selection and bounded pose correction.
use super::grind_contact::{control::rotate, Primitive};
use super::skeleton_animation_record::{AnimationPartTransform as Frame, IDENTITY};
use crate::riding::ground_correction_math::dot_product as dot;
type V = [f32; 4];
const UP: V = [0., 1., 0., 0.];
const DEG: f32 = core::f32::consts::PI / 180.0;
#[derive(Clone, Debug)]
pub struct Settings {
    pub frames: usize,
    pub stomp: f32,
    pub points: [V; 5],
    pub ranges: [f32; 7],
    pub distances: [f32; 7],
    pub yaw_assist: [f32; 7],
    pub max_offset: f32,
    pub max_delta: f32,
    pub max_angle: f32,
}
#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub edge: Primitive,
    pub limits: [V; 3],
}
#[derive(Clone, Copy, Debug)]
pub struct Adjustment {
    pub axis: V,
    pub offset: V,
    pub angles: V,
}
#[derive(Default)]
pub struct GrindAir {
    pub target: Option<Target>,
    offset_delta: V,
    offset: V,
    angle_delta: V,
    angles: V,
    previous_kind: Option<usize>,
}
impl GrindAir {
    ///Start82D34FE8 clears retained displacement and angle integrators.
    pub fn start(&mut self, target: Target) {
        *self = Self {
            target: Some(target),
            ..Self::default()
        };
    }
    pub fn update(
        &mut self,
        board: Frame,
        velocity: V,
        angular_velocity: V,
        up: V,
        dt: f32,
        flags: [u32; 3],
        inverted: bool,
        s: &Settings,
    ) -> Option<Adjustment> {
        let target = self.target?;
        //82D712E0 suppression: Processed2480 bit26,2472 bit15,2468 bit3.
        if flags[0] & 0x0400_0000 != 0 || flags[1] & 0x8000 != 0 || flags[2] & 8 != 0 {
            self.offset = [0.; 4];
            self.angles = [0.; 4];
            return None;
        }
        let rail = unit(sub(target.edge.end, target.edge.start));
        let normal = unit(cross(rail, cross(up, rail)));
        let count = s.frames;
        if count < 2 {
            return None;
        }
        //82D71430 integrates physical angular velocity and native gravity/stomp.

        let mut samples = Vec::with_capacity(count);
        let mut headings = Vec::with_capacity(count);
        let mut current = board;
        let mut vel = velocity;
        let speed = length(angular_velocity);
        let axis = unit(angular_velocity);
        let acceleration = add([0., -9.8, 0., 0.], scale(up, s.stomp));
        for _ in 0..count {
            let points = s.points.map(|p| point(current, p));
            let forward = sub(points[1], points[4]);
            let flat = unit(sub(forward, scale(normal, dot(forward, normal))));
            let aligned = if dot(flat, rail) < 0.0 {
                scale(flat, -1.0)
            } else {
                flat
            };
            let mut angle = crate::trigonometry::acos(dot(aligned, rail).clamp(-1.0, 1.0));
            if angle.abs() > core::f32::consts::FRAC_PI_2 {
                angle = core::f32::consts::PI - angle;
            }
            let sign = if dot(normal, cross(aligned, rail)) < 0.0 {
                -1.0
            } else {
                1.0
            };
            headings.push(-sign * angle / DEG);
            samples.push(points);
            if speed >= f32::from_bits(0x37800000) {
                for column in &mut current[..3] {
                    *column = rotate(axis, *column, speed * dt);
                }
            }
            vel = add(vel, scale(acceleration, dt));
            current[3] = add(current[3], scale(vel, dt));
        }
        //82D721A0: first strict plane crossing for each of the five deck probes.
        let mut times = [-1.0; 5];
        let mut distances = [0.0; 5];
        let mut corrections = [[0.; 4]; 5];
        let mut earliest = count as f32;
        for probe in 0..5 {
            for i in 1..count {
                let a = dot(normal, sub(samples[i - 1][probe], target.edge.start));
                let b = dot(normal, sub(samples[i][probe], target.edge.start));
                if a * b < 0.0 {
                    let fraction = (a / (a - b)).abs();
                    let time = (i - 1) as f32 + fraction;
                    let at = add(
                        scale(samples[i - 1][probe], 1.0 - fraction),
                        scale(samples[i][probe], fraction),
                    );
                    let on = add(
                        target.edge.start,
                        scale(rail, dot(sub(at, target.edge.start), rail)),
                    );
                    let correction = sub(on, at);
                    times[probe] = time;
                    distances[probe] = length(correction);
                    corrections[probe] = correction;
                    earliest = earliest.min(time);
                    break;
                }
            }
        }
        //82D723D0/82D724E8. Entries0..6: inverted, tips, deck, both trucks, each truck.
        let probes = [0, 1, 4, 0, 2, 2, 3];
        let desired = [90., 90., 90., 90., 0., 0., 0.];
        let valid = |kind: usize| {
            let p = probes[kind];
            let t = times[p];
            if kind == 4 {
                if times[2].min(times[3]) < 4.0 && (times[2] - times[3]).abs() > 2.0 {
                    return false;
                }
            } else if earliest < 5.0 && (earliest - t).abs() > 3.0 {
                return false;
            }
            t > 0.0
                && distances[p] < s.distances[kind]
                && (desired[kind] - headings[(t + 0.5) as usize].abs()).abs() < s.ranges[kind]
        };
        let mut selected = None;
        for mut kind in 0..7 {
            if inverted != (kind == 0) || !valid(kind) {
                continue;
            }
            if kind == 3 {
                if let Some(previous @ (1 | 2)) = self.previous_kind {
                    if valid(previous) {
                        kind = previous;
                    }
                }
            }
            if selected.is_none_or(|old: usize| distances[probes[kind]] < distances[probes[old]]) {
                selected = Some(kind);
            }
            if selected.is_some_and(|k| k >= 5) {
                break;
            }
        }
        let kind = selected?;
        self.previous_kind = Some(kind);
        let p = probes[kind];
        let time = times[p];
        let sample = (time + 0.5) as usize;
        //82D72610: integrate correction/time; bound each step and total offset.
        self.offset_delta = limited(
            add(self.offset_delta, scale(corrections[p], 1.0 / time)),
            s.max_delta,
        );
        self.offset = limited(add(self.offset, self.offset_delta), s.max_offset);
        //82D72810: native angle integrators use degrees for the yaw error only.
        let heading = headings[sample];
        let desired = if heading > 0.0 {
            desired[kind]
        } else {
            -desired[kind]
        };
        let speed =
            ((desired - heading) / time * s.yaw_assist[kind]).clamp(-s.max_angle, s.max_angle);
        self.angle_delta[1] =
            (self.angle_delta[1] / DEG + speed).clamp(-s.max_angle, s.max_angle) * DEG;
        let forward = sub(samples[sample][1], samples[sample][4]);
        let across_board = unit(sub(forward, scale(rail, dot(forward, rail))));
        let roll_error = match kind {
            0 => {
                let v = signed_angle(across_board, target.limits[0], rail);
                if v > core::f32::consts::FRAC_PI_2 {
                    v - core::f32::consts::PI
                } else if v < -core::f32::consts::FRAC_PI_2 {
                    v + core::f32::consts::PI
                } else {
                    v
                }
            }
            1 | 2 | 3 => {
                let from = if kind == 1 {
                    scale(across_board, -1.0)
                } else {
                    across_board
                };
                let a = signed_angle(from, target.limits[1], rail);
                let b = signed_angle(from, target.limits[2], rail);
                if a.abs() < b.abs() {
                    a
                } else {
                    b
                }
            }
            _ => 0.0,
        };
        self.angle_delta[0] =
            (self.angle_delta[0] + roll_error / time).clamp(-s.max_angle * DEG, s.max_angle * DEG);
        //82D71BB0: projection of effective deck-right relative to support/up.
        let projected = unit(board[0].map(|v| v - dot(board[0], normal)));
        let pitch_error = if dot(projected, projected) > 0.9 {
            signed_angle(projected, normal, board[2]) * 0.9
        } else {
            0.0
        };
        self.angle_delta[2] = if matches!(kind, 0 | 1 | 2 | 3) {
            pitch_error / time
        } else {
            0.0
        };
        self.angles = add(self.angles, self.angle_delta);
        Some(Adjustment {
            axis: rail,
            offset: self.offset,
            angles: self.angles,
        })
    }
}
impl Adjustment {
    ///82BDCFB0 writes the existing15-frame board/IK offset owner15696.
    pub fn local_transform(
        self,
        animation_to_world: Frame,
        board_position: V,
        physical_forward: V,
    ) -> Frame {
        let across = unit(cross(UP, self.axis));
        let up = unit(cross(self.axis, across));
        let rotation = |v| {
            rotate(
                up,
                rotate(
                    self.axis,
                    rotate(physical_forward, v, self.angles[2]),
                    self.angles[0],
                ),
                self.angles[1],
            )
        };
        let mut world = IDENTITY;
        for i in 0..3 {
            world[i] = rotation(IDENTITY[i]);
        }
        world[3] = sub(add(board_position, self.offset), rotation(board_position));
        let inverse = inverse(animation_to_world);
        super::skeleton_animation_record::compose_affine(
            &inverse,
            &super::skeleton_animation_record::compose_affine(&world, &animation_to_world),
        )
    }
}
fn inverse(f: Frame) -> Frame {
    let mut r = IDENTITY;
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = f[j][i];
        }
    }
    r[3] = [-dot(f[0], f[3]), -dot(f[1], f[3]), -dot(f[2], f[3]), 0.];
    r
}
fn point(f: Frame, v: V) -> V {
    super::skeleton_animation_record::transform_point(&f, v)
}
fn signed_angle(a: V, b: V, axis: V) -> f32 {
    let a = unit(a);
    let b = unit(b);
    let angle = crate::trigonometry::acos(dot(a, b).clamp(-1.0, 1.0));
    if dot(cross(a, b), axis) < 0.0 {
        -angle
    } else {
        angle
    }
}
fn sub(a: V, b: V) -> V {
    core::array::from_fn(|i| a[i] - b[i])
}
fn add(a: V, b: V) -> V {
    core::array::from_fn(|i| a[i] + b[i])
}
fn scale(v: V, s: f32) -> V {
    v.map(|v| v * s)
}
fn length(v: V) -> f32 {
    dot(v, v).sqrt()
}
fn unit(v: V) -> V {
    let l = length(v);
    if l > 0.000001 {
        scale(v, 1.0 / l)
    } else {
        [0.; 4]
    }
}
fn limited(v: V, max: f32) -> V {
    let l = length(v);
    if l > max {
        scale(v, max / l)
    } else {
        v
    }
}
fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
        0.,
    ]
}
