//! Force-based kart handling. Rapier owns suspension queries and rigid-body collision;
//! this module owns steering, wheel angular momentum and combined tire traction.
use crate::{Vehicle, rapier3d::prelude::*};

#[derive(Default)]
pub(crate) struct Handling {
    steering: f32,
    omega: [f32; 8],
    spin: [f32; 8],
}

pub(crate) fn prepare(v: &mut Vehicle, bodies: &mut RigidBodySet, dt: f32) {
    let body = &mut bodies[v.body];
    let speed = body.linvel().dot(body.rotation() * Vector::Z);
    let d = &v.definition;
    let c = v.controls;
    if c.throttle != 0. || c.brake != 0. || c.handbrake {
        body.wake_up(true);
    }
    // Full low-speed lock, progressively reduced at speed. Rate limiting models
    // steering travel, including keyboard input; it never rotates the chassis.
    let target = c.steering * d.steering_angle / (1. + speed.abs().powi(2) * 0.008);
    v.handling.steering += (target - v.handling.steering).clamp(-2.5 * dt, 2.5 * dt);
    let front = d.wheels.iter().filter(|w| w.steering).map(|w| w.position[2]).fold(f32::NEG_INFINITY, f32::max);
    let rear = d.wheels.iter().filter(|w| !w.steering).map(|w| w.position[2]).fold(f32::INFINITY, f32::min);
    let wheelbase = front - rear;
    for (wheel, def) in v.controller.wheels_mut().iter_mut().zip(&d.wheels) {
        let angle = v.handling.steering;
        // Ackermann: inside front wheel turns more sharply around the rear axle.
        wheel.steering = if !def.steering { 0. } else if wheelbase > 0.1 && angle.abs() > 0.001 {
            let radius = wheelbase / angle.tan();
            (wheelbase / (radius - def.position[0])).atan()
        } else { angle };
        // Disable Rapier's arcade tire impulses: there must be exactly one tire owner.
        wheel.engine_force = 0.;
        wheel.brake = 0.;
        wheel.friction_slip = 0.;
        wheel.side_friction_stiffness = 0.;
    }
    // Quadratic aerodynamic drag, applied at COM; no artificial speed cap.
    let velocity = body.linvel();
    let drag = -velocity * velocity.length() * (0.5 * 1.225 * 0.65) * dt;
    body.apply_impulse(drag, false);
}

pub(crate) fn tires(v: &mut Vehicle, bodies: &mut RigidBodySet, colliders: &ColliderSet, dt: f32) {
    let d = &v.definition;
    let c = v.controls;
    let speed = bodies[v.body].linvel().dot(bodies[v.body].rotation() * Vector::Z);
    // Opposing pedal first brakes, then engages reverse close to rest.
    let opposing = c.throttle * speed < -0.5;
    let brake = c.brake.max(if opposing { c.throttle.abs() } else { 0. });
    let throttle = if opposing || brake > 0.01 { 0. } else { c.throttle };
    let driven = d.wheels.iter().filter(|w| w.driven).count() as f32;
    let wheel_count = d.wheels.len() as f32;
    for (i, (wheel, def)) in v.controller.wheels_mut().iter_mut().zip(&d.wheels).enumerate() {
        let r = def.radius;
        // Solid-cylinder rotational inertia, 2% of chassis mass per wheel.
        let inertia = 0.5 * (d.mass * 0.02) * r * r;
        let omega = &mut v.handling.omega[i];
        let limit = if throttle < 0. { d.max_speed.min(8.) } else { d.max_speed };
        let taper = (1. - (omega.abs() * r / limit).powi(2)).max(0.);
        let engine = if def.driven { throttle * d.engine_force / driven * taper } else { 0. };
        *omega += engine * r * dt / inertia;
        // Preserve legacy brake setting scale, but interpret at the reference 120 Hz.
        // Brake torque is time-scaled and can lock a wheel without reversing its spin.
        let braking = if c.handbrake && !def.steering { 1. } else { brake };
        let brake_step = braking * d.brake_impulse * 120. * r * dt / inertia;
        *omega -= omega.clamp(-brake_step, brake_step);
        let contact = *wheel.raycast_info();
        if contact.is_in_contact {
            let p = contact.contact_point_ws;
            let normal = contact.contact_normal_ws;
            let side = (wheel.axle() - normal * wheel.axle().dot(normal)).normalize_or_zero();
            let forward = normal.cross(side).normalize_or_zero();
            let ground = contact.ground_object.and_then(|h| colliders[h].parent());
            let ground_velocity = ground.map(|h| bodies[h].velocity_at_point(p)).unwrap_or(Vector::ZERO);
            let relative = bodies[v.body].velocity_at_point(p) - ground_velocity;
            let longitudinal = relative.dot(forward);
            let lateral = relative.dot(side);
            let load = wheel.wheel_suspension_force.min(wheel.max_suspension_force);
            // Tire and surface friction coefficients multiply; force scales with load.
            let surface = contact.ground_object.map(|h| colliders[h].friction()).unwrap_or(1.);
            let capacity = load * d.tire_grip * surface * dt;
            let inv_long = effective_inverse_mass(&bodies[v.body], p, forward)
                + ground.map(|h| effective_inverse_mass(&bodies[h], p, forward)).unwrap_or(0.);
            let inv_side = effective_inverse_mass(&bodies[v.body], p, side)
                + ground.map(|h| effective_inverse_mass(&bodies[h], p, side)).unwrap_or(0.);
            // Implicit longitudinal slip solve includes wheel inertia. Locked wheels
            // instead use the brake's available holding torque.
            let locked = braking > 0. && omega.abs() < 0.001;
            let mut jx = if locked {
                (-longitudinal / inv_long.max(1e-6)).clamp(-braking*d.brake_impulse*120.*dt, braking*d.brake_impulse*120.*dt)
            } else { (*omega * r - longitudinal) / (inv_long + r*r/inertia) };
            let slip_angle = lateral.atan2(longitudinal.abs().max(1.));
            let cornering = -load * 8. * slip_angle * dt;
            let stopping = -lateral / inv_side.max(1e-6) / wheel_count;
            let mut jy = cornering.signum() * cornering.abs().min(stopping.abs());
            // Braking, acceleration and turning share one friction circle.
            let magnitude = jx.hypot(jy);
            if magnitude > capacity && magnitude > 0. {
                jx *= capacity / magnitude;
                jy *= capacity / magnitude;
            }
            if !locked { *omega -= jx * r / inertia; }
            // Rolling resistance acts as an axle torque, so it cannot accelerate a wheel.
            let rolling = 0.015 * load * r * dt / inertia;
            *omega -= omega.clamp(-rolling, rolling);
            let impulse = forward * jx + side * jy;
            bodies[v.body].apply_impulse_at_point(impulse, p, false);
            if let Some(h) = ground {
                // Suspension was applied to the chassis by Rapier. Return both the
                // tire and suspension reactions to movable support bodies.
                if bodies[h].is_dynamic() {
                    bodies[h].apply_impulse_at_point(-impulse - normal * load * dt, p, true);
                }
            }
            wheel.forward_impulse = jx;
            wheel.side_impulse = jy;
        }
        v.handling.spin[i] += *omega * dt;
        wheel.rotation = v.handling.spin[i];
    }
}

fn effective_inverse_mass(body: &RigidBody, point: Vector, direction: Vector) -> f32 {
    if !body.is_dynamic() { return 0.; }
    let lever = (point - body.center_of_mass()).cross(direction);
    body.mass_properties().local_mprops.inv_mass
        + lever.dot(body.mass_properties().effective_world_inv_inertia * lever)
}
