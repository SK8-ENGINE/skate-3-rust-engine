//! The replay cameras' world constraint (b449; 82C8B698 -> 82683680): leash, swept sphere, line of sight; data.
//! Details and gaps: docs/hails-additions/37-instant-replay.md#replay-camera-collision
use bevy::prelude::*;

/// The constraint's data; defaults are retail's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CameraCollision {
    /// Off = the cameras pass through the world (a mod's noclip shots); the leash still holds.
    pub enabled: bool,
    /// Sphere radius (m) times its scale: retail 0.1 x 1.3 (0x820641A8, 0x82099484).
    pub radius: f32,
    pub radius_scale: f32,
    /// A move with no component above this skips the query (FLT_EPSILON, 0x821EB480).
    pub move_epsilon: f32,
    /// How far the camera may go from the player, m (0x821EB794).
    pub leash: f32,
    /// Keyed cameras are also constrained while the clip plays (Playback, Timeline, Trim): on, as retail looks (user
    /// 2026-10-29: retail's camera never clips); its code gate reads 0 there (m353, b658), mechanism open.
    pub playback: bool,
}

impl Default for CameraCollision {
    fn default() -> Self {
        Self { enabled: true, radius: 0.1, radius_scale: 1.3, move_epsilon: f32::EPSILON, leash: 40.0, playback: true }
    }
}

/// A sweep hit: the fraction along the segment and the hit normal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Hit {
    pub fraction: f32,
    pub normal: Vec3,
}

/// The constrained camera position, and whether the camera should be turned back to its look point (a non-Free camera
/// that slid while the player is hidden, retail `82670600`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Constrained {
    pub position: Vec3,
    pub relook: bool,
}

/// `new` constrained against the last accepted position `old` (None = a camera cut: only the line of sight is checked
/// and the camera is pulled in front of an occluder, our stand-in for retail's snap request), the player `anchor`.
/// `free` = retail camera type 2 (keeps its rotation). `sweep(from, to, radius)` is the world query.
pub(crate) fn constrain(old: Option<Vec3>, new: Vec3, anchor: Vec3, free: bool, rules: &CameraCollision,
    mut sweep: impl FnMut(Vec3, Vec3, f32) -> Option<Hit>) -> Constrained {
    // One-sided: a surface stops a move only into its front face. A move leaving a surface it touches or coming from
    // behind it (the anchor is the skater root on the ground; a camera that got under the ground) passes, so the
    // camera is never pulled to the skater's feet or held under the map (laptop 2026-10-25, USER-ASKS 339).
    let mut sweep = |from: Vec3, to: Vec3, radius: f32| sweep(from, to, radius).filter(|h| (to - from).dot(h.normal) < 0.0);
    let offset = new - anchor;
    let new = if offset.length() > rules.leash { anchor + offset.normalize_or_zero() * rules.leash } else { new };
    let keep = Constrained { position: new, relook: false };
    if !rules.enabled {
        return keep;
    }
    let radius = rules.radius * rules.radius_scale;
    let Some(old) = old else {
        return match sweep(anchor, new, radius) {
            Some(hit) => Constrained { position: anchor + (new - anchor) * hit.fraction, relook: false },
            None => keep,
        };
    };
    let delta = new - old;
    if delta.abs().max_element() <= rules.move_epsilon {
        return keep;
    }
    let Some(hit) = sweep(old, new, radius) else { return keep };
    let slid = new - hit.normal * delta.dot(hit.normal);
    let position = if sweep(old, slid, radius).is_none() { slid } else { old };
    // Retail queues the line of sight to the new position with the motion sweep (sweep B).
    let hidden = sweep(anchor, new, radius).is_some();
    Constrained { position, relook: hidden && !free }
}

/// The map query for [`constrain`]: the gameplay camera's fat line (`camera::world_query::line`).
pub(crate) fn sweep(world: &skate_core::physics::board_world::BoardWorld, from: Vec3, to: Vec3, radius: f32) -> Option<Hit> {
    let lanes = |v: Vec3| [v.x, v.y, v.z, 0.0];
    let hit = crate::camera::world_query::line(world, lanes(from), lanes(to), radius).ok()?;
    (hit.hit != 0).then(|| Hit { fraction: hit.fraction, normal: Vec3::new(hit.normal[0], hit.normal[1], hit.normal[2]) })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A wall at x = 1 facing -x.
    fn wall(from: Vec3, to: Vec3, r: f32) -> Option<Hit> {
        let plane = 1.0 - r;
        (from.x < plane && to.x >= plane).then(|| Hit { fraction: (plane - from.x) / (to.x - from.x), normal: Vec3::NEG_X })
    }

    #[test]
    fn slides_along_a_wall() {
        let c = constrain(Some(Vec3::ZERO), Vec3::new(2.0, 0.0, 1.0), Vec3::new(-1.0, 0.0, 0.0), true, &Default::default(), wall);
        assert_eq!(c.position, Vec3::new(0.0, 0.0, 1.0));
    }

    #[test]
    fn a_blocked_slide_stops_at_the_old_position() {
        let corner = |from: Vec3, to: Vec3, r: f32| wall(from, to, r).or_else(|| (to.z > 0.5).then_some(Hit { fraction: 0.5, normal: Vec3::NEG_Z }));
        let c = constrain(Some(Vec3::ZERO), Vec3::new(2.0, 0.0, 1.0), Vec3::new(-1.0, 0.0, 0.0), true, &Default::default(), corner);
        assert_eq!(c.position, Vec3::ZERO);
    }

    #[test]
    fn no_motion_runs_no_query() {
        let mut calls = 0;
        let p = Vec3::new(0.5, 0.0, 0.0);
        let c = constrain(Some(p), p, Vec3::ZERO, false, &Default::default(), |_, _, _| { calls += 1; None });
        assert_eq!((c.position, calls), (p, 0));
    }

    #[test]
    fn a_cut_behind_a_wall_pulls_in_front() {
        let c = constrain(None, Vec3::new(3.0, 0.0, 0.0), Vec3::ZERO, false, &Default::default(), wall);
        assert!((c.position.x - (1.0 - 0.13)).abs() < 1e-5, "{c:?}");
    }

    #[test]
    fn leash_first_then_collision() {
        let mut seen = Vec::new();
        constrain(Some(Vec3::ZERO), Vec3::new(0.0, 0.0, 50.0), Vec3::ZERO, true, &Default::default(), |_, to, r| { seen.push((to, r)); None });
        assert_eq!(seen, vec![(Vec3::new(0.0, 0.0, 40.0), 0.1 * 1.3)]);
    }

    #[test]
    fn a_hidden_player_turns_a_non_free_camera_back() {
        let behind = |from: Vec3, to: Vec3, r: f32| wall(from, to, r).or_else(|| (from == Vec3::new(5.0, 0.0, 0.0)).then_some(Hit { fraction: 0.5, normal: Vec3::X }));
        let anchor = Vec3::new(5.0, 0.0, 0.0);
        assert!(constrain(Some(Vec3::ZERO), Vec3::new(2.0, 0.0, 1.0), anchor, false, &Default::default(), behind).relook);
        assert!(!constrain(Some(Vec3::ZERO), Vec3::new(2.0, 0.0, 1.0), anchor, true, &Default::default(), behind).relook);
    }

    /// A floor at y = 0 facing up (USER-ASKS 339): the anchor stands on it, so the line of sight starts in contact.
    fn floor(from: Vec3, to: Vec3, r: f32) -> Option<Hit> {
        let lo = from.y.min(to.y) - r;
        (lo <= 0.0 && from.y.max(to.y) + r >= 0.0).then(|| {
            let fraction = if (to.y - from.y).abs() < 1e-6 { 0.0 } else { ((r - from.y) / (to.y - from.y)).clamp(0.0, 1.0) };
            Hit { fraction, normal: Vec3::Y }
        })
    }

    #[test]
    fn a_cut_from_the_skaters_feet_keeps_the_camera_above_the_ground() {
        let eye = Vec3::new(3.0, 2.0, 0.0);
        let c = constrain(None, eye, Vec3::ZERO, false, &Default::default(), floor);
        assert_eq!(c.position, eye, "not pulled to the feet");
    }

    #[test]
    fn a_camera_under_the_ground_can_move_up_and_along_but_not_down_through_it() {
        let under = Vec3::new(0.0, -0.5, 0.0);
        assert_eq!(constrain(Some(under), Vec3::new(0.0, 1.0, 0.0), Vec3::new(5.0, 0.0, 0.0), true, &Default::default(), floor).position, Vec3::new(0.0, 1.0, 0.0));
        let above = Vec3::new(0.0, 1.0, 0.0);
        let c = constrain(Some(above), Vec3::new(0.0, -1.0, 0.0), Vec3::new(5.0, 0.0, 0.0), true, &Default::default(), floor);
        assert!(c.position.y > 0.0, "{c:?}");
    }

    #[test]
    fn disabled_passes_through() {
        let rules = CameraCollision { enabled: false, ..Default::default() };
        let c = constrain(Some(Vec3::ZERO), Vec3::new(2.0, 0.0, 0.0), Vec3::new(-1.0, 0.0, 0.0), true, &rules, wall);
        assert_eq!(c.position, Vec3::new(2.0, 0.0, 0.0));
    }
}
