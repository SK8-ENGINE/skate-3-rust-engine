//! Single-track handling the way the arcade motocross games do it.
//!
//! The chassis body is **held upright over the contact line** whenever a wheel
//! is down. Lean is not body roll: it is a handling state that steers the bike
//! (a leaned bike carves the radius its lean angle dictates), rolls the visual
//! model about the contact line and drives the rider's posture. A physically
//! leaning chassis on centreline raycasts could never work: Rapier casts each
//! wheel ray along the chassis' own down axis, so a body rolled 50° read its
//! suspension as extended, sank, and ground its collider on the floor.
//!
//! In the air the body is free. Whips, flips and tabletops are real rotation,
//! a neutral stick levels and straightens the bike for the landing, and the
//! landing itself is judged against the ground it lands on.
//!
//! Same contract as [`crate::assists`]: apply impulses through the rigid body,
//! never overwrite its transform and never add lift. Authored arcade values,
//! not recovered constants.
//!
//! ## Sign conventions, stated once
//!
//! The chassis frame is +Y up, +Z forward, +X driver-left, and `steering` is
//! positive-left (`docs/vehicle-sdk.md`). From that:
//!
//! - A **positive local-Z** rotation tilts the up axis toward -X, so it leans the
//!   bike **right**. Leaning left — the way you turn left — is **negative Z**.
//!   The `lean` state uses the same sign: negative is left.
//! - A **positive local-X** rotation tilts forward toward -Y, so it is **nose
//!   down**. Nose up, the wheelie direction, is negative X.
//! - A **positive local-Y** rotation sends forward toward +X, so it yaws **left**.
use crate::rapier3d::utils::AngularInertiaOps;
use crate::{Vehicle, rapier3d::prelude::*};

/// Speed at which the bars fully command lean rather than steering lock.
const LEAN_SPEED: f32 = 6.;
/// Preload must reach this fraction of travel before a release will pop.
const ARM_FRACTION: f32 = 0.35;
/// Below this much remaining rider weight, an armed preload fires.
const RELEASE_FRACTION: f32 = 0.15;
/// A hop shorter than this is a bump, not an air: no landing judgement.
const MIN_AIR: f32 = 0.25;
/// Landings slower than this are never judged on heading: you can land a
/// bike sideways at walking pace.
const YAW_JUDGEMENT_SPEED: f32 = 4.;
/// How long a clutch dump boosts the engine, seconds.
const BOOST_SECONDS: f32 = 0.5;
/// How long the bike is helped to plant itself after a real air, seconds.
/// Freestyle landings are the point of the whole mod: coming down square
/// enough should stick, not spit the rider over the bars because one wheel
/// touched a degree out.
const LANDING_ASSIST: f32 = 0.45;

#[derive(Default)]
pub(crate) struct Bike {
    /// Stored suspension compression from rider weight, 0..1.
    preload: f32,
    /// One pop per compression: set while loaded, cleared when it fires.
    armed: bool,
    /// Handling lean, radians. Negative leans left.
    pub(crate) lean: f32,
    /// No wheel has touched since the last contact.
    pub(crate) airborne: bool,
    /// Seconds since the last wheel left the ground.
    pub(crate) air_time: f32,
    /// Chassis-local height of the contact line, the axis the model leans about.
    pub(crate) contact_line: Option<f32>,
    /// Suspension compression of the most loaded wheel, 0..1 of travel.
    pub(crate) compression: f32,
    /// Engine speed held against the clutch, rad/s at the driven wheel.
    pub(crate) revs: f32,
    /// Seconds of clutch-dump boost remaining.
    boost: f32,
    /// Seconds of post-touchdown planting assist remaining.
    pub(crate) landing: f32,
}

impl Bike {
    pub(crate) fn preload(&self) -> f32 {
        self.preload
    }
}

/// Lean the bars and rider ask for, radians, negative left.
pub(crate) fn lean_target(d: &crate::BikeProfile, c: &crate::Controls, speed: f32) -> f32 {
    let speed_factor = (speed.abs() / LEAN_SPEED).clamp(0., 1.);
    // Bars are the main lean input at speed; rider weight hangs off on top.
    // Positive steering and positive lean both mean left, which is negative Z.
    -(c.steering * d.counter_steer + c.lean * 0.4).clamp(-1., 1.) * d.lean_max * speed_factor
}

/// Yaw rate a bike leaned this far carves at this speed, rad/s, positive left.
///
/// Steady cornering balances gravity against centripetal acceleration:
/// `v²/R = g·tan(lean)`, so the turn rate is `v/R = g·tan(lean)/v`. Faster is
/// wider for the same lean, exactly as on a real bike. Clamped so a walking
/// pace lean cannot spin the bike on the spot: steering lock does that job.
pub(crate) fn carve_rate(lean: f32, speed: f32) -> f32 {
    let speed_factor = (speed / LEAN_SPEED).clamp(0., 1.);
    (-9.81 * lean.tan() / speed.max(3.)).clamp(-2.5, 2.5) * speed_factor
}

/// Airborne pitch/yaw/roll acceleration, rad/s², in the chassis frame.
///
/// Rate targets keep repeated input controllable. A neutral axis does more
/// than damp: it levels roll toward world up and swings the nose back toward
/// the direction of travel, which is what lets a whip land. `heading_error`
/// is the signed yaw from the forward axis to the horizontal velocity,
/// positive when the velocity is to the left; `roll_error` is the roll that
/// would stand the bike up, positive when the bike is leaning left.
pub(crate) fn air_acceleration(
    d: &crate::BikeProfile,
    c: &crate::Controls,
    local_omega: Vector,
    heading_error: f32,
    roll_error: f32,
) -> Vector {
    let rate = |target: f32, current: f32, authority: f32| {
        ((target - current) * 3.).clamp(-authority, authority)
    };
    // Stick back is nose up, which is negative X. A held stick is a flip.
    let pitch_target = -c.weight * d.flip_rate;
    let pitch = rate(
        pitch_target,
        local_omega.x,
        if pitch_target >= local_omega.x {
            d.air_pitch_down
        } else {
            d.air_pitch_up
        },
    );
    let whipping = c.whip.abs() > 0.05;
    let leaning = c.lean.abs() > 0.05;
    // The bars in the air are a whip: yaw, with the back end laid over.
    let yaw_target = if whipping {
        c.whip * d.whip_rate
    } else {
        (heading_error * d.air_level).clamp(-d.whip_rate, d.whip_rate)
    };
    let roll_target = if whipping || leaning {
        -(c.whip * 0.5 + c.lean * 0.6) * d.whip_rate
    } else {
        (roll_error * d.air_level).clamp(-d.whip_rate, d.whip_rate)
    };
    Vector::new(
        pitch,
        rate(yaw_target, local_omega.y, d.air_yaw),
        rate(roll_target, local_omega.z, d.air_roll),
    )
}

/// Signed yaw from `forward` to `velocity` in the ground plane, positive when
/// the velocity is to the left of the nose. Zero below walking pace.
fn heading_error(forward: Vector, velocity: Vector) -> f32 {
    let f = Vector::new(forward.x, 0., forward.z);
    let v = Vector::new(velocity.x, 0., velocity.z);
    if v.length() < YAW_JUDGEMENT_SPEED || f.length_squared() < 1e-6 {
        return 0.;
    }
    // Left of the nose is +X in the chassis frame, so left of `f` in world
    // space is the direction `Y × f`.
    let left = Vector::Y.cross(f);
    v.dot(left).atan2(v.dot(f))
}

pub(crate) fn apply(v: &mut Vehicle, bodies: &mut RigidBodySet, dt: f32) {
    let d = v.definition.bike;
    let c = v.controls;
    let mut contacts = 0;
    let mut normal = Vector::ZERO;
    let mut contact_line = 0.;
    let mut compression: f32 = 0.;
    let mut rear_contact = None;
    let mut front_contact = None;
    let mut contact_point = Vector::ZERO;
    let mut tire_torque = Vector::ZERO;
    let centre = bodies[v.body].center_of_mass();
    for (wheel, def) in v.controller.wheels().iter().zip(&v.definition.wheels) {
        let info = wheel.raycast_info();
        if info.is_in_contact {
            contacts += 1;
            normal += info.contact_normal_ws;
            contact_point += info.contact_point_ws;
            contact_line += def.position[1] - info.suspension_length - def.radius;
            compression = compression.max(
                (v.definition.suspension_length - info.suspension_length)
                    / v.definition.suspension_length.max(1e-3),
            );
            if def.steering {
                front_contact = Some(info.contact_point_ws);
            } else {
                rear_contact = Some(info.contact_point_ws);
            }
            // The moment `handling::tires` just applied by driving or braking
            // this wheel at its contact patch. Drive at the back lifts the
            // nose; the front brake drives it down. On a bike with real grip
            // both are far stronger than gravity's restoring moment, so full
            // throttle alone stands the bike vertical unless this is answered.
            // Lateral force is applied at the mass centre for a bike and so
            // contributes nothing here.
            let n = info.contact_normal_ws;
            let side = (wheel.axle() - n * wheel.axle().dot(n)).normalize_or_zero();
            let along = n.cross(side).normalize_or_zero();
            tire_torque += (info.contact_point_ws - centre).cross(along * wheel.forward_impulse);
        }
    }
    v.bike.compression = compression.clamp(0., 1.);
    if contacts > 0 {
        v.bike.contact_line = Some(contact_line / contacts as f32);
    } else if v.bike.contact_line.is_none() {
        v.bike.contact_line = v
            .definition
            .wheels
            .first()
            .map(|w| w.position[1] - v.definition.suspension_length - w.radius);
    }
    // Clutch and boost, ahead of the body borrow: `handling` reads
    // `engine_scale` on the next tick.
    let rear_radius = v
        .definition
        .wheels
        .iter()
        .find(|w| w.driven)
        .map_or(0.33, |w| w.radius);
    let redline = v.definition.max_speed / rear_radius;
    if c.clutch {
        v.bike.revs += (c.throttle.max(0.) * redline - v.bike.revs).clamp(-4. * redline * dt, 3. * redline * dt);
    } else {
        if v.bike.revs > 0.5 * redline {
            v.bike.boost = BOOST_SECONDS;
            // Dumping the clutch hands the stored engine speed straight to the
            // wheel. That is the launch: the tyre is instantly asking for far
            // more than it can hold, so it lights up and drives rather than
            // waiting for the engine to spin up from rest.
            v.launch = v.bike.revs;
        }
        v.bike.revs -= v.bike.revs.min(4. * redline * dt);
    }
    v.bike.boost = (v.bike.boost - dt).max(0.);
    v.engine_scale = if v.bike.boost > 0. { d.clutch_boost } else { 1. };

    let rotation = *bodies[v.body].rotation();
    let local_omega = rotation.inverse() * bodies[v.body].angvel();
    let forward = rotation * Vector::Z;
    let up = rotation * Vector::Y;
    let velocity = bodies[v.body].linvel();
    let speed = velocity.dot(forward);
    let mut acceleration = Vector::ZERO;
    let normal = normal.normalize_or_zero();
    // One wheel down is still grounded: a bike spends real time on the front or
    // rear alone, which is exactly why `ground_stability`'s two-contact gate
    // does not suit it.
    let upright = up.dot(normal) > 0.3;
    let grounded = contacts >= 1 && upright;
    if contacts >= 1 && v.bike.airborne {
        // Touchdown. Judge it against the surface, not world up: landing on a
        // steep face is fine when the bike matches the face.
        if v.bike.air_time > MIN_AIR && v.occupied {
            let n = rotation.inverse() * normal;
            let roll = n.x.atan2(n.y).abs();
            let pitch = n.z.atan2(n.y).abs();
            let yaw = heading_error(forward, velocity).abs();
            if roll > d.landing_roll || pitch > d.landing_pitch || yaw > d.landing_yaw {
                let body = &bodies[v.body];
                v.ejection = Some(crate::safety::ejection(
                    &v.definition,
                    body,
                    body.angvel(),
                    "landing",
                ));
            }
        }
        if v.ejection.is_none() {
            v.bike.landing = LANDING_ASSIST;
        }
        v.bike.airborne = false;
        v.bike.air_time = 0.;
    }
    // Toppling acceleration per radian of roll, m*g*h / I_roll, with h
    // measured live from the mass centre to the mean contact point so it
    // tracks suspension travel instead of assuming a ride height. The
    // suspension pushes up at the contact patch, which stays on the
    // centreline, so any roll puts that force off to one side and the moment
    // grows with the roll. Without cancelling it the upright controller is
    // fighting a load that grows faster than it does, and `upright_gain` stops
    // meaning anything you can tune by eye: below about 84 the bike slowly
    // lies down, above it the number is arbitrary.
    let [hx, hy, _] = v
        .definition
        .inertia_half_extents
        .unwrap_or(v.definition.half_extents);
    let roll_inertia = ((hx * hx + hy * hy) * v.definition.mass / 3.).max(1e-3);
    let topple = if contacts > 0 {
        let height = (bodies[v.body].center_of_mass() - contact_point / contacts as f32)
            .dot(up)
            .clamp(0., 3.);
        v.definition.mass * 9.81 * height / roll_inertia
    } else {
        0.
    };
    let body = &mut bodies[v.body];
    if grounded {
        let target = lean_target(&d, &c, speed);
        v.bike.lean += (target - v.bike.lean) * (1. - (-d.lean_rate * dt).exp());
        // Hold the chassis over the contact line. Damping is derived from the
        // gain, so tuning the gain cannot tune the bike into an oscillation.
        //
        // Two different roll errors, and using one for both jobs is a bug that
        // only shows up off the flat. The controller steers toward the
        // *surface*; gravity pulls toward *world up*. On level ground they are
        // the same vector and cancelling one cancels the other, which is why
        // this read as correct for so long. On a cambered slope they diverge,
        // and cancelling gravity along the surface error leaves the real
        // toppling moment untouched: the bike settled about 55% of the way
        // into a 30 degree camber and barely rolled at all on a slope that
        // also climbed. Feed gravity forward along its own lever instead.
        let roll_to_surface = (rotation.inverse() * up.cross(normal)).z;
        let planted = 1. + 2.5 * (v.bike.landing / LANDING_ASSIST).clamp(0., 1.);
        // Cancel the roll moment the ground reaction makes about the mass
        // centre, by the same construction the wheelie controller uses for
        // pitch: the support pushes at the contact patch, and once the bike is
        // leaned that lever is off to one side. Feeding it forward is what
        // lets the PD's target angle be the angle it settles at.
        //
        // The support acts along the **contact normal**, not along world up,
        // and that distinction is the whole difference on a cambered slope.
        // A bike already square to the camber has its contact patch directly
        // beneath its mass centre along that normal, so the real moment is
        // zero -- while a feed-forward written against world up computes
        // `m*g*h*sin(camber)` there and injects a torque that drags the bike
        // back toward world-vertical. That is why it used to take up only
        // about half of a camber and almost none of one that also climbed.
        // On the flat the normal *is* world up, so nothing here changes.
        let gravity_roll = (rotation.inverse()
            * (contact_point / contacts as f32 - centre)
                .cross(normal * v.definition.mass * 9.81))
        .z / roll_inertia;
        acceleration.z = roll_to_surface * d.upright_gain * planted
            - gravity_roll
            - local_omega.z * 2. * (d.upright_gain * planted).sqrt();
        // Carve: the lean angle dictates the turn rate, the tyres supply the
        // force, and the friction circle in `handling` decides whether the
        // back end holds or steps out.
        let carve = carve_rate(v.bike.lean, speed);
        acceleration.y = (carve - local_omega.y) * d.lean_yaw;
        // Just landed: square the bike up under the rider rather than letting
        // a few degrees of yaw become a slide, and let the roll controller
        // above pull harder for the same window.
        if v.bike.landing > 0. {
            let home = heading_error(forward, velocity);
            acceleration.y += home * d.air_level * 2.;
            v.bike.landing = (v.bike.landing - dt).max(0.);
        }

        // Wheelies and stoppies are asked for, never stumbled into. Pitch is
        // measured against the ground rather than the world, so a ramp does
        // not read as a wheelie and this controller does not fight one.
        //
        // Positive `nose_up` is a wheelie and positive `acceleration.x` is
        // nose down, so with `t = nose_up` the target dynamic is
        // `t_dotdot = k(target - t) - c*t_dot`, where `t_dot = -omega.x`.
        let nose_up = forward.dot(normal).clamp(-1., 1.).asin();
        // Rider weight back on the throttle, or a dumped clutch: the clutch-up
        // is how you loft the front over a log or the face of a jump without
        // needing the run-up a weight shift alone would take.
        let clutched = if v.bike.boost > 0. { 0.8 } else { 0. };
        let wheelie = (c.weight.clamp(0., 1.) * (c.throttle * 3.).clamp(0., 1.))
            .max(clutched)
            * (speed - 1.).clamp(0., 1.);
        let stoppie =
            (-c.weight).clamp(0., 1.) * (c.brake * 2.).clamp(0., 1.) * (speed - 3.).clamp(0., 1.);
        let lift = wheelie.max(stoppie);
        let target = if wheelie >= stoppie {
            c.weight.max(0.).max(clutched) * d.wheelie_limit
        } else {
            c.weight.min(0.) * d.stoppie_limit
        };
        // Reaching a commanded angle wants real authority; a planted bike
        // needs only enough to stay planted.
        let gain = 10. + 30. * lift;
        acceleration.x -= (target - nose_up) * gain + local_omega.x * 2. * gain.sqrt();
        // Gravity's restoring moment through the support wheel, fed forward so
        // the balance point is the angle asked for rather than wherever the
        // PD happens to settle.
        let [_, hy, hz] = v
            .definition
            .inertia_half_extents
            .unwrap_or(v.definition.half_extents);
        let pitch_inertia = ((hy * hy + hz * hz) * v.definition.mass / 3.).max(1e-3);
        let support = if wheelie >= stoppie {
            rear_contact
        } else {
            front_contact
        };
        if let Some(p) = support.filter(|_| lift > 0.) {
            let moment = (rotation.inverse()
                * (p - centre).cross(Vector::Y * v.definition.mass * 9.81))
            .x / pitch_inertia;
            acceleration.x -= moment * lift;
        }
        // Loop-out and endo backstops, regardless of what the rider asks.
        if nose_up > d.wheelie_limit + 0.2 {
            acceleration.x += (nose_up - d.wheelie_limit - 0.2) * 120.;
        } else if -nose_up > d.stoppie_limit + 0.2 {
            acceleration.x -= (-nose_up - d.stoppie_limit - 0.2) * 120.;
        }
        // Answer the tyres own pitch moment in full. Drive and brake torque at
        // the contact patch are several times gravity's restoring moment on a
        // bike with real grip, so left alone, opening the throttle is a
        // wheelie and touching the front brake is an endo, neither of which
        // the rider chose. With this, pitch is exactly what was asked for and
        // nothing else -- and terrain still pitches the bike freely, because
        // `nose_up` is measured against the ground rather than the world.
        let local_tire = rotation.inverse() * tire_torque;
        body.apply_torque_impulse(rotation * Vector::new(-local_tire.x, 0., 0.), true);
    } else {
        // Airborne, or down on its side. The lean state has no meaning here
        // and fades so the model comes back to the body.
        v.bike.lean -= v.bike.lean * (1. - (-d.lean_rate * dt).exp());
        v.bike.landing = 0.;
        if contacts >= 1 && !upright {
            // On its side with a wheel still touching. Pick it back up: the
            // grounded roll controller cannot, because it is gated on the
            // chassis already being over its wheels, and without this the
            // bike lies there until `rider_safety` calls it an inversion.
            let roll = (rotation.inverse() * up.cross(normal)).z;
            acceleration.z =
                roll * (d.upright_gain + topple) - local_omega.z * 2. * d.upright_gain.sqrt();
            let _ = topple;
        }
    }
    if contacts >= 1 {
        // Rider weight compresses the suspension. The asymmetric rates are the
        // point: loading is slower than the snap that releases it.
        let want = c.weight.max(0.);
        v.bike.preload += (want - v.bike.preload).clamp(-8. * dt, 4. * dt);
        if v.bike.preload > ARM_FRACTION {
            v.bike.armed = true;
        }
        if v.bike.armed && want < RELEASE_FRACTION {
            // Stored travel becomes a pop. This is what makes jump height a
            // skill rather than a speed lookup.
            body.apply_impulse(normal * v.bike.preload * d.preload_release, true);
            v.bike.armed = false;
            v.bike.preload = 0.;
        } else {
            body.apply_impulse(-normal * v.bike.preload * d.preload_force * dt, true);
        }
    } else {
        v.bike.airborne = true;
        v.bike.air_time += dt;
        // Airborne preload decays rather than surviving the whole jump.
        v.bike.preload -= v.bike.preload.min(4. * dt);
        v.bike.armed = false;
        if v.occupied {
            let roll_error = (rotation.inverse() * up.cross(Vector::Y)).z;
            acceleration = air_acceleration(
                &d,
                &c,
                local_omega,
                heading_error(forward, velocity),
                roll_error,
            );
        }
    }
    if acceleration.length_squared() > 0. {
        let angular_impulse = body.mass_properties().effective_world_inv_inertia.inverse()
            * (rotation * acceleration * dt);
        body.apply_torque_impulse(angular_impulse, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BikeProfile, Controls};

    fn profile() -> BikeProfile {
        BikeProfile {
            enabled: true,
            ..Default::default()
        }
    }

    /// The whole module hangs off the sign table in the header comment. These
    /// pin it down directly, because a flipped sign here is far cheaper to catch
    /// than to diagnose from a bike that leans out of its turns.
    #[test]
    fn a_left_turn_commands_a_left_lean_and_a_left_carve() {
        let d = profile();
        let left = lean_target(
            &d,
            &Controls {
                steering: 1.,
                ..Default::default()
            },
            LEAN_SPEED,
        );
        assert!(left < -0.5, "left steer should lean left (negative), got {left}");
        let right = lean_target(
            &d,
            &Controls {
                steering: -1.,
                ..Default::default()
            },
            LEAN_SPEED,
        );
        assert!((left + right).abs() < 1e-5, "lean must be symmetric");
        // A left lean carves left, which is positive yaw.
        assert!(carve_rate(left, 10.) > 0.3);
        assert!(carve_rate(right, 10.) < -0.3);
    }

    #[test]
    fn the_same_lean_carves_wider_at_speed_and_not_at_all_at_rest() {
        let slow = carve_rate(-0.6, 8.);
        let fast = carve_rate(-0.6, 24.);
        assert!(slow > fast && fast > 0., "{slow} vs {fast}");
        assert_eq!(carve_rate(-0.6, 0.), 0.);
    }

    #[test]
    fn rider_weight_leans_without_steering_and_adds_to_it() {
        let d = profile();
        let lean_only = lean_target(
            &d,
            &Controls {
                lean: 1.,
                ..Default::default()
            },
            LEAN_SPEED,
        );
        assert!(lean_only < -0.1);
        let both = lean_target(
            &d,
            &Controls {
                lean: 1.,
                steering: 1.,
                ..Default::default()
            },
            LEAN_SPEED,
        );
        assert!(both <= lean_only, "steer and lean should agree in sign");
    }

    #[test]
    fn a_stopped_bike_commands_no_lean_however_the_stick_is_held() {
        let d = profile();
        let a = lean_target(
            &d,
            &Controls {
                lean: 1.,
                steering: 1.,
                ..Default::default()
            },
            0.,
        );
        assert_eq!(a, 0.);
    }

    #[test]
    fn air_whip_yaws_toward_the_requested_side_and_lays_the_bike_over() {
        let d = profile();
        let left = air_acceleration(
            &d,
            &Controls {
                whip: 1.,
                ..Default::default()
            },
            Vector::ZERO,
            0.,
            0.,
        );
        // Positive yaw is left; the back end lays over to the left, negative Z.
        assert!(left.y > 1.);
        assert!(left.z < -0.5);
    }

    #[test]
    fn neutral_air_input_straightens_and_levels_for_the_landing() {
        let d = profile();
        // Yawing left with no input: the rate target damps it back down.
        let damping = air_acceleration(&d, &Controls::default(), Vector::new(0., 3., 0.), 0., 0.);
        assert!(damping.y < -1.);
        // Velocity is off to the left of the nose: yaw left to meet it.
        let straighten = air_acceleration(&d, &Controls::default(), Vector::ZERO, 0.5, 0.);
        assert!(straighten.y > 0.5);
        // Leaning left (a positive correction stands it up): roll right.
        let level = air_acceleration(&d, &Controls::default(), Vector::ZERO, 0., 0.4);
        assert!(level.z > 0.5);
        assert_eq!(
            air_acceleration(&d, &Controls::default(), Vector::ZERO, 0., 0.),
            Vector::ZERO
        );
    }

    #[test]
    fn stick_back_in_the_air_is_a_backflip_rate() {
        let d = profile();
        let flip = air_acceleration(
            &d,
            &Controls {
                weight: 1.,
                ..Default::default()
            },
            Vector::ZERO,
            0.,
            0.,
        );
        // Nose up is negative X.
        assert!(flip.x < -1.);
        let scrub = air_acceleration(
            &d,
            &Controls {
                weight: -1.,
                ..Default::default()
            },
            Vector::ZERO,
            0.,
            0.,
        );
        assert!(scrub.x > 1.);
    }

    #[test]
    fn heading_error_is_signed_left_and_silent_at_walking_pace() {
        let forward = Vector::Z;
        // Velocity to driver-left (+X) is a positive error.
        assert!(heading_error(forward, Vector::new(5., 0., 5.)) > 0.5);
        assert!(heading_error(forward, Vector::new(-5., 0., 5.)) < -0.5);
        assert_eq!(heading_error(forward, Vector::new(1., 0., 1.)), 0.);
    }
}
