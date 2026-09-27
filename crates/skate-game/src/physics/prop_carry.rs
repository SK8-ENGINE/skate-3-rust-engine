//! Carrying dynamic props while offboard (Phase 3 gameplay glue).
//!
//! One button press (A; see `frame.rs` for the edge detection) picks up the
//! nearest prop in front of the on-foot skater, a second press drops it.
//! Toggle rather than hold: carrying is locomotion-compatible and a drop
//! command reads better than release-on-release when the point of the
//! feature is moving props deliberately. Stock offboard graphs give A no
//! action of its own (X jumps, B sprints, Y mounts, RB grabs the world), so
//! the tap only briefly suppresses sprint.
//!
//! While held, the prop kinematic-follows a carry point in front of the
//! carrier by setting its velocity each tick — never teleporting — so the
//! narrowphase and rebaked triangle layer keep working. Gravity is not
//! suspended explicitly: overwriting the velocity every tick leaves only a
//! `g·dt²` sag (~3 mm), corrected by the next tick's follow. Rotation is
//! frozen by zeroing angular velocity. Drop keeps the current velocity, so
//! releasing while moving throws gently; re-sleep is the natural cool-down.
//!
//! Restrictions: grabbing requires `BipedGround` (not on the board, airborne,
//! or wiping out); a single prop at a time; the prop is dropped automatically
//! if it ends up farther than `MAX_HOLD_DISTANCE` from the carrier (stuck
//! against geometry) or the state leaves `BipedGround`.
//!
//! Multiplayer: prop bodies are host-local; skate-net would need to replicate
//! the held id plus each body's pose/velocity (or the full dynamic state) to
//! proxies, and arbitrate grabs. Not implemented.
use skate_core::{math::Vector3, player::state::PhysicalStateId};

use super::prop_dynamics::PropDynamics;

/// Pickup reach from the carrier's root.
const GRAB_RADIUS: f32 = 1.8;
/// Minimum forward alignment for a pickup, unless the prop is very close.
const FRONT_DOT: f32 = 0.25;
/// Carry point: this far ahead of the root and this high above it.
const CARRY_FORWARD: f32 = 0.7;
const CARRY_UP: f32 = 0.9;
/// Follow velocity cap; above this the prop lags instead of snapping.
const MAX_CARRY_SPEED: f32 = 6.0;
/// Auto-drop distance: the prop is stuck or was left behind.
const MAX_HOLD_DISTANCE: f32 = 3.0;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PropCarry {
    held: Option<u32>,
}

/// Per-tick carrier observation from the skater runtime.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Carrier {
    pub state: PhysicalStateId,
    pub position: Vector3,
    /// Horizontal facing direction, normalized.
    pub forward: Vector3,
    pub time_step: f32,
}

impl PropCarry {
    pub fn held(&self) -> Option<u32> {
        self.held
    }

    pub(crate) fn update(
        &mut self,
        dynamics: &mut PropDynamics,
        grab_rising: bool,
        carrier: Carrier,
    ) {
        let on_foot = carrier.state == PhysicalStateId::BipedGround;
        if let Some(id) = self.held {
            let position = dynamics.position_of(id);
            let too_far = position.is_some_and(|p| {
                let d = sub(p, carrier.position);
                dot(d, d) > MAX_HOLD_DISTANCE * MAX_HOLD_DISTANCE
            });
            if grab_rising || !on_foot || position.is_none() || too_far {
                // Velocity is kept: releasing while moving throws gently.
                self.held = None;
                return;
            }
            let target = add(
                add(carrier.position, scale(carrier.forward, CARRY_FORWARD)),
                Vector3::new(0.0, CARRY_UP, 0.0),
            );
            dynamics.carry_to(id, target, MAX_CARRY_SPEED, carrier.time_step);
            return;
        }
        if !grab_rising || !on_foot {
            return;
        }
        let Some((id, position)) = dynamics.nearest_body(carrier.position, GRAB_RADIUS) else {
            return;
        };
        let offset = sub(position, carrier.position);
        let flat = Vector3::new(offset.x, 0.0, offset.z);
        let distance = dot(flat, flat).sqrt();
        let facing = if distance > 1e-3 {
            dot(scale(flat, 1.0 / distance), carrier.forward)
        } else {
            1.0
        };
        if distance > 0.5 && facing < FRONT_DOT {
            return;
        }
        self.held = Some(id);
        let target = add(
            add(carrier.position, scale(carrier.forward, CARRY_FORWARD)),
            Vector3::new(0.0, CARRY_UP, 0.0),
        );
        dynamics.carry_to(id, target, MAX_CARRY_SPEED, carrier.time_step);
    }
}

fn add(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

fn sub(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

fn scale(v: Vector3, s: f32) -> Vector3 {
    Vector3::new(v.x * s, v.y * s, v.z * s)
}

fn dot(a: Vector3, b: Vector3) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z
}
