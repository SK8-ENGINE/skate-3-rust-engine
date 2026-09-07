//! Optional 360-flip body replacement, baked into the stock G/A clip slots.
//! The stock graph still owns timing, attributes, trajectory and mirroring.
use super::*;
use bevy::math::{Mat4, Quat, Vec3};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Replacements(BTreeMap<String, ClipFrames>);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    version: u32,
    bone_names: Vec<String>,
    ground_last_frame: usize,
    air_first_frame: usize,
    /// Absolute native-space joint matrices, column major; not Blender axes.
    frames: Vec<Vec<[f32; 16]>>,
}

impl Replacements {
    pub fn load(root: &Path, frames: &AnimationFrames) -> Result<Self, String> {
        let path = root.join("private/custom/360flip.json");
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        Self::parse(&text, frames).map_err(|e| format!("{}: {e}", path.display()))
    }

    fn parse(text: &str, frames: &AnimationFrames) -> Result<Self, String> {
        let file: File = serde_json::from_str(text).map_err(|e| e.to_string())?;
        file.validate(&frames.bone_names)?;
        let bind = &frames.named_pose("RIG_TPOSE")?.samples;
        let index = |name: &str| {
            frames
                .bone_names
                .iter()
                .position(|n| n == name)
                .ok_or_else(|| format!("360-flip skeleton is missing {name}"))
        };
        let hips = index("HIPS")?;
        let board = index("SKATEBOARD_ROOT")?;
        let helpers = [
            (index("RIGHTTOEBASE_REPARENTED")?, index("RIGHTTOEBASE")?),
            (index("LEFTTOEBASE_REPARENTED")?, index("LEFTTOEBASE")?),
            (index("RIGHTHAND_REPARENTED")?, index("RIGHTHAND")?),
            (index("LEFTHAND_REPARENTED")?, index("LEFTHAND")?),
        ];
        let mut replacements = BTreeMap::new();
        // Ordinary ollie 360 flips only. Nollie and dark-catch slots stay stock.
        for style in ["D", "GONZ", "HSU"] {
            for height in ["LOW", "HIGH"] {
                for phase in ["G", "A"] {
                    let name = format!("360FLIP_{style}_{height}_{phase}");
                    let stock = frames.clip(&name)?;
                    let mut baked = stock.frames.clone();
                    let (start, end) = if phase == "G" {
                        (0, file.ground_last_frame)
                    } else {
                        (file.air_first_frame, file.frames.len() - 1)
                    };
                    for (i, row) in baked.iter_mut().enumerate() {
                        let position = start as f32
                            + (end - start) as f32 * i as f32
                                / (stock.frames.len() - 1).max(1) as f32;
                        // Match the stock entry/exit poses over three source
                        // frames, without restarting the fade at the G/A seam.
                        let distance = position.min((file.frames.len() - 1) as f32 - position);
                        let t = (distance / 3.0).clamp(0.0, 1.0);
                        let weight = t * t * (3.0 - 2.0 * t);
                        if weight == 0.0 {
                            continue;
                        }
                        let mut globals = file.sample(position);
                        // Restore board hierarchy from THIS stock style/height
                        // and frame. Reparented IK targets use that moving board.
                        let mut stock_globals = vec![Mat4::IDENTITY; row.len()];
                        for bone in 1..row.len() {
                            let parent = frames.parents[bone];
                            let local =
                                matrix(pose_add::add(sqt(row[bone]), sqt(bind[bone]), true));
                            stock_globals[bone] = if parent > 0 {
                                stock_globals[parent as usize] * local
                            } else {
                                local
                            };
                        }
                        globals[0] = Mat4::IDENTITY;
                        for bone in board..helpers.iter().map(|p| p.0).min().unwrap() {
                            globals[bone] = stock_globals[bone];
                        }
                        for &(helper, joint) in &helpers {
                            globals[helper] = globals[joint];
                        }
                        for bone in (hips..board).chain(helpers.iter().map(|p| p.0)) {
                            let parent = frames.parents[bone];
                            let local = if parent > 0 {
                                globals[parent as usize].inverse() * globals[bone]
                            } else {
                                globals[bone]
                            };
                            let replacement = remove_reference(local, sqt(bind[bone]));
                            let blended =
                                pose_blend::blend_sample(sqt(row[bone]), replacement, weight);
                            row[bone] = words(blended);
                        }
                    }
                    replacements.insert(
                        name.clone(),
                        ClipFrames {
                            name,
                            source_offset: stock.source_offset,
                            fps_bits: stock.fps_bits,
                            loop_translation_bits: stock.loop_translation_bits,
                            loop_rotation_bits: stock.loop_rotation_bits,
                            channel_animation: stock.channel_animation,
                            channel_weights: stock.channel_weights.clone(),
                            frames: baked,
                        },
                    );
                }
            }
        }
        Ok(Self(replacements))
    }

    pub fn clip(&self, name: &str) -> Option<&ClipFrames> {
        self.0.get(name)
    }
}

impl File {
    fn validate(&self, names: &[String]) -> Result<(), String> {
        if self.version != 1
            || self.bone_names != names
            || self.frames.len() < 4
            || self.frames.len() > 600
            || self.ground_last_frame == 0
            || self.ground_last_frame.checked_add(1) != Some(self.air_first_frame)
            || self.air_first_frame >= self.frames.len() - 1
        {
            return Err("Invalid 360-flip format, skeleton or ground/air split".into());
        }
        for row in &self.frames {
            if row.len() != names.len() {
                return Err("Incomplete 360-flip frame".into());
            }
            for values in row {
                let m = Mat4::from_cols_array(values);
                let (scale, rotation, _) = m.to_scale_rotation_translation();
                if !m.is_finite()
                    || (m.w_axis.w - 1.0).abs() > 0.001
                    || [m.x_axis.w, m.y_axis.w, m.z_axis.w]
                        .iter()
                        .any(|w| w.abs() > 0.001)
                    || scale.min_element() < 0.95
                    || scale.max_element() > 1.05
                    || !rotation.is_finite()
                    || (rotation.length_squared() - 1.0).abs() > 0.002
                {
                    return Err("Invalid 360-flip joint transform".into());
                }
            }
        }
        Ok(())
    }

    fn sample(&self, position: f32) -> Vec<Mat4> {
        let first = position.floor() as usize;
        let second = (first + 1).min(self.frames.len() - 1);
        self.frames[first]
            .iter()
            .zip(&self.frames[second])
            .map(|(a, b)| {
                let (sa, qa, ta) = Mat4::from_cols_array(a).to_scale_rotation_translation();
                let (sb, qb, tb) = Mat4::from_cols_array(b).to_scale_rotation_translation();
                let t = position.fract();
                Mat4::from_scale_rotation_translation(
                    sa.lerp(sb, t),
                    qa.slerp(qb, t),
                    ta.lerp(tb, t),
                )
            })
            .collect()
    }
}

fn matrix(s: Sqt) -> Mat4 {
    Mat4::from_scale_rotation_translation(
        Vec3::from_slice(&s.scale),
        Quat::from_array(s.rotation),
        Vec3::from_slice(&s.translation),
    )
}

fn remove_reference(local: Mat4, reference: Sqt) -> Sqt {
    let (scale, rotation, translation) = local.to_scale_rotation_translation();
    let inverse = Quat::from_array(reference.rotation).inverse();
    let scale = scale / Vec3::from_slice(&reference.scale);
    // AddSQT rotates translation without applying the reference scale.
    let translation = inverse * (translation - Vec3::from_slice(&reference.translation));
    Sqt {
        scale: [scale.x, scale.y, scale.z, 1.0],
        rotation: (inverse * rotation).normalize().to_array(),
        translation: [translation.x, translation.y, translation.z, 1.0],
    }
}

fn words(s: Sqt) -> SampleWords {
    [
        s.scale[0],
        s.scale[1],
        s.scale[2],
        s.rotation[0],
        s.rotation[1],
        s.rotation[2],
        s.rotation[3],
        s.translation[0],
        s.translation[1],
        s.translation[2],
    ]
    .map(f32::to_bits)
}

#[cfg(test)]
#[path = "custom_tricks_tests.rs"]
mod tests;
