//! The replay editor's basic cameras (Follow, Tripod, Free, Game; b401 / b402, b404, b406); limits are data.
//! Details and gaps: docs/hails-additions/37-instant-replay.md#replay-basic-cameras
use bevy::prelude::*;

/// Which basic camera a clip uses (retail's camera type: 0 Follow, 1 Tripod, 2 Free, 3 Game; `82508CE8`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CameraKey {
    /// The recorded gameplay camera.
    Game,
    /// The eye in the Follow eye frame ([`Anchors::eye`]), looking at the skater root plus the look height (m).
    Follow { eye: Vec3, look_height: f32 },
    /// A world eye looking at the skater's root plus the look height.
    Tripod { eye: Vec3, look_height: f32 },
    /// A world pose: eye, yaw / pitch (degrees) and the look distance (m; retail C+0x24, set at the pick and kept by
    /// every edit, b652).
    Free { eye: Vec3, yaw: f32, pitch: f32, distance: f32 },
}

/// The menu rows of the basic cameras (screen order Follow, Tripod, Game, Free; item table `8302CCE0`, b401).
impl CameraKey {
    pub fn row(&self) -> usize {
        match self {
            CameraKey::Follow { .. } => 0,
            CameraKey::Tripod { .. } => 1,
            CameraKey::Game => 2,
            CameraKey::Free { .. } => 3,
        }
    }

    /// The edit mode the row's A opens (b401: Follow 7, Tripod 8, Game 9, Free 10; the advanced rows 4..6 the same
    /// modes, b405) and the movie state the C++ posts for it (kFollowEditMode 1, kTripodEditMode 2, kDefaultCameraMode
    /// 10, kFreeCameraMode 7; advanced 12 / 13 / 14).
    pub fn edit_mode_and_state(row: usize) -> Option<(u8, i32)> {
        match row {
            0 => Some((7, 1)),
            1 => Some((8, 2)),
            2 => Some((9, 10)),
            3 => Some((10, 7)),
            4 => Some((7, 12)),
            5 => Some((8, 13)),
            6 => Some((10, 14)),
            _ => None,
        }
    }

    /// The basic row of a menu row (the advanced rows 4..6 are Follow / Tripod / Free) and whether it is advanced.
    pub fn basic_row(row: usize) -> (usize, bool) {
        match row {
            4 => (0, true),
            5 => (1, true),
            6 => (3, true),
            r => (r, false),
        }
    }

    /// Retail's camera type (`82508CE8`: 0 Follow, 1 Tripod, 2 Free, 3 Game), the timeline band's colour index.
    pub fn kind(&self) -> u8 {
        match self {
            CameraKey::Follow { .. } => 0,
            CameraKey::Tripod { .. } => 1,
            CameraKey::Free { .. } => 2,
            CameraKey::Game => 3,
        }
    }

    /// Between two keys of the same camera (`u` 0..1): the parameters blended (OURS: linear for every camera; retail's
    /// Free camera eye is cubic, `curve`); None across different cameras (a cut).
    pub fn lerp(&self, other: &CameraKey, u: f32) -> Option<CameraKey> {
        let l = |a: f32, b: f32| a + (b - a) * u;
        Some(match (*self, *other) {
            (CameraKey::Game, CameraKey::Game) => CameraKey::Game,
            (CameraKey::Follow { eye, look_height }, CameraKey::Follow { eye: e2, look_height: h2 }) => CameraKey::Follow { eye: eye.lerp(e2, u), look_height: l(look_height, h2) },
            (CameraKey::Tripod { eye, look_height }, CameraKey::Tripod { eye: e2, look_height: h2 }) => CameraKey::Tripod { eye: eye.lerp(e2, u), look_height: l(look_height, h2) },
            (CameraKey::Free { eye, yaw, pitch, distance }, CameraKey::Free { eye: e2, yaw: y2, pitch: p2, distance: d2 }) => CameraKey::Free { eye: eye.lerp(e2, u), yaw: l(yaw, y2), pitch: l(pitch, p2), distance: l(distance, d2) },
            _ => return None,
        })
    }
}

/// Limits and per-console-frame rates (b402, b549), data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CameraRules {
    /// Orbit: degrees per stick unit and the per-frame cap; pitch limit; distance cap per frame and range.
    pub orbit_deg_per_unit: f32,
    pub orbit_deg_cap: f32,
    pub pitch_limit: f32,
    /// Distance and free moves are the stick clamped to the step, unscaled (b549: full speed at 20 % stick).
    pub distance_step: f32,
    pub distance_range: (f32, f32),
    /// Orbit look height: per frame and range.
    pub look_step: f32,
    pub look_range: (f32, f32),
    /// Free: degrees per stick unit and turn cap, move cap per axis (m), height step / velocity cap / decay per
    /// frame and the height offset range (b549 `82610F18`).
    pub free_deg_per_unit: f32,
    pub free_deg_cap: f32,
    pub free_move: f32,
    pub free_height_step: f32,
    pub free_height_cap: f32,
    pub free_height_decay: f32,
    pub free_height_range: f32,
    /// Lens (mm): per frame step and the editor's clamp, default.
    pub lens_step: f32,
    pub lens_range: (f32, f32),
    pub lens_default: f32,
    pub console_hz: f32,
    /// Height over the skater root the Follow / Tripod cameras look at, m (data; the key's own D-pad lift adds on top).
    /// Retail 0: its look object 4 is the skater root at foot / deck level and a new key's lift starts at 0 (b406:
    /// `826824F8`, recomp trace CAMTGT; `82C7FFB8` resets LOOK_Z).
    pub look_target_height: f32,
    /// A pick of another camera on a clip with no keys gives the whole clip that camera (retail `82614148` replace,
    /// b603 / b604; `sdk.replay.set_camera{fill_section}`).
    pub fill_section: bool,
}

impl Default for CameraRules {
    fn default() -> Self {
        Self {
            orbit_deg_per_unit: 2.5,
            orbit_deg_cap: 3.0,
            pitch_limit: 85.0,
            distance_step: 0.2,
            distance_range: (0.5, 1000.0),
            look_step: 0.01,
            look_range: (-1.0, 1.0),
            free_deg_per_unit: 5.0,
            free_deg_cap: 1.6,
            free_move: 0.2,
            free_height_step: 0.015,
            free_height_cap: 0.2,
            free_height_decay: 0.6,
            free_height_range: 10.0,
            lens_step: 0.5,
            lens_range: (24.0, 75.0),
            lens_default: 24.0,
            console_hz: 30.0,
            look_target_height: 0.0,
            fill_section: true,
        }
    }
}

/// The pad input of one frame for the key edit modes (the screen's prompts): right stick rotates, left stick y =
/// distance (orbit) / move (free, both axes), d-pad up / down = height, left / right (the arrows) = lens.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CameraInput {
    pub turn: Vec2,
    pub move_: Vec2,
    pub height: f32,
    pub zoom: f32,
}

/// The free camera's eased height (retail S+0x7C) and the offset it has added since the edit began; the Follow / Tripod
/// look lift since the pick (retail C+0x38, `82C80EA0`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct FreeHeight {
    pub velocity: f32,
    pub offset: f32,
    pub lift: f32,
}

/// Retail `82C80EA0`: the lift since the pick is clamped to `look_range`; the key's look height moves only while the
/// lift stays inside it: docs/hails-additions/37-instant-replay.md#look-lift-since-the-pick-2026-10-29-research-b655
fn lift_look(look_height: &mut f32, lift: &mut f32, step: f32, range: (f32, f32)) {
    let next = *lift + step;
    *lift = next.clamp(range.0, range.1);
    if next < range.1 && next > range.0 && step != 0.0 {
        *look_height += step;
    }
}

/// Which frame a Follow key's eye rides (data, `sdk.replay.set_camera{follow_eye}`). Retail: its eye object 2 is the
/// gameplay camera (m346) made upright (m350, doc 37 "The Follow eye frame is upright"); `CameraFull` keeps the
/// camera's pitch and roll; `Skater` is the skater root, ours before 2026-10-25.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum FollowEye {
    #[default]
    Camera,
    CameraFull,
    Skater,
}

/// The camera's side axis flattened onto the ground, up = world up, at the camera's position.
fn upright(t: Transform) -> Transform {
    let side = t.rotation * Vec3::X;
    let flat = Vec3::new(side.x, 0.0, side.z);
    if flat.length_squared() < 1e-8 {
        return t;
    }
    let x = flat.normalize();
    t.with_rotation(Quat::from_mat3(&Mat3::from_cols(x, Vec3::Y, x.cross(Vec3::Y))))
}

/// The frames the keys ride at the cursor: the Follow eye frame and the skater root (retail's look object 4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Anchors {
    pub eye: Transform,
    pub root: Transform,
}

impl Anchors {
    /// `camera` is the recorded gameplay camera at the cursor.
    pub fn new(camera: Transform, root: Transform, follow_eye: FollowEye) -> Self {
        let eye = match follow_eye {
            FollowEye::Camera => upright(camera),
            FollowEye::CameraFull => camera,
            FollowEye::Skater => root,
        };
        Self { eye, root }
    }

    fn to_eye_frame(&self, p: Vec3) -> Vec3 {
        self.eye.rotation.inverse() * (p - self.eye.translation)
    }

    pub(crate) fn look(&self, look_height: f32) -> Vec3 {
        self.root.translation + Vec3::Y * (CameraRules::default().look_target_height + look_height)
    }
}

/// A new key of `row`'s camera from the current view (`view`). A Free key's look distance reaches the skater root
/// (OURS: retail takes the Game camera's look object 3, actor slot 2, not identified yet; b652).
pub(crate) fn key_from_view(row: usize, view: &Transform, anchors: &Anchors) -> Option<CameraKey> {
    free_from(row, view, anchors, anchors.root.translation)
}

fn free_from(row: usize, view: &Transform, anchors: &Anchors, look: Vec3) -> Option<CameraKey> {
    let (yaw, pitch, _) = view.rotation.to_euler(EulerRot::YXZ);
    Some(match row {
        0 => CameraKey::Follow { eye: anchors.to_eye_frame(view.translation), look_height: 0.0 },
        1 => CameraKey::Tripod { eye: view.translation, look_height: 0.0 },
        2 => CameraKey::Game,
        3 => CameraKey::Free { eye: view.translation, yaw: yaw.to_degrees(), pitch: pitch.to_degrees(), distance: view.translation.distance(look) },
        _ => return None,
    })
}

/// A Free key's world look point: the eye plus the view forward times the look distance (`82C82E40`, b652).
pub(crate) fn free_look(eye: Vec3, yaw: f32, pitch: f32, distance: f32) -> Vec3 {
    eye + Quat::from_euler(EulerRot::YXZ, yaw.to_radians(), pitch.to_radians(), 0.0) * Vec3::NEG_Z * distance
}

/// A section's start key re-expressed as `row`'s camera when a pick replaces the section (b610: the stored eye / look
/// go old space -> world -> new space with the frames at the playhead; the look height stays between Follow and
/// Tripod; a Free source's look point P sets the look height, its lateral part is dropped (b613 / b652); a Free key
/// takes the old look point's distance). None for a Game source (no stored pose: the caller keys the view).
pub(crate) fn convert_key(old: &CameraKey, row: usize, anchors: &Anchors) -> Option<CameraKey> {
    if old.row() == row {
        return Some(*old);
    }
    let world = |eye: Vec3| anchors.eye.transform_point(eye);
    Some(match (row, *old) {
        (_, CameraKey::Game) => return None,
        (2, _) => CameraKey::Game,
        (3, CameraKey::Follow { look_height, .. } | CameraKey::Tripod { look_height, .. }) => {
            return free_from(3, &pose(old, anchors)?, anchors, anchors.look(look_height));
        }
        (0, CameraKey::Tripod { eye, look_height }) => CameraKey::Follow { eye: anchors.to_eye_frame(eye), look_height },
        // Retail keeps P in the Follow eye frame and re-applies it at the skater root (b613).
        (0, CameraKey::Free { eye, yaw, pitch, distance }) => CameraKey::Follow {
            eye: anchors.to_eye_frame(eye),
            look_height: anchors.to_eye_frame(free_look(eye, yaw, pitch, distance)).y - CameraRules::default().look_target_height,
        },
        (1, CameraKey::Follow { eye, look_height }) => CameraKey::Tripod { eye: world(eye), look_height },
        (1, CameraKey::Free { eye, yaw, pitch, distance }) => CameraKey::Tripod {
            eye,
            // Measured along the skater's tilted up, applied along world up each frame (b659; doc 37).
            look_height: (anchors.root.rotation.inverse() * (free_look(eye, yaw, pitch, distance) - anchors.root.translation)).y - CameraRules::default().look_target_height,
        },
        _ => return None,
    })
}

/// One console frame of the key edit controls, scaled by `dt` x the console rate (retail steps per frame, no dt).
/// `anchors` are the frames at the cursor: Follow and Tripod orbit their eye around the look point (b550).
pub(crate) fn edit(key: &mut CameraKey, lens: &mut f32, height: &mut FreeHeight, input: CameraInput, rules: &CameraRules,
                   anchors: Option<&Anchors>, dt: f32) {
    let frames = dt * rules.console_hz;
    let cap = |v: f32, c: f32| v.clamp(-c, c);
    *lens = (*lens + input.zoom * rules.lens_step * frames).clamp(rules.lens_range.0, rules.lens_range.1);
    // `82C82E40` with the pivot byte set (Follow and Tripod): the look point is the pivot, eye = look + v(yaw, pitch,
    // distance) in world angles; the lift moves the look point only (`82C80EA0` re-derives v).
    let orbit = |eye: Vec3, look: Vec3| {
        let v = eye - look;
        let distance = v.length().max(1e-3);
        let yaw = v.x.atan2(v.z).to_degrees() + cap(input.turn.x * rules.orbit_deg_per_unit, rules.orbit_deg_cap) * frames;
        let pitch = ((v.y / distance).clamp(-1.0, 1.0).asin().to_degrees()
            + cap(input.turn.y * rules.orbit_deg_per_unit, rules.orbit_deg_cap) * frames).clamp(-rules.pitch_limit, rules.pitch_limit);
        let distance = (distance - cap(input.move_.y, rules.distance_step) * frames).clamp(rules.distance_range.0, rules.distance_range.1);
        let (yaw, pitch) = (yaw.to_radians(), pitch.to_radians());
        look + Vec3::new(yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos()) * distance
    };
    match key {
        CameraKey::Follow { eye, look_height } => {
            if let Some(a) = anchors {
                *eye = a.to_eye_frame(orbit(a.eye.transform_point(*eye), a.look(*look_height)));
            }
            lift_look(look_height, &mut height.lift, input.height * rules.look_step * frames, rules.look_range);
        }
        CameraKey::Tripod { eye, look_height } => {
            if let Some(a) = anchors {
                *eye = orbit(*eye, a.look(*look_height));
            }
            lift_look(look_height, &mut height.lift, input.height * rules.look_step * frames, rules.look_range);
        }
        CameraKey::Free { eye, yaw, pitch, .. } => {
            *yaw -= cap(input.turn.x * rules.free_deg_per_unit, rules.free_deg_cap) * frames;
            *pitch = (*pitch + cap(input.turn.y * rules.free_deg_per_unit, rules.free_deg_cap) * frames)
                .clamp(-rules.pitch_limit, rules.pitch_limit);
            let heading = Quat::from_rotation_y(yaw.to_radians());
            let step = heading * Vec3::new(cap(input.move_.x, rules.free_move), 0.0, -cap(input.move_.y, rules.free_move)) * frames;
            // Height eases: the velocity gains a step, is applied, then decays (per console frame).
            height.velocity = cap(height.velocity + input.height * rules.free_height_step * frames, rules.free_height_cap);
            let offset = (height.offset + height.velocity * frames).clamp(-rules.free_height_range, rules.free_height_range);
            let rise = offset - height.offset;
            height.offset = offset;
            height.velocity *= rules.free_height_decay.powf(frames);
            *eye += step + Vec3::Y * rise;
        }
        CameraKey::Game => {}
    }
}

/// The camera pose of `key` at `anchors` (None: the recorded camera), looking at the root raised by the rules' look
/// target height.
pub(crate) fn pose(key: &CameraKey, anchors: &Anchors) -> Option<Transform> {
    let look_at = |eye: Vec3, look: Vec3| Transform::from_translation(eye).looking_at(look, Vec3::Y);
    match *key {
        CameraKey::Game => None,
        CameraKey::Follow { eye, look_height } => Some(look_at(anchors.eye.transform_point(eye), anchors.look(look_height))),
        CameraKey::Tripod { eye, look_height } => Some(look_at(eye, anchors.look(look_height))),
        CameraKey::Free { eye, yaw, pitch, .. } => Some(Transform::from_translation(eye).with_rotation(Quat::from_euler(
            EulerRot::YXZ,
            yaw.to_radians(),
            pitch.to_radians(),
            0.0,
        ))),
    }
}

/// Moves `key`'s eye to `eye` (the constraint's result written back into the key, `82683450` -> `82C82E40`, b593):
/// Tripod and Free store it, Follow in its eye frame; Game has no eye.
pub(crate) fn set_eye(key: &mut CameraKey, eye: Vec3, anchors: &Anchors) {
    match key {
        CameraKey::Tripod { eye: e, .. } | CameraKey::Free { eye: e, .. } => *e = eye,
        CameraKey::Follow { eye: e, .. } => *e = anchors.to_eye_frame(eye),
        CameraKey::Game => {}
    }
}

/// Vertical field of view (radians) of a lens in mm. NOT RETAIL YET: a 24 mm high gate (retail's mm -> FOV is open).
pub(crate) fn lens_fov(mm: f32) -> f32 {
    2.0 * (12.0 / mm.max(1.0)).atan()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(root: Transform) -> Anchors {
        Anchors::new(Transform::from_xyz(0.0, 3.0, 8.0), root, FollowEye::Camera)
    }

    /// m350: retail's Follow eye frame is the recorded camera made upright (heading only, up = world up), so the
    /// camera pitching or rolling does not swing a low eye into the ground; `CameraFull` keeps the old full frame.
    #[test]
    fn follow_eye_frame_is_the_camera_heading_only() {
        let root = Transform::from_xyz(10.0, 0.0, 0.0);
        let level = Transform::from_xyz(10.0, 1.5, 6.0).with_rotation(Quat::from_rotation_y(0.4));
        let tilted = level.with_rotation(level.rotation * Quat::from_rotation_x(-0.136) * Quat::from_rotation_z(0.05));
        let anchors = Anchors::new(level, root, FollowEye::Camera);
        let pitched = level.with_rotation(level.rotation * Quat::from_rotation_x(-0.136));
        assert!(Anchors::new(pitched, root, FollowEye::Camera).eye.rotation.angle_between(anchors.eye.rotation) < 1e-3);
        // Roll tips the flattened side axis a little (retail flattens the same row).
        assert!(Anchors::new(tilted, root, FollowEye::Camera).eye.rotation.angle_between(anchors.eye.rotation) < 0.01);
        let view = Transform::from_xyz(11.0, 0.3, 5.0);
        let key = key_from_view(0, &view, &anchors).unwrap();
        let low = pose(&key, &Anchors::new(tilted, root, FollowEye::Camera)).unwrap().translation;
        assert!(low.distance(view.translation) < 0.02, "pitch and roll leave the eye where it was placed");
        let full = pose(&key, &Anchors::new(tilted, root, FollowEye::CameraFull)).unwrap().translation;
        assert!(full.distance(view.translation) > 0.05, "the full frame swings with the camera");
    }

    /// m346: a Follow key's eye rides the recorded gameplay camera (retail's eye object 2), so the skater pitching on a
    /// landing does not swing it into the ground; the sticks orbit it around the look point with the retail caps.
    #[test]
    fn follow_eye_rides_the_gameplay_camera_and_orbits_the_look_point() {
        let root = Transform::from_xyz(10.0, 0.0, 0.0);
        let camera = Transform::from_xyz(10.0, 1.0, 4.0).looking_at(root.translation, Vec3::Y);
        let anchors = Anchors::new(camera, root, FollowEye::Camera);
        let view = Transform::from_xyz(10.0, 0.5, 4.0);
        let mut key = key_from_view(0, &view, &anchors).unwrap();
        assert!(pose(&key, &anchors).unwrap().translation.distance(view.translation) < 1e-4);
        let pitched = Anchors::new(camera, root.with_rotation(Quat::from_rotation_x(-0.6)), FollowEye::Camera);
        assert!(pose(&key, &pitched).unwrap().translation.distance(view.translation) < 1e-4, "the skater's pitch moves the eye");
        let moved = Anchors::new(Transform::from_xyz(0.0, 2.0, 0.0) * camera, root, FollowEye::Camera);
        assert!((pose(&key, &moved).unwrap().translation - view.translation - Vec3::Y * 2.0).length() < 1e-4);
        let skater = Anchors::new(camera, root.with_rotation(Quat::from_rotation_x(-0.6)), FollowEye::Skater);
        let on_skater = key_from_view(0, &view, &Anchors::new(camera, root, FollowEye::Skater)).unwrap();
        assert!(pose(&on_skater, &skater).unwrap().translation.distance(view.translation) > 0.5, "the skater frame turns it");
        // A full stick for one second: capped at 3 degrees per console frame -> a quarter turn about the look point.
        let (mut lens, rules) = (24.0, CameraRules::default());
        let look = anchors.look(0.0);
        let before = pose(&key, &anchors).unwrap().translation - look;
        for _ in 0..30 {
            edit(&mut key, &mut lens, &mut FreeHeight::default(), CameraInput { turn: Vec2::new(10.0, 0.0), ..default() }, &rules, Some(&anchors), 1.0 / 30.0);
        }
        let after = pose(&key, &anchors).unwrap().translation - look;
        let turn = Vec2::new(before.x, before.z).angle_to(Vec2::new(after.x, after.z)).to_degrees().abs();
        assert!((turn - 90.0).abs() < 1e-2 && (after.length() - before.length()).abs() < 1e-3, "{turn} {before} {after}");
        // Pitch stops at 85, the lens at 75 mm.
        for _ in 0..120 {
            edit(&mut key, &mut lens, &mut FreeHeight::default(), CameraInput { turn: Vec2::new(0.0, 1.0), zoom: 1.0, ..default() }, &rules, Some(&anchors), 1.0 / 30.0);
        }
        let v = pose(&key, &anchors).unwrap().translation - look;
        assert!(((v.y / v.length()).asin().to_degrees() - 85.0).abs() < 1e-2 && lens == 75.0, "{v}");
        assert_eq!(CameraKey::edit_mode_and_state(2), Some((9, 10)));
        assert!(pose(&CameraKey::Game, &anchors).is_none());
    }

    /// b549: free turn clamp(5 s, 1.6) deg, moves clamp(s, 0.2) m per axis, height eases to 0.0375 m per frame.
    #[test]
    fn free_camera_rates_follow_the_editor_constants() {
        let rules = CameraRules::default();
        let (mut lens, mut height) = (24.0, FreeHeight::default());
        let mut key = CameraKey::Free { eye: Vec3::ZERO, yaw: 0.0, pitch: 0.0, distance: 4.0 };
        let input = CameraInput { turn: Vec2::new(0.32, 0.0), move_: Vec2::new(0.2, 0.5), height: 1.0, zoom: 0.0 };
        for _ in 0..60 {
            edit(&mut key, &mut lens, &mut height, input, &rules, None, 1.0 / 30.0);
        }
        let CameraKey::Free { eye, yaw, .. } = key else { panic!() };
        // 32 % stick already turns at the 1.6 degree cap.
        assert!((yaw + 96.0).abs() < 1e-3, "{yaw}");
        assert!((height.velocity * (1.0 - rules.free_height_decay) / rules.free_height_decay - 0.015).abs() < 1e-4, "{height:?}");
        let per_frame = height.velocity / rules.free_height_decay;
        assert!((per_frame - 0.0375).abs() < 1e-4, "{per_frame}");
        assert!(eye.y > 0.0 && eye.y < 60.0 * 0.0375 + 1e-3, "{eye}");
        // Each axis moves at most 0.2 m per frame: 60 frames, 12 m along each before the turn spreads them.
        assert!(Vec2::new(eye.x, eye.z).length() <= 60.0 * 0.2 * 2f32.sqrt() + 1e-3);
    }

    /// b550: a Tripod key orbits its world eye around the look point; distance moves the eye along the sight line.
    #[test]
    fn tripod_orbits_its_eye_around_the_look_point() {
        let rules = CameraRules::default();
        let root = Transform::from_xyz(10.0, 0.0, 5.0);
        let look = root.translation + Vec3::Y * rules.look_target_height;
        let mut key = CameraKey::Tripod { eye: look + Vec3::new(0.0, 0.0, 4.0), look_height: 0.0 };
        let (mut lens, mut height) = (24.0, FreeHeight::default());
        for _ in 0..30 {
            edit(&mut key, &mut lens, &mut height, CameraInput { turn: Vec2::new(10.0, 0.0), ..default() }, &rules, Some(&at(root)), 1.0 / 30.0);
        }
        let CameraKey::Tripod { eye, .. } = key else { panic!() };
        // 3 degrees per frame for 30 frames: a quarter turn at the same 4 m.
        assert!((eye - (look + Vec3::new(4.0, 0.0, 0.0))).length() < 1e-3, "{eye}");
        for _ in 0..5 {
            edit(&mut key, &mut lens, &mut height, CameraInput { move_: Vec2::new(0.0, 1.0), ..default() }, &rules, Some(&at(root)), 1.0 / 30.0);
        }
        let CameraKey::Tripod { eye, .. } = key else { panic!() };
        assert!((eye.distance(look) - 3.0).abs() < 1e-3, "{eye}");
    }

    #[test]
    fn set_eye_round_trips_a_follow_eye() {
        let anchors = at(Transform::from_xyz(3.0, 1.0, -2.0).with_rotation(Quat::from_rotation_y(0.7)));
        let mut key = CameraKey::Follow { eye: Vec3::new(1.0, 0.5, -2.0), look_height: 0.5 };
        let eye = pose(&key, &anchors).unwrap().translation;
        set_eye(&mut key, eye, &anchors);
        let CameraKey::Follow { eye: local, .. } = key else { panic!() };
        assert!(local.distance(Vec3::new(1.0, 0.5, -2.0)) < 1e-4, "{key:?}");
    }

    /// b652: Free keeps its look distance through edits, takes it from the old look point on a conversion, and its
    /// look point P sets the Follow / Tripod look height.
    #[test]
    fn free_look_distance_follows_retail() {
        let a = at(Transform::from_xyz(0.0, 0.0, 0.0));
        let tripod = CameraKey::Tripod { eye: Vec3::new(0.0, 2.0, 6.0), look_height: 0.5 };
        let Some(CameraKey::Free { eye, yaw, pitch, distance }) = convert_key(&tripod, 3, &a) else { panic!() };
        assert!((distance - eye.distance(a.look(0.5))).abs() < 1e-4);
        assert!(free_look(eye, yaw, pitch, distance).distance(a.look(0.5)) < 1e-3, "P is the old look point");
        let mut key = CameraKey::Free { eye, yaw, pitch, distance };
        let input = CameraInput { turn: Vec2::new(1.0, 0.5), move_: Vec2::new(0.3, -0.2), height: 1.0, zoom: 0.0 };
        edit(&mut key, &mut 24.0, &mut FreeHeight::default(), input, &CameraRules::default(), Some(&a), 1.0 / 30.0);
        let CameraKey::Free { distance: kept, .. } = key else { panic!() };
        assert_eq!(kept, distance, "edits keep D");
        let free = CameraKey::Free { eye: Vec3::new(0.0, 3.0, 5.0), yaw: 0.0, pitch: -10.0, distance: 5.0 };
        let p = free_look(Vec3::new(0.0, 3.0, 5.0), 0.0, -10.0, 5.0);
        let Some(CameraKey::Tripod { look_height, .. }) = convert_key(&free, 1, &a) else { panic!() };
        assert!((look_height - (p.y - a.root.translation.y)).abs() < 1e-4);
        let Some(CameraKey::Follow { look_height, .. }) = convert_key(&free, 0, &a) else { panic!() };
        assert!((look_height - a.to_eye_frame(p).y).abs() < 1e-4);
        let Some(CameraKey::Free { distance, .. }) = key_from_view(3, &Transform::from_xyz(3.0, 0.0, 4.0), &a) else { panic!() };
        assert!((distance - 5.0).abs() < 1e-4, "a view key reaches the skater root (ours)");
    }

    /// b659: a Tripod pick measures the look height along the tilted skater's up; the aim stays on world up.
    #[test]
    fn tripod_pick_measures_height_along_the_tilted_skater() {
        let root = Transform::from_xyz(1.0, 2.0, 3.0).with_rotation(Quat::from_rotation_x(0.6));
        let a = at(root);
        let free = CameraKey::Free { eye: Vec3::new(1.0, 5.0, 9.0), yaw: 0.0, pitch: -20.0, distance: 6.0 };
        let p = free_look(Vec3::new(1.0, 5.0, 9.0), 0.0, -20.0, 6.0);
        let Some(CameraKey::Tripod { look_height, .. }) = convert_key(&free, 1, &a) else { panic!() };
        let along = (root.rotation.inverse() * (p - root.translation)).y - CameraRules::default().look_target_height;
        assert!((look_height - along).abs() < 1e-4);
        assert!((look_height - (p.y - root.translation.y - CameraRules::default().look_target_height)).abs() > 1e-2, "not world Y");
        let aim = a.look(look_height) - root.translation;
        assert!(aim.x.abs() < 1e-6 && aim.z.abs() < 1e-6, "aim along world up");
    }

    /// b655: the D-pad lift is limited since the pick (retail `82C80EA0`), not the key's absolute look height.
    #[test]
    fn look_lift_is_limited_since_the_pick_not_absolutely() {
        let a = at(Transform::from_xyz(0.0, 0.0, 0.0));
        let rules = CameraRules::default();
        let mut key = CameraKey::Tripod { eye: Vec3::new(0.0, 2.0, 6.0), look_height: 1.8 };
        let mut height = FreeHeight::default();
        let up = CameraInput { height: 1.0, ..default() };
        edit(&mut key, &mut 24.0, &mut height, up, &rules, Some(&a), 1.0 / 30.0);
        let CameraKey::Tripod { look_height, .. } = key else { panic!() };
        assert!((look_height - 1.81).abs() < 1e-5, "a converted height above 1 m survives an edit: {look_height}");
        for _ in 0..300 {
            edit(&mut key, &mut 24.0, &mut height, up, &rules, Some(&a), 1.0 / 30.0);
        }
        let CameraKey::Tripod { look_height, .. } = key else { panic!() };
        assert!(look_height < 1.8 + 1.0 && look_height > 1.8 + 0.98, "the lift stops at 1 m since the pick: {look_height}");
        assert_eq!(height.lift, 1.0);
        edit(&mut key, &mut 24.0, &mut height, CameraInput { height: -1.0, ..default() }, &rules, Some(&a), 1.0 / 30.0);
        let CameraKey::Tripod { look_height: down, .. } = key else { panic!() };
        assert!((down - (look_height - 0.01)).abs() < 1e-5, "back down from the bound");
    }
}
