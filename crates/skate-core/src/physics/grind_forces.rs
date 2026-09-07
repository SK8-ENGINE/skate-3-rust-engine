//! TU3 grind force leaves. Their inputs belong to the retained grind manager;
//! contact admission and lifecycle are separate from these force calculations.
use super::native_arithmetic::dot3;
type V = [f32; 4];

///82D3FA18, called by the 50-50 update82D41D70 with800,0,.07.
/// Returns a force at the deck origin; never moves the body to a spline.
pub fn lateral_pin(
    board: [V; 4],
    point: V,
    across: V,
    velocity: V,
    strength: f32,
    forward_offset: f32,
    up_offset: f32,
    forward_selected: bool,
    slope_multiplier: f32,
) -> V {
    let direction = if forward_selected {
        board[2]
    } else {
        scale(board[2], -1.0)
    };
    let reference = core::array::from_fn(|i| {
        direction[i].mul_add(forward_offset, board[3][i]) - board[1][i] * up_offset
    });
    let force = scale(across, dot3(sub(point, reference), across) * strength);
    let length = dot3(force, force).sqrt();
    if length <= 0.0 {
        return [0.0; 4];
    }
    let axis = scale(force, length.recip());
    let damping = scale(
        axis,
        strength * f32::from_bits(0x3e08_3127) * dot3(velocity, axis),
    );
    scale(sub(force, damping), slope_multiplier)
}

///82D3FD88 after its native surface/material selection. It damps velocity in
/// the plane perpendicular to the grind normal, including the along-rail lane.
pub fn friction(
    velocity: V,
    grind_normal: V,
    support: V,
    time_multiplier: f32,
    flagged_surface: bool,
    surface_multiplier: f32,
    engagement: u32,
    strengths: [f32; 3],
) -> V {
    let tangent_velocity = sub(velocity, scale(grind_normal, dot3(velocity, grind_normal)));
    let speed = dot3(tangent_velocity, tangent_velocity).sqrt();
    if speed <= 0.001 {
        return [0.0; 4];
    }
    let strength = strengths[match engagement {
        0 => 0,
        1 => 1,
        _ => 2,
    }];
    let load = support[1].max(0.0);
    let surface_flag_multiplier = if flagged_surface { 1.9 } else { 1.0 };
    let multiplier = load / speed
        * time_multiplier
        * surface_flag_multiplier
        * surface_multiplier
        * strength
        * f32::from_bits(0xbef5_c28f);
    scale(tangent_velocity, multiplier)
}

fn sub(a: V, b: V) -> V {
    core::array::from_fn(|i| a[i] - b[i])
}
fn scale(a: V, scale: f32) -> V {
    a.map(|v| v * scale)
}

///Boardslide82D419A0, static rail branch (investigation kind!=2).
///Returns the native point forces in order. Translation comes from the stock
///PhysGrindTranslation animation attribute, not controller magnitude.
pub fn boardslide_control(
    position: V,
    point: V,
    direction: V,
    normal: V,
    velocity: V,
    translation: f32,
    total_mass: f32,
    exiting: bool,
    ledge: bool,
) -> Vec<V> {
    let across = cross(direction, normal);
    let offset = dot3(sub(position, point), across);
    let inward = scale(across, if offset > 0.0 { -1.0 } else { 1.0 });
    if exiting {
        return if offset.abs() > 0.12 {
            vec![scale(inward, 201.0)]
        } else {
            Vec::new()
        };
    }
    let perpendicular = sub(velocity, scale(direction, dot3(velocity, direction)));
    if ledge {
        return vec![scale(across, translation * 25.0)];
    }
    if offset.abs() > 0.16 {
        let mut forces = vec![scale(inward, 10.0)];
        if dot3(inward, perpendicular) < 0.0 {
            let mut stop = scale(perpendicular, -(total_mass * 60.0));
            stop[1] = 0.0;
            forces.push(stop);
        }
        forces
    } else {
        let mut damping = scale(perpendicular, -20.0);
        damping[1] = 0.0;
        vec![scale(across, translation * 25.0), damping]
    }
}

///82D73AB0 straight, static 50-50 branch: both support normals coincide,
///primitive motion336 is zero. The manager age controls the native exit nudge.
pub fn fifty_fifty_pop(
    velocity: V,
    normal: V,
    direction: V,
    board_position: V,
    point: V,
    height: f32,
    nudge: f32,
    manager_age: f32,
) -> V {
    let across = cross(direction, normal);
    let balance = if manager_age < 0.31 {
        if nudge < 0.0 {
            nudge.min(-0.42)
        } else if nudge > 0.0 {
            nudge.max(0.42)
        } else if dot3(sub(board_position, point), across) > 0.0 {
            0.42
        } else {
            -0.42
        }
    } else {
        nudge
    };
    let lateral = scale(across, balance * 1.8);
    let length = dot3(lateral, lateral).sqrt();
    let lateral = if length > 1.8 {
        scale(lateral, 1.8 / length)
    } else {
        lateral
    };
    let planar = sub(velocity, scale(normal, dot3(velocity, normal)));
    core::array::from_fn(|i| normal[i].mul_add(height * 1.03, planar[i]) + lateral[i])
}
fn cross(a: V, b: V) -> V {
    [
        (-a[2]).mul_add(b[1], a[1] * b[2]),
        (-a[0]).mul_add(b[2], a[2] * b[0]),
        (-a[1]).mul_add(b[0], a[0] * b[1]),
        0.0,
    ]
}
