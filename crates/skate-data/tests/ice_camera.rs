//! Data-gated: `SKATE3_ICE` = the disc's `data/scene/ice_000.bin` (default: the setup export).
use skate_data::ice_camera::{scene_tracks, VIEW_SCENE};
use std::path::PathBuf;

fn ice() -> Option<Vec<u8>> {
    let path = std::env::var_os("SKATE3_ICE").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/private/hud/scene/ice_000.bin")
    });
    std::fs::read(path).ok()
}

#[test]
#[ignore = "needs the disc's ice_000.bin (SKATE3_ICE)"]
fn view_scene_has_the_21_trick_guide_tracks() {
    let Some(bytes) = ice() else { panic!("set SKATE3_ICE") };
    let tracks = scene_tracks(&bytes, VIEW_SCENE).unwrap();
    assert_eq!(tracks.len(), 21);
    let lengths = [4.433, 3.25, 2.395, 3.6, 2.3, 3.053, 3.033, 2.381, 4.342, 3.841, 3.033, 3.020, 3.039, 3.019, 3.483,
        3.008, 3.973, 2.407, 4.367, 4.6, 5.517];
    for (track, length) in tracks.iter().zip(lengths) {
        assert!((track.length - length).abs() < 1e-3, "{} vs {length}", track.length);
        assert!(!track.keys.is_empty());
        assert!(track.keys.windows(2).all(|w| w[0].time <= w[1].time));
        assert!(track.keys.iter().all(|k| (10.0..=500.0).contains(&k.lens_mm)));
    }
    // Track 0 (trickguide_default): constant low side view of the torso, 58.8 mm.
    let k = tracks[0].keys[0];
    assert!((k.eye[0] + 1.619).abs() < 1e-3 && (k.eye[1] - 4.465).abs() < 1e-3 && (k.eye[2] - 0.241).abs() < 1e-3);
    assert!((k.look[2] - 1.054).abs() < 1e-3);
    assert!((k.lens_mm - 58.8).abs() < 0.1);
    // Track 3 (trickguide_airs): one interior key at 32782/65535 of 3.6 s.
    assert_eq!(tracks[3].keys.len(), 3);
    assert!((tracks[3].keys[1].time - 1.8008).abs() < 1e-3);
    // Per-segment space settings, as the recomp's camera player held them (elements 88..92, 2026-10-12 trace):
    // (track, CUBIC_LOOK, SPACE_EYE, SPACE_LOOK, AVATAR_EYE, AVATAR_LOOK).
    for (t, cubic_look, space_eye, space_look, avatar_eye, avatar_look) in
        [(0, true, 2, 2, 5, 5), (1, true, 2, 2, 5, 5), (3, true, 1, 2, 7, 7), (18, true, 2, 2, 5, 5),
         (19, true, 1, 1, 0, 0), (20, true, 2, 2, 5, 5)]
    {
        assert_eq!(tracks[t].segments.len(), tracks[t].keys.len() - 1, "track {t}");
        for g in &tracks[t].segments {
            assert!(g.cubic_eye, "track {t}");
            assert_eq!((g.cubic_look, g.eye_space.mode, g.look_space.mode, g.eye_space.avatar, g.look_space.avatar),
                (cubic_look, space_eye, space_look, avatar_eye, avatar_look), "track {t}");
        }
    }
    // Every sample stays finite over each track.
    for track in &tracks {
        for i in 0..=20 {
            let s = track.sample(track.length * i as f32 / 20.0).unwrap();
            assert!(s.eye.iter().chain(&s.look).all(|v| v.is_finite()));
        }
    }
}
