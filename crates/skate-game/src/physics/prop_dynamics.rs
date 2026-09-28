//! Dynamic rigid bodies for DMO prop instances (Phase 2a).
//!
//! Every spawned prop gets a box body built on the TU3 rigid-body integrator
//! (`integrate_body_rates`): gravity and cool-down/sleep come from the retail
//! simulation step; the mass properties are the retail rounded-box finalize
//! path (`primitive_mass_properties`) with the instance's scaled template AABB.
//! Density, friction, restitution and damping are the authored MOBJ per-object
//! values (`ObjectPhysics`, schema 3+); the project defaults are density
//! 100 kg/m³, friction 0.55, restitution 0.05 and damping 0.05/0.15.
//!
//! Narrowphase uses the recovered GP pair query (`primitive_pair_contacts`):
//! box vs static-world triangles, box vs box for other props, and box vs the
//! skater's board/skeleton volumes for pushes. Contact response is a compact
//! impulse pass producing `RetailReactionCorrections`; the retail compiled-row
//! contact solver is explicitly not gameplay-ready (`build_contact_jacobian`).
//!
//! Props start asleep and cost one AABB test per skater volume per tick. A
//! skater contact or a moving prop wakes them. Moved instances re-bake their
//! triangle range in the prop collision layer so skater queries stay exact.
//! The held (carried) prop is exempt from both skater pushes and the rebake:
//! while carried it is velocity-driven and its layer triangles are parked far
//! below the world so they cannot push the carrier.
use bevy::prelude::*;
use skate_core::{
    math::{Basis3, Vector3},
    physics::{
        board_world::{BoardWorld, BoardWorldVolume},
        contact::{RetailContactMaterial, combine_contact_materials},
        mass::{RETAIL_UNBOUNDED_VELOCITY, primitive_mass_properties},
        rigid_body::{
            RetailInertiaDynamics, RetailQuaternion, RetailReactionCorrections, RetailBodyRates,
            RetailSimulationStep, integrate_body_rates, world_inverse_inertia,
        },
        world_contact::{
            ContactPrimitive, PrimitiveContactManifold, PrimitivePairSettings,
            primitive_pair_contacts,
        },
    },
};

/// Contact band and Baumgarte constants for the prop impulse pass.
const PROP_CONTACT_PADDING: f32 = 0.02;
const PROP_PENETRATION_SLOP: f32 = 0.005;
const PROP_PENETRATION_CORRECTION: f32 = 0.4;
/// Restitution applies only above this closing speed; below it contacts are
/// inelastic so resting stacks settle instead of jittering.
const PROP_RESTITUTION_THRESHOLD: f32 = 1.0;
/// Effective skater mass (kg) for prop pushes.
const SKATER_PUSH_MASS: f32 = 75.0;
/// Fraction of the closing speed transferred to a prop by a skater push.
const PROP_PUSH_TRANSFER: f32 = 0.5;
/// Where the held prop's collision triangles are parked so skater queries
/// cannot see them while it is carried.
pub(crate) const HELD_PARK: Vector3 = Vector3::new(0.0, -10000.0, 0.0);

/// Props get their own simulation step: the board's simulation carries
/// cool_down = 0 (the host never sleeps it), which would freeze props after a
/// single tick, and its FreezingEnergy threshold is tuned for a ~kg-scale
/// board, while props are density-100 boxes (energy scales with mass, so
/// resting contact jitter alone keeps a prop above the board's threshold).
pub(crate) fn prop_simulation(
    base: skate_core::physics::rigid_body::RetailSimulationStep,
) -> skate_core::physics::rigid_body::RetailSimulationStep {
    skate_core::physics::rigid_body::RetailSimulationStep {
        cool_down: 30,
        minimum_energy: 0.5,
        ..base
    }
}

pub(crate) struct PropBody {
    /// Index into the collision layer's instance list (and its rebake target).
    instance: usize,
    id: u32,
    /// Box centre and half extents in template space, scale folded in.
    local_center: Vector3,
    half_extents: Vector3,
    rates: RetailBodyRates,
    inertia: RetailInertiaDynamics,
    /// Authored MOBJ contact material (friction/restitution).
    material: RetailContactMaterial,
    enable_sleep: bool,
    asleep: bool,
}

pub(crate) struct PropDynamics {
    bodies: Vec<PropBody>,
    by_id: std::collections::HashMap<u32, usize>,
    simulation: RetailSimulationStep,
    pair: PrimitivePairSettings,
    /// Prop currently carried: exempt from skater pushes and rebake.
    held: Option<u32>,
}

fn mul_basis(basis: Basis3, v: Vector3) -> Vector3 {
    Vector3::new(
        basis.columns[0][0] * v.x + basis.columns[1][0] * v.y + basis.columns[2][0] * v.z,
        basis.columns[0][1] * v.x + basis.columns[1][1] * v.y + basis.columns[2][1] * v.z,
        basis.columns[0][2] * v.x + basis.columns[1][2] * v.y + basis.columns[2][2] * v.z,
    )
}

fn cross(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

fn dot(a: Vector3, b: Vector3) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z
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

fn length(v: Vector3) -> f32 {
    dot(v, v).sqrt()
}

/// Shepperd's method; the basis is a pure rotation by construction.
fn quaternion_from_basis(basis: Basis3) -> RetailQuaternion {
    let m = |c: usize, r: usize| basis.columns[c][r];
    let trace = m(0, 0) + m(1, 1) + m(2, 2);
    let (x, y, z, w) = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        (
            (m(1, 2) - m(2, 1)) / s,
            (m(2, 0) - m(0, 2)) / s,
            (m(0, 1) - m(1, 0)) / s,
            0.25 * s,
        )
    } else if m(0, 0) > m(1, 1) && m(0, 0) > m(2, 2) {
        let s = (1.0 + m(0, 0) - m(1, 1) - m(2, 2)).sqrt() * 2.0;
        (
            0.25 * s,
            (m(1, 0) + m(0, 1)) / s,
            (m(2, 0) + m(0, 2)) / s,
            (m(1, 2) - m(2, 1)) / s,
        )
    } else if m(1, 1) > m(2, 2) {
        let s = (1.0 + m(1, 1) - m(0, 0) - m(2, 2)).sqrt() * 2.0;
        (
            (m(1, 0) + m(0, 1)) / s,
            0.25 * s,
            (m(2, 1) + m(1, 2)) / s,
            (m(2, 0) - m(0, 2)) / s,
        )
    } else {
        let s = (1.0 + m(2, 2) - m(0, 0) - m(1, 1)).sqrt() * 2.0;
        (
            (m(2, 0) + m(0, 2)) / s,
            (m(2, 1) + m(1, 2)) / s,
            0.25 * s,
            (m(0, 1) - m(1, 0)) / s,
        )
    };
    let inverse = 1.0 / (x * x + y * y + z * z + w * w).sqrt().max(1e-20);
    RetailQuaternion {
        x: x * inverse,
        y: y * inverse,
        z: z * inverse,
        w: w * inverse,
    }
}

impl PropBody {
    fn box_primitive(&self) -> ContactPrimitive {
        ContactPrimitive::RoundedBox {
            center: self.rates.position,
            basis: self.rates.basis,
            half_extents: self.half_extents,
            radius: 0.0,
        }
    }

    fn bounds(&self) -> skate_core::physics::board_world::query_metadata::Bounds {
        let half = [self.half_extents.x, self.half_extents.y, self.half_extents.z];
        let extent = |axis: usize| {
            self.rates
                .basis
                .columns
                .iter()
                .zip(half)
                .map(|(column, h)| h * column[axis].abs())
                .sum::<f32>()
        };
        let (ex, ey, ez) = (extent(0), extent(1), extent(2));
        let c = self.rates.position;
        skate_core::physics::board_world::query_metadata::Bounds {
            min: Vector3::new(c.x - ex, c.y - ey, c.z - ez),
            max: Vector3::new(c.x + ex, c.y + ey, c.z + ez),
        }
    }

    fn wake(&mut self) {
        self.asleep = false;
        self.rates.cool_down = 0;
    }

    /// Impulse applied at a world point, directly to velocities. Returns the
    /// impulse vector for the other body / wake accounting.
    fn apply_impulse(&mut self, impulse: Vector3, point: Vector3) -> Vector3 {
        self.wake();
        self.rates.linear_velocity = add(
            self.rates.linear_velocity,
            scale(impulse, self.inertia.inverse_mass),
        );
        let r = sub(point, self.rates.position);
        let angular = cross(r, impulse);
        let delta = mul_basis(self.rates.world_inverse_inertia, angular);
        self.rates.angular_velocity = add(self.rates.angular_velocity, delta);
        impulse
    }

    fn velocity_at(&self, point: Vector3) -> Vector3 {
        add(
            self.rates.linear_velocity,
            cross(self.rates.angular_velocity, sub(point, self.rates.position)),
        )
    }

    /// Template-origin pose for rendering and collision rebake.
    fn origin(&self) -> Vector3 {
        sub(
            self.rates.position,
            mul_basis(self.rates.basis, self.local_center),
        )
    }
}

impl PropDynamics {
    /// One box body per collision-layer instance, asleep at its authored pose.
    /// Placement rows are the world images of the local axes; their lengths
    /// are the constant per-axis scale folded into the box extents. Density,
    /// damping, friction, restitution and sleep flags come from the authored
    /// MOBJ physics block (`ObjectPhysics`).
    pub(crate) fn new(
        objects: &[skate_data::skate_map::StaticObject],
        instances: &[crate::skate_world::PropCollisionInstance],
        simulation: RetailSimulationStep,
    ) -> Self {
        let mut bodies = Vec::new();
        let mut by_id = std::collections::HashMap::new();
        for (index, entry) in instances.iter().enumerate() {
            let object = &objects[entry.object];
            let authored = object.physics;
            let t = &object.transform;
            let axis_scale = [
                (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt(),
                (t[3] * t[3] + t[4] * t[4] + t[5] * t[5]).sqrt(),
                (t[6] * t[6] + t[7] * t[7] + t[8] * t[8]).sqrt(),
            ];
            let basis = Basis3 {
                columns: [
                    [t[0] / axis_scale[0], t[1] / axis_scale[0], t[2] / axis_scale[0]],
                    [t[3] / axis_scale[1], t[4] / axis_scale[1], t[5] / axis_scale[1]],
                    [t[6] / axis_scale[2], t[7] / axis_scale[2], t[8] / axis_scale[2]],
                ],
            };
            let first = entry.local_points()[0][0];
            let mut min = first;
            let mut max = first;
            for point in entry.local_points().iter().flatten() {
                min = Vector3::new(min.x.min(point.x), min.y.min(point.y), min.z.min(point.z));
                max = Vector3::new(max.x.max(point.x), max.y.max(point.y), max.z.max(point.z));
            }
            let local_center = scale(add(min, max), 0.5);
            let half_extents = scale(sub(max, min), 0.5);
            let volume = 8.0 * half_extents.x * half_extents.y * half_extents.z;
            let Some(properties) = primitive_mass_properties(
                skate_core::physics::mass::PartMassInput {
                    shape: skate_core::physics::mass::MassShape::RoundedBox {
                        half_extents,
                        radius: 0.0,
                    },
                    requested_mass: volume * authored.density.max(0.001),
                },
                RETAIL_UNBOUNDED_VELOCITY,
                authored.angular_damping,
            ) else {
                continue;
            };
            let mut inertia = properties.dynamics;
            inertia.linear_drag = authored.linear_damping;
            inertia.maximum_linear_velocity = RETAIL_UNBOUNDED_VELOCITY;
            let origin = Vector3::new(t[9], t[10], t[11]);
            let center = add(origin, mul_basis(basis, local_center));
            bodies.push(PropBody {
                instance: index,
                id: entry.id,
                local_center,
                half_extents,
                rates: RetailBodyRates {
                    orientation: quaternion_from_basis(basis),
                    basis,
                    world_inverse_inertia: world_inverse_inertia(basis, inertia.inverse_tensor),
                    position: center,
                    linear_velocity: Vector3::ZERO,
                    angular_velocity: Vector3::ZERO,
                    force_acceleration: scale(
                        simulation.gravity_acceleration,
                        authored.gravity_scale,
                    ),
                    torque_acceleration: Vector3::ZERO,
                    kinetic_energy: 0.0,
                    cool_down: simulation.cool_down,
                },
                inertia,
                material: RetailContactMaterial {
                    static_friction: authored.friction,
                    dynamic_friction: authored.friction,
                    restitution: authored.restitution,
                },
                enable_sleep: authored.enable_sleep,
                asleep: !authored.initially_awake,
            });
            by_id.insert(entry.id, bodies.len() - 1);
        }
        Self {
            bodies,
            by_id,
            simulation,
            pair: PrimitivePairSettings {
                padding_a: PROP_CONTACT_PADDING,
                padding_b: PROP_CONTACT_PADDING,
                additional_padding: 0.0,
                edge_cos_bend_normal_threshold: 0.999,
                convexity_epsilon: 0.01,
            },
            held: None,
        }
    }

    /// Template-origin pose of one prop for render sync.
    pub(crate) fn pose(&self, id: u32) -> Option<(Vector3, Basis3)> {
        let body = self.bodies.get(*self.by_id.get(&id)?)?;
        Some((body.origin(), body.rates.basis))
    }

    /// World position of one body's box centre.
    pub(crate) fn position_of(&self, id: u32) -> Option<Vector3> {
        Some(self.bodies.get(*self.by_id.get(&id)?)?.rates.position)
    }

    /// Nearest body within `radius` of `point`, measured to the box SURFACE
    /// (not the centre, so big ramps are grabbable by their edge), as
    /// `(id, centre)`.
    pub(crate) fn nearest_body(&self, point: Vector3, radius: f32) -> Option<(u32, Vector3)> {
        let mut best: Option<(u32, Vector3, f32)> = None;
        for body in &self.bodies {
            // Local-space point clamped into the box: the gap vector to it is
            // the surface distance (zero when the point is inside).
            let d = sub(point, body.rates.position);
            let b = body.rates.basis.columns;
            let local = [
                d.x * b[0][0] + d.y * b[0][1] + d.z * b[0][2],
                d.x * b[1][0] + d.y * b[1][1] + d.z * b[1][2],
                d.x * b[2][0] + d.y * b[2][1] + d.z * b[2][2],
            ];
            let he = body.half_extents;
            let gap = Vector3::new(
                (local[0].abs() - he.x).max(0.0),
                (local[1].abs() - he.y).max(0.0),
                (local[2].abs() - he.z).max(0.0),
            );
            let distance_squared = dot(gap, gap);
            if distance_squared > radius * radius {
                continue;
            }
            if best.map_or(true, |(_, _, b)| distance_squared < b) {
                best = Some((body.id, body.rates.position, distance_squared));
            }
        }
        best.map(|(id, position, _)| (id, position))
    }

    /// Collision-layer instance index for one body (rebake target).
    pub(crate) fn instance_of(&self, id: u32) -> Option<usize> {
        Some(self.bodies.get(*self.by_id.get(&id)?)?.instance)
    }

    /// Mark the carried prop: it stops receiving skater pushes and its layer
    /// triangles stay parked until the drop rebakes them.
    pub(crate) fn set_held(&mut self, held: Option<u32>) {
        self.held = held;
    }

    fn is_held(&self, index: usize) -> bool {
        self.held == Some(self.bodies[index].id)
    }

    /// Skate 3 style drag: the prop stays on the ground and is pulled
    /// horizontally toward `target` (its Y is untouched, so gravity and
    /// ground contacts keep working). Rotation stays frozen. Returns false if
    /// the id is unknown.
    pub(crate) fn drag_to(
        &mut self,
        id: u32,
        target: Vector3,
        max_speed: f32,
        time_step: f32,
    ) -> bool {
        let Some(&index) = self.by_id.get(&id) else {
            return false;
        };
        let body = &mut self.bodies[index];
        body.wake();
        let delta = sub(target, body.rates.position);
        let flat = Vector3::new(delta.x, 0.0, delta.z);
        let distance = dot(flat, flat).sqrt();
        let speed = (distance / time_step).min(max_speed);
        body.rates.linear_velocity = if distance > 1e-6 {
            let pulled = scale(flat, speed / distance);
            Vector3::new(pulled.x, body.rates.linear_velocity.y, pulled.z)
        } else {
            Vector3::new(0.0, body.rates.linear_velocity.y, 0.0)
        };
        body.rates.angular_velocity = Vector3::ZERO;
        true
    }

    /// Kinematic follow while carried: wake and steer the body toward
    /// `target` by velocity (never teleport), capped at `max_speed`, with
    /// rotation frozen. Returns false if the id is unknown.
    pub(crate) fn carry_to(
        &mut self,
        id: u32,
        target: Vector3,
        max_speed: f32,
        time_step: f32,
    ) -> bool {
        let Some(&index) = self.by_id.get(&id) else {
            return false;
        };
        let body = &mut self.bodies[index];
        body.wake();
        let delta = sub(target, body.rates.position);
        let distance = dot(delta, delta).sqrt();
        let speed = (distance / time_step).min(max_speed);
        body.rates.linear_velocity = if distance > 1e-6 {
            scale(delta, speed / distance)
        } else {
            Vector3::ZERO
        };
        body.rates.angular_velocity = Vector3::ZERO;
        true
    }

    /// Placement follow: like `carry_to`, but also snaps the orientation to
    /// `basis` (ghost yaw edit). Position still moves by velocity only.
    pub(crate) fn carry_to_pose(
        &mut self,
        id: u32,
        target: Vector3,
        basis: Basis3,
        max_speed: f32,
        time_step: f32,
    ) -> bool {
        if !self.carry_to(id, target, max_speed, time_step) {
            return false;
        }
        let body = &mut self.bodies[self.by_id[&id]];
        body.rates.basis = basis;
        body.rates.orientation = quaternion_from_basis(basis);
        body.rates.world_inverse_inertia =
            world_inverse_inertia(basis, body.inertia.inverse_tensor);
        true
    }

    /// Confirming a placement sets the prop down gently: velocity zeroed so
    /// the leftover follow velocity does not throw it.
    pub(crate) fn release_still(&mut self, id: u32) {
        if let Some(&index) = self.by_id.get(&id) {
            self.bodies[index].rates.linear_velocity = Vector3::ZERO;
            self.bodies[index].rates.angular_velocity = Vector3::ZERO;
        }
    }

    /// Teleport a body to a saved layout pose, asleep. Returns the collision
    /// instance index so the caller can rebake its triangles.
    pub(crate) fn teleport(&mut self, id: u32, origin: Vector3, basis: Basis3) -> Option<usize> {
        let cool_down = self.simulation.cool_down;
        let body = self.bodies.get_mut(*self.by_id.get(&id)?)?;
        body.rates.basis = basis;
        body.rates.orientation = quaternion_from_basis(basis);
        body.rates.world_inverse_inertia =
            world_inverse_inertia(basis, body.inertia.inverse_tensor);
        body.rates.position = add(origin, mul_basis(basis, body.local_center));
        body.rates.linear_velocity = Vector3::ZERO;
        body.rates.angular_velocity = Vector3::ZERO;
        body.rates.kinetic_energy = 0.0;
        body.rates.cool_down = cool_down;
        body.asleep = true;
        Some(body.instance)
    }

    /// Advance awake bodies one tick; wake bodies the skater touches. Moved
    /// instances re-bake their triangles in the collision layer afterwards.
    pub(crate) fn step(
        &mut self,
        world: &BoardWorld,
        layer: &mut crate::skate_world::PropCollisionLayer,
        skater_volumes: &[BoardWorldVolume],
    ) {
        for index in 0..self.bodies.len() {
            let held = self.is_held(index);
            // Skater push: cheap bounds reject, then the retail pair query.
            // The carried prop is velocity-driven by the carrier; letting the
            // skater push it (or be pushed by it) fights the drag.
            if !held {
                let body_bounds = self.bodies[index].bounds();
                for volume in skater_volumes {
                    let Some(volume_bounds) = volume_bounds(volume.primitive) else {
                        continue;
                    };
                    if !body_bounds.overlaps(volume_bounds) {
                        continue;
                    }
                    let box_primitive = self.bodies[index].box_primitive();
                    let Some(manifold) =
                        primitive_pair_contacts(volume.primitive, box_primitive, self.pair)
                    else {
                        continue;
                    };
                    self.push_from_skater(index, volume, &manifold);
                }
            }
            if self.bodies[index].asleep {
                continue;
            }
            let corrections = self.contact_corrections(index, world);
            let body_sleep_capable = self.bodies[index].enable_sleep;
            // Snap to rest below the sleep threshold, but only while something
            // is actually touching the body: without the contact gate the snap
            // zeroes the first ticks of a fall (g·dt is far below the sleep
            // threshold) and the prop descends at g·dt² per tick forever.
            let resting = body_sleep_capable
                && (dot(corrections.linear_displacement, corrections.linear_displacement)
                    > 0.0
                    || dot(corrections.position_displacement, corrections.position_displacement)
                        > 0.0
                    || dot(corrections.angular_displacement, corrections.angular_displacement)
                        > 0.0);
            let body = &mut self.bodies[index];
            let step = integrate_body_rates(body.rates, body.inertia, self.simulation, corrections);
            body.rates = step.state;
            if resting && body.rates.kinetic_energy < self.simulation.minimum_energy {
                body.rates.linear_velocity = Vector3::ZERO;
                body.rates.angular_velocity = Vector3::ZERO;
                body.rates.kinetic_energy = 0.0;
                // The snap zeroes the energy the integrator compares against
                // its previous value, so its own cool-down counter stalls
                // (post-gravity energy is always greater than zero). Count
                // snapped resting ticks here instead.
                body.rates.cool_down =
                    (body.rates.cool_down + 1).min(self.simulation.cool_down);
            }
            if body_sleep_capable && body.rates.cool_down >= self.simulation.cool_down {
                body.asleep = true;
            }
            // The held prop's triangles stay parked (set_held/HELD_PARK) so
            // skater queries never see them while carrying.
            if held {
                continue;
            }
            let body = &self.bodies[index];
            if let Err(error) = layer.rebake(body.instance, body.rates.basis.columns, body.origin())
            {
                warn!("SKATE_PROP_DYNAMICS: rebake instance {}: {error}", body.instance);
            }
        }
    }

    /// Skater volumes treat the prop as a pushable weight: the prop receives a
    /// fraction of the closing speed through the reduced mass of the pair, as
    /// an impulse at the contact point. The skater's own response still comes
    /// from the exact triangle layer.
    fn push_from_skater(
        &mut self,
        index: usize,
        volume: &BoardWorldVolume,
        manifold: &PrimitiveContactManifold,
    ) {
        // The manifold normal points from the prop (B) toward the skater (A).
        let push = scale(manifold.normal, -1.0);
        let mut strongest = 0.0_f32;
        let mut point = self.bodies[index].rates.position;
        for pair in &manifold.points[..manifold.count] {
            let closing = dot(sub(volume.linear_velocity, self.bodies[index].velocity_at(pair.b)), push);
            let penetration = dot(sub(pair.a, pair.b), manifold.normal);
            let drive = closing.max(if penetration < 0.0 { 0.5 } else { 0.0 });
            if drive > strongest {
                strongest = drive;
                point = pair.b;
            }
        }
        if strongest <= 0.0 {
            return;
        }
        // Momentum-style transfer: the skater shares its closing speed through
        // the reduced mass of the pair, so a 20 kg box skips away while a
        // 500 kg ramp barely budges. Δv = strongest × transfer × M/(M+m).
        let mass = 1.0 / self.bodies[index].inertia.inverse_mass;
        let reduced = mass * SKATER_PUSH_MASS / (SKATER_PUSH_MASS + mass);
        let impulse = scale(push, strongest * reduced * PROP_PUSH_TRANSFER);
        self.bodies[index].apply_impulse(impulse, point);
    }

    /// Impulse and positional corrections for one awake body against the
    /// static world and every other prop box (asleep props are immovable).
    fn contact_corrections(&mut self, index: usize, world: &BoardWorld) -> RetailReactionCorrections {
        let mut corrections = RetailReactionCorrections::default();
        let box_primitive = self.bodies[index].box_primitive();
        let bounds = self.bodies[index].bounds().expanded(PROP_CONTACT_PADDING + 0.05);
        for range in world.candidate_ranges(Some(bounds)) {
            for triangle in &world.triangles()[range] {
                let Some(manifold) = primitive_pair_contacts(
                    box_primitive,
                    ContactPrimitive::Triangle(triangle.triangle),
                    self.pair,
                ) else {
                    continue;
                };
                let material =
                    combine_contact_materials(self.bodies[index].material, triangle.material);
                self.resolve_static(index, &manifold, material, &mut corrections);
            }
        }
        let box_primitive = self.bodies[index].box_primitive();
        for other in 0..self.bodies.len() {
            if other == index {
                continue;
            }
            if !self.bodies[index].bounds().overlaps(self.bodies[other].bounds().expanded(PROP_CONTACT_PADDING)) {
                continue;
            }
            let Some(manifold) = primitive_pair_contacts(
                box_primitive,
                self.bodies[other].box_primitive(),
                self.pair,
            ) else {
                continue;
            };
            let material = combine_contact_materials(
                self.bodies[index].material,
                self.bodies[other].material,
            );
            if self.bodies[other].asleep {
                // An asleep prop is an immovable support; a hard hit wakes it.
                let closing = manifold.points[..manifold.count]
                    .iter()
                    .map(|pair| {
                        dot(
                            sub(
                                self.bodies[index].velocity_at(pair.a),
                                self.bodies[other].velocity_at(pair.b),
                            ),
                            manifold.normal,
                        )
                    })
                    .fold(0.0_f32, |a, b| a.min(b));
                self.resolve_static(index, &manifold, material, &mut corrections);
                if closing < -1.0 {
                    self.bodies[other].wake();
                }
            } else {
                self.resolve_dynamic(index, other, &manifold, material, &mut corrections);
            }
        }
        corrections
    }

    /// Resolve contacts against an immovable surface (static world or asleep
    /// prop). The manifold normal points from the surface toward this prop.
    fn resolve_static(
        &mut self,
        index: usize,
        manifold: &PrimitiveContactManifold,
        material: RetailContactMaterial,
        corrections: &mut RetailReactionCorrections,
    ) {
        let dt = self.simulation.time_step;
        let count = manifold.count.max(1) as f32;
        for pair in &manifold.points[..manifold.count] {
            let normal = manifold.normal;
            let gap = dot(sub(pair.a, pair.b), normal);
            if gap > PROP_CONTACT_PADDING {
                continue;
            }
            let body = &self.bodies[index];
            let r = sub(pair.a, body.rates.position);
            let velocity = body.velocity_at(pair.a);
            let vn = dot(velocity, normal);
            let inverse_mass = body.inertia.inverse_mass;
            let angular = mul_basis(body.rates.world_inverse_inertia, cross(r, normal));
            let denominator = inverse_mass + dot(normal, cross(angular, r));
            if denominator <= 1e-9 {
                continue;
            }
            if vn < 0.0 {
                let restitution = if vn < -PROP_RESTITUTION_THRESHOLD {
                    material.restitution.max(0.0)
                } else {
                    0.0
                };
                // Each manifold point applies its share of the impulse: with
                // N simultaneous points at the same closing speed (a face
                // landing flat), the unshared impulses would sum to N× the
                // needed correction and bounce the body off the surface.
                let impulse = -(1.0 + restitution) * vn / (denominator * count);
                let mut delta = scale(normal, impulse * inverse_mass);
                let mut spin = mul_basis(
                    body.rates.world_inverse_inertia,
                    cross(r, scale(normal, impulse)),
                );
                // Retail combine friction caps the tangential impulse.
                let tangent = sub(velocity, scale(normal, vn));
                let speed = length(tangent);
                if speed > 1e-6 {
                    let limit = material.dynamic_friction.max(0.0) * impulse;
                    let friction = (-speed / denominator).clamp(-limit, limit);
                    let direction = scale(tangent, 1.0 / speed);
                    delta = add(delta, scale(direction, friction * inverse_mass));
                    spin = add(
                        spin,
                        mul_basis(
                            body.rates.world_inverse_inertia,
                            cross(r, scale(direction, friction)),
                        ),
                    );
                }
                corrections.linear_displacement =
                    add(corrections.linear_displacement, scale(delta, dt));
                corrections.angular_displacement =
                    add(corrections.angular_displacement, scale(spin, dt));
            }
            let penetration = (-gap - PROP_PENETRATION_SLOP).max(0.0) * PROP_PENETRATION_CORRECTION;
            corrections.position_displacement = add(
                corrections.position_displacement,
                scale(normal, penetration / count),
            );
        }
    }

    /// Two awake props split the impulse by their inverse masses. Positional
    /// correction is applied to this body only; the other accumulates its own
    /// when its turn comes (the pair is visited twice per tick).
    fn resolve_dynamic(
        &mut self,
        index: usize,
        other: usize,
        manifold: &PrimitiveContactManifold,
        material: RetailContactMaterial,
        corrections: &mut RetailReactionCorrections,
    ) {
        let dt = self.simulation.time_step;
        let normal = manifold.normal;
        let count = manifold.count.max(1) as f32;
        for pair in &manifold.points[..manifold.count] {
            let gap = dot(sub(pair.a, pair.b), normal);
            if gap > PROP_CONTACT_PADDING {
                continue;
            }
            let (body, other_body) = if index < other {
                let (a, b) = self.bodies.split_at_mut(other);
                (&a[index], &b[0])
            } else {
                let (a, b) = self.bodies.split_at_mut(index);
                (&b[0], &a[other])
            };
            let inverse_mass = body.inertia.inverse_mass + other_body.inertia.inverse_mass;
            let velocity = sub(body.velocity_at(pair.a), other_body.velocity_at(pair.b));
            let vn = dot(velocity, normal);
            if vn >= 0.0 {
                continue;
            }
            let restitution = if vn < -PROP_RESTITUTION_THRESHOLD {
                material.restitution.max(0.0)
            } else {
                0.0
            };
            // Same per-point sharing as the static contact above.
            let impulse = -(1.0 + restitution) * vn / (inverse_mass * count);
            let share = impulse * body.inertia.inverse_mass;
            corrections.linear_displacement = add(
                corrections.linear_displacement,
                scale(normal, share * dt),
            );
            let penetration = (-gap - PROP_PENETRATION_SLOP).max(0.0) * PROP_PENETRATION_CORRECTION;
            corrections.position_displacement = add(
                corrections.position_displacement,
                scale(normal, penetration * 0.5),
            );
        }
    }
}

fn volume_bounds(
    primitive: ContactPrimitive,
) -> Option<skate_core::physics::board_world::query_metadata::Bounds> {
    let expanded = |center: Vector3, radius: f32| {
        let r = radius.abs();
        skate_core::physics::board_world::query_metadata::Bounds::from_points([
            Vector3::new(center.x - r, center.y - r, center.z - r),
            Vector3::new(center.x + r, center.y + r, center.z + r),
        ])
    };
    match primitive {
        ContactPrimitive::Sphere(sphere) => expanded(sphere.center, sphere.radius),
        ContactPrimitive::Capsule {
            center,
            axis,
            half_length,
            radius,
        } => {
            let offset = scale(axis, half_length);
            let a = sub(center, offset);
            let b = add(center, offset);
            let lo = Vector3::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z));
            let hi = Vector3::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z));
            let r = radius.abs();
            skate_core::physics::board_world::query_metadata::Bounds::from_points([
                Vector3::new(lo.x - r, lo.y - r, lo.z - r),
                Vector3::new(hi.x + r, hi.y + r, hi.z + r),
            ])
        }
        _ => None,
    }
}

/// Publish dynamic prop poses to the spawned Bevy entities. The component
/// transform holds the template-origin placement; scale stays as spawned.
pub(crate) fn sync_prop_transforms(
    physics: Res<super::GamePhysics>,
    mut props: Query<(&crate::skate_world::PropInstance, &mut Transform)>,
) {
    let Some(dynamics) = physics.prop_dynamics() else {
        return;
    };
    for (prop, mut transform) in &mut props {
        let Some((origin, basis)) = dynamics.pose(prop.id) else {
            continue;
        };
        let translation = Vec3::new(origin.x, origin.y, origin.z);
        let rotation = Quat::from_mat3(&Mat3::from_cols(
            Vec3::from_array(basis.columns[0]),
            Vec3::from_array(basis.columns[1]),
            Vec3::from_array(basis.columns[2]),
        ));
        if (transform.translation - translation).length_squared() > 1e-12
            || (transform.rotation - rotation).length_squared() > 1e-12
        {
            transform.translation = translation;
            transform.rotation = rotation;
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::skate_world::build_prop_layer;
    use skate_core::physics::{
        board::BodyId,
        board_step::CollisionBody,
        collision::Sphere,
    };

    fn material() -> RetailContactMaterial {
        RetailContactMaterial {
            static_friction: 0.8,
            dynamic_friction: 0.6,
            restitution: 0.0,
        }
    }

    fn simulation() -> RetailSimulationStep {
        super::prop_simulation(RetailSimulationStep::fixed_60_hz(
            0,
            0.01,
            Vector3::new(0., -9.81, 0.),
        ))
    }

    /// Unit-cube template (±0.5) with a single instance placed at `origin`.
    fn fixture(
        origin: [f32; 3],
    ) -> (
        BoardWorld,
        crate::skate_world::PropCollisionLayer,
        PropDynamics,
    ) {
        let corners = [
            [-0.5, -0.5, -0.5], [0.5, -0.5, -0.5], [0.5, -0.5, 0.5], [-0.5, -0.5, 0.5],
            [-0.5, 0.5, -0.5], [0.5, 0.5, -0.5], [0.5, 0.5, 0.5], [-0.5, 0.5, 0.5],
        ];
        let faces = [
            [4, 7, 6], [4, 6, 5], // +Y top
            [0, 1, 2], [0, 2, 3], // -Y bottom
            [1, 5, 6], [1, 6, 2], // +X
            [0, 7, 4], [0, 3, 7], // -X
            [3, 2, 6], [3, 6, 7], // +Z
            [0, 5, 1], [0, 4, 5], // -Z
        ];
        let vertex = |position| skate_data::skate_map::Vertex {
            position,
            normal: [0., 1., 0.],
            uv: [0.; 2],
            lightmap_uv: [0.; 2],
            material: 1,
            decal_uv: None,
            tangent_frame: None,
        };
        let map = skate_data::skate_map::SkateMap {
            version: 14,
            name: "props".into(),
            spawn: [0.; 3],
            heading: 0.,
            environment: vec![0.; 45],
            materials: vec![skate_data::skate_map::Material {
                name: "prop".into(),
                flags: 0,
                friction: 0.5,
                restitution: 0.1,
                color: [1.; 3],
                roughness: 0.5,
                emissive: 0.,
                textures: [0; 5],
                indirect_strength: 0.,
                alpha_mode: 0,
                alpha_cutoff: 0.5,
                audio: 3,
                physics: 1,
                pattern: 0,
                depth_layer: None,
                retail_definition: None,
            }],
            textures: vec![],
            geometry: skate_data::skate_map::Geometry {
                vertices: corners.into_iter().map(vertex).collect(),
                indices: faces.into_iter().flatten().collect(),
                collision: vec![],
            },
            rails: vec![],
            doors: vec![],
            lights: vec![],
            routes: vec![],
            extensions: vec![],
        };
        let objects = vec![skate_data::skate_map::StaticObject {
            id: 7,
            name: "template/crate".into(),
            transform: [
                1., 0., 0., 0., 1., 0., 0., 0., 1., origin[0], origin[1], origin[2],
            ],
            first_index: 0,
            index_count: 36,
            first_collision: 0,
            collision_count: 0,
            rails: vec![],
            physics: Default::default(),
        }];
        let layer = build_prop_layer(&map, &objects, material()).unwrap().unwrap();
        let dynamics = PropDynamics::new(&objects, layer.instances(), simulation());
        let world = super::super::ground::Terrain::Flat.world(material());
        (world, layer, dynamics)
    }

    const REST_Y: f32 = super::super::ground::HEIGHT + 0.5;

    /// A prop dropped from the air falls, settles on the floor and sleeps; its
    /// collision triangles are re-baked at the new pose.
    #[test]
    fn dropped_prop_falls_settles_and_sleeps() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y + 5., 0.]);
        dynamics.bodies[0].wake();
        for _ in 0..240 {
            dynamics.step(&world, &mut layer, &[]);
        }
        let body = &dynamics.bodies[0];
        assert!(body.asleep, "prop should cool down to sleep: {:#?}", body.rates);
        assert!(
            (body.rates.position.y - REST_Y).abs() < 0.1,
            "resting height {}",
            body.rates.position.y
        );
        // Re-baked triangles: a probe from the drop height hits the top face.
        let hit = layer
            .world()
            .query_thin_line(Vector3::new(0., REST_Y + 5., 0.), Vector3::new(0., REST_Y - 1., 0.))
            .unwrap()
            .unwrap();
        assert!((hit.geometry.position.y - (REST_Y + 0.5)).abs() < 0.1);
    }

    /// A moving skater sphere wakes a resting prop and pushes it sideways.
    #[test]
    fn skater_volume_pushes_resting_prop() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 0.]);
        let start_x = dynamics.bodies[0].rates.position.x;
        assert!(dynamics.bodies[0].asleep);
        let volumes = [BoardWorldVolume {
            body: CollisionBody::Board(BodyId::Deck),
            primitive: ContactPrimitive::Sphere(Sphere {
                center: Vector3::new(-0.55, REST_Y, 0.),
                radius: 0.2,
            }),
            linear_velocity: Vector3::new(2., 0., 0.),
            material: material(),
        }];
        for _ in 0..30 {
            dynamics.step(&world, &mut layer, &volumes);
        }
        let body = &dynamics.bodies[0];
        assert!(
            body.rates.position.x > start_x + 0.02,
            "pushed from {start_x} to {}",
            body.rates.position.x
        );
    }

    /// A settled prop stays put and finite over long idle ticks.
    #[test]
    fn settled_prop_does_not_sink_or_diverge() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y + 5., 0.]);
        dynamics.bodies[0].wake();
        for _ in 0..240 {
            dynamics.step(&world, &mut layer, &[]);
        }
        let settled = dynamics.bodies[0].rates.position;
        for _ in 0..120 {
            dynamics.step(&world, &mut layer, &[]);
        }
        let body = &dynamics.bodies[0];
        let p = body.rates.position;
        assert!(p.x.is_finite() && p.y.is_finite() && p.z.is_finite());
        assert!(p.y > REST_Y - 0.1, "sank to {}", p.y);
        assert!((p.y - settled.y).abs() < 0.02, "drifted {} -> {}", settled.y, p.y);
    }

    // Phase 3: offboard carry glue (`crate::physics::prop_carry`).

    fn carrier(state: skate_core::player::state::PhysicalStateId, z: f32) -> crate::physics::prop_carry::Carrier {
        crate::physics::prop_carry::Carrier {
            state,
            position: Vector3::new(0., super::super::ground::HEIGHT + 0.9, z),
            forward: Vector3::new(0., 0., 1.),
            time_step: simulation().time_step,
        }
    }

    fn tick() -> crate::physics::prop_carry::Tick {
        crate::physics::prop_carry::Tick::default()
    }

    /// Grabbing the prop ahead picks it up; it follows as the carrier moves.
    #[test]
    fn grabbed_prop_follows_carrier() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 1.2]);
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        let state = skate_core::player::state::PhysicalStateId::BipedGround;
        carry.update(&mut dynamics, crate::physics::prop_carry::Tick { grab: true, ..tick() }, carrier(state, 0.));
        assert_eq!(carry.held(), Some(7));
        for i in 0..60 {
            carry.update(&mut dynamics, tick(), carrier(state, 0.05 * i as f32));
            dynamics.step(&world, &mut layer, &[]);
        }
        let p = dynamics.position_of(7).unwrap();
        assert!(p.z > 2.0, "prop followed to z={}", p.z);
        assert!(p.y > super::super::ground::HEIGHT, "carried prop underground: {p:?}");
    }

    /// Dropping releases the prop; it falls, keeps no NaN, and sleeps again.
    #[test]
    fn dropped_carry_falls_and_sleeps() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 1.2]);
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        let state = skate_core::player::state::PhysicalStateId::BipedGround;
        carry.update(&mut dynamics, crate::physics::prop_carry::Tick { grab: true, ..tick() }, carrier(state, 0.));
        for _ in 0..30 {
            carry.update(&mut dynamics, tick(), carrier(state, 0.));
            dynamics.step(&world, &mut layer, &[]);
        }
        carry.update(&mut dynamics, crate::physics::prop_carry::Tick { grab: true, ..tick() }, carrier(state, 0.));
        assert_eq!(carry.held(), None);
        for _ in 0..300 {
            dynamics.step(&world, &mut layer, &[]);
        }
        let p = dynamics.position_of(7).unwrap();
        assert!((p.y - REST_Y).abs() < 0.1, "resting height {}", p.y);
        assert!(dynamics.bodies[0].asleep, "dropped prop never slept");
    }

    /// The grab is ignored unless the skater is on foot.
    #[test]
    fn grab_requires_biped_ground() {
        let (_world, _layer, mut dynamics) = fixture([0., REST_Y, 1.2]);
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        for state in [
            skate_core::player::state::PhysicalStateId::PhysicsGround,
            skate_core::player::state::PhysicalStateId::BipedAir,
            skate_core::player::state::PhysicalStateId::WipeoutGround,
        ] {
            carry.update(&mut dynamics, crate::physics::prop_carry::Tick { grab: true, ..tick() }, carrier(state, 0.));
            assert_eq!(carry.held(), None, "{state:?} must not grab");
        }
        // Grabbing, then mounting the board, drops the prop automatically.
        carry.update(
            &mut dynamics,
            crate::physics::prop_carry::Tick { grab: true, ..tick() },
            carrier(skate_core::player::state::PhysicalStateId::BipedGround, 0.),
        );
        assert_eq!(carry.held(), Some(7));
        carry.update(
            &mut dynamics,
            tick(),
            carrier(skate_core::player::state::PhysicalStateId::PhysicsGround, 0.),
        );
        assert_eq!(carry.held(), None);
    }

    // Phase 4: placement mode and layout persistence.

    /// Placement adjusts the ghost pose; confirming drops the prop there,
    /// records the layout pose, and the prop falls and sleeps in place.
    #[test]
    fn placement_adjust_confirm_and_sleep() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 1.2]);
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        let state = skate_core::player::state::PhysicalStateId::BipedGround;
        let grab = crate::physics::prop_carry::Tick { grab: true, ..tick() };
        carry.update(&mut dynamics, grab, carrier(state, 0.));
        assert_eq!(carry.held(), Some(7));
        // Enter placement; push the ghost far (distance axis) and yaw it.
        carry.update(
            &mut dynamics,
            crate::physics::prop_carry::Tick { placement: true, ..tick() },
            carrier(state, 0.),
        );
        assert!(carry.placing());
        for _ in 0..60 {
            carry.update(
                &mut dynamics,
                crate::physics::prop_carry::Tick {
                    distance_axis: 1.0,
                    yaw_axis: 0.25,
                    ..tick()
                },
                carrier(state, 0.),
            );
            dynamics.step(&world, &mut layer, &[]);
        }
        let held_pose = dynamics.position_of(7).unwrap();
        let horizontal = (held_pose.x * held_pose.x + held_pose.z * held_pose.z).sqrt();
        assert!(horizontal > 2.0, "ghost pushed out to r={horizontal}");
        let recorded_basis = dynamics.pose(7).unwrap().1;
        assert!(
            recorded_basis.columns[2][0] > 0.3,
            "ghost yaw never applied: {:?}",
            recorded_basis.columns
        );
        // Confirm: drop at the ghost pose; the prop stays there and sleeps.
        carry.update(&mut dynamics, grab, carrier(state, 0.));
        assert_eq!(carry.held(), None);
        assert!(!carry.placing());
        let recorded = carry.layout().get(&7).copied();
        assert!(recorded.is_some(), "confirmed placement was not recorded");
        for _ in 0..300 {
            dynamics.step(&world, &mut layer, &[]);
        }
        let p = dynamics.position_of(7).unwrap();
        assert!((p.y - REST_Y).abs() < 0.1, "placed prop rests at {}", p.y);
        let placed_horizontal = (p.x * p.x + p.z * p.z).sqrt();
        assert!(placed_horizontal > 1.5, "placed prop kept its distance: {placed_horizontal}");
        assert!(dynamics.bodies[0].asleep, "placed prop never slept");
    }

    /// Cancelling placement returns to plain carry with the prop still held.
    #[test]
    fn placement_cancel_returns_to_carry() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 1.2]);
        let mut carry = crate::physics::prop_carry::PropCarry::default();
        let state = skate_core::player::state::PhysicalStateId::BipedGround;
        let grab = crate::physics::prop_carry::Tick { grab: true, ..tick() };
        let place = crate::physics::prop_carry::Tick { placement: true, ..tick() };
        carry.update(&mut dynamics, grab, carrier(state, 0.));
        carry.update(&mut dynamics, place, carrier(state, 0.));
        assert!(carry.placing());
        carry.update(&mut dynamics, place, carrier(state, 0.));
        assert!(!carry.placing());
        assert_eq!(carry.held(), Some(7), "cancel must keep the carry");
        assert!(carry.layout().is_empty(), "cancel must not record a pose");
        // Carry follow still works after the cancel.
        carry.update(&mut dynamics, tick(), carrier(state, 1.0));
        dynamics.step(&world, &mut layer, &[]);
        assert!(dynamics.position_of(7).unwrap().z > 1.2);
    }

    /// A saved layout teleports a fresh body to the stored pose, asleep, and
    /// the rebaked triangles follow.
    #[test]
    fn layout_teleports_fresh_body() {
        let (world, mut layer, mut dynamics) = fixture([0., REST_Y, 1.2]);
        let basis = skate_core::math::Basis3 {
            columns: [[0., 0., -1.], [0., 1., 0.], [1., 0., 0.]],
        };
        let origin = Vector3::new(4., REST_Y + 0.5, -3.);
        let instance = dynamics.teleport(7, origin, basis).unwrap();
        layer.rebake(instance, basis.columns, origin).unwrap();
        let (pose_origin, pose_basis) = dynamics.pose(7).unwrap();
        assert_eq!(pose_origin, origin);
        assert_eq!(pose_basis.columns[2], [1., 0., 0.]);
        assert!(dynamics.bodies[0].asleep);
        // Rotated 90° about Y: the cube is symmetric, but the rebaked probe
        // confirms the range moved to the new origin.
        let hit = layer
            .world()
            .query_thin_line(
                Vector3::new(4., REST_Y + 2., -3.),
                Vector3::new(4., REST_Y - 1., -3.),
            )
            .unwrap()
            .unwrap();
        assert!((hit.geometry.position.y - (REST_Y + 1.)).abs() < 0.05);
        let _ = world;
    }
}
