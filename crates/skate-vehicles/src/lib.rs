//! Bevy-independent, fixed-step Rapier vehicle simulation and validated mod definitions.
mod definition;
pub use definition::*;
pub use rapier3d;
use rapier3d::{
    control::{DynamicRayCastVehicleController, WheelTuning},
    prelude::*,
};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, Default, serde::Deserialize, serde::Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Controls {
    pub throttle: f32,
    pub steering: f32,
    pub brake: f32,
    pub handbrake: bool,
}
impl Controls {
    pub fn valid(&self) -> bool {
        self.throttle.is_finite()
            && self.steering.is_finite()
            && self.brake.is_finite()
            && (-1. ..=1.).contains(&self.throttle)
            && (-1. ..=1.).contains(&self.steering)
            && (0. ..=1.).contains(&self.brake)
    }
}
pub struct Vehicle {
    pub definition: VehicleDefinition,
    pub body: RigidBodyHandle,
    pub controller: DynamicRayCastVehicleController,
    pub controls: Controls,
}
pub struct Simulation {
    pub world: PhysicsWorld,
    pub vehicles: BTreeMap<u64, Vehicle>,
    next: u64,
}
impl Default for Simulation {
    fn default() -> Self {
        Self {
            world: PhysicsWorld::default(),
            vehicles: BTreeMap::new(),
            next: 1,
        }
    }
}
impl Simulation {
    pub fn ground(&mut self, triangles: impl Iterator<Item = [[f32; 3]; 3]>) -> Result<(), String> {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for triangle in triangles {
            let n = vertices.len() as u32;
            vertices.extend(triangle.map(Vector::from_array));
            indices.push([n, n + 1, n + 2]);
        }
        if vertices.is_empty() {
            return Err("Vehicle collision world has no triangles".into());
        }
        let collider = ColliderBuilder::trimesh(vertices, indices)
            .map_err(|e| e.to_string())?
            .friction(1.);
        self.world.insert(RigidBodyBuilder::fixed(), collider);
        self.world.step();
        Ok(())
    }
    pub fn spawn(
        &mut self,
        definition: VehicleDefinition,
        position: [f32; 3],
        heading: f32,
    ) -> Result<u64, String> {
        definition.validate()?;
        if !point(&position, 100000.) || !heading.is_finite() {
            return Err("Invalid vehicle spawn pose".into());
        }
        let d = &definition;
        let (body, _) = self.world.insert(
            RigidBodyBuilder::dynamic()
                .translation(Vector::from_array(position))
                .rotation(Vector::Y * heading)
                .ccd_enabled(true)
                .linear_damping(0.08)
                .angular_damping(0.5),
            ColliderBuilder::cuboid(d.half_extents[0], d.half_extents[1], d.half_extents[2])
                .mass(d.mass)
                .friction(0.3),
        );
        let mut controller = DynamicRayCastVehicleController::new(body);
        controller.index_up_axis = 1;
        controller.index_forward_axis = 2;
        let tuning = WheelTuning {
            suspension_stiffness: d.suspension_stiffness,
            suspension_compression: d.suspension_damping,
            suspension_damping: d.suspension_damping,
            max_suspension_travel: d.suspension_length,
            friction_slip: d.tire_grip,
            ..Default::default()
        };
        for w in &d.wheels {
            controller.add_wheel(
                Vector::from_array(w.position),
                -Vector::Y,
                -Vector::X,
                d.suspension_length,
                w.radius,
                &tuning,
            );
        }
        let id = self.next;
        self.next += 1;
        self.vehicles.insert(
            id,
            Vehicle {
                definition,
                body,
                controller,
                controls: Controls::default(),
            },
        );
        Ok(id)
    }
    pub fn remove(&mut self, id: u64) {
        if let Some(v) = self.vehicles.remove(&id) {
            self.world.remove_body(v.body);
        }
    }
    pub fn step(&mut self, dt: f32) {
        if !dt.is_finite() || dt <= 0. {
            return;
        }
        // Bound substeps even when a host uses a coarse fixed timestep.
        let steps = (dt / 0.008334).ceil().clamp(1., 16.) as u32;
        let h = dt.min(0.1) / steps as f32;
        for _ in 0..steps {
            self.world.integration_parameters.dt = h;
            for v in self.vehicles.values_mut() {
                let c = v.controls;
                let speed = v.controller.current_vehicle_speed;
                let d = &v.definition;
                let driven = d.wheels.iter().filter(|w| w.driven).count() as f32;
                for (wheel, def) in v.controller.wheels_mut().iter_mut().zip(&d.wheels) {
                    wheel.steering = if def.steering {
                        -c.steering * d.steering_angle / (1. + speed.abs() * 0.025)
                    } else {
                        0.
                    };
                    wheel.engine_force =
                        if def.driven && (speed.abs() < d.max_speed || c.throttle * speed < 0.) {
                            c.throttle * d.engine_force / driven
                        } else {
                            0.
                        };
                    wheel.brake = if c.handbrake && !def.steering {
                        d.brake_impulse
                    } else {
                        c.brake * d.brake_impulse
                    };
                }
                let queries = self.world.broad_phase.as_query_pipeline_mut(
                    self.world.narrow_phase.query_dispatcher(),
                    &mut self.world.bodies,
                    &mut self.world.colliders,
                    QueryFilter::default().exclude_rigid_body(v.body),
                );
                v.controller.update_vehicle(h, queries);
            }
            self.world.step();
        }
    }
    pub fn pose(&self, id: u64) -> Option<([f32; 3], [f32; 4])> {
        let body = &self.world.bodies[self.vehicles.get(&id)?.body];
        Some((body.translation().to_array(), body.rotation().to_array()))
    }
    pub fn reset(&mut self, id: u64, position: [f32; 3], heading: f32) -> Result<(), String> {
        if !point(&position, 100000.) || !heading.is_finite() {
            return Err("Invalid reset pose".into());
        }
        let v = self.vehicles.get_mut(&id).ok_or("Unknown vehicle")?;
        let b = &mut self.world.bodies[v.body];
        b.set_translation(Vector::from_array(position), true);
        b.set_rotation(Rotation::from_rotation_y(heading), true);
        b.set_linvel(Vector::ZERO, true);
        b.set_angvel(Vector::ZERO, true);
        v.controls = Controls::default();
        Ok(())
    }
    pub fn floor(&self, position: [f32; 3]) -> Option<[f32; 3]> {
        let origin = Vector::from_array(position) + Vector::Y * 2.;
        let (_, hit) = self.world.cast_ray(
            &Ray::new(origin, -Vector::Y),
            12.,
            true,
            QueryFilter::only_fixed(),
        )?;
        let floor = origin - Vector::Y * hit;
        let shape = rapier3d::parry::shape::Capsule::new_y(0.55, 0.3);
        if self
            .world
            .intersect_shape(
                Pose::from_translation(floor + Vector::Y * 1.),
                &shape,
                QueryFilter::default(),
            )
            .next()
            .is_some()
        {
            return None;
        }
        Some(floor.to_array())
    }
}
