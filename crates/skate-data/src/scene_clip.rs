//! Scene clips: one-clip banks from `scene.big` (Trick Guide), deltas added onto the file's pose (inferred).
//! Details: docs/hails-additions/31-retail-menus.md#scene-clips
use crate::{
    abin::{Bank, Codec, PartEntry, RecordData},
    animation_frames::SampleWords,
};
use skate_core::animation::{output::Sqt, pose_add::add, raw::RawDecoder};

fn sqt(w: SampleWords) -> Sqt {
    let [sx, sy, sz, x, y, z, w, tx, ty, tz] = w.map(f32::from_bits);
    Sqt { scale: [sx, sy, sz, 1.0], rotation: [x, y, z, w], translation: [tx, ty, tz, 1.0] }
}
fn words(s: Sqt) -> SampleWords {
    let [sx, sy, sz, _] = s.scale;
    let [x, y, z, w] = s.rotation;
    let [tx, ty, tz, _] = s.translation;
    [sx, sy, sz, x, y, z, w, tx, ty, tz].map(f32::to_bits)
}

#[derive(Clone, Debug)]
pub struct SceneClip {
    pub name: String,
    pub bone_names: Vec<String>,
    pub parents: Vec<i32>,
    pub fps: f32,
    ///Per frame, per hierarchy bone: local scaleXYZ, quaternionXYZW, translationXYZ.
    pub frames: Vec<Vec<SampleWords>>,
}

impl SceneClip {
    pub fn parse(bytes: Vec<u8>) -> Result<Self, String> {
        Self::parse_named(bytes, None)
    }

    /// The clip record named `name` (ASCII case ignored), or the bank's first clip with `None`.
    pub fn parse_named(bytes: Vec<u8>, name: Option<&str>) -> Result<Self, String> {
        let bank = Bank::parse(bytes).map_err(|e| e.to_string())?;
        let hierarchy = bank.hierarchy().ok_or("Scene clip has no hierarchy")?;
        let bones = hierarchy.bone_count as usize;
        let mut clip = None;
        let mut pose = None;
        for record in bank.records() {
            match &record.data {
                RecordData::Clip(c) if clip.is_none() && name.is_none_or(|n| record.header.name.eq_ignore_ascii_case(n)) => {
                    clip = Some((&record.header, c))
                }
                RecordData::Pose(p) if pose.is_none() => pose = Some((&record.header, p)),
                _ => {}
            }
        }
        let (header, clip) = clip.ok_or_else(|| format!("Scene clip bank has no clip {}", name.unwrap_or("")))?;
        let (pose_header, pose) = pose.ok_or("Scene clip bank has no pose")?;
        for codec in [header.codec(), pose_header.codec()] {
            if codec != Codec::Raw {
                return Err(format!("{}: scene clip codec {codec:?} is not RAW", header.name));
            }
        }
        let count = f32::from_bits(clip.frame_count_bits);
        if !count.is_finite() || count < 1.0 || count.fract() != 0.0 {
            return Err(format!("{}: invalid frame count", header.name));
        }
        let fps = f32::from_bits(clip.fps_bits);
        if !(fps.is_finite() && fps > 0.0) {
            return Err(format!("{}: invalid frame rate", header.name));
        }
        let identity = [1.0f32, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0].map(f32::to_bits);
        let base = fill(&bank, &pose.parts, 1, vec![vec![identity; bones]], true)
            .map_err(|e| format!("{} pose: {e}", header.name))?
            .remove(0);
        let deltas = fill(&bank, &clip.parts, count as usize, vec![vec![identity; bones]; count as usize], false)
            .map_err(|e| format!("{}: {e}", header.name))?;
        let frames = deltas.into_iter().map(|frame| frame.into_iter().zip(&base)
            .map(|(delta, pose)| words(add(sqt(delta), sqt(*pose), true))).collect()).collect();
        Ok(Self {
            name: header.name.clone(),
            bone_names: hierarchy.bone_names.clone(),
            parents: hierarchy.parents.clone(),
            fps,
            frames,
        })
    }
    pub fn duration(&self) -> f32 {
        self.frames.len() as f32 / self.fps
    }
}

///Write each stored part over its hierarchy slot. The slot is the part header's
///low six bits / 2 (checked: the 17 pose parts read 0, 2 .. 32 in order).
fn fill(
    bank: &Bank,
    parts: &[PartEntry],
    count: usize,
    mut frames: Vec<Vec<SampleWords>>,
    complete: bool,
) -> Result<Vec<Vec<SampleWords>>, String> {
    let hierarchy = bank.hierarchy().unwrap();
    let mut assigned = vec![false; hierarchy.parts.len()];
    for part in parts.iter().filter_map(|p| p.part.as_ref()) {
        let slot = ((part.header_word & 0x3f) >> 1) as usize;
        let layout = hierarchy.parts.get(slot).ok_or("Part slot outside hierarchy")?;
        if std::mem::replace(&mut assigned[slot], true) {
            return Err(format!("{}: part stored twice", layout.name));
        }
        if part.channel_count as u32 != layout.bone_count {
            return Err(format!("{}: channel count disagrees with hierarchy", layout.name));
        }
        if part.compressed_size == 0 {
            continue;
        }
        let decoder = RawDecoder::new(
            bank.bytes(),
            part.offset,
            part.compression_header_relative as usize,
            part.compressed_data_relative as usize,
            part.channel_count as usize,
        )?;
        if decoder.frames_in(part.compressed_size as usize) < count {
            return Err(format!("{}: fewer RAW frames than the clip header", layout.name));
        }
        let start = usize::try_from(layout.sqt_offset).map_err(|_| "Negative part offset")?;
        let channels = part.channel_count as usize;
        for index in 0..count {
            // Components the frame does not store keep the pose's (frames start as copies of it).
            let mut samples = frames[index][start..start + channels].to_vec();
            decoder.decode_into(index, &mut samples)?;
            for (channel, sample) in samples.into_iter().enumerate() {
                // Authored NaNs exist (trickguide_bs_dark_slide1 frame 103, RIGHT_WHEELBACK
                // translation): hold the previous frame's channel (frame 0: the pose).
                let finite = sample.iter().all(|w| f32::from_bits(*w).is_finite());
                if finite {
                    frames[index][start + channel] = sample;
                } else if index > 0 {
                    frames[index][start + channel] = frames[index - 1][start + channel];
                }
            }
        }
    }
    if complete && assigned.iter().any(|a| !a) {
        return Err("pose leaves hierarchy parts unset".into());
    }
    if frames.iter().flatten().flatten().any(|w| !f32::from_bits(*w).is_finite()) {
        return Err("nonfinite RAW sample".into());
    }
    Ok(frames)
}
