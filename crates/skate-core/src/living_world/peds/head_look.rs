//! The ped head look-at ("post adjust", `82E27F90`, built by `82E27BF8`; research b1019): turns the head and spine
//! towards the attention stack's point on top of the animation. Yaw spills down a bone chain with per-bone limits,
//! pitch is on the head only, both rate-limited per 1/60 s step. Values come from `livingworld_entity_animation_postadjust`.

/// The chain from the head down (name table `0x83044758`).
pub const BONES: [&str; 6] = ["Head", "Neck1", "Neck", "Spine3", "Spine2", "Spine1"];

/// One record of `livingworld_entity_animation_postadjust` (retail `default`: 10 / 5 / 5 / 5 / 5 / 5 deg).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeadLookSettings {
    /// Each [`BONES`] bone's yaw limit, degrees (`0x43A2B449378045FB`).
    pub bones: [f32; 6],
    /// Pitch limit base, degrees (`0x803744CFB614F57C`, 15), times the up factor (`0x69B179F1FC89BCFE`, 2.0) or the
    /// down factor (`0x592536A4699594D9`, 1.0).
    pub pitch_limit: f32,
    pub up_factor: f32,
    pub down_factor: f32,
    /// Turn rates, degrees per second (`0x493D0BAC2404AC73` / `0xFCB1908C9C97607C`, 120 each).
    pub yaw_speed: f32,
    pub pitch_speed: f32,
    /// The yaw rate while the target is past the chain's total limit (`0xF23888EC33EAEFB9`, 180).
    pub overflow_yaw_speed: f32,
    /// No yaw update while the target is this close to the root horizontally, m (`0x820641A8`, 0.1).
    pub min_distance: f32,
}

impl Default for HeadLookSettings {
    fn default() -> Self {
        Self { bones: [10.0, 5.0, 5.0, 5.0, 5.0, 5.0], pitch_limit: 15.0, up_factor: 2.0, down_factor: 1.0, yaw_speed: 120.0, pitch_speed: 120.0, overflow_yaw_speed: 180.0, min_distance: 0.1 }
    }
}

impl HeadLookSettings {
    fn total_yaw(&self) -> f32 {
        self.bones.iter().sum()
    }
}

/// The controller's state: each bone's yaw (degrees, same order as the settings) and the head's pitch.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeadLook {
    pub yaw: Vec<f32>,
    pub pitch: f32,
    /// The rig index of each [`BONES`] bone, found once (`None` = the rig lacks it; empty = not looked up yet).
    pub bones: Vec<Option<usize>>,
}

/// The fixed step (`82E27F90` steps at 1/60 s).
pub const STEP: f32 = 1.0 / 60.0;

impl HeadLook {
    /// One 1/60 s step towards a target `yaw` / `pitch` (degrees, relative to the ped's facing; pitch positive up);
    /// `distance` = the target's horizontal distance from the root; `None` turns back to center.
    pub fn step(&mut self, s: &HeadLookSettings, target: Option<(f32, f32)>, distance: f32) {
        if self.yaw.len() != s.bones.len() {
            self.yaw = vec![0.0; s.bones.len()];
        }
        let (want_yaw, want_pitch) = target.unwrap_or((0.0, 0.0));
        if target.is_none() || distance > s.min_distance {
            let total_limit = s.total_yaw();
            let speed = if want_yaw.abs() > total_limit { s.overflow_yaw_speed } else { s.yaw_speed };
            let goal = want_yaw.clamp(-total_limit, total_limit);
            let now: f32 = self.yaw.iter().sum();
            let d = (goal - now).clamp(-speed * STEP, speed * STEP);
            self.spill(s, d);
        }
        let pitch_goal = want_pitch.clamp(-s.pitch_limit * s.down_factor, s.pitch_limit * s.up_factor);
        self.pitch += (pitch_goal - self.pitch).clamp(-s.pitch_speed * STEP, s.pitch_speed * STEP);
    }

    /// Moves the chain's total yaw by `d`: away from center the head takes it first and the next bone joins once the one
    /// above is at its limit; towards center the lowest bone turned that way gives it back first.
    fn spill(&mut self, s: &HeadLookSettings, mut d: f32) {
        if d == 0.0 {
            return;
        }
        let now: f32 = self.yaw.iter().sum();
        let outward = now == 0.0 || d.signum() == now.signum();
        let order: Vec<usize> = if outward { (0..self.yaw.len()).collect() } else { (0..self.yaw.len()).rev().collect() };
        for i in order {
            if d == 0.0 {
                break;
            }
            let limit = s.bones[i];
            let y = &mut self.yaw[i];
            let next = if outward {
                (*y + d).clamp(-limit, limit)
            } else if y.signum() == -d.signum() && *y != 0.0 {
                // Back towards 0, not past it.
                if d > 0.0 { (*y + d).min(0.0) } else { (*y + d).max(0.0) }
            } else {
                continue;
            };
            d -= next - *y;
            *y = next;
        }
    }
}

impl HeadLook {
    /// Turns the posed bones (local rotations, `[x, y, z, w]`) on top of the animation: each chain bone by its yaw
    /// about its local X, the head also by the pitch about its local Z (`82E27F90`'s packed per-bone rotations, b1019).
    /// `bones[i]` = the rig index of `settings.bones[i]`, `None` when the rig lacks it.
    pub fn apply(&self, bones: &[Option<usize>], rotations: &mut [[f32; 4]]) {
        for (i, (&yaw, bone)) in self.yaw.iter().zip(bones).enumerate() {
            let Some(r) = bone.and_then(|b| rotations.get_mut(b)) else { continue };
            let mut q = axis_angle([1.0, 0.0, 0.0], yaw.to_radians());
            if i == 0 {
                // A positive angle about the head's Z tips the face down on our rig (test
                // living_world_head_look_turns_the_head_about_world_up), so up is negative.
                q = mul(axis_angle([0.0, 0.0, 1.0], -self.pitch.to_radians()), q);
            }
            *r = mul(*r, q);
        }
    }

    /// At rest: nothing to add to the pose.
    pub fn is_centered(&self) -> bool {
        self.pitch == 0.0 && self.yaw.iter().all(|y| *y == 0.0)
    }
}

fn axis_angle(axis: [f32; 3], angle: f32) -> [f32; 4] {
    let (s, c) = (angle * 0.5).sin_cos();
    [axis[0] * s, axis[1] * s, axis[2] * s, c]
}

fn mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// The target's yaw (degrees, positive to the ped's left when +x is left of +z facing) and pitch (degrees, up positive)
/// from the ped's root, heading (radians about up, 0 = +z) and eye height; also the horizontal distance.
pub fn target_angles(root: [f32; 3], heading: f32, eye_height: f32, target: [f32; 3]) -> (f32, f32, f32) {
    let (dx, dz) = (target[0] - root[0], target[2] - root[2]);
    let flat = (dx * dx + dz * dz).sqrt();
    let (fx, fz) = (heading.sin(), heading.cos());
    let yaw = (fz * dx - fx * dz).atan2(fx * dx + fz * dz).to_degrees();
    let pitch = (target[1] - (root[1] + eye_height)).atan2(flat.max(1e-4)).to_degrees();
    (yaw, pitch, flat)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(h: &mut HeadLook, s: &HeadLookSettings, target: Option<(f32, f32)>, steps: usize) {
        for _ in 0..steps {
            h.step(s, target, 5.0);
        }
    }

    #[test]
    fn yaw_fills_the_head_first_then_spills_down_and_holds_at_the_edge() {
        let s = HeadLookSettings::default();
        let mut h = HeadLook::default();
        h.step(&s, Some((8.0, 0.0)), 5.0);
        assert_eq!(h.yaw[0], 2.0, "120 deg/s for 1/60 s");
        run(&mut h, &s, Some((12.0, 0.0)), 20);
        assert_eq!((h.yaw[0], h.yaw[1], h.yaw[2]), (10.0, 2.0, 0.0), "the head at its 10 deg, the neck takes the rest");
        run(&mut h, &s, Some((90.0, 0.0)), 60);
        assert_eq!(h.yaw, [10.0, 5.0, 5.0, 5.0, 5.0, 5.0], "past the total 35: held at the edge, no snap back");
        // Back to center: the spine gives it back first.
        h.step(&s, None, 5.0);
        assert_eq!((h.yaw[5], h.yaw[0]), (3.0, 10.0));
        run(&mut h, &s, None, 60);
        assert!(h.yaw.iter().all(|y| *y == 0.0));
    }

    #[test]
    fn pitch_is_clamped_up_and_down_separately_and_close_targets_keep_the_yaw() {
        let s = HeadLookSettings::default();
        let mut h = HeadLook::default();
        run(&mut h, &s, Some((0.0, 80.0)), 60);
        assert_eq!(h.pitch, 30.0, "15 x 2.0 up");
        run(&mut h, &s, Some((0.0, -80.0)), 60);
        assert_eq!(h.pitch, -15.0, "15 x 1.0 down");
        let before = h.yaw.clone();
        h.step(&s, Some((30.0, 0.0)), 0.05);
        assert_eq!(h.yaw, before, "within 0.1 m: no yaw update");
    }
}
