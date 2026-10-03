//! Non-solid water (retail surface type 12). The world skips water triangles
//! for contacts (`BoardWorld::query_primitives`), so water is found by position
//! instead: a body is in water when it lies under a water surface. That feeds
//! the same signals contacts used to: the board's collision bit 25 and
//! surface-12 height, and the skater's material-12 contact, which drive the
//! water bail, the wipeout special-surface path and its buoyancy.
//! Project choice; see docs/hails-additions/09-water.md.
use super::{GamePhysics, SkaterRuntime};
use skate_core::{math::Vector3, physics::board_world::BoardWorld, riding::grounded::drag::DRAG_FREQUENCY};

/// A body counts as in water from just above the surface (a part touching it)
/// down to this depth. Deeper is a different, lower volume of space.
const ABOVE_SURFACE: f32 = 0.05;
const MAX_DEPTH: f32 = 4.0;
/// Floating (body buoyancy, board buoyancy and drag) only over deep water:
/// shallow water is solid (BoardWorld::query_primitives) and a part resting on
/// it must not be pushed up every step (that jittered in University's channel).
const FLOAT_DEPTH: f32 = skate_core::physics::board_world::FLOAT_DEPTH;
/// Board in water (retail footage: the board floats at the surface). Drag is
/// a fraction of velocity per step at 60 Hz. Buoyancy cancels gravity with
/// the part centre at the surface and rises linearly to `BOARD_BUOYANCY_MAX`
/// times gravity `1 / BOARD_BUOYANCY_STIFFNESS` metres below it, reaching zero
/// that far above it. Project-chosen values.
const BOARD_LINEAR_DRAG: f32 = 0.1;
const BOARD_ANGULAR_DRAG: f32 = 0.1;
const BOARD_BUOYANCY_STIFFNESS: f32 = 20.0;
const BOARD_BUOYANCY_MAX: f32 = 3.0;

fn surface(world: &BoardWorld, p: Vector3) -> Option<f32> {
    world.water_surface_at(p, ABOVE_SURFACE, MAX_DEPTH)
}

/// Deep water at a point, for floating.
fn deep_surface(world: &BoardWorld, p: Vector3) -> Option<f32> {
    world.deep_water_surface_at(p, ABOVE_SURFACE, MAX_DEPTH, FLOAT_DEPTH)
}

/// Whether a skeleton part is in or over deep water (for the wipeout buoyancy).
pub(super) fn part_floats(world: &BoardWorld, p: Vector3) -> bool {
    world.deep_water_surface_at(p, f32::INFINITY, MAX_DEPTH, FLOAT_DEPTH).is_some()
}

/// The highest water surface over any of `points`.
fn highest(world: &BoardWorld, points: impl Iterator<Item = Vector3>) -> Option<f32> {
    points.filter_map(|p| surface(world, p)).reduce(f32::max)
}

/// After the board's ground state is rebuilt from contacts: water no longer
/// produces contacts, so set the surface-12 classification from position.
pub(super) fn mark_board(physics: &mut GamePhysics) {
    let height = highest(
        &physics.world,
        physics.board.bodies().iter().map(|b| b.rates.position),
    );
    if let Some(height) = height {
        physics.riding.ground.collision_flags |= 1 << 25;
        physics.riding.ground.surface_twelve_height = height;
    }
}

/// After the skater's contact feedback for the frame.
pub(super) fn mark_skater(physics: &GamePhysics, skater: &mut SkaterRuntime) {
    let height = highest(
        &physics.world,
        skater.skeleton.bodies().iter().map(|b| b.rates.position),
    );
    if let Some(height) = height {
        skater.collision_feedback.flags.material_12 = true;
        skater.collision_feedback.material_12_height = height;
    }
}

/// Before the solve, after every state has set its drag: board parts in water
/// get water drag and buoyancy, so the board floats at the surface.
pub(super) fn apply_board_drag(physics: &mut GamePhysics) {
    let gravity = physics.settings.step.simulation.gravity_acceleration;
    let surfaces: Vec<Option<f32>> = physics
        .board
        .bodies()
        .iter()
        .map(|b| deep_surface(&physics.world, b.rates.position))
        .collect();
    for (body, surface) in physics.board.bodies_mut().iter_mut().zip(surfaces) {
        let Some(surface) = surface else { continue };
        body.inertia.linear_drag = body.inertia.linear_drag.max(BOARD_LINEAR_DRAG * DRAG_FREQUENCY);
        body.inertia.angular_drag = body.inertia.angular_drag.max(BOARD_ANGULAR_DRAG * DRAG_FREQUENCY);
        let depth = surface - body.rates.position.y;
        let lift = (1.0 + depth * BOARD_BUOYANCY_STIFFNESS).clamp(0.0, BOARD_BUOYANCY_MAX);
        // Added to this step's acceleration; integration resets it to gravity.
        body.rates.force_acceleration = Vector3::new(
            body.rates.force_acceleration.x - gravity.x * lift,
            body.rates.force_acceleration.y - gravity.y * lift,
            body.rates.force_acceleration.z - gravity.z * lift,
        );
    }
}
