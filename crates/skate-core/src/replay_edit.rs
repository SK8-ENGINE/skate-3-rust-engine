//! The replay editor model: clips, effect lanes of hard and soft keys, retail length rules; pure data.
//! Retail rules and limits: docs/hails-additions/37-instant-replay.md#replay-editor-model

use crate::colour_matrix::ColourMatrixRecord;

/// The effect lanes, in retail's lane order after the camera lane (0). Stable ids: [`Lane::id`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Lane {
    Speed,
    Shake,
    Brightness,
    Contrast,
    Saturation,
    TintR,
    TintG,
    TintB,
    TintA,
}

pub const LANES: [Lane; 9] = [
    Lane::Speed,
    Lane::Shake,
    Lane::Brightness,
    Lane::Contrast,
    Lane::Saturation,
    Lane::TintR,
    Lane::TintG,
    Lane::TintB,
    Lane::TintA,
];

impl Lane {
    pub fn index(self) -> usize {
        self as usize
    }

    /// Stable string id (mod records, saved edits).
    pub fn id(self) -> &'static str {
        match self {
            Lane::Speed => "speed",
            Lane::Shake => "shake",
            Lane::Brightness => "brightness",
            Lane::Contrast => "contrast",
            Lane::Saturation => "saturation",
            Lane::TintR => "tint_r",
            Lane::TintG => "tint_g",
            Lane::TintB => "tint_b",
            Lane::TintA => "tint_a",
        }
    }

    pub fn from_id(id: &str) -> Option<Lane> {
        LANES.into_iter().find(|l| l.id() == id)
    }
}

/// One lane's range: integer editor values, `value * scale + offset` is the effect value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LaneSpec {
    pub min: i32,
    pub max: i32,
    pub step: i32,
    pub default: i32,
    /// Past the end wraps to the other end (retail's shake) instead of clamping.
    pub wraps: bool,
    pub scale: f32,
    /// Added after the scale (retail's band builder takes saturation as SATURATE_NEAR + 1.0, `827F3D20`).
    pub offset: f32,
    /// Ramps linearly across a segment instead of stepping (retail's interpolated elements, descriptor +0x14 == 0:
    /// TIME_SCALE speed, b463 `82C8FAD8`, and the looks, b408; shake's level steps, b464).
    pub interpolate: bool,
    /// Holds whole numbers: the ramped value is rounded before scaling (retail's int elements, `82C89A50`: speed).
    pub round: bool,
}

impl LaneSpec {
    /// Retail's ranges. Defaults: speed 100, brightness 0, saturation 0 (`82506CD8`) [code]; contrast 10 = x1 and
    /// tint 0 = overlay off are the neutral values [ours]. Saturation 0 is x1 and 100 doubles it.
    pub fn retail(lane: Lane) -> LaneSpec {
        let spec = |min, max, step, default, scale| LaneSpec { min, max, step, default, wraps: false, scale, offset: 0.0, interpolate: false, round: false };
        match lane {
            Lane::Speed => LaneSpec { interpolate: true, round: true, ..spec(10, 100, 10, 100, 0.01) },
            Lane::Shake => LaneSpec { wraps: true, ..spec(0, 3, 1, 0, 1.0) },
            Lane::Brightness => LaneSpec { interpolate: true, ..spec(-100, 100, 20, 0, 0.01) },
            Lane::Contrast => LaneSpec { interpolate: true, ..spec(10, 30, 2, 10, 0.1) },
            Lane::Saturation => LaneSpec { interpolate: true, offset: 1.0, ..spec(0, 100, 10, 0, 0.01) },
            Lane::TintR | Lane::TintG | Lane::TintB | Lane::TintA => LaneSpec { interpolate: true, ..spec(0, 250, 25, 0, 1.0 / 255.0) },
        }
    }

    /// One press up (+1) or down (-1).
    pub fn adjust(&self, value: i32, direction: i32) -> i32 {
        let next = value + self.step * direction.signum();
        if self.wraps {
            if next > self.max {
                self.min
            } else if next < self.min {
                self.max
            } else {
                next
            }
        } else {
            next.clamp(self.min, self.max)
        }
    }
}

/// The editor's limits, data driven.
#[derive(Clone, Debug, PartialEq)]
pub struct EditLimits {
    /// Frames the recording must hold before the editor opens (retail 150, at 30 fps = 5 s).
    pub min_frames_to_open: u32,
    /// The timeline's frame rate (retail 30).
    pub timeline_fps: f32,
    /// Longest encoded length that can be shared (retail 60 s).
    pub share_max_seconds: u32,
    /// Markers per lane (retail refuses at 100: `82613500`).
    pub max_markers_per_lane: usize,
    /// A key within this fraction of the clip from the cursor is "at the cursor", and no key goes this close to
    /// the clip's ends (retail 0.0089 of the normalised timeline, 0x822428EC / 0.9911 at 0x822F8F74, `825084C8`,
    /// research b382; the unit, the clip span, is inferred).
    pub marker_spacing: f32,
    /// The rounding of a lane's sample: floor, plus one when the fraction is at least this (retail 0.5, `8209975C`).
    pub round_threshold: f32,
    /// Added to the encoded length before it is truncated to whole seconds (retail 0.5, `8250B590`).
    pub length_round_bias: f32,
    /// Whether a commit on a lane without keys lands on the whole clip. Retail loses it (false; the sampler reads the
    /// lane default while a lane has no keys, b463 / b465); a mod may turn it on.
    pub empty_lane_commit: bool,
    pub lanes: [LaneSpec; 9],
}

impl Default for EditLimits {
    fn default() -> Self {
        Self {
            min_frames_to_open: 150,
            timeline_fps: 30.0,
            share_max_seconds: 60,
            max_markers_per_lane: 100,
            marker_spacing: 0.0089,
            round_threshold: 0.5,
            length_round_bias: 0.5,
            empty_lane_commit: false,
            lanes: LANES.map(LaneSpec::retail),
        }
    }
}

/// One key on a lane (retail's lane records, b462 section 2): it starts a segment that runs to the next key or the
/// clip's end. A hard key is a cut: the segment before it ends on its own stored end value. A soft key shares one value
/// with the segment before it, which ramps into it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaneKey {
    pub frame: u32,
    /// The value at the start of this key's segment.
    pub value: i32,
    /// The value at the end of this key's segment when the segment ends at a cut (a hard key or the clip's end).
    pub end: i32,
    pub soft: bool,
}

impl LaneKey {
    pub fn hard(frame: u32, value: i32) -> Self {
        Self { frame, value, end: value, soft: false }
    }

    pub fn soft(frame: u32, value: i32) -> Self {
        Self { frame, value, end: value, soft: true }
    }
}

/// One clip: a span of the recording in timeline frames, and per lane its keys. A fresh clip has no keys on any lane
/// (retail's blank take, b466).
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    /// The in / out heads.
    pub start: u32,
    pub end: u32,
    /// The take the lanes span (the recording the clip was made over): segments run from its start to its end, and
    /// the heads only clip them (b472: retail keeps every key when a head moves).
    pub take: [u32; 2],
    /// Per lane, the value at the clip's start (the first segment's start).
    pub values: [i32; 9],
    /// Per lane, the first segment's end value when it ends at a cut.
    pub ends: [i32; 9],
    /// Per lane, keys sorted by frame.
    pub markers: [Vec<LaneKey>; 9],
    /// Per lane, whether a value was committed into the first segment (retail's OVERRIDE flag at the clip's key, b408):
    /// a look lane applies only once set, here or by a key at or before the frame.
    pub set: [bool; 9],
}

/// A segment of a lane: from `start` (value `from`) to `end` (value `to`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    /// The key that starts it (None = the clip's start).
    pub key: Option<usize>,
    pub start: u32,
    pub end: u32,
    pub from: i32,
    pub to: i32,
}

impl Clip {
    /// The segment of `lane` holding `frame` (the key at or before it, else the clip's start).
    pub fn segment(&self, lane: Lane, frame: u32) -> Segment {
        let keys = &self.markers[lane.index()];
        let i = keys.partition_point(|k| k.frame <= frame).checked_sub(1);
        let (start, from, end_value) = match i {
            Some(i) => (keys[i].frame, keys[i].value, keys[i].end),
            None => (self.take[0], self.values[lane.index()], self.ends[lane.index()]),
        };
        let next = keys.get(i.map_or(0, |i| i + 1));
        let (end, to) = match next {
            Some(n) if n.soft => (n.frame, n.value),
            Some(n) => (n.frame, end_value),
            None => (self.take[1], end_value),
        };
        Segment { key: i, start, end, from, to }
    }

    /// Whether the segment holding `frame` starts and ends at a cut (retail `82C7F440` both sides): the clip's ends and
    /// hard keys are cuts, and an empty lane is cut on both sides.
    pub fn cut_both_sides(&self, lane: Lane, frame: u32) -> bool {
        let keys = &self.markers[lane.index()];
        let i = keys.partition_point(|k| k.frame <= frame);
        let starts_cut = i.checked_sub(1).is_none_or(|k| !keys[k].soft);
        let ends_cut = keys.get(i).is_none_or(|k| !k.soft);
        starts_cut && ends_cut
    }

    /// Write `value` into one segment's start and end (`which`: the key, None = the clip's start): the end is the next
    /// key's value when that key is soft (a shared record), else the segment's own end.
    fn write_segment(&mut self, lane: Lane, which: Option<usize>, value: i32) {
        let l = lane.index();
        let next = which.map_or(0, |i| i + 1);
        match which {
            Some(i) => {
                self.markers[l][i].value = value;
                self.markers[l][i].end = value;
            }
            None => {
                self.values[l] = value;
                self.ends[l] = value;
                self.set[l] = true;
            }
        }
        if let Some(n) = self.markers[l].get_mut(next).filter(|n| n.soft) {
            n.value = value;
        }
    }
}

/// What a commit did (retail `82507438`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Commit {
    /// The value went into the segment under the playhead.
    Segment,
    /// A soft key was added at the playhead with the value.
    SoftKey,
    /// Nothing was written: an empty lane (retail's lost write, b465), or the playhead over a hard key.
    Lost,
}

/// Retail trigger scrub per console frame (82610F18 -> 825071B8 -> 82C7EF90, b387 / b388): docs/hails-additions/37-instant-replay.md#trigger-scrub
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrubSpec {
    pub max_rate: f32,
    pub smoothing: f32,
    pub dead_target: f32,
    pub dead_rate: f32,
    pub frame_hz: f32,
}

impl Default for ScrubSpec {
    fn default() -> Self {
        Self { max_rate: 0.005, smoothing: 0.25, dead_target: 1.0e-5, dead_rate: 1.0e-4, frame_hz: 30.0 }
    }
}

/// The scrub's state; [`ScrubFilter::step`] runs whole console frames, so any engine frame rate gives the same
/// movement.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrubFilter {
    pub rate: f32,
    carry: f32,
}

impl ScrubFilter {
    /// Advance by `dt` seconds with the trigger axis `axis` (RT - LT, -1..1); returns how far the playhead moves, as
    /// a fraction of the recording.
    pub fn step(&mut self, spec: &ScrubSpec, axis: f32, dt: f32) -> f32 {
        self.carry += dt.clamp(0.0, 0.25) * spec.frame_hz;
        let target = axis.clamp(-1.0, 1.0) * spec.max_rate;
        let mut moved = 0.0;
        // A frame due up to float error counts now (144 steps of 30/144 sum to 29.9999...).
        while self.carry >= 1.0 - 1.0e-4 {
            self.carry -= 1.0;
            self.rate = (self.rate + (target - self.rate) * spec.smoothing).clamp(-spec.max_rate, spec.max_rate);
            if target.abs() < spec.dead_target && self.rate.abs() <= spec.dead_rate {
                self.rate = 0.0;
            }
            moved += self.rate;
        }
        moved
    }
}

/// A clip's look at one frame: the colour matrix record to build ([`crate::colour_matrix::build`]) and the RGBA
/// screen tint overlay.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReplayLook {
    pub record: ColourMatrixRecord,
    pub tint: [f32; 4],
}

/// The edit: clips in play order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReplayEdit {
    pub limits: EditLimits,
    pub clips: Vec<Clip>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    NoClip,
    EmptySpan,
    /// Retail's "no more markers" popup (`ID_REPLAY_EDITOR_NO_MORE_MARKERS`).
    NoMoreMarkers,
    OutsideClip,
    /// Too close to the clip's in / out point (retail's error sound, `82612C28`).
    TooClose,
}

impl ReplayEdit {
    pub fn new(limits: EditLimits) -> Self {
        Self { limits, clips: Vec::new() }
    }

    /// Whether the editor can open on a recording of `frames` timeline frames.
    pub fn can_open(&self, frames: u32) -> bool {
        frames >= self.limits.min_frames_to_open
    }

    /// A new clip over `start..end` (timeline frames) with no keys and the lane defaults; returns its index.
    pub fn add_clip(&mut self, start: u32, end: u32) -> Result<usize, EditError> {
        if end <= start {
            return Err(EditError::EmptySpan);
        }
        let values = self.limits.lanes.map(|s| s.default);
        self.clips.push(Clip { start, end, take: [start, end], values, ends: values, markers: Default::default(), set: [false; 9] });
        Ok(self.clips.len() - 1)
    }

    /// Move a clip's in / out points inside its take; the keys stay (b472).
    pub fn trim(&mut self, clip: usize, start: u32, end: u32) -> Result<(), EditError> {
        let c = self.clips.get_mut(clip).ok_or(EditError::NoClip)?;
        let (start, end) = (start.max(c.take[0]), end.min(c.take[1]));
        if end <= start {
            return Err(EditError::EmptySpan);
        }
        c.start = start;
        c.end = end;
        Ok(())
    }

    /// Step the clip's first segment directly (a mod or test write; the editor's buttons step a live value and commit
    /// it with [`ReplayEdit::commit`]).
    pub fn adjust(&mut self, clip: usize, lane: Lane, direction: i32) -> Result<i32, EditError> {
        let spec = self.limits.lanes[lane.index()];
        let c = self.clips.get_mut(clip).ok_or(EditError::NoClip)?;
        let v = spec.adjust(c.values[lane.index()], direction);
        c.write_segment(lane, None, v);
        Ok(v)
    }

    /// Frames from `frame` within which a key is "at the cursor" (retail `825084C8`: the marker spacing of the take, at
    /// least one frame).
    fn near(&self, c: &Clip) -> u32 {
        ((self.limits.marker_spacing * (c.take[1] - c.take[0]) as f32).floor() as u32).max(1)
    }

    /// Whether `frame` lies inside the take's marker window (retail 0.0089 .. 0.9911 of the timeline, `825084C8`).
    pub fn in_marker_window(&self, clip: usize, frame: u32) -> bool {
        let Some(c) = self.clips.get(clip) else { return false };
        let at = (frame as f32 - c.take[0] as f32) / (c.take[1] - c.take[0]).max(1) as f32;
        let spacing = self.limits.marker_spacing;
        at > spacing && at < 1.0 - spacing
    }

    /// The key of `lane` at the cursor, if any.
    pub fn key_near(&self, clip: usize, lane: Lane, frame: u32) -> Option<usize> {
        let c = self.clips.get(clip)?;
        let near = self.near(c);
        c.markers[lane.index()].iter().position(|k| k.frame.abs_diff(frame) <= near)
    }

    /// Room for a new key at `frame`: inside the clip, away from its ends (the marker spacing) and under the cap.
    fn key_room(&self, c: &Clip, lane: Lane, frame: u32) -> Result<(), EditError> {
        if !(c.take[0]..c.take[1]).contains(&frame) {
            return Err(EditError::OutsideClip);
        }
        let spacing = self.limits.marker_spacing;
        let at = (frame - c.take[0]) as f32 / (c.take[1] - c.take[0]) as f32;
        if at <= spacing || at >= 1.0 - spacing {
            return Err(EditError::TooClose);
        }
        if c.markers[lane.index()].len() >= self.limits.max_markers_per_lane {
            return Err(EditError::NoMoreMarkers);
        }
        Ok(())
    }

    /// D-pad Down's key on a lane (retail `82835F08(take, 1)`, b461 / b462): a hard key at `frame` that writes no
    /// value. Both sides keep the curve in effect: the segment it splits ends on the value there, and the new key starts
    /// from it. Refused at a key, near the clip's ends and at the cap.
    pub fn insert_hard(&mut self, clip: usize, lane: Lane, frame: u32) -> Result<(), EditError> {
        let c = self.clips.get(clip).ok_or(EditError::NoClip)?;
        if self.key_near(clip, lane, frame).is_some() {
            return Err(EditError::TooClose);
        }
        self.key_room(c, lane, frame)?;
        let x = self.value_at(clip, lane, frame).unwrap_or(self.limits.lanes[lane.index()].default);
        let c = &mut self.clips[clip];
        let s = c.segment(lane, frame);
        let l = lane.index();
        let old_end = match s.key {
            Some(i) => std::mem::replace(&mut c.markers[l][i].end, x),
            None => std::mem::replace(&mut c.ends[l], x),
        };
        let i = c.markers[l].partition_point(|k| k.frame < frame);
        c.markers[l].insert(i, LaneKey { frame, value: x, end: old_end, soft: false });
        Ok(())
    }

    /// Commit `value` on `lane` at `frame` (82507438, b461 to b465): docs/hails-additions/37-instant-replay.md#lane-commit
    pub fn commit(&mut self, clip: usize, lane: Lane, frame: u32, value: i32, add_key: bool) -> Result<Commit, EditError> {
        let spec = self.limits.lanes[lane.index()];
        let value = value.clamp(spec.min, spec.max);
        let c = self.clips.get(clip).ok_or(EditError::NoClip)?;
        let l = lane.index();
        let at = self.key_near(clip, lane, frame);
        let (which, add) = match at {
            Some(i) if !c.markers[l][i].soft => return Ok(Commit::Lost),
            Some(i) => (Some(i), false),
            None => (c.segment(lane, frame).key, add_key && self.key_room(c, lane, frame).is_ok()),
        };
        let c = &mut self.clips[clip];
        if add {
            let ends_cut = c.markers[l].iter().find(|k| k.frame > frame).is_none_or(|k| !k.soft);
            let i = c.markers[l].partition_point(|k| k.frame < frame);
            let mut key = LaneKey::soft(frame, value);
            if !ends_cut {
                // The segment keeps the end of the one it split (unused while the next key is soft).
                key.end = c.segment(lane, frame).to;
            }
            c.markers[l].insert(i, key);
            return Ok(Commit::SoftKey);
        }
        if c.markers[l].is_empty() && !self.limits.empty_lane_commit {
            return Ok(Commit::Lost);
        }
        c.write_segment(lane, which, value);
        Ok(Commit::Segment)
    }

    /// Remove the key of `lane` at the cursor. A hard key also takes the soft keys on both sides, up to the next hard
    /// key each way (as the camera lane's clip marker delete, `82C7FB08`); the segment before then runs on to the next
    /// key. Returns the removed key's kind (true = soft), None when no key is there.
    pub fn remove_key(&mut self, clip: usize, lane: Lane, frame: u32) -> Option<bool> {
        let i = self.key_near(clip, lane, frame)?;
        let keys = &mut self.clips[clip].markers[lane.index()];
        let soft = keys[i].soft;
        if soft {
            keys.remove(i);
        } else {
            let lo = keys[..i].iter().rposition(|k| !k.soft).map_or(0, |p| p + 1);
            let hi = keys[i + 1..].iter().position(|k| !k.soft).map_or(keys.len(), |p| i + 1 + p);
            keys.drain(lo..hi);
        }
        Some(soft)
    }

    /// A clip marker delete's reset over the merged span `from..to` (research b460: `825086F0` writes the lane default
    /// at the merged clip's key, `82C82490` speed / `8250A6E8` shake): the lane's keys in the span go, the lane's
    /// default (data, `EditLimits::lanes`) holds from `from`, and the value in effect at `to` is kept from `to` on.
    pub fn reset_span(&mut self, clip: usize, lane: Lane, from: u32, to: u32) -> Result<(), EditError> {
        let default = self.limits.lanes[lane.index()].default;
        let c = self.clips.get(clip).ok_or(EditError::NoClip)?;
        let (from, to) = (from.clamp(c.start, c.end), to.clamp(c.start, c.end));
        let after = (to < c.end).then(|| self.value_at(clip, lane, to)).flatten();
        let c = &mut self.clips[clip];
        let l = lane.index();
        c.markers[l].retain(|k| k.frame < from || k.frame >= to);
        if from == c.start {
            c.values[l] = default;
            c.ends[l] = default;
            c.set[l] = true;
        } else {
            let i = c.markers[l].partition_point(|k| k.frame < from);
            c.markers[l].insert(i, LaneKey::hard(from, default));
        }
        if let Some(after) = after {
            if !c.markers[l].iter().any(|k| k.frame == to) && after != default {
                let i = c.markers[l].partition_point(|k| k.frame < to);
                c.markers[l].insert(i, LaneKey::hard(to, after));
            }
        }
        Ok(())
    }

    /// The film pack's timeline reset of one lane (b557, `82C7F728`): every key goes and the lane is back to a fresh
    /// clip's state (retail re-inserts one base key at the start); the clip's heads stay.
    pub fn reset_lane(&mut self, clip: usize, lane: Lane) -> Result<(), EditError> {
        let default = self.limits.lanes[lane.index()].default;
        let c = self.clips.get_mut(clip).ok_or(EditError::NoClip)?;
        let l = lane.index();
        c.markers[l].clear();
        c.values[l] = default;
        c.ends[l] = default;
        c.set[l] = false;
        Ok(())
    }

    /// The film pack's form of that reset (b460): only the segment in effect at `frame` takes the lane default (the
    /// setter's single write); no key is removed.
    pub fn reset_at(&mut self, clip: usize, lane: Lane, frame: u32) -> Result<(), EditError> {
        let default = self.limits.lanes[lane.index()].default;
        let c = self.clips.get_mut(clip).ok_or(EditError::NoClip)?;
        let which = c.segment(lane, frame).key;
        c.write_segment(lane, which, default);
        Ok(())
    }

    /// A lane's sampled value at `frame` in editor units, unscaled (retail `82C89A50`): linear from the segment's start
    /// to its end on an interpolating lane, the start value on a stepped one.
    pub fn sample(&self, clip: usize, lane: Lane, frame: u32) -> Option<f32> {
        let c = self.clips.get(clip)?;
        let s = c.segment(lane, frame);
        let spec = self.limits.lanes[lane.index()];
        if !spec.interpolate || s.end <= s.start {
            return Some(s.from as f32);
        }
        let u = (frame.clamp(s.start, s.end) - s.start) as f32 / (s.end - s.start) as f32;
        Some(s.from as f32 + (s.to - s.from) as f32 * u)
    }

    /// A lane's editor value at `frame` (what the menu loads, retail take+0x74): the sample, rounded the retail way
    /// (floor, plus one when the fraction reaches the threshold, `82C89A50` / 0.5 at `8209975C`).
    pub fn value_at(&self, clip: usize, lane: Lane, frame: u32) -> Option<i32> {
        let x = self.sample(clip, lane, frame)?;
        let f = x.floor();
        Some(f as i32 + i32::from(x - f >= self.limits.round_threshold))
    }

    /// A lane's effect value at `frame` (editor value x the lane's scale, e.g. speed 50 -> 0.5): rounded on a lane that
    /// holds whole numbers (retail's int elements: speed), the raw sample on a float lane (the looks).
    pub fn lane_effect(&self, clip: usize, lane: Lane, frame: u32) -> Option<f32> {
        let spec = self.limits.lanes[lane.index()];
        let v = if spec.round { self.value_at(clip, lane, frame)? as f32 } else { self.sample(clip, lane, frame)? };
        Some(v * spec.scale + spec.offset)
    }

    /// Whether a look lane applies at `frame` (retail's stepped OVERRIDE flag, b408): a value was committed on the clip,
    /// or a key sits at or before `frame`. A committed neutral value still overrides the world's term.
    pub fn lane_set(&self, clip: usize, lane: Lane, frame: u32) -> bool {
        self.clips.get(clip).is_some_and(|c| c.set[lane.index()] || c.markers[lane.index()].first().is_some_and(|k| k.frame <= frame))
    }

    /// The look at `frame`: world grade with the clip's look lanes replacing terms, plus tint (b380 / b408): docs/hails-additions/37-instant-replay.md#clip-look
    pub fn look_at(&self, clip: usize, frame: u32, base: &ColourMatrixRecord) -> Option<ReplayLook> {
        self.clips.get(clip)?;
        let set = |lane: Lane| self.lane_set(clip, lane, frame).then(|| self.lane_effect(clip, lane, frame)).flatten();
        let mut record = *base;
        for band in [&mut record.near, &mut record.far] {
            if let Some(b) = set(Lane::Brightness) {
                band.add = [b; 3];
            }
            if let Some(c) = set(Lane::Contrast) {
                band.contrast = c;
            }
            if let Some(s) = set(Lane::Saturation) {
                band.saturation = s;
            }
        }
        let tint = [Lane::TintR, Lane::TintG, Lane::TintB, Lane::TintA].map(|l| set(l).unwrap_or(0.0));
        Some(ReplayLook { record, tint })
    }

    /// Retail encoded length in seconds (8250B590, b472): docs/hails-additions/37-instant-replay.md#encoded-length
    pub fn encoded_seconds(&self) -> u32 {
        let scale = self.limits.lanes[Lane::Speed.index()].scale;
        let mut frames = 0u32;
        for c in &self.clips {
            let keys = &c.markers[Lane::Speed.index()];
            let starts = std::iter::once((c.take[0], c.values[Lane::Speed.index()])).chain(keys.iter().map(|k| (k.frame, k.value)));
            let ends = keys.iter().map(|k| k.frame).chain(std::iter::once(c.take[1]));
            for ((s, v), e) in starts.zip(ends) {
                if e < c.start || s > c.end {
                    continue;
                }
                let span = e.min(c.end).saturating_sub(s.max(c.start));
                frames += (span as f32 / (v as f32 * scale).max(1.0e-3)) as u32;
            }
        }
        (frames as f32).mul_add(1.0 / self.limits.timeline_fps, self.limits.length_round_bias) as u32
    }

    /// Retail's "surpassed the designated sharing length" check.
    pub fn can_share(&self) -> bool {
        self.encoded_seconds() <= self.limits.share_max_seconds
    }
}

pub mod session;
pub mod curve;
pub mod shake;

#[cfg(test)]
mod tests;
