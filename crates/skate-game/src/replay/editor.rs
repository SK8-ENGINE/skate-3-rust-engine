//! The replay editor on the recording (FE 12 timeline, mode 6, and its sub modes; research b386 / b387): one clip of
//! skate-core's model over the recording, in retail's 30 fps timeline frames, with the session's mode. A from the
//! instant replay opens it (`82613988`: leave instant, mode 6, SetState 21, 9).
use skate_core::replay_edit::{
    session::{EditorMode, EditorSession},
    Commit, EditError, EditLimits, Lane, ReplayEdit,
};

/// The row of the game camera (key type 3 -> row 2 in `8250A8E8`, b398), the clip's camera until another is picked.
pub(crate) const GAME_CAMERA_ROW: usize = 2;
/// The effects menu rows without the DLC (research b398, input `82612038`, item table `8302CCE0`): rows 0..6 pick the
/// key's camera (A: 0..3 the basic cameras, `camera_key`; the advanced 4..6 are not ported), row 7 speed and row 8
/// shake take Left / Right (`8250ADF8` / `8250AB28`). Data: a mod may reorder or extend it.
pub(crate) const EFFECT_ROWS: [Option<Lane>; 13] = [
    None, None, None, None, None, None, None,
    Some(Lane::Speed), Some(Lane::Shake),
    // The Create pack's look items (b410 `8250B218`: 9 brightness, 10 contrast, 11 saturation, 12 tint; the tint's
    // lane is the component under the tint cursor).
    Some(Lane::Brightness), Some(Lane::Contrast), Some(Lane::Saturation), Some(Lane::TintR),
];

/// Rows the base screen (replay_editor2) cycles: the cameras, speed and shake.
pub(crate) const BASE_ROWS: usize = 9;

/// The Create pack's edit channels (Y cycles them, b405 / b410 / b411): the label string ids (`GetModeText`) and the
/// effects menu's items per channel. Data: a mod may relabel or reorder them.
pub(crate) const CHANNELS: [(&str, &[usize]); 7] = [
    ("ID_REPLAYEDITOR_CHANNEL_CAMERA", &[0, 1, 2, 3, 4, 5, 6]),
    ("ID_REPLAYEDITOR_CHANNEL_SPEED", &[7]),
    ("ID_REPLAYEDITOR_CHANNEL_SHAKE", &[8]),
    ("ID_REPLAYEDITOR_CHANNEL_BRIGHTNESS", &[9]),
    ("ID_REPLAYEDITOR_CHANNEL_CONTRAST", &[10]),
    ("ID_REPLAYEDITOR_CHANNEL_SATURATE", &[11]),
    ("ID_REPLAYEDITOR_CHANNEL_TINT", &[12]),
];
const TINT_LANES: [Lane; 4] = [Lane::TintR, Lane::TintG, Lane::TintB, Lane::TintA];
/// Each channel's lanes (the camera channel's keys are the camera keys).
const CHANNEL_LANES: [&[Lane]; 7] = [&[], &[Lane::Speed], &[Lane::Shake], &[Lane::Brightness], &[Lane::Contrast], &[Lane::Saturation], &TINT_LANES];

pub(crate) struct Editor {
    pub session: EditorSession,
    pub edit: ReplayEdit,
    pub clip: usize,
    /// The effects menu's row (`GetEditIndex`).
    pub row: usize,
    /// The Create pack's edit channel (`GetMode`, 0..6) and tint component cursor (`GetCurrentTint`, 0 R .. 3 A).
    pub channel: usize,
    pub tint: usize,
    /// The effects menu's live values per lane (retail E+0x19C speed, E+0x1B8 shake, the look fields; b461 / b462):
    /// loaded from the take when the menu opens, stepped by Left / Right, written only by a commit.
    pub live: [i32; 9],
    /// Recording time of timeline frame 0, and the timeline's frames per second.
    base: f64,
    fps: f64,
    /// The recording's length in timeline frames, and the trim moves' fractional frames (start, end).
    frames: u32,
    trim_carry: [f64; 2],
    trimming_end: bool,
    /// The camera keys (sorted by frame; none = the recorded camera), the key being edited, and the camera limits /
    /// rates (data).
    pub cameras: Vec<CamKey>,
    pub editing: Option<usize>,
    pub camera_rules: super::camera_key::CameraRules,
    /// The free camera's eased height for the key being edited (reset when another key is edited).
    free_height: (Option<usize>, super::camera_key::FreeHeight),
    /// The edited key's last accepted eye for the constraint; None = snap (an edit just opened).
    edit_last: Option<bevy::prelude::Vec3>,
    /// Seconds since the last `REPLAY_CAMERA edit` line while the sticks move a key.
    edit_log: f32,
}

/// One camera key for a `REPLAY_CAMERA` line: its camera, its pose numbers (cm) and lens.
pub(crate) fn log_key(camera: &super::camera_key::CameraKey, lens: f32) -> String {
    use super::camera_key::CameraKey;
    let v = |p: bevy::prelude::Vec3| format!("[{:.2}, {:.2}, {:.2}]", p.x, p.y, p.z);
    match *camera {
        CameraKey::Game => format!("game lens={lens}"),
        CameraKey::Follow { eye, look_height } => format!("follow eye={:?} look={look_height:.2} lens={lens}", eye.to_array()),
        CameraKey::Tripod { eye, look_height } => format!("tripod eye={} look={look_height:.2} lens={lens}", v(eye)),
        CameraKey::Free { eye, yaw, pitch, distance } => format!("free eye={} yaw={yaw:.1} pitch={pitch:.1} distance={distance:.2} lens={lens}", v(eye)),
    }
}

/// The two marker kinds retail's delete asks about (b407, `82508D88`): 1 a clip marker (a hard key), 2 a key marker
/// (a soft key).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MarkerKind {
    Clip,
    Key,
}

/// Where A in the timeline goes (b462 / b463).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ATarget {
    /// The effects menu, the cursor snapped to a soft lane key first (Some).
    Effects(Option<f64>),
    /// Re-edit the soft camera key at the cursor.
    CameraKey,
    /// `core_error`, nothing else (a hard key, or outside the marker window).
    Refused,
}

/// One camera key on the timeline (b405): inserted at the playhead when a camera row is picked; a basic camera's key is
/// hard (the pose holds and cuts at the next key), an advanced camera's soft (blends into the next soft key).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CamKey {
    pub frame: u32,
    pub camera: super::camera_key::CameraKey,
    pub lens: f32,
    pub soft: bool,
}

/// Retail's camera key cap (b381 / b405: 100 markers, `82613500`).
pub(crate) const MAX_CAMERA_KEYS: usize = 100;

/// Retail's trim rate: at full stick, 0.005 of the timeline per console frame (the scrub filter's clamp, `82610F18`
/// mode 2, b387), at the console's 30 frames per second.
pub(crate) const TRIM_RATE: f64 = 0.005;
pub(crate) const CONSOLE_HZ: f64 = 30.0;

impl Editor {
    /// The editor over the recording `start..end` (s): None while it holds fewer than the limits' frames (retail
    /// 150, `8250B8F8`).
    pub fn open(start: f64, end: f64, limits: EditLimits) -> Option<Self> {
        let fps = f64::from(limits.timeline_fps);
        let frames = ((end - start).max(0.0) * fps).floor() as u32;
        let mut edit = ReplayEdit::new(limits);
        if !edit.can_open(frames) {
            return None;
        }
        let clip = edit.add_clip(0, frames).ok()?;
        let mut session = EditorSession::instant();
        session.mode = EditorMode::Timeline;
        let live = edit.limits.lanes.map(|s| s.default);
        Some(Self { session, edit, clip, row: 0, channel: 0, tint: 0, live, base: start, fps, frames, trim_carry: [0.0; 2], trimming_end: false, cameras: Vec::new(), editing: None, camera_rules: Default::default(), free_height: Default::default(), edit_last: None, edit_log: 0.0 })
    }

    pub fn frame(&self, time: f64) -> u32 {
        ((time - self.base) * self.fps).round().max(0.0) as u32
    }

    pub fn time(&self, frame: u32) -> f64 {
        self.base + f64::from(frame) / self.fps
    }

    /// The clip's in / out points as recording times.
    pub fn heads(&self) -> (f64, f64) {
        let c = &self.edit.clips[self.clip];
        (self.time(c.start), self.time(c.end))
    }

    /// Playback rate at `time` (the speed lane: 1.0 = real time; retail slows only, 10 % steps).
    /// The clip's look at `time` over the world grade `base` (the look lanes, b408), or None outside a clip.
    pub fn look(&self, time: f64, base: &skate_core::colour_matrix::ColourMatrixRecord) -> Option<skate_core::replay_edit::ReplayLook> {
        self.edit.look_at(self.clip, self.frame(time), base)
    }

    pub fn speed(&self, time: f64) -> f64 {
        f64::from(self.edit.lane_effect(self.clip, Lane::Speed, self.frame(time)).unwrap_or(1.0))
    }

    /// Move the in / out points by whole timeline frames (LB held: the sticks, `82610F18` mode 2). A move that
    /// would empty the clip or leave the recording stops at the limit.
    pub fn trim_by(&mut self, start: i64, end: i64, frames: u32) {
        let c = &self.edit.clips[self.clip];
        let s = (i64::from(c.start) + start).clamp(0, i64::from(frames)) as u32;
        let e = (i64::from(c.end) + end).clamp(0, i64::from(frames)) as u32;
        if e > s {
            let _ = self.edit.trim(self.clip, s, e);
        }
    }

    /// LB held (mode 2): the left stick's x moves the in point, the right stick's x the out point.
    pub fn trim_sticks(&mut self, left: f32, right: f32, dt: f32) {
        let per_second = TRIM_RATE * f64::from(self.frames) * CONSOLE_HZ;
        let mut whole = [0i64; 2];
        for (i, axis) in [left, right].into_iter().enumerate() {
            self.trim_carry[i] += f64::from(axis) * per_second * f64::from(dt);
            whole[i] = self.trim_carry[i].trunc() as i64;
            self.trim_carry[i] -= whole[i] as f64;
        }
        let frames = self.frames;
        self.trim_by(whole[0], whole[1], frames);
        if whole[0] != 0 {
            self.trimming_end = false;
        } else if whole[1] != 0 {
            self.trimming_end = true;
        }
    }

    /// Where the playhead sits while trimming: on the head that moved last (the movie draws the head at the
    /// playhead, `GetCursorOffset`, b398).
    pub fn trim_cursor(&self) -> f64 {
        let (start, end) = self.heads();
        if self.trimming_end { end } else { start }
    }

    /// The camera keys as fractions of the recording (the timeline strip's markers, x / 817, b403).
    pub fn key_fractions(&self) -> Vec<f32> {
        self.cameras.iter().map(|k| k.frame as f32 / self.frames.max(1) as f32).collect()
    }

    /// The timeline band's segments: (start fraction, camera type), the recorded camera before the first key.
    pub fn segments(&self) -> Vec<(f32, u8)> {
        let mut out = Vec::new();
        if self.cameras.first().is_none_or(|k| k.frame > 0) {
            out.push((0.0, 3));
        }
        out.extend(self.cameras.iter().map(|k| (k.frame as f32 / self.frames.max(1) as f32, k.camera.kind())));
        out
    }

    /// The key at the playhead: within the marker spacing of the clip (at least one frame; `825084C8`, b382).
    pub fn key_near(&self, time: f64) -> Option<usize> {
        let frame = self.frame(time);
        let c = &self.edit.clips[self.clip];
        let near = ((self.edit.limits.marker_spacing * (c.take[1] - c.take[0]) as f32).floor() as u32).max(1);
        self.cameras.iter().position(|k| k.frame.abs_diff(frame) <= near)
    }

    /// The key in effect at `time` (the last at or before it).
    fn active(&self, time: f64) -> Option<usize> {
        let frame = self.frame(time);
        self.cameras.iter().rposition(|k| k.frame <= frame)
    }

    /// The menu row of the camera in effect (advanced rows for soft keys; the game camera without keys).
    pub fn active_row(&self, time: f64) -> usize {
        self.active(time).map_or(GAME_CAMERA_ROW, |i| {
            let k = &self.cameras[i];
            match (k.camera.row(), k.soft) {
                (0, true) => 4,
                (1, true) => 5,
                (3, true) => 6,
                (r, _) => r,
            }
        })
    }

    /// Retail's camera type of a menu row (`82508CE8` values; rows 0..6 = items 0, 1, 3, 2, 4, 5, 6, b609).
    fn pick_kind(row: usize) -> u8 {
        [0, 1, 3, 2][super::camera_key::CameraKey::basic_row(row).0.min(3)]
    }

    /// Whether a pick of `row` at `time` replaces the camera's section instead of inserting a key (`82614148`, b609):
    /// a basic row inserts only over its own camera with hard keys (or the In / Out points) on both sides, an advanced
    /// row over its own camera. Off with `CameraRules::fill_section` false (inserts only).
    pub fn pick_replaces(&self, row: usize, time: f64) -> bool {
        if !self.camera_rules.fill_section {
            return false;
        }
        let active = self.active(time);
        let same = active.map_or(3, |i| self.cameras[i].camera.kind()) == Self::pick_kind(row);
        if super::camera_key::CameraKey::basic_row(row).1 {
            return !same;
        }
        let frame = self.frame(time);
        let next_soft = self.cameras.iter().find(|k| k.frame > frame).is_some_and(|k| k.soft);
        !(same && !active.is_some_and(|i| self.cameras[i].soft) && !next_soft)
    }

    /// A replace asks first ("Change Camera", No / Yes) unless the channel is one Game section (no keys), b609.
    pub fn pick_asks(&self, row: usize, time: f64) -> bool {
        self.pick_replaces(row, time) && !self.cameras.is_empty()
    }

    /// A on a camera row (b405, `82C7FFB8`): a key of that camera at the playhead from the current view (a key already
    /// there takes the new camera), then its edit mode. A replace (b609 / b610) first clears the soft keys of the
    /// section around the playhead (between hard keys or the In / Out points) and re-expresses its start key as the
    /// new camera. None when refused (the cap) or the row is not a camera.
    pub fn pick_camera(&mut self, row: usize, time: f64, view: &bevy::prelude::Transform, anchors: &super::camera_key::Anchors) -> Option<u8> {
        let (mode, _) = super::camera_key::CameraKey::edit_mode_and_state(row)?;
        let (basic, soft) = super::camera_key::CameraKey::basic_row(row);
        let from = self.pose(time, anchors).0.unwrap_or(*view);
        let camera = super::camera_key::key_from_view(basic, &from, anchors)?;
        let lens = self.active(time).map_or(self.camera_rules.lens_default, |i| self.cameras[i].lens);
        let frame = self.frame(time);
        if self.pick_replaces(row, time) {
            if soft && self.cameras.len() >= MAX_CAMERA_KEYS {
                return None;
            }
            let c = &self.edit.clips[self.clip];
            let lo = self.cameras.iter().rposition(|k| !k.soft && k.frame <= frame);
            let lo_frame = lo.map_or(c.start, |i| self.cameras[i].frame);
            let hi_frame = self.cameras.iter().find(|k| !k.soft && k.frame > frame).map_or(c.end, |k| k.frame);
            self.cameras.retain(|k| !(k.soft && k.frame >= lo_frame && k.frame < hi_frame));
            match self.cameras.iter().rposition(|k| !k.soft && k.frame <= frame) {
                Some(i) => {
                    self.cameras[i].camera = super::camera_key::convert_key(&self.cameras[i].camera, basic, anchors).unwrap_or(camera);
                }
                // The section starts at the In point on the recorded camera: it holds the new key's pose (b603).
                None if camera != super::camera_key::CameraKey::Game && frame > c.start => {
                    let start = c.start;
                    self.cameras.insert(0, CamKey { frame: start, camera, lens, soft });
                }
                None => {}
            }
        }
        let index = match self.key_near(time) {
            Some(i) => {
                self.cameras[i].camera = camera;
                self.cameras[i].soft = soft;
                i
            }
            None if self.cameras.len() >= MAX_CAMERA_KEYS => return None,
            None => {
                let i = self.cameras.partition_point(|k| k.frame < frame);
                self.cameras.insert(i, CamKey { frame, camera, lens, soft });
                i
            }
        };
        self.editing = Some(index);
        self.edit_last = None;
        Some(mode)
    }

    /// A on a camera key in the timeline (b405, `82612C28`): that key's edit mode, None when no key is there or the
    /// key is a Game camera (retail's kind >= 3 returns without opening anything, b555).
    pub fn reopen_at(&mut self, time: f64) -> Option<u8> {
        let i = self.key_near(time)?;
        let k = &self.cameras[i];
        if matches!(k.camera, super::camera_key::CameraKey::Game) {
            return None;
        }
        let row = if k.soft { [4, 5, 2, 6][k.camera.row().min(3)] } else { k.camera.row() };
        let mode = super::camera_key::CameraKey::edit_mode_and_state(row).map(|(mode, _)| mode)?;
        self.editing = Some(i);
        self.edit_last = None;
        Some(mode)
    }

    /// The key edit controls on the key being edited. While the sticks / D-pad move it, a `REPLAY_CAMERA edit` line
    /// every half second (the key and the input; laptop 2026-10-25 asked for the pose in the log).
    pub fn edit_key(&mut self, input: super::camera_key::CameraInput, anchors: Option<&super::camera_key::Anchors>, dt: f32) {
        let rules = self.camera_rules;
        if self.free_height.0 != self.editing {
            self.free_height = (self.editing, Default::default());
        }
        if let Some(k) = self.editing.and_then(|i| self.cameras.get_mut(i)) {
            super::camera_key::edit(&mut k.camera, &mut k.lens, &mut self.free_height.1, input, &rules, anchors, dt);
        }
        let moving = input.turn.length() > 0.0 || input.move_.length() > 0.0 || input.height != 0.0 || input.zoom != 0.0;
        self.edit_log = if moving { self.edit_log + dt } else { 0.5 };
        if moving && self.edit_log >= 0.5 {
            self.edit_log = 0.0;
            if let Some(k) = self.editing.and_then(|i| self.cameras.get(i)) {
                bevy::log::info!("REPLAY_CAMERA edit {} turn={:?} move={:?} height={} zoom={} root={:?}", log_key(&k.camera, k.lens),
                    input.turn.to_array(), input.move_.to_array(), input.height, input.zoom, anchors.map(|a| a.root.translation.to_array()));
            }
        }
    }

    /// Retail's camera constraint runs only while a key edit is open (`82683680` returns unless the byte 0x83037414 is
    /// set; the edit openers `82614658`.. set it, the timeline `82612C28` clears it) and the held pose is stored in the
    /// key, so playback needs no constraint (b593). The anchor is the skater root at the key's frame.
    pub fn constrain_editing(&mut self, anchors: &super::camera_key::Anchors, rules: &super::camera_collision::CameraCollision,
        sweep: impl FnMut(bevy::prelude::Vec3, bevy::prelude::Vec3, f32) -> Option<super::camera_collision::Hit>) {
        let Some(k) = self.editing.and_then(|i| self.cameras.get_mut(i)) else { return };
        let Some(pose) = super::camera_key::pose(&k.camera, anchors) else { return };
        let free = matches!(k.camera, super::camera_key::CameraKey::Free { .. });
        let c = super::camera_collision::constrain(self.edit_last, pose.translation, anchors.root.translation, free, rules, sweep);
        super::camera_key::set_eye(&mut k.camera, c.position, anchors);
        self.edit_last = Some(c.position);
    }

    /// The key being edited, for the log (`REPLAY_CAMERA`).
    pub fn editing_line(&self) -> Option<String> {
        self.editing.and_then(|i| self.cameras.get(i)).map(|k| format!("{} frame={}", log_key(&k.camera, k.lens), k.frame))
    }

    /// The camera at `time` on `anchors`: the key in effect (a soft key blends into the next soft key of the
    /// same camera), and its lens; (None, None) = the recorded camera at its own lens.
    pub fn pose(&self, time: f64, anchors: &super::camera_key::Anchors) -> (Option<bevy::prelude::Transform>, Option<f32>) {
        let Some((_, camera, lens)) = self.camera_at(time) else { return (None, None) };
        let fov = (lens != self.camera_rules.lens_default || camera != super::camera_key::CameraKey::Game)
            .then(|| super::camera_key::lens_fov(lens));
        (super::camera_key::pose(&camera, anchors), fov)
    }

    /// The camera key at `time` (soft keys blended) with the index of its clip camera, and its lens.
    pub fn camera_at(&self, time: f64) -> Option<(usize, super::camera_key::CameraKey, f32)> {
        let i = self.active(time)?;
        let (mut camera, mut lens) = (self.cameras[i].camera, self.cameras[i].lens);
        if let Some(next) = self.cameras.get(i + 1).filter(|n| self.cameras[i].soft && n.soft) {
            let a = self.cameras[i].frame as f64;
            let u = ((self.frame(time) as f64 - a) / (next.frame as f64 - a).max(1.0)).clamp(0.0, 1.0) as f32;
            if let Some(blend) = camera.lerp(&next.camera, u) {
                camera = blend;
                lens += (next.lens - lens) * u;
            }
        }
        Some((i, camera, lens))
    }

    /// D-pad Left / Right released: the previous / next marker (the screen's "Skip to previous / next marker"),
    /// else the in / out point (b386).
    pub fn skip(&self, time: f64, forward: bool) -> f64 {
        let frame = self.frame(time);
        let c = &self.edit.clips[self.clip];
        let markers = self.cameras.iter().map(|k| k.frame);
        let target = if forward {
            markers.filter(|&f| f > frame).min().unwrap_or(c.end)
        } else {
            markers.filter(|&f| f < frame).max().unwrap_or(c.start)
        };
        self.time(target)
    }

    /// The lanes of the Create pack's active channel (1 speed .. 6 the four tint components); none on the camera
    /// channel or without the pack.
    pub fn channel_lanes(&self, channels: bool) -> &'static [Lane] {
        if channels { CHANNEL_LANES[self.channel % CHANNEL_LANES.len()] } else { &[] }
    }

    /// The film pack's Reset Timeline (b557, `82C7F728`): every key of the current channel, or of every channel, goes
    /// (the camera channel's keys: back to the recorded camera); the playhead, the channel and the clip heads stay.
    pub fn reset_channels(&mut self, all: bool) {
        let channels: Vec<usize> = if all { (0..CHANNEL_LANES.len()).collect() } else { vec![self.channel % CHANNEL_LANES.len()] };
        for channel in channels {
            if channel == 0 {
                self.cameras.clear();
                self.editing = None;
                self.free_height = Default::default();
            }
            for &lane in CHANNEL_LANES[channel] {
                for clip in 0..self.edit.clips.len() {
                    let _ = self.edit.reset_lane(clip, lane);
                }
            }
        }
    }

    /// D-pad Down released (b461 / b462): a hard key at the cursor (82C7F610, 82835F08): docs/hails-additions/37-instant-replay.md#drop-marker
    pub fn drop_marker(&mut self, time: f64, channels: bool) -> Result<(), EditError> {
        let frame = self.frame(time);
        let lanes = self.channel_lanes(channels);
        if !lanes.is_empty() {
            for &lane in lanes {
                self.edit.insert_hard(self.clip, lane, frame)?;
            }
            return Ok(());
        }
        if self.cameras.len() >= MAX_CAMERA_KEYS {
            return Err(EditError::NoMoreMarkers);
        }
        if self.key_near(time).is_some() || !self.edit.in_marker_window(self.clip, frame) {
            return Err(EditError::TooClose);
        }
        let (camera, lens) = self.active(time).map_or((super::camera_key::CameraKey::Game, self.camera_rules.lens_default), |i| (self.cameras[i].camera, self.cameras[i].lens));
        let i = self.cameras.partition_point(|k| k.frame < frame);
        self.cameras.insert(i, CamKey { frame, camera, lens, soft: false });
        if !channels {
            for lane in [Lane::Speed, Lane::Shake] {
                // A lane key already at the frame (a mod's edit) stays; the camera key is the marker.
                let _ = self.edit.insert_hard(self.clip, lane, frame);
            }
        }
        Ok(())
    }

    /// What D-pad Up asks about before removing the key at the cursor (b407, `82613650` via `82508D88`, on the active
    /// channel): a hard key is a clip marker, a soft key a key marker; None = no key there (retail's error sound).
    pub fn marker_kind(&self, time: f64, channels: bool) -> Option<MarkerKind> {
        let kind = |soft: bool| if soft { MarkerKind::Key } else { MarkerKind::Clip };
        match self.channel_lanes(channels).first() {
            Some(&lane) => {
                let i = self.edit.key_near(self.clip, lane, self.frame(time))?;
                Some(kind(self.edit.clips[self.clip].markers[lane.index()][i].soft))
            }
            None => self.key_near(time).map(|i| kind(self.cameras[i].soft)),
        }
    }

    /// D-pad Down's confirm (b407, `82508828`): retail asks before a clip marker lands inside a run of soft keys (the
    /// key in effect on the active channel is soft); not at the cap, whose refusal comes first (`82613500`).
    pub fn drop_needs_confirm(&self, time: f64, channels: bool) -> bool {
        match self.channel_lanes(channels).first() {
            Some(&lane) => {
                let c = &self.edit.clips[self.clip];
                let keys = &c.markers[lane.index()];
                keys.len() < self.edit.limits.max_markers_per_lane && c.segment(lane, self.frame(time)).key.is_some_and(|i| keys[i].soft)
            }
            None => self.cameras.len() < MAX_CAMERA_KEYS && self.active(time).is_some_and(|i| self.cameras[i].soft),
        }
    }

    /// D-pad Up released, Yes (b407, 82613910 -> 82C7FD60): remove the key at the cursor: docs/hails-additions/37-instant-replay.md#remove-marker
    pub fn remove_marker(&mut self, time: f64, channels: bool) -> bool {
        let lanes = self.channel_lanes(channels);
        if !lanes.is_empty() {
            let frame = self.frame(time);
            return lanes.iter().fold(false, |any, &lane| self.edit.remove_key(self.clip, lane, frame).is_some() || any);
        }
        let Some(i) = self.key_near(time) else { return false };
        if self.cameras[i].soft {
            self.cameras.remove(i);
        } else {
            let lo = self.cameras[..i].iter().rposition(|k| !k.soft).map_or(0, |p| p + 1);
            let hi = self.cameras[i + 1..].iter().position(|k| !k.soft).map_or(self.cameras.len(), |p| i + 1 + p);
            self.cameras.drain(lo..hi);
            let (start, end) = self.head_frames();
            let from = lo.checked_sub(1).map_or(start, |p| self.cameras[p].frame);
            let to = self.cameras.get(lo).map_or(end, |k| k.frame);
            for lane in [Lane::Speed, Lane::Shake] {
                let _ = if channels {
                    self.edit.reset_at(self.clip, lane, self.frame(time))
                } else {
                    self.edit.reset_span(self.clip, lane, from, to)
                };
            }
            if let Some(p) = lo.checked_sub(1) {
                let (min, max) = self.camera_rules.lens_range;
                self.cameras[p].lens = self.camera_rules.lens_default.clamp(min, max);
            }
        }
        self.editing = None;
        true
    }

    /// A in the timeline (b462 / b463, `82612C28` case 2) on the active channel: over a soft key the cursor snaps to
    /// it (`825088B8`); over a hard key, or outside the clip's marker window, A is refused (`core_error`).
    pub fn a_target(&self, time: f64, channels: bool) -> ATarget {
        let frame = self.frame(time);
        let c = &self.edit.clips[self.clip];
        match self.channel_lanes(channels).first() {
            Some(&lane) => match self.edit.key_near(self.clip, lane, frame) {
                Some(i) if c.markers[lane.index()][i].soft => return ATarget::Effects(Some(self.time(c.markers[lane.index()][i].frame))),
                Some(_) => return ATarget::Refused,
                None => {}
            },
            None => match self.key_near(time) {
                Some(i) if self.cameras[i].soft => return ATarget::CameraKey,
                Some(_) => return ATarget::Refused,
                None => {}
            },
        }
        if self.edit.in_marker_window(self.clip, frame) { ATarget::Effects(None) } else { ATarget::Refused }
    }

    /// The effects menu opens (retail state 0xB, `825079A0`): the live values load from the take at the cursor.
    /// Without the pack speed and shake; with it only the active channel's lanes (the others keep their values).
    pub fn load_live(&mut self, time: f64, channels: bool) {
        let frame = self.frame(time);
        let lanes: &[Lane] = if channels { self.channel_lanes(true) } else { &[Lane::Speed, Lane::Shake] };
        for &lane in lanes {
            if let Some(v) = self.edit.value_at(self.clip, lane, frame) {
                self.live[lane.index()] = v;
            }
        }
    }

    /// The base game's commit (`82507438(E, 0)`: B in the effects menu, or leaving a camera key edit): speed and shake
    /// into the segment under the cursor, unless a camera key is there. Returns whether anything landed.
    pub fn commit_base(&mut self, time: f64) -> bool {
        if self.key_near(time).is_some() {
            return false;
        }
        let frame = self.frame(time);
        [Lane::Speed, Lane::Shake].into_iter().fold(false, |any, lane| {
            matches!(self.edit.commit(self.clip, lane, frame, self.live[lane.index()], false), Ok(Commit::Segment)) || any
        })
    }

    /// The film pack's commit on the active channel (`82507438` DLC branch): A (`x` false) adds a soft key only when
    /// the segment touches a soft key (`82508828`), X (`x` true) always, but not on shake (b461). Returns whether the
    /// commit ran (false: not a lane channel, X on shake, or the cursor on a hard key).
    pub fn commit_pack(&mut self, time: f64, x: bool) -> bool {
        let lanes = self.channel_lanes(true);
        if lanes.is_empty() || (x && lanes == [Lane::Shake]) {
            return false;
        }
        let frame = self.frame(time);
        let c = &self.edit.clips[self.clip];
        if self.edit.key_near(self.clip, lanes[0], frame).is_some_and(|i| !c.markers[lanes[0].index()][i].soft) {
            return false;
        }
        let add = x || !c.cut_both_sides(lanes[0], frame);
        for &lane in lanes {
            let _ = self.edit.commit(self.clip, lane, frame, self.live[lane.index()], add);
        }
        true
    }

    /// A lane's value for the screen (`GetSimpleSpeedSetting` speed %, `GetShakeCamType` shake 0..3, the look values):
    /// the live value while the effects menu is open, else the take's value at the cursor.
    pub fn lane_value(&self, lane: Lane, time: f64) -> i32 {
        if self.session.mode == EditorMode::Effects {
            return self.live[lane.index()];
        }
        self.edit.value_at(self.clip, lane, self.frame(time)).unwrap_or(0)
    }

    /// The in / out points in timeline frames from the recording's start (`GetStartHeadPosition` /
    /// `GetEndHeadPosition`: whole 30 fps frames, b398).
    pub fn head_frames(&self) -> (u32, u32) {
        let c = &self.edit.clips[self.clip];
        (c.start, c.end)
    }

    /// A row's lane: the tint item edits the component under the tint cursor.
    pub fn row_lane(&self, row: usize) -> Option<Lane> {
        if row == 12 {
            return Some(TINT_LANES[self.tint % 4]);
        }
        EFFECT_ROWS.get(row).copied().flatten()
    }

    /// Y in the timeline (Create pack, `82612C28` case 4): the next channel, wrapping; the effects menu opens on its
    /// first row (the camera channel keeps a camera row).
    pub fn cycle_channel(&mut self) {
        self.channel = (self.channel + 1) % CHANNELS.len();
        let rows = CHANNELS[self.channel].1;
        if !rows.contains(&self.row) {
            self.row = rows[0];
        }
    }

    /// Up (-1) / Down (+1) in the effects menu. Base screen: every row, wrapping. Create pack: the camera channel's
    /// rows wrap (`8250A9C8` / `8250AA78`); on the tint channel the component cursor wraps R .. A (`82612328`); the
    /// other channels have one row. Returns whether the tint cursor moved (retail posts kRefreshTint for it).
    pub fn move_row(&mut self, direction: i32, channels: bool) -> bool {
        let wrap = |value: usize, len: usize| (value as i32 + direction).rem_euclid(len as i32) as usize;
        if !channels {
            self.row = wrap(self.row.min(BASE_ROWS - 1), BASE_ROWS);
            return false;
        }
        let rows = CHANNELS[self.channel].1;
        if self.channel == 6 {
            self.tint = wrap(self.tint, 4);
            return true;
        }
        let at = rows.iter().position(|&r| r == self.row).unwrap_or(0);
        self.row = rows[wrap(at, rows.len())];
        false
    }

    /// `GetModeValue` (825AE070): the raw brightness / contrast / saturation at the cursor on channels 3 / 4 / 5, else -1.
    pub fn mode_value(&self, time: f64) -> i32 {
        match self.channel {
            3 => self.lane_value(Lane::Brightness, time),
            4 => self.lane_value(Lane::Contrast, time),
            5 => self.lane_value(Lane::Saturation, time),
            _ => -1,
        }
    }

    /// `GetModeSliderValue` (825AE0B8): the slider notch 0..10 (brightness v / 20 + 5, contrast (v - 10) / 2,
    /// saturation v / 10), else 2.
    pub fn mode_slider(&self, time: f64) -> i32 {
        match self.channel {
            3 => self.lane_value(Lane::Brightness, time) / 20 + 5,
            4 => (self.lane_value(Lane::Contrast, time) - 10) / 2,
            5 => self.lane_value(Lane::Saturation, time) / 10,
            _ => 2,
        }
    }

    /// A tint component at the cursor (0 R .. 3 A), raw.
    pub fn tint_value(&self, component: usize, time: f64) -> i32 {
        self.lane_value(TINT_LANES[component % 4], time)
    }

    /// Left / Right in the effects menu: step the row's live value only (`8250ADF8` / `8250AB28` write no key; a
    /// commit does). Returns whether it changed.
    pub fn adjust_row(&mut self, direction: i32) -> bool {
        let Some(lane) = self.row_lane(self.row) else { return false };
        let spec = self.edit.limits.lanes[lane.index()];
        let v = &mut self.live[lane.index()];
        let before = *v;
        *v = spec.adjust(*v, direction);
        *v != before
    }

    /// The retail editor mode value (`+0x30`) the screen is in.
    pub fn mode(&self) -> u8 {
        self.session.mode as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skate_core::replay_edit::LaneKey;

    #[test]
    fn opens_over_the_recording_trims_marks_and_slows() {
        assert!(Editor::open(0.0, 4.0, EditLimits::default()).is_none(), "under 150 frames (5 s)");
        let mut e = Editor::open(10.0, 30.0, EditLimits::default()).unwrap();
        assert_eq!(e.heads(), (10.0, 30.0));
        assert_eq!(e.mode(), 6);
        e.trim_by(30, -60, 600);
        assert_eq!(e.heads(), (11.0, 28.0));
        // A full left stick for one second at 30 Hz: 0.005 x 600 frames x 30 = 90 frames = 3 s later in point.
        for _ in 0..60 {
            e.trim_sticks(1.0, 0.0, 1.0 / 60.0);
        }
        assert_eq!(e.heads(), (14.0, 28.0));
        e.trim_by(-90, 0, 600);
        // Speed 100 % until a clip marker and a committed lower value slow the clip after it (b461).
        assert_eq!(e.speed(20.0), 1.0);
        e.drop_marker(20.0, false).unwrap();
        assert_eq!(e.cameras.len(), 1, "a camera key (the timeline's marker)");
        assert_eq!(e.edit.clips[e.clip].markers[Lane::Speed.index()].len(), 1, "and hard keys on speed and shake");
        e.session.mode = EditorMode::Effects;
        e.load_live(21.0, false);
        e.row = 7;
        assert!(e.adjust_row(-1));
        assert_eq!(e.lane_value(Lane::Speed, 21.0), 90, "the menu shows the live value");
        assert_eq!(e.speed(21.0), 1.0, "Left / Right write nothing");
        // Camera rows take no Left / Right; shake wraps 0..3.
        e.row = 0;
        assert!(!e.adjust_row(1));
        e.row = 8;
        assert!(e.adjust_row(-1));
        // B: the base commit, refused over the marker, else into the clip after it.
        assert!(!e.commit_base(20.0));
        assert!(e.commit_base(21.0));
        e.session.mode = EditorMode::Timeline;
        assert!((e.speed(21.0) - 0.9).abs() < 1e-6 && e.speed(15.0) == 1.0);
        assert_eq!(e.lane_value(Lane::Shake, 21.0), 3);
        assert_eq!((e.skip(15.0, true), e.skip(25.0, false), e.skip(25.0, true)), (20.0, 20.0, 28.0));
        assert!(e.remove_marker(20.0, false));
        // b460: the merged clip (here the whole clip) takes the speed / shake defaults.
        assert!(e.cameras.is_empty(), "the camera key went");
        assert_eq!((e.speed(21.0), e.lane_value(Lane::Shake, 21.0)), (1.0, 0));
    }

    /// The constraint runs inside the key edit and stores the held eye in the key (b593): a Tripod eye pushed past
    /// the 40 m leash is stored on the sphere, and playback poses the stored eye unchanged.
    #[test]
    fn a_key_edit_stores_the_leashed_eye_and_playback_keeps_it() {
        use bevy::prelude::{Transform, Vec3};
        use super::super::camera_key::CameraKey;
        let mut e = Editor::open(0.0, 20.0, EditLimits::default()).unwrap();
        let root = super::super::camera_key::Anchors::new(Transform::IDENTITY, Transform::IDENTITY, Default::default());
        assert_eq!(e.pick_camera(1, 5.0, &Transform::from_xyz(0.0, 2.0, 5.0), &root), Some(8));
        let i = e.editing.unwrap();
        e.cameras[i].camera = CameraKey::Tripod { eye: Vec3::new(0.0, 0.0, 55.0), look_height: 0.0 };
        e.constrain_editing(&root, &Default::default(), |_, _, _| None);
        let CameraKey::Tripod { eye, .. } = e.cameras[i].camera else { panic!() };
        assert!((eye - Vec3::new(0.0, 0.0, 40.0)).length() < 1e-4, "{eye}");
        let far = super::super::camera_key::Anchors::new(Transform::IDENTITY, Transform::from_xyz(30.0, 0.0, 0.0), Default::default());
        assert_eq!(e.pose(9.0, &far).0.unwrap().translation, eye);
    }

    /// b609 / b610: a pick of another camera inside a keyed section asks first, then replaces the section between its
    /// hard keys: the soft keys inside go, the start key turns into the new camera (pose converted at the playhead),
    /// one key goes in at the playhead; keys outside stay. The same camera's advanced row only inserts.
    #[test]
    fn a_camera_pick_on_a_keyed_clip_replaces_its_section() {
        use super::super::camera_key::{Anchors, CameraKey};
        use bevy::prelude::{Transform, Vec3};
        let mut e = Editor::open(0.0, 20.0, EditLimits::default()).unwrap();
        let anchors = Anchors::new(Transform::from_xyz(0.0, 2.0, 6.0), Transform::IDENTITY, Default::default());
        let tripod = |x: f32| CameraKey::Tripod { eye: Vec3::new(x, 2.0, 5.0), look_height: 0.25 };
        let free = CameraKey::Free { eye: Vec3::new(0.0, 3.0, 0.0), yaw: 0.0, pitch: 0.0, distance: 5.0 };
        let key = |t: f64, camera, soft| CamKey { frame: e.frame(t), camera, lens: 24.0, soft };
        let keys = vec![key(2.0, tripod(1.0), false), key(4.0, tripod(2.0), true), key(6.0, tripod(3.0), true),
                        key(8.0, free, false), key(10.0, free, true)];
        e.cameras = keys.clone();
        let view = Transform::from_xyz(0.0, 2.0, 5.0);
        assert!(e.pick_asks(0, 5.0), "Follow over a Tripod section asks");
        assert!(!e.pick_asks(5, 5.0) && !e.pick_replaces(5, 5.0), "Advanced Tripod over Tripod only inserts");
        assert!(e.pick_replaces(1, 5.0), "basic Tripod between soft keys replaces");
        assert_eq!(e.pick_camera(0, 5.0, &view, &anchors), Some(7));
        let frames: Vec<_> = e.cameras.iter().map(|k| (k.frame, k.camera.row(), k.soft)).collect();
        assert_eq!(frames, vec![(e.frame(2.0), 0, false), (e.frame(5.0), 0, false), (e.frame(8.0), 3, false), (e.frame(10.0), 3, true)]);
        // The start key keeps its world eye and its look height, now in the Follow eye frame.
        let CameraKey::Follow { look_height, .. } = e.cameras[0].camera else { panic!() };
        assert_eq!(look_height, 0.25);
        let before = super::super::camera_key::pose(&tripod(1.0), &anchors).unwrap().translation;
        assert!(super::super::camera_key::pose(&e.cameras[0].camera, &anchors).unwrap().translation.distance(before) < 1e-4);
        // Without the section rule (a mod's fill_section = false) a pick only inserts.
        e.cameras = keys;
        e.camera_rules.fill_section = false;
        assert!(!e.pick_asks(0, 5.0));
        assert_eq!(e.pick_camera(0, 5.0, &view, &anchors), Some(7));
        assert_eq!(e.cameras.len(), 6);
    }

    /// Camera keys (b405): picking a basic camera inserts a hard key at the playhead (the pose holds and cuts), an
    /// advanced camera a soft key (blends into the next soft key of the same camera); A on a key re-edits it.
    #[test]
    fn camera_keys_hold_cut_and_blend() {
        use bevy::prelude::{Transform, Vec3};
        let mut e = Editor::open(0.0, 20.0, EditLimits::default()).unwrap();
        let root = super::super::camera_key::Anchors::new(Transform::IDENTITY, Transform::IDENTITY, Default::default());
        let view = Transform::from_xyz(0.0, 2.0, 5.0);
        // No keys: the recorded camera.
        assert!(e.pose(5.0, &root).0.is_none());
        // Tripod (basic) at 5 s on a clip with no keys: the whole clip takes it (b603 / b604), the part before the key
        // holding the key's unedited pose.
        assert_eq!(e.pick_camera(1, 5.0, &view, &root), Some(8));
        assert_eq!(e.pose(4.0, &root).0, e.pose(9.0, &root).0);
        assert!(e.pose(4.0, &root).0.is_some() && e.active_row(2.0) == 1);
        assert_eq!(e.active_row(9.0), 1);
        // Advanced Free keys at 10 s and 14 s: soft, blending between them.
        // (A new key starts from the camera in effect at the playhead; the eyes are then edited, set here directly.)
        assert_eq!(e.pick_camera(6, 10.0, &view, &root), Some(10));
        assert_eq!(e.pick_camera(6, 14.0, &view, &root), Some(10));
        for (i, x) in [(2, 0.0), (3, 8.0)] {
            e.cameras[i].camera = super::super::camera_key::CameraKey::Free { eye: Vec3::new(x, 2.0, 0.0), yaw: 0.0, pitch: 0.0, distance: 5.0 };
        }
        let mid = e.pose(12.0, &root).0.unwrap().translation;
        assert!((mid - Vec3::new(4.0, 2.0, 0.0)).length() < 1e-3, "{mid}");
        assert_eq!(e.active_row(12.0), 6);
        assert_eq!(e.key_fractions().len(), 4);
        // The Free pick at 10 s replaced the Tripod section (b609): its start key at 5 s became a Free key (b610).
        assert_eq!(e.segments().iter().map(|s| s.1).collect::<Vec<_>>(), vec![1, 2, 2, 2]);
        // A on the key at 5 s re-edits it (Free, mode 10); not on a key: None.
        assert_eq!(e.reopen_at(5.0), Some(10));
        assert_eq!(e.reopen_at(7.0), None);
        // A on a Game camera key opens nothing (`82612C28`, kind >= 3 returns, b555) and leaves the edited key alone.
        e.cameras[2].camera = super::super::camera_key::CameraKey::Game;
        let editing = e.editing;
        assert_eq!(e.reopen_at(10.0), None);
        assert_eq!(e.editing, editing);
        // A in the timeline (b462 / b463): over the hard Tripod key refused, over a soft key its edit, else the menu;
        // the clip's very ends are refused.
        assert_eq!(e.a_target(5.0, false), ATarget::Refused);
        assert_eq!(e.a_target(10.0, false), ATarget::CameraKey);
        assert_eq!(e.a_target(7.0, false), ATarget::Effects(None));
        assert_eq!(e.a_target(0.0, false), ATarget::Refused);
    }

    /// The Create pack's channels (b410 / b411): Y cycles camera .. tint; Up / Down stay in the channel's rows (the tint
    /// cursor wraps R .. A); Left / Right edit the channel's lane; the natives' values follow.
    #[test]
    fn create_pack_channels_edit_the_look_lanes() {
        let mut e = Editor::open(0.0, 20.0, EditLimits::default()).unwrap();
        // Base screen: Up from row 0 wraps to shake (row 8).
        assert!(!e.move_row(-1, false));
        assert_eq!(e.row, 8);
        e.row = 0;
        // Camera channel: rows 0..6 wrap.
        e.move_row(-1, true);
        assert_eq!(e.row, 6);
        for (channel, row, label) in [(1, 7, "SPEED"), (2, 8, "SHAKE"), (3, 9, "BRIGHTNESS"), (4, 10, "CONTRAST"), (5, 11, "SATURATE"), (6, 12, "TINT")] {
            e.cycle_channel();
            assert_eq!((e.channel, e.row), (channel, row));
            assert!(CHANNELS[channel].0.ends_with(label));
        }
        // Tint: the cursor wraps, Right on blue steps it by 25.
        assert!(e.move_row(-1, true));
        assert_eq!(e.tint, 3);
        e.move_row(1, true);
        e.move_row(1, true);
        e.move_row(1, true);
        assert_eq!(e.tint, 2);
        e.session.mode = EditorMode::Effects;
        assert!(e.adjust_row(1));
        assert_eq!(e.tint_value(2, 5.0), 25);
        assert_eq!((e.mode_value(5.0), e.mode_slider(5.0)), (-1, 2));
        // Brightness: +20 per press, slider notch v / 20 + 5.
        e.cycle_channel();
        assert_eq!(e.channel, 0);
        for _ in 0..3 {
            e.cycle_channel();
        }
        assert_eq!(e.channel, 3);
        e.load_live(5.0, true);
        e.adjust_row(1);
        assert_eq!((e.mode_value(5.0), e.mode_slider(5.0)), (20, 6));
        // A on the empty lane is lost (b465); X adds a soft key with the value (b461).
        assert!(e.commit_pack(5.0, false));
        e.session.mode = EditorMode::Timeline;
        assert_eq!(e.mode_value(5.0), 0);
        assert!(e.commit_pack(5.0, true));
        assert_eq!(e.mode_value(5.0), 20);
        assert_eq!(e.marker_kind(5.0, true), Some(MarkerKind::Key));
    }

    /// The film pack's lane channels (b461 / b462): D-pad Down puts a hard key on the active lane only; A in the
    /// timeline is refused on it and snaps to a soft key; A in a segment cut on both sides writes it, X adds a soft
    /// key, X on shake does nothing; D-pad Up deletes on the lane.
    /// The film pack's Reset Timeline (b557): the current channel's keys go (cameras for channel 0), or every channel's;
    /// the clip heads stay.
    #[test]
    fn reset_timeline_clears_the_channel_or_all() {
        use bevy::prelude::Transform;
        let mut e = Editor::open(0.0, 30.0, EditLimits::default()).unwrap();
        e.camera_rules.fill_section = false; // the key layout below, without the In-point key of a first pick
        let (root, view) = (super::super::camera_key::Anchors::new(Transform::IDENTITY, Transform::IDENTITY, Default::default()), Transform::from_xyz(0.0, 2.0, 5.0));
        assert!(e.pick_camera(1, 5.0, &view, &root).is_some());
        e.cycle_channel();
        e.drop_marker(10.0, true).unwrap();
        e.live[Lane::Speed.index()] = 50;
        assert!(e.commit_pack(20.0, true));
        let heads = e.heads();
        // Current channel (speed): its keys go, the camera key stays.
        e.reset_channels(false);
        assert!(e.edit.clips[e.clip].markers[Lane::Speed.index()].is_empty() && e.speed(25.0) == 1.0);
        assert_eq!(e.cameras.len(), 1);
        // All channels: the camera key goes too; the heads and the channel stay.
        e.drop_marker(12.0, true).unwrap();
        e.reset_channels(true);
        assert!(e.cameras.is_empty() && e.edit.clips[e.clip].markers.iter().all(|m| m.is_empty()));
        assert_eq!((e.heads(), e.channel), (heads, 1));
    }

    #[test]
    fn pack_lane_channels_drop_commit_and_delete_on_their_lane() {
        let mut e = Editor::open(0.0, 30.0, EditLimits::default()).unwrap();
        e.cycle_channel();
        assert_eq!(e.channel_lanes(true), [Lane::Speed]);
        e.drop_marker(10.0, true).unwrap();
        assert!(e.cameras.is_empty() && e.edit.clips[e.clip].markers[Lane::Shake.index()].is_empty());
        assert_eq!(e.marker_kind(10.0, true), Some(MarkerKind::Clip));
        assert_eq!(e.a_target(10.0, true), ATarget::Refused);
        // A at 15 s: the segment 10 s .. end is cut on both sides: written, no key.
        e.load_live(15.0, true);
        for _ in 0..5 {
            e.adjust_row(-1);
        }
        assert!(e.commit_pack(15.0, false));
        assert_eq!(e.edit.clips[e.clip].markers[Lane::Speed.index()].len(), 1);
        assert!((e.speed(25.0) - 0.5).abs() < 1e-6 && e.speed(9.0) == 1.0);
        // X at 20 s with 100: a soft key; the speed ramps 50 -> 100 from 10 s to 20 s, then holds.
        e.live[Lane::Speed.index()] = 100;
        assert!(e.commit_pack(20.0, true));
        assert!((e.speed(15.0) - 0.75).abs() < 1e-6 && e.speed(25.0) == 1.0);
        assert_eq!(e.marker_kind(20.0, true), Some(MarkerKind::Key));
        assert_eq!(e.a_target(20.1, true), ATarget::Effects(Some(20.0)));
        assert!(e.drop_needs_confirm(25.0, true) && !e.drop_needs_confirm(15.0, true));
        // Shake: X does nothing.
        e.cycle_channel();
        assert!(!e.commit_pack(15.0, true));
        e.channel = 1;
        assert!(e.remove_marker(10.0, true), "the clip marker takes the soft key after it");
        assert!(e.edit.clips[e.clip].markers[Lane::Speed.index()].is_empty());
    }

    /// Deleting (b407): a soft key goes alone; a clip marker takes the soft keys in the clips on both sides, up to
    /// the next hard key each way. D-pad Down asks first only inside a run of soft keys.
    #[test]
    fn deleting_a_clip_marker_takes_the_key_markers_on_both_sides() {
        use bevy::prelude::Transform;
        let mut e = Editor::open(0.0, 30.0, EditLimits::default()).unwrap();
        e.camera_rules.fill_section = false; // the key layout below, without the In-point key of a first pick
        let (root, view) = (super::super::camera_key::Anchors::new(Transform::IDENTITY, Transform::IDENTITY, Default::default()), Transform::from_xyz(0.0, 2.0, 5.0));
        // Keys: hard 4, soft 8, soft 10, hard 14, soft 18, soft 20, hard 24.
        for (t, row) in [(4.0, 1), (8.0, 6), (10.0, 6), (14.0, 0), (18.0, 4), (20.0, 4), (24.0, 2)] {
            assert!(e.pick_camera(row, t, &view, &root).is_some(), "{t}");
        }
        assert_eq!(e.marker_kind(14.0, false), Some(MarkerKind::Clip));
        assert_eq!(e.marker_kind(18.0, false), Some(MarkerKind::Key));
        assert_eq!(e.marker_kind(16.0, false), None);
        assert!(e.drop_needs_confirm(9.0, false) && !e.drop_needs_confirm(16.0, false));
        // Lane values to watch: speed 50 from 14, shake 2 from 16, a look marker at 15, a speed marker at 26.
        let f = |t: f64| e.frame(t);
        let (f14, f15, f16, f26) = (f(14.0), f(15.0), f(16.0), f(26.0));
        e.edit.clips[e.clip].markers[Lane::Speed.index()] = vec![LaneKey::hard(f14, 50), LaneKey::hard(f26, 70)];
        e.edit.clips[e.clip].markers[Lane::Shake.index()] = vec![LaneKey::hard(f16, 2)];
        e.edit.clips[e.clip].markers[Lane::Brightness.index()] = vec![LaneKey::hard(f15, 40)];
        e.cameras[0].lens = 50.0;
        // A key marker goes alone, with no reset.
        assert!(e.remove_marker(20.0, false));
        assert_eq!(e.cameras.len(), 6);
        assert_eq!(e.lane_value(Lane::Speed, 15.0), 50);
        // The clip marker at 14 takes 8, 10 and 18; the hard keys at 4 and 24 stay.
        assert!(e.remove_marker(14.0, false));
        assert_eq!(e.cameras.iter().map(|k| (k.frame, k.soft)).collect::<Vec<_>>(), vec![(e.frame(4.0), false), (e.frame(24.0), false)]);
        assert!(!e.remove_marker(14.0, false));
        // b460: the merged clip 4..24 takes the speed / shake defaults and the key at 4 the default lens; the clip
        // after 24 keeps its speed; look lanes are untouched.
        assert_eq!((e.lane_value(Lane::Speed, 15.0), e.lane_value(Lane::Shake, 17.0)), (100, 0));
        assert_eq!(e.lane_value(Lane::Speed, 27.0), 70);
        assert_eq!(e.lane_value(Lane::Brightness, 16.0), 40);
        assert_eq!(e.cameras[0].lens, e.camera_rules.lens_default);
    }

    /// b460, with the film pack's channels: only the lane value in effect at the cursor resets.
    #[test]
    fn a_clip_delete_with_the_film_pack_resets_only_the_values_at_the_cursor() {
        use bevy::prelude::Transform;
        let mut e = Editor::open(0.0, 30.0, EditLimits::default()).unwrap();
        let (root, view) = (super::super::camera_key::Anchors::new(Transform::IDENTITY, Transform::IDENTITY, Default::default()), Transform::from_xyz(0.0, 2.0, 5.0));
        for (t, row) in [(20.0, 1), (28.0, 0)] {
            assert!(e.pick_camera(row, t, &view, &root).is_some(), "{t}");
        }
        let (f22, f29) = (e.frame(22.0), e.frame(29.0));
        e.edit.clips[e.clip].markers[Lane::Speed.index()] = vec![LaneKey::hard(f22, 50), LaneKey::hard(f29, 30)];
        assert!(e.remove_marker(28.0, true));
        assert_eq!(e.edit.clips[e.clip].markers[Lane::Speed.index()], vec![LaneKey::hard(f22, 100), LaneKey::hard(f29, 30)]);
    }
}
