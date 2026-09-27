//! Carrying and placing dynamic props while offboard (Phases 3-4 glue).
//!
//! Buttons (edge detection in `frame.rs`): **A** toggles grab/drop; **B**
//! toggles placement mode while carrying. Stock offboard graphs give neither
//! button an action of its own (X jumps, B only sprints while held, Y mounts,
//! RB grabs the world), so taps are free. Toggle rather than hold: carrying
//! is locomotion-compatible and placing is a deliberate edit.
//!
//! Placement mode (Phase 4): the prop keeps following a target pose relative
//! to the carrier — frozen mid-air by the per-tick velocity overwrite — while
//! the right stick adjusts distance (Y) and yaw (X) and DPad up/down adjusts
//! height. A confirms: the prop is dropped at the ghost pose and the layout
//! sidecar is rewritten. B cancels back to plain carry. Yaw applies a direct
//! orientation snap; position still moves by velocity so contacts and the
//! rebaked triangle layer keep working.
//!
//! Layout persistence (`prop_layout.rs`): confirming a placement records
//! `id → (origin, basis)` and saves `settings/prop-layouts/<map>.json` next
//! to the asset root (same convention as `settings/gameplay.json`). Loading
//! a map teleports saved bodies to their stored poses before the first sync.
//!
//! Restrictions: grabbing requires `BipedGround`; a single prop at a time;
//! auto-drop beyond `MAX_HOLD_DISTANCE` or when leaving `BipedGround` (never
//! saves). Drop keeps the current velocity, so releasing while moving throws
//! gently; re-sleep is the natural cool-down.
//!
//! Multiplayer: prop bodies and layouts are host-local; skate-net would need
//! to replicate held id, body poses/velocities and layout writes. Not
//! implemented.
use skate_core::{math::Vector3, player::state::PhysicalStateId};
use bevy::prelude::warn;
use std::collections::BTreeMap;

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
/// Placement adjust rates and clamps.
const PLACE_YAW_RATE: f32 = 2.5;
const PLACE_DISTANCE_RATE: f32 = 2.0;
const PLACE_HEIGHT_RATE: f32 = 1.5;
const PLACE_DISTANCE: std::ops::Range<f32> = 0.3..4.0;
const PLACE_HEIGHT: std::ops::Range<f32> = 0.0..2.5;

/// One tick of carry/placement input, sampled in `frame.rs`.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Tick {
    /// A rising edge: grab, drop, or confirm placement.
    pub grab: bool,
    /// B rising edge: enter placement or cancel back to carry.
    pub placement: bool,
    /// Right stick X/Y and DPad up-down, already scaled to [-1, 1].
    pub yaw_axis: f32,
    pub distance_axis: f32,
    pub height_axis: f32,
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

impl Carrier {
    /// Rotation about +Y mapping (0,0,1) onto `forward`.
    fn facing_yaw(&self) -> f32 {
        f32::atan2(self.forward.x, self.forward.z)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Carry,
    /// Ghost pose offsets relative to the carrier's root and facing.
    Placement { distance: f32, height: f32, yaw: f32 },
}

#[derive(Clone, Debug, Default)]
pub(crate) struct PropCarry {
    held: Option<u32>,
    mode: Mode,
    /// Confirmed placements this session, seeded from the layout sidecar so
    /// re-saving never drops props placed in earlier sessions.
    layout: BTreeMap<u32, super::prop_layout::PropPose>,
    layout_path: Option<std::path::PathBuf>,
}

impl Default for Mode {
    fn default() -> Self {
        Self::Carry
    }
}

impl PropCarry {
    pub fn held(&self) -> Option<u32> {
        self.held
    }

    pub fn placing(&self) -> bool {
        matches!(self.mode, Mode::Placement { .. })
    }

    /// Session state after loading a map: saved poses plus the sidecar path
    /// (None keeps placement working without persistence).
    pub(crate) fn with_layout(
        layout: BTreeMap<u32, super::prop_layout::PropPose>,
        layout_path: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            held: None,
            mode: Mode::Carry,
            layout,
            layout_path,
        }
    }

    /// Saved pose overrides, for load-time application.
    pub(crate) fn layout(&self) -> &BTreeMap<u32, super::prop_layout::PropPose> {
        &self.layout
    }

    /// The prop currently grabbable by this carrier, if any. Shared by the
    /// grab path and the HUD indicator.
    pub(crate) fn candidate(&self, dynamics: &PropDynamics, carrier: Carrier) -> Option<u32> {
        if self.held.is_some() || carrier.state != PhysicalStateId::BipedGround {
            return None;
        }
        let (id, position) = dynamics.nearest_body(carrier.position, GRAB_RADIUS)?;
        let offset = sub(position, carrier.position);
        let flat = Vector3::new(offset.x, 0.0, offset.z);
        let distance = dot(flat, flat).sqrt();
        let facing = if distance > 1e-3 {
            dot(scale(flat, 1.0 / distance), carrier.forward)
        } else {
            1.0
        };
        (distance <= 0.5 || facing >= FRONT_DOT).then_some(id)
    }

    pub(crate) fn update(&mut self, dynamics: &mut PropDynamics, tick: Tick, carrier: Carrier) {
        let on_foot = carrier.state == PhysicalStateId::BipedGround;
        let Some(id) = self.held else {
            if tick.grab {
                if let Some(id) = self.candidate(dynamics, carrier) {
                    self.held = Some(id);
                    self.follow(dynamics, id, carrier);
                }
            }
            return;
        };
        let position = dynamics.position_of(id);
        let too_far = position.is_some_and(|p| {
            let d = sub(p, carrier.position);
            dot(d, d) > MAX_HOLD_DISTANCE * MAX_HOLD_DISTANCE
        });
        if !on_foot || position.is_none() || too_far {
            // Auto-drop: never a confirmed placement, so nothing is saved.
            self.held = None;
            self.mode = Mode::Carry;
            return;
        }
        match self.mode {
            Mode::Carry => {
                if tick.grab {
                    // Velocity is kept: releasing while moving throws gently.
                    self.held = None;
                    return;
                }
                if tick.placement {
                    self.mode = Mode::Placement {
                        distance: CARRY_FORWARD,
                        height: CARRY_UP,
                        yaw: 0.0,
                    };
                    return;
                }
                self.follow(dynamics, id, carrier);
            }
            Mode::Placement {
                mut distance,
                mut height,
                mut yaw,
            } => {
                if tick.grab {
                    self.confirm(dynamics, id);
                    self.held = None;
                    self.mode = Mode::Carry;
                    return;
                }
                if tick.placement {
                    self.mode = Mode::Carry;
                    return;
                }
                let dt = carrier.time_step;
                yaw += tick.yaw_axis * PLACE_YAW_RATE * dt;
                distance = (distance + tick.distance_axis * PLACE_DISTANCE_RATE * dt)
                    .clamp(PLACE_DISTANCE.start, PLACE_DISTANCE.end);
                height = (height + tick.height_axis * PLACE_HEIGHT_RATE * dt)
                    .clamp(PLACE_HEIGHT.start, PLACE_HEIGHT.end);
                self.mode = Mode::Placement {
                    distance,
                    height,
                    yaw,
                };
                let (target, basis) = ghost_pose(carrier, distance, height, yaw);
                dynamics.carry_to_pose(id, target, basis, MAX_CARRY_SPEED, dt);
            }
        }
    }

    /// Plain carry follow: ahead of and above the root, orientation frozen.
    fn follow(&self, dynamics: &mut PropDynamics, id: u32, carrier: Carrier) {
        let target = add(
            add(carrier.position, scale(carrier.forward, CARRY_FORWARD)),
            Vector3::new(0.0, CARRY_UP, 0.0),
        );
        dynamics.carry_to(id, target, MAX_CARRY_SPEED, carrier.time_step);
    }

    /// Record the confirmed pose and rewrite the layout sidecar.
    fn confirm(&mut self, dynamics: &mut PropDynamics, id: u32) {
        dynamics.release_still(id);
        let Some((origin, basis)) = dynamics.pose(id) else {
            return;
        };
        self.layout.insert(
            id,
            super::prop_layout::PropPose {
                id,
                origin: [origin.x, origin.y, origin.z],
                basis: basis.columns,
            },
        );
        if let Some(path) = &self.layout_path {
            if let Err(error) = super::prop_layout::save(path, &self.layout) {
                warn!("SKATE_PROP_LAYOUT: {}: {error}", path.display());
            }
        }
    }
}

/// Ghost pose: `distance` ahead of the carrier along facing+yaw, `height`
/// above the root, rotated `yaw` from the carrier's facing.
fn ghost_pose(
    carrier: Carrier,
    distance: f32,
    height: f32,
    yaw: f32,
) -> (Vector3, skate_core::math::Basis3) {
    let angle = carrier.facing_yaw() + yaw;
    let (sin, cos) = angle.sin_cos();
    let target = add(
        carrier.position,
        Vector3::new(sin * distance, height, cos * distance),
    );
    let basis = skate_core::math::Basis3 {
        columns: [[cos, 0.0, -sin], [0.0, 1.0, 0.0], [sin, 0.0, cos]],
    };
    (target, basis)
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
