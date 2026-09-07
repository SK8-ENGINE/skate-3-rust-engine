//! Geometry fields consumed by trajectory assist:82C20728,82C20C08,
//!82C21828 and82C21930. Material/foot-placement flags have other consumers.
use super::{grind::GrindSurfaceEvidence, math::*};
use crate::math::Vector3;
use crate::physics::{
    board_world::BoardWorld,
    ground_hang_geometry::{HangGeometryInput, hang_lines},
};

#[derive(Clone, Copy, Debug)]
pub struct GrindSurface {
    pub evidence: GrindSurfaceEvidence,
    pub normal: Vector,
}

///The query consists of six consecutive48-byte descriptors; descriptor4 is
///the vertical centre line and descriptor5 the raised cross line. The optional
///seventh descriptor is a separate caller probe. Result indexing stays intact.
pub fn investigate(
    world: &BoardWorld,
    start: Vector,
    end: Vector,
    reference: Vector,
    deck_center_to_truck: f32,
) -> Result<Option<GrindSurface>, &'static str> {
    let Some(lines) = hang_lines(
        HangGeometryInput {
            edge_start: xyz(start),
            edge_end: xyz(end),
            reference_point: xyz(reference),
        },
        deck_center_to_truck,
    ) else {
        return Ok(None);
    };
    let mut hits = [None; 6];
    for (i, line) in lines.iter().enumerate() {
        hits[i] = world.query_swept_line(line.start, line.end, line.radius)?;
    }
    let direction = normalize(sub(end, start));
    let up = normalize(cross(direction, cross(UP, direction)));
    let side = normalize(cross(UP, direction));
    //82C20ED8 reads result5 (stack448), not the centre probe atstack384.
    let kind = if (hits[0].is_some() && hits[1].is_some()) || hits[5].is_some() {
        3
    } else if hits[0].is_some() {
        if hits[2].is_some_and(|h| h.geometry.fraction < 0.65) {
            2
        } else {
            1
        }
    } else if hits[1].is_some() {
        if hits[3].is_some_and(|h| h.geometry.fraction < 0.65) {
            2
        } else {
            1
        }
    } else {
        0
    };
    let side = if hits[0].is_none() && hits[1].is_some() {
        scale(side, -1.0)
    } else {
        side
    };
    let centre = madd(direction, dot(sub(reference, start), direction), start);
    let far: [Vector; 2] = std::array::from_fn(|i| {
        let p = hits[i + 2].map_or(lines[i + 2].end, |h| h.geometry.position);
        [p.x, p.y, p.z, 0.0]
    });
    //82C212CC..82C213F4: clearance angle16degrees expands both sides.
    let clearance = f32::from_bits(0x3e8e_fa35);
    let angle0 = crate::trigonometry::acos(
        dot(scale(up, -1.0), normalize(sub(far[0], centre))).clamp(-1.0, 1.0),
    ) + std::f32::consts::FRAC_PI_2
        + clearance;
    let angle1 = std::f32::consts::TAU
        - crate::trigonometry::acos(
            dot(scale(up, -1.0), normalize(sub(far[1], centre))).clamp(-1.0, 1.0),
        )
        - std::f32::consts::FRAC_PI_2
        - clearance;
    let pi = std::f32::consts::PI;
    let normal = if angle0 < angle1 && angle0 < pi && angle1 > pi {
        up
    } else {
        let angle = if angle0 < angle1 {
            if (pi - angle0).abs() < (pi - angle1).abs() {
                angle0
            } else {
                angle1
            }
        } else {
            (angle0 + angle1) * 0.5
        };
        rotate(direction, scale(up, -1.0), angle)
    };
    Ok(Some(GrindSurface {
        evidence: GrindSurfaceEvidence { kind, side },
        normal,
    }))
}

///82C1E220 constructs a half-angle quaternion and rotates the supplied vector.
fn rotate(axis: Vector, value: Vector, angle: f32) -> Vector {
    let (sin, cos) = crate::trigonometry::sin_cos(angle * 0.5);
    let q = scale(axis, sin);
    madd(cross(q, madd(value, cos, cross(q, value))), 2.0, value)
}
fn xyz(v: Vector) -> Vector3 {
    Vector3::new(v[0], v[1], v[2])
}
