//! Ped hand prop release (b25, b87, b90): the throw at a trash bin, the drop, the release frame and the unlink.
//!
//! Retail [code]: `sub_82E3E648` starts a throw (holding required; pending + aimed bits, target, launch speed, release
//! time, the clip by the angle to the target), the per-ped update `sub_82E3ED50` releases the held object when the
//! release time runs out, aimed with the launch velocity of `sub_82E16AB0` and timer 35 = the flight time, through
//! `sub_82E3EBE0` (holding cleared, the object goes into physics); a released object still linked to the ped is
//! unlinked by `sub_82E3FAE0` once it is outside the unlink box around the ped. The ped never destroys it (b90 §3).
//!
//! The attack throw (ThrowHandPropAtWantTarget, `sub_82E3E960`, b92 / b93) aims at the target first: both the target
//! and the ped are predicted ahead, the XZ intercept solve `sub_82E15CD0` picks the earliest point (or, with no
//! solution, the ped's own predicted position), a random horizontal jitter and a lift follow. A released prop that hits
//! the skater is an ordinary prop contact: retail has no hand-prop hit, bail, speech or score code (b92 Q2).

use super::super::Vec3;
use super::brain::PedBrain;

/// Timer 34 ThrowHandPropTimer and timer 35 ThrowHandPropReactionTimer (`brain.rs` `timers::NAMES`).
pub const THROW_TIMER: i32 = 34;
pub const THROW_REACTION_TIMER: i32 = 35;

/// Release values with retail defaults (mod-overridable).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HandPropSettings {
    /// The light throw (ThrowHandPropAtTrashBin, `82E3E648` attack 0): launch speed (`0x821F1790` 5.0) and the time
    /// to the release (`0x82063BE0` 0.91667 s).
    pub light_speed: f32,
    pub light_release_seconds: f32,
    /// The attack throw (ThrowHandPropAtWantTarget): 10.0 (`0x82063BF0`) and 0.8333 s (`0x82063BE4`).
    pub attack_speed: f32,
    pub attack_release_seconds: f32,
    /// The attack aim's look-ahead for the target and the ped (`82E3E960`, `0x82063BE4` 0.8333 s).
    pub attack_lead_seconds: f32,
    /// The intercept solve's epsilon (`sub_82E15CD0`); the image word at `0x82195100` does not read as a float, the
    /// value is [inferred] (1.19e-7, the constant the same code uses elsewhere, b93 B1).
    pub intercept_epsilon: f32,
    /// The aim jitter's length range, metres (`sub_82E17508`: `0x82165A10` 0.0, `0x820C6D98` 0.25).
    pub jitter_min: f32,
    pub jitter_max: f32,
    /// The lift added to the aim point after the jitter (`0x82139A20` (0, 1, 0) x `0x8231A844` 1.0).
    pub aim_lift: f32,
    /// Half the launch solve's gravity (`sub_82E16AB0`, `0x822F8FA4` 4.9).
    pub half_gravity: f32,
    /// The unlink box half sizes around the ped, metres x / y (up) / z (`82E3F090`: 0.5, 2.0, 0.5).
    pub unlink_box: [f32; 3],
    /// A released prop takes part in the skater's body contact (retail: an ordinary DMO contact, b92 Q2 / b94); a mod
    /// may turn it off.
    pub skater_contact: bool,
    /// Walking peds roll a starting prop from their type's chance and list (`82E33198`); a mod may turn it off.
    pub starting_props: bool,
    /// The released prop's contact group by mass (`82C56BA0`: 12 at or above the small-object mass, else 14; the
    /// threshold is the skater's `SmallObjectMassThreshold` 5.5 [inferred same field, b94]).
    pub heavy_group: u32,
    pub small_group: u32,
    /// The throw clips' blend times (ped vfunc +240, 0.2).
    pub clip_blend: f32,
    /// The dynamic-object pool: the manager's create (`826B8830`) refuses at 49 live objects, so a ped's hand prop
    /// is then not created [code, b91].
    pub max_live: usize,
    /// A released prop is culled beyond this distance from the census observer (`livingworld_census_ranges`
    /// `dynamicobjects` cull 100 m [data, dmo-plan]; retail's cull `826BAD98` for a DMO without a placement is not
    /// read, b91).
    pub cull_distance: f32,
}

impl Default for HandPropSettings {
    fn default() -> Self {
        Self {
            light_speed: 5.0,
            light_release_seconds: 0.916_67,
            attack_speed: 10.0,
            attack_release_seconds: 0.833_3,
            attack_lead_seconds: 0.833_3,
            intercept_epsilon: 1.19e-7,
            jitter_min: 0.0,
            jitter_max: 0.25,
            aim_lift: 1.0,
            half_gravity: 4.9,
            unlink_box: [0.5, 2.0, 0.5],
            skater_contact: true,
            starting_props: true,
            heavy_group: 12,
            small_group: 14,
            clip_blend: 0.2,
            max_live: 49,
            cull_distance: 100.0,
        }
    }
}

/// A started throw (`brain+3152` target, `+3252` speed; `3279` bit 0x40 aimed).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HandPropThrow {
    pub target: Vec3,
    pub speed: f32,
}

/// The launch solve `sub_82E16AB0` [code]: the horizontal speed is `speed`, the flight time is the flat distance over
/// it, the vertical speed reaches `target` at that time under `2 * half_gravity`. `None` (retail: zero velocity, time
/// -1) when the speed is not positive or the target is straight above or below (|dx| and |dz| <= 1.19e-7).
pub fn launch(origin: Vec3, target: Vec3, speed: f32, half_gravity: f32) -> Option<(Vec3, f32)> {
    if speed <= 0.0 {
        return None;
    }
    let d = [target[0] - origin[0], target[1] - origin[1], target[2] - origin[2]];
    if d[0].abs() <= f32::EPSILON && d[2].abs() <= f32::EPSILON {
        return None;
    }
    let flat = (d[0] * d[0] + d[2] * d[2]).sqrt();
    let time = flat / speed;
    let vy = d[1] / time + half_gravity * time;
    Some(([d[0] / flat * speed, vy, d[2] / flat * speed], time))
}

/// The intercept solve `sub_82E15CD0` [code, b93 B2 / B3]: a projectile of flat speed `speed` from `origin` meeting
/// a target at `aim` moving at `velocity` (XZ only). Solved along the dominant axis (strict: equal |dx| and |dz| have
/// no solution); returns the target's point at the earliest non-negative time (Y of `aim`), `None` with no solution.
/// Ported as read, including the `v_minor + k * v_major` term (an exact intercept would subtract; b93 B2).
pub fn intercept(aim: Vec3, velocity: Vec3, origin: Vec3, speed: f32, epsilon: f32) -> Option<Vec3> {
    if speed <= 0.0 {
        return None;
    }
    let (dx, dz) = (aim[0] - origin[0], aim[2] - origin[2]);
    let (major, minor, v_major, v_minor) = if dx.abs() > dz.abs() && dx.abs() > epsilon {
        (dx, dz, velocity[0], velocity[2])
    } else if dz.abs() > dx.abs() && dz.abs() > epsilon {
        (dz, dx, velocity[2], velocity[0])
    } else {
        return None;
    };
    let k = minor / major;
    let b = (v_minor + k * v_major) / speed;
    let a = k * k + 1.0;
    let disc = (2.0 * k * b).powi(2) - 4.0 * a * (b * b - 1.0);
    if disc < 0.0 {
        return None;
    }
    let root = disc.sqrt();
    let time = |u: f32| {
        let den = v_major - speed * u;
        (den.abs() > epsilon).then(|| -major / den)
    };
    let t = [(-2.0 * k * b + root) / (2.0 * a), (-2.0 * k * b - root) / (2.0 * a)]
        .into_iter()
        .filter_map(time)
        .filter(|t| *t >= 0.0)
        .reduce(f32::min)?;
    Some([aim[0] + velocity[0] * t, aim[1], aim[2] + velocity[2] * t])
}

/// The aim jitter `sub_82E17508(rng, lo, hi, base)` [code, b92 Q1a]: three draws (z, x, length), a unit direction in
/// the XZ plane from the square [-0.5, 0.5)^2 and a length uniform in [lo, hi). The draws come from the brain's
/// seeded RNG (retail: its global generator), so a host decides them.
pub fn jitter(rng: &mut crate::living_world::Rng, base: Vec3, lo: f32, hi: f32) -> Vec3 {
    let z = rng.unit() - 0.5;
    let x = rng.unit() - 0.5;
    let length = lo + rng.unit() * (hi - lo);
    let norm = (x * x + z * z).sqrt();
    if norm <= f32::EPSILON {
        return base;
    }
    [base[0] + x / norm * length, base[1], base[2] + z / norm * length]
}

/// What the attack throw reads: the ped's and the target's position and velocity (m/s).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AttackAim {
    pub position: Vec3,
    pub velocity: Vec3,
    pub target: Vec3,
    pub target_velocity: Vec3,
}

/// `sub_82E3E960`'s aim point [code, b93 B4]: predict the target and the ped `lead` seconds ahead, intercept at
/// `speed` (no solution: the ped's own predicted position), jitter, lift.
pub fn attack_point(settings: &HandPropSettings, aim: AttackAim, rng: &mut crate::living_world::Rng) -> Vec3 {
    let ahead = |p: Vec3, v: Vec3| [0, 1, 2].map(|i| p[i] + v[i] * settings.attack_lead_seconds);
    let (target, origin) = (ahead(aim.target, aim.target_velocity), ahead(aim.position, aim.velocity));
    let point = intercept(target, aim.target_velocity, origin, settings.attack_speed, settings.intercept_epsilon).unwrap_or(origin);
    let mut point = jitter(rng, point, settings.jitter_min, settings.jitter_max);
    point[1] += settings.aim_lift;
    point
}

/// The flat angle from the ped's facing to `point`, radians in [0, 2pi), counter-clockwise seen from above (toward the
/// ped's left with `heading` 0 = +z). Retail `sub_8296EC98` [code]; its sign convention is [inferred] from the clip
/// names (small angles = L).
pub fn flat_angle(heading: f32, from: Vec3, point: Vec3) -> f32 {
    let yaw = (point[0] - from[0]).atan2(point[2] - from[2]);
    (yaw - heading).rem_euclid(std::f32::consts::TAU)
}

/// The light throw's clip by angle (`82E3E648`, thresholds `0x822F9144..0x822F9158`) [code].
pub fn light_throw_clip(angle: f32) -> &'static str {
    match angle {
        a if !(0.6854..=5.5978).contains(&a) => "HandPropThrowLightForward",
        a if a < 0.8854 => "HandPropThrowLightL45",
        a if a < 1.6708 => "HandPropThrowLightL90",
        a if a < 4.6124 => "HandPropThrowLightR180",
        a if a < 4.8124 => "HandPropThrowLightR90",
        _ => "HandPropThrowLightR45",
    }
}

/// The attack throw's clip by angle (`82E3E648` attack 1, thresholds `0x82063B38` 0.5236, `0x822F9138` 5.7596,
/// `0x822F913C` 2.618, `0x822F9140` 3.6652; names `0x820649A4..0x820649E4`) [code]. Retail plays the plain throw both
/// in front and behind (150 to 210 degrees).
pub fn attack_throw_clip(angle: f32) -> &'static str {
    match angle {
        a if !(0.5236..=5.7596).contains(&a) => "HandPropAttackThrow",
        a if a < 2.618 => "HandPropAttackThrowLeft",
        a if a < 3.6652 => "HandPropAttackThrow",
        _ => "HandPropAttackThrowRight",
    }
}

impl PedBrain {
    /// `sub_82E3E648(ped, point, attack 0)`: start the light throw at `target` when the ped holds its prop. Returns the
    /// clip to play.
    pub fn start_light_throw(&mut self, settings: &HandPropSettings, heading: f32, position: Vec3, target: Vec3) -> Option<&'static str> {
        if !self.hand_prop.holding {
            return None;
        }
        self.hand_prop.linked = true;
        self.hand_prop.throw = Some(HandPropThrow { target, speed: settings.light_speed });
        self.set_timer(THROW_TIMER, settings.light_release_seconds);
        Some(light_throw_clip(flat_angle(heading, position, target)))
    }

    /// `sub_82E3E960(ped, target)`: the aimed attack throw. The aim (and its three RNG draws) is computed first, then
    /// `82E3E648(ped, point, attack 1)` starts it when the ped holds its prop. Returns the clip to play.
    pub fn start_attack_throw(&mut self, settings: &HandPropSettings, heading: f32, aim: AttackAim) -> Option<&'static str> {
        let target = attack_point(settings, aim, self.rng());
        if !self.hand_prop.holding {
            return None;
        }
        self.hand_prop.linked = true;
        self.hand_prop.throw = Some(HandPropThrow { target, speed: settings.attack_speed });
        self.set_timer(THROW_TIMER, settings.attack_release_seconds);
        Some(attack_throw_clip(flat_angle(heading, aim.position, target)))
    }

    /// `sub_82E3EBE0(ped, 0, zero)` (DropHandProp): release in place. Retail's release vfunc +136 (`82C56C70`) writes
    /// no velocity for flag 0, so the body keeps the kinematic hand's velocity [code, b91]; ours: zero (the hand's
    /// velocity is not tracked). Returns the release velocity.
    pub fn drop_hand_prop(&mut self) -> Option<Vec3> {
        if !self.hand_prop.holding {
            return None;
        }
        self.hand_prop.holding = false;
        self.hand_prop.linked = true;
        self.hand_prop.throw = None;
        Some([0.0; 3])
    }

    /// The per-ped release step (`sub_82E3ED50`): a pending throw releases when timer 34 has run out (ours; retail
    /// compares a clip time query against the same value, b90 §2 [inferred equal]) with the launch velocity from the
    /// prop's hand position; timer 35 gets the flight time. Returns the release velocity on the release frame (the
    /// host releases the object at once; DropHandProp posts [`super::brain::ChaseRequest::HandPropReleased`]).
    pub fn update_hand_prop_release(&mut self, settings: &HandPropSettings, prop_position: Vec3) -> Option<Vec3> {
        let throw = self.hand_prop.throw?;
        if !self.hand_prop.holding || self.timer(THROW_TIMER) > 0.0 {
            return None;
        }
        let (velocity, time) = launch(prop_position, throw.target, throw.speed, settings.half_gravity).unwrap_or(([0.0; 3], -1.0));
        self.set_timer(THROW_REACTION_TIMER, time);
        self.hand_prop.holding = false;
        self.hand_prop.throw = None;
        Some(velocity)
    }

    /// `82E3F090` -> `sub_82E3FAE0`: a released prop still linked to the ped is unlinked once it leaves the box around
    /// the ped. Returns true on the unlink (the host forgets the link; the object stays in the world).
    pub fn update_hand_prop_link(&mut self, settings: &HandPropSettings, prop_position: Vec3, ped_position: Vec3) -> bool {
        if self.hand_prop.holding || !self.hand_prop.linked {
            return false;
        }
        let b = settings.unlink_box;
        let d = (0..3).map(|i| (prop_position[i] - ped_position[i]).abs()).collect::<Vec<_>>();
        if d[0] > b[0] || d[1] > b[1] || d[2] > b[2] {
            self.hand_prop.clear();
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `sub_82E16AB0`: flat speed = `speed`, time = flat distance / speed, and the arc under 2 x 4.9 reaches the
    /// target at that time; no solution straight up or with no speed (retail: zero velocity, time -1).
    #[test]
    fn launch_reaches_the_target() {
        let (origin, target) = ([1.0, 1.5, 2.0], [4.0, 0.8, 6.0]);
        let (v, t) = launch(origin, target, 5.0, 4.9).unwrap();
        assert!((t - 1.0).abs() < 1e-6, "5 m flat at 5 m/s: {t}");
        assert!(((v[0] * v[0] + v[2] * v[2]).sqrt() - 5.0).abs() < 1e-5);
        let y = origin[1] + v[1] * t - 4.9 * t * t;
        assert!((y - target[1]).abs() < 1e-5, "lands at the target height: {y}");
        assert_eq!(launch(origin, [1.0, 9.0, 2.0], 5.0, 4.9), None);
        assert_eq!(launch(origin, target, 0.0, 4.9), None);
    }

    /// The light throw clips by the angle to the target (`82E3E648`).
    #[test]
    fn light_throw_clip_by_angle() {
        let deg = |d: f32| d.to_radians();
        assert_eq!(light_throw_clip(deg(0.0)), "HandPropThrowLightForward");
        assert_eq!(light_throw_clip(deg(330.0)), "HandPropThrowLightForward");
        assert_eq!(light_throw_clip(deg(45.0)), "HandPropThrowLightL45");
        assert_eq!(light_throw_clip(deg(90.0)), "HandPropThrowLightL90");
        assert_eq!(light_throw_clip(deg(180.0)), "HandPropThrowLightR180");
        assert_eq!(light_throw_clip(deg(270.0)), "HandPropThrowLightR90");
        assert_eq!(light_throw_clip(deg(300.0)), "HandPropThrowLightR45");
        // Heading 0 faces +z; a target on +x is 90 deg counter-clockwise (left).
        assert!((flat_angle(0.0, [0.0; 3], [1.0, 0.0, 0.0]) - deg(90.0)).abs() < 1e-5);
    }

    /// Throw at a bin: nothing until the holding ped starts it, release when timer 34 runs out with the launch
    /// velocity (timer 35 = flight time, HasHandProp false from then), unlink once the prop leaves the box.
    #[test]
    fn bin_throw_releases_then_unlinks() {
        let s = HandPropSettings::default();
        let mut b = PedBrain::default();
        assert_eq!(b.start_light_throw(&s, 0.0, [0.0; 3], [0.0, 0.0, 2.0]), None, "nothing held");
        b.hand_prop.request("pop");
        b.hand_prop.requested = false;
        b.hand_prop.holding = true;
        assert_eq!(b.start_light_throw(&s, 0.0, [0.0; 3], [0.0, 0.5, 2.0]), Some("HandPropThrowLightForward"));
        let hand = [0.2, 1.2, 0.3];
        b.tick_timers(0.5);
        assert_eq!(b.update_hand_prop_release(&s, hand), None, "before the release time");
        b.tick_timers(0.5);
        let v = b.update_hand_prop_release(&s, hand).expect("released");
        let (expected, time) = launch(hand, [0.0, 0.5, 2.0], 5.0, 4.9).unwrap();
        assert_eq!(v, expected);
        assert!((b.timer(THROW_REACTION_TIMER) - time).abs() < 1e-6);
        assert!(!b.hand_prop.has() && b.hand_prop.linked);
        assert!(!b.update_hand_prop_link(&s, [0.3, 1.0, 0.4], [0.0; 3]), "still inside the box");
        assert!(b.update_hand_prop_link(&s, [0.0, 0.5, 2.0], [0.0; 3]));
        assert_eq!(b.hand_prop, Default::default());
    }

    /// `sub_82E15CD0`: a still target is met where it stands; a target crossing the major axis' normal (no major
    /// velocity, where the retail term is exact) is met at a point the prop reaches in the same time; equal |dx| and
    /// |dz|, no speed, or a target outrunning the prop have no solution.
    #[test]
    fn intercept_solve() {
        let eps = HandPropSettings::default().intercept_epsilon;
        assert_eq!(intercept([8.0, 1.0, 3.0], [0.0; 3], [0.0; 3], 10.0, eps), Some([8.0, 1.0, 3.0]));
        let (aim, v) = ([8.0, 0.0, 2.0], [0.0, 0.0, 3.0]);
        let p = intercept(aim, v, [0.0; 3], 10.0, eps).unwrap();
        let t = (p[2] - aim[2]) / v[2];
        assert!(t > 0.0 && ((p[0] * p[0] + p[2] * p[2]).sqrt() - 10.0 * t).abs() < 1e-4, "{p:?} t={t}");
        assert_eq!(intercept([4.0, 0.0, 4.0], [0.0; 3], [0.0; 3], 10.0, eps), None);
        assert_eq!(intercept([8.0, 0.0, 3.0], [0.0; 3], [0.0; 3], 0.0, eps), None);
        assert_eq!(intercept([8.0, 0.0, 0.0], [20.0, 0.0, 0.0], [0.0; 3], 10.0, eps), None);
    }

    /// `sub_82E17508`: a horizontal offset of length in [lo, hi).
    #[test]
    fn jitter_is_horizontal_and_bounded() {
        let mut rng = crate::living_world::Rng::new(7);
        for _ in 0..200 {
            let p = jitter(&mut rng, [1.0, 2.0, 3.0], 0.0, 0.25);
            let l = ((p[0] - 1.0).powi(2) + (p[2] - 3.0).powi(2)).sqrt();
            assert!(p[1] == 2.0 && l < 0.25, "{p:?}");
        }
    }

    /// The attack clips by angle (`82E3E648` attack 1): plain in front and behind, left / right between.
    #[test]
    fn attack_throw_clip_by_angle() {
        let deg = |d: f32| d.to_radians();
        assert_eq!(attack_throw_clip(deg(10.0)), "HandPropAttackThrow");
        assert_eq!(attack_throw_clip(deg(340.0)), "HandPropAttackThrow");
        assert_eq!(attack_throw_clip(deg(90.0)), "HandPropAttackThrowLeft");
        assert_eq!(attack_throw_clip(deg(180.0)), "HandPropAttackThrow");
        assert_eq!(attack_throw_clip(deg(270.0)), "HandPropAttackThrowRight");
    }

    /// `sub_82E3E960`: both predicted 0.8333 s ahead, the intercept point lifted 1 m (no jitter here); with no
    /// solution the ped aims at its own predicted position. The throw then releases at 10 m/s after 0.8333 s.
    #[test]
    fn attack_throw_aims_and_releases() {
        let s = HandPropSettings { jitter_max: 0.0, ..Default::default() };
        let mut rng = crate::living_world::Rng::new(1);
        let still = AttackAim { position: [0.0; 3], velocity: [0.0; 3], target: [6.0, 0.5, 1.0], target_velocity: [0.0; 3] };
        assert_eq!(attack_point(&s, still, &mut rng), [6.0, 1.5, 1.0]);
        let diagonal = AttackAim { position: [1.0, 0.0, 0.0], velocity: [0.0; 3], target: [5.0, 0.0, 4.0], target_velocity: [0.0; 3] };
        let p = attack_point(&s, diagonal, &mut rng);
        assert_eq!(p, [1.0, 1.0, 0.0], "equal |dx| and |dz|: the ped's own position");

        let mut b = PedBrain::default();
        assert_eq!(b.start_attack_throw(&s, 0.0, still), None, "nothing held");
        b.hand_prop.holding = true;
        assert_eq!(b.start_attack_throw(&s, 6.0f32.atan2(1.0), still), Some("HandPropAttackThrow"), "facing the target");
        assert_eq!(b.hand_prop.throw, Some(HandPropThrow { target: [6.0, 1.5, 1.0], speed: 10.0 }));
        b.tick_timers(0.8);
        assert_eq!(b.update_hand_prop_release(&s, [0.2, 1.2, 0.3]), None);
        b.tick_timers(0.05);
        let v = b.update_hand_prop_release(&s, [0.2, 1.2, 0.3]).expect("released");
        assert!(((v[0] * v[0] + v[2] * v[2]).sqrt() - 10.0).abs() < 1e-4);
    }

    /// DropHandProp releases in place with zero velocity.
    #[test]
    fn drop_releases_with_zero_velocity() {
        let mut b = PedBrain::default();
        assert_eq!(b.drop_hand_prop(), None);
        b.hand_prop.holding = true;
        assert_eq!(b.drop_hand_prop(), Some([0.0; 3]));
        assert!(!b.hand_prop.has() && b.hand_prop.linked);
    }
}
