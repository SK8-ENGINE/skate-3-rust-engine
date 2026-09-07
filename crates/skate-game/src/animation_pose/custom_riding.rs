//! Optional retargeted riding samples inside the existing stock playback tree.
//! Graph selection, channel weights, trajectory, board and mirroring stay stock.
use super::*;
use serde::Deserialize;
use std::collections::BTreeMap;
#[derive(Default)]
pub(super) struct Replacements(BTreeMap<String, Replacement>);
#[derive(Deserialize)]
struct File {
    version: u32,
    bone_names: Vec<String>,
    replacements: Vec<Replacement>,
}
#[derive(Deserialize)]
struct Replacement {
    name: String,
    fps: f32,
    frames: Vec<Vec<[f32; 10]>>,
}
impl Replacements {
    pub fn load(root: &Path, frames: &AnimationFrames) -> Result<Self, String> {
        let path = root.join("private/custom/riding.json");
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let file: File =
            serde_json::from_str(&text).map_err(|e| format!("Riding replacements: {e}"))?;
        if file.version != 1 || file.bone_names != frames.bone_names {
            return Err("Riding replacement skeleton differs from the stock rig".into());
        }
        let mut result = BTreeMap::new();
        for c in file.replacements {
            let approved = ["R_IDLE_HCOM_000", "R_IDLE_HCOM_N100", "R_IDLE_HCOM_P100"]
                .contains(&c.name.as_str())
                || ["MED", "ML", "LNG"]
                    .iter()
                    .any(|prefix| (0..10).any(|i| c.name == format!("{prefix}_CRV_POSE_{i}")));
            if !approved
                || !c.fps.is_finite()
                || c.fps <= 0.
                || c.frames.len() < 2
                || c.frames.iter().any(|f| {
                    f.len() != frames.bone_names.len()
                        || f.iter().any(|s| {
                            !s.iter().all(|v| v.is_finite())
                                || s[..3].iter().any(|v| *v <= 0.)
                                || (s[3..7].iter().map(|v| v * v).sum::<f32>() - 1.).abs() > 0.002
                        })
                })
            {
                return Err(format!("Invalid riding replacement {}", c.name));
            }
            frames.clip(&c.name)?;
            let name = c.name.clone();
            if result.insert(name.clone(), c).is_some() {
                return Err(format!("Duplicate riding replacement {name}"));
            }
        }
        Ok(Self(result))
    }
    pub fn apply(&self, clip: &ClipFrames, time: f32, pose: &mut [Sqt]) {
        let Some(c) = self.0.get(&clip.name) else {
            return;
        };
        let duration = (clip.frames.len() - 1) as f32 / f32::from_bits(clip.fps_bits);
        let frame = (time / duration.max(0.0001)).clamp(0., 1.) * (c.frames.len() - 1) as f32;
        let i = frame.floor() as usize;
        let j = (i + 1).min(c.frames.len() - 1);
        for bone in (1..25).chain(32..36) {
            let decode = |s: [f32; 10]| sqt(s.map(f32::to_bits));
            let weight = pose[bone].translation[3];
            pose[bone] = pose_blend::blend_sample(
                decode(c.frames[i][bone]),
                decode(c.frames[j][bone]),
                frame.fract(),
            );
            pose[bone].translation[3] = weight;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires private stock banks and retargeted riding clips"]
    fn riding_replacements_keep_board_trajectory_and_unrelated_clips_stock() {
        let root = std::env::var_os("SKATE3_ASSET_ROOT").unwrap();
        let banks = AnimationBanks::load(Path::new(&root)).unwrap();
        let stock = PoseEvaluator::from_banks(&banks).unwrap();
        let mut custom = PoseEvaluator::from_banks(&banks).unwrap();
        custom.load_riding(Path::new(&root)).unwrap();
        assert_eq!(custom.riding.0.len(), 33);
        let command = |name: &str, time| PoseCommand::Clip {
            name: name.into(),
            time,
            previous_time: 0.,
            loops: 0,
        };
        for name in custom.riding.0.keys() {
            let clip = custom.frames.clip(name).unwrap();
            for fraction in [0., 0.25, 0.5, 1.] {
                let time =
                    fraction * (clip.frames.len() - 1) as f32 / f32::from_bits(clip.fps_bits);
                let a = stock.evaluate(&[command(name, time)]).unwrap();
                let b = custom.evaluate(&[command(name, time)]).unwrap();
                for i in std::iter::once(0).chain(25..32) {
                    assert_eq!(a[i], b[i], "{name} changed board/trajectory bone {i}");
                }
                for (a, b) in a.iter().zip(&b) {
                    assert_eq!(a.translation[3], b.translation[3]);
                }
                assert_ne!(a[5], b[5], "{name} did not replace torso");
                assert!(b.iter().all(|s| {
                    s.rotation
                        .iter()
                        .chain(&s.translation)
                        .all(|v| v.is_finite())
                }));
            }
        }
        for name in [
            "R_IDLE_LCOM_000",
            "R_IDLE_SPEEDTUCK_N_0_CYC",
            "R_CRPUSHHSP_HSTR_LEFT_0_CYC1",
        ] {
            assert_eq!(
                stock.evaluate(&[command(name, 0.2)]).unwrap(),
                custom.evaluate(&[command(name, 0.2)]).unwrap()
            );
        }
    }
}
