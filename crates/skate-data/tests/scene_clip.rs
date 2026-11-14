use skate_data::scene_clip::SceneClip;
use std::path::PathBuf;

///Owned scene clips: `SKATE3_SCENE_CLIPS` or the dev install's stock extract.
fn clip(name: &str) -> Option<SceneClip> {
    let dir = std::env::var_os("SKATE3_SCENE_CLIPS").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/private/hud/scene/trickguide")
    });
    let bytes = std::fs::read(dir.join(name)).ok()?;
    Some(SceneClip::parse(bytes).unwrap())
}

#[test]
#[ignore = "needs owned scene.big clips"]
fn trick_guide_clip_decodes_the_full_character_pose() {
    let Some(c) = clip("trickguide_late_kickflip_011.abin") else {
        eprintln!("skipped: no scene clips");
        return;
    };
    assert_eq!(c.name, "TRICKGUIDE_LATE_KICKFLIP_01");
    assert_eq!((c.bone_names.len(), c.frames.len(), c.fps), (131, 272, 60.0));
    assert_eq!(c.bone_names[0], "TRAJECTORY");
    let f = |frame: usize, bone: usize| c.frames[frame][bone].map(f32::from_bits);
    // Float translations, read independently with the pinned skate3_anim importer (its packed rotations are
    // wrong: they predate the TU3 sampler read, see raw.rs `unpack_rotation`).
    let root = f(0, 0);
    assert!((root[7] - 39.128899).abs() < 1e-4 && (root[8] - 0.864247).abs() < 1e-4);
    assert!((f(271, 0)[7] - 70.606941).abs() < 1e-4);
    // Every rotation is a unit quaternion, including the pose-filled face bones.
    for frame in &c.frames {
        for s in frame {
            let q = &s.map(f32::from_bits)[3..7];
            assert!((q.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 1e-3);
        }
    }
}

#[test]
#[ignore = "needs owned scene.big clips"]
fn every_trick_guide_clip_decodes() {
    let dir = std::env::var_os("SKATE3_SCENE_CLIPS").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/private/hud/scene/trickguide")
    });
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("skipped: no scene clips");
        return;
    };
    let mut count = 0;
    for entry in entries {
        let path = entry.unwrap().path();
        let clip = SceneClip::parse(std::fs::read(&path).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(clip.bone_names.len(), 131, "{}", path.display());
        count += 1;
    }
    assert_eq!(count, 312);
}

#[test]
#[ignore = "needs owned scene.big clips"]
fn bind_pose_stands_upright_over_the_scene_origin() {
    use skate_core::animation::output::{self, Sqt};
    let Some(c) = clip("trickguide_bindpose1.abin") else {
        eprintln!("skipped: no scene clips");
        return;
    };
    let mut g: Vec<_> = c.frames[0].iter().map(|w| {
        let [sx, sy, sz, x, y, z, w, tx, ty, tz] = w.map(f32::from_bits);
        output::sqt_to_matrix(Sqt { scale: [sx, sy, sz, 1.0], rotation: [x, y, z, w], translation: [tx, ty, tz, 1.0] })
    }).collect();
    // Root bones hang from the trajectory (the scene hierarchy leaves HIPS at -1).
    let parents: Vec<i32> = c.parents.iter().enumerate().map(|(b, &p)| if p == -1 && b != 0 { 0 } else { p }).collect();
    output::compose_hierarchy_in_place(g.len() as i32, &parents, -1, &mut g).unwrap();
    let at = |n: &str| g[c.bone_names.iter().position(|b| b == n).unwrap()][3];
    let (hips, head, left, right) = (at("HIPS"), at("HEAD"), at("LEFTFOOT"), at("RIGHTFOOT"));
    // Standing T-pose: hips at the pelvis height, head over them, feet on the ground, mirrored left / right.
    assert!((hips[1] - 0.921).abs() < 0.01 && head[1] > 1.5 && head[0].abs() < 0.02, "hips {hips:?} head {head:?}");
    assert!(left[1] < 0.2 && right[1] < 0.2 && (left[0] + right[0]).abs() < 0.01, "feet {left:?} {right:?}");
}

#[test]
#[ignore = "needs owned scene.big clips"]
fn every_trick_guide_clip_starts_and_ends_upright() {
    use skate_core::animation::output::{self, Sqt};
    let dir = std::env::var_os("SKATE3_SCENE_CLIPS").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/private/hud/scene/trickguide")
    });
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("skipped: no scene clips");
        return;
    };
    let mut failed = Vec::new();
    for entry in entries {
        let path = entry.unwrap().path();
        let c = SceneClip::parse(std::fs::read(&path).unwrap()).unwrap();
        let parents: Vec<i32> = c.parents.iter().enumerate().map(|(b, &p)| if p == -1 && b != 0 { 0 } else { p }).collect();
        let bone = |n: &str| c.bone_names.iter().position(|b| b == n).unwrap();
        for frame in [0, c.frames.len() - 1] {
            let mut g: Vec<_> = c.frames[frame].iter().map(|w| {
                let [sx, sy, sz, x, y, z, w, tx, ty, tz] = w.map(f32::from_bits);
                output::sqt_to_matrix(Sqt { scale: [sx, sy, sz, 1.0], rotation: [x, y, z, w], translation: [tx, ty, tz, 1.0] })
            }).collect();
            output::compose_hierarchy_in_place(g.len() as i32, &parents, -1, &mut g).unwrap();
            let y = |n: &str| g[bone(n)][3][1];
            let upright = y("HEAD") > y("HIPS") + 0.3 && y("LEFTFOOT") < y("HIPS") && y("RIGHTFOOT") < y("HIPS")
                && y("LEFTHAND") < y("HEAD") + 0.3 && y("RIGHTHAND") < y("HEAD") + 0.3;
            if !upright {
                failed.push(format!("{} frame {frame}: hips {:.2} head {:.2} feet {:.2} {:.2} hands {:.2} {:.2}",
                    c.name, y("HIPS"), y("HEAD"), y("LEFTFOOT"), y("RIGHTFOOT"), y("LEFTHAND"), y("RIGHTHAND")));
            }
        }
    }
    for f in &failed {
        eprintln!("{f}");
    }
    eprintln!("{} clip ends not upright", failed.len());
}

/// FE 62's preview clip (MarqueeViewer, b579): `MARQUEE_STAND` from the stock `sk83_marquee_viewer.abin`, 30 fps, 501
/// frames, upright. Data-gated (`SKATE3_ASSET_ROOT`).
#[test]
fn marquee_stand_parses_from_the_stock_bank() {
    let Some(root) = std::env::var_os("SKATE3_ASSET_ROOT").map(std::path::PathBuf::from) else { return };
    let path = root.join("private/stock/data/anim/sk83_marquee_viewer.abin");
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("skipped: no {}", path.display());
        return;
    };
    let c = SceneClip::parse_named(bytes, Some("MARQUEE_STAND")).unwrap();
    assert_eq!((c.fps, c.frames.len()), (30.0, 501));
    assert!(c.bone_names.iter().any(|b| b == "HIPS"));
}
