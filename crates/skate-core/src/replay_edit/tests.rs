use super::*;

#[test]
fn replay_editor_opens_at_retail_150_frames() {
    let edit = ReplayEdit::default();
    assert!(!edit.can_open(149));
    assert!(edit.can_open(150));
}

#[test]
fn replay_editor_steps_clamps_and_wraps_like_retail() {
    let mut edit = ReplayEdit::default();
    let clip = edit.add_clip(0, 300).unwrap();
    // Speed: default 100, steps of 10, never below 10 or above 100.
    assert_eq!(edit.clips[clip].values[Lane::Speed.index()], 100);
    assert_eq!(edit.adjust(clip, Lane::Speed, 1).unwrap(), 100);
    for _ in 0..20 {
        edit.adjust(clip, Lane::Speed, -1).unwrap();
    }
    assert_eq!(edit.clips[clip].values[Lane::Speed.index()], 10);
    // Shake wraps 3 -> 0 and 0 -> 3.
    assert_eq!(edit.adjust(clip, Lane::Shake, -1).unwrap(), 3);
    assert_eq!(edit.adjust(clip, Lane::Shake, 1).unwrap(), 0);
    // Contrast 10..30 step 2; brightness -100..100 step 20; tint 0..250 step 25.
    assert_eq!(edit.adjust(clip, Lane::Contrast, 1).unwrap(), 12);
    assert_eq!(edit.adjust(clip, Lane::Brightness, -1).unwrap(), -20);
    assert_eq!(edit.adjust(clip, Lane::TintR, 1).unwrap(), 25);
    assert_eq!(Lane::from_id("tint_a"), Some(Lane::TintA));
    assert!(LANES.iter().all(|l| Lane::from_id(l.id()) == Some(*l)));
}

#[test]
fn replay_editor_keys_keep_retail_spacing_and_cap() {
    let mut edit = ReplayEdit::default();
    // Span 1000: 0.0089 = 8.9 frames; a key within 8 frames of the cursor is "at the cursor".
    let clip = edit.add_clip(0, 1000).unwrap();
    assert_eq!(edit.insert_hard(clip, Lane::Shake, 5), Err(EditError::TooClose));
    assert_eq!(edit.insert_hard(clip, Lane::Shake, 995), Err(EditError::TooClose));
    assert_eq!(edit.insert_hard(clip, Lane::Shake, 1200), Err(EditError::OutsideClip));
    edit.insert_hard(clip, Lane::Shake, 500).unwrap();
    assert_eq!(edit.insert_hard(clip, Lane::Shake, 506), Err(EditError::TooClose));
    assert_eq!(edit.key_near(clip, Lane::Shake, 506), Some(0));
    edit.insert_hard(clip, Lane::Shake, 520).unwrap();
    assert_eq!(edit.clips[clip].markers[Lane::Shake.index()].len(), 2);
    // The cap (retail 100, `82613500`).
    let clip = edit.add_clip(0, 10_000).unwrap();
    for k in 1..=100 {
        edit.insert_hard(clip, Lane::Brightness, 90 * k).unwrap();
    }
    assert_eq!(edit.insert_hard(clip, Lane::Brightness, 9500), Err(EditError::NoMoreMarkers));
    // Trimming keeps every key (the heads only clip the take, b472).
    edit.trim(clip, 0, 1000).unwrap();
    assert_eq!(edit.clips[clip].markers[Lane::Brightness.index()].len(), 100);
}

/// b465 / b466: a fresh clip has no keys; a commit that adds no key to an empty lane is lost (the lane reads its
/// default), unless a mod turns `empty_lane_commit` on.
#[test]
fn a_commit_on_an_empty_lane_is_lost_like_retail() {
    let mut edit = ReplayEdit::default();
    let clip = edit.add_clip(0, 900).unwrap();
    assert_eq!(edit.commit(clip, Lane::Speed, 300, 50, false), Ok(Commit::Lost));
    assert_eq!(edit.value_at(clip, Lane::Speed, 300), Some(100));
    assert_eq!(edit.commit(clip, Lane::Brightness, 300, 40, false), Ok(Commit::Lost));
    assert!(!edit.lane_set(clip, Lane::Brightness, 300));
    // After a D-pad Down key at 10 s the same commit lands on the segment under the playhead.
    edit.insert_hard(clip, Lane::Speed, 300).unwrap();
    assert_eq!(edit.commit(clip, Lane::Speed, 450, 50, false), Ok(Commit::Segment));
    assert_eq!((edit.value_at(clip, Lane::Speed, 299), edit.value_at(clip, Lane::Speed, 300)), (Some(100), Some(50)));
    assert_eq!(edit.value_at(clip, Lane::Speed, 899), Some(50));
    // A mod lets the empty-lane commit land on the whole clip.
    let limits = EditLimits { empty_lane_commit: true, ..Default::default() };
    let mut modded = ReplayEdit::new(limits);
    let clip = modded.add_clip(0, 900).unwrap();
    assert_eq!(modded.commit(clip, Lane::Speed, 300, 50, false), Ok(Commit::Segment));
    assert_eq!(modded.value_at(clip, Lane::Speed, 10), Some(50));
}

/// b461 tests 1 / 2 / 4 / 5: the base game's B writes the segment between the clip markers (hard keys on speed and
/// shake), the neighbours keep their values, and over a hard key nothing is written.
#[test]
fn a_base_commit_writes_the_segment_between_clip_markers() {
    let mut edit = ReplayEdit::default();
    let clip = edit.add_clip(0, 1200).unwrap();
    for f in [600, 840, 1020] {
        edit.insert_hard(clip, Lane::Speed, f).unwrap();
        edit.insert_hard(clip, Lane::Shake, f).unwrap();
    }
    assert_eq!(edit.commit(clip, Lane::Speed, 900, 50, false), Ok(Commit::Segment));
    assert_eq!(edit.value_at(clip, Lane::Speed, 839), Some(100));
    assert_eq!((edit.value_at(clip, Lane::Speed, 840), edit.value_at(clip, Lane::Speed, 1019)), (Some(50), Some(50)));
    assert_eq!(edit.value_at(clip, Lane::Speed, 1020), Some(100));
    assert_eq!(edit.commit(clip, Lane::Shake, 630, 2, false), Ok(Commit::Segment));
    assert_eq!((edit.value_at(clip, Lane::Shake, 700), edit.value_at(clip, Lane::Shake, 850)), (Some(2), Some(0)));
    // Over the 840 key: nothing.
    assert_eq!(edit.commit(clip, Lane::Speed, 842, 20, false), Ok(Commit::Lost));
    assert_eq!(edit.value_at(clip, Lane::Speed, 900), Some(50));
    // The lane's keys stay exactly the clip markers.
    let frames: Vec<u32> = edit.clips[clip].markers[Lane::Speed.index()].iter().map(|k| k.frame).collect();
    assert_eq!(frames, [600, 840, 1020]);
}

/// b461 tests 7 / 9 / 10 / 11, b463 / b464: the film pack's soft keys. X adds one (an empty lane too), A adds one only
/// next to a soft key, A over a soft key writes its segment (a soft next key shares the value), over a hard key
/// nothing. Speed ramps across soft keys, rounded half up; it holds up to a hard key.
#[test]
fn pack_commits_add_soft_keys_and_speed_ramps_into_them() {
    let mut edit = ReplayEdit::default();
    let clip = edit.add_clip(0, 1800).unwrap();
    // X on an empty lane: a soft key at 20 s (frame 600) with 50; the clip's start ramps 100 -> 50 into it.
    assert_eq!(edit.commit(clip, Lane::Speed, 600, 50, true), Ok(Commit::SoftKey));
    assert_eq!(edit.value_at(clip, Lane::Speed, 300), Some(75));
    assert_eq!(edit.value_at(clip, Lane::Speed, 1500), Some(50), "ends at the clip end (a cut) on its own value");
    // A next to that soft key adds a second one at 40 s: 30 s = 75, and 50.5 rounds half up.
    assert!(!edit.clips[clip].cut_both_sides(Lane::Speed, 1200));
    assert_eq!(edit.commit(clip, Lane::Speed, 1200, 100, true), Ok(Commit::SoftKey));
    assert_eq!(edit.value_at(clip, Lane::Speed, 900), Some(75));
    assert_eq!(edit.sample(clip, Lane::Speed, 606), Some(50.5));
    assert_eq!(edit.value_at(clip, Lane::Speed, 606), Some(51));
    assert!((edit.lane_effect(clip, Lane::Speed, 900).unwrap() - 0.75).abs() < 1e-6);
    // A over the 20 s soft key: that key and the shared next soft key (40 s) take the value.
    assert_eq!(edit.commit(clip, Lane::Speed, 603, 30, false), Ok(Commit::Segment));
    assert_eq!((edit.value_at(clip, Lane::Speed, 900), edit.value_at(clip, Lane::Speed, 1200)), (Some(30), Some(30)));
    assert_eq!(edit.clips[clip].markers[Lane::Speed.index()].len(), 2);
    // A hard key splits without changing the curve.
    let mut edit = ReplayEdit::default();
    let clip = edit.add_clip(0, 1800).unwrap();
    edit.commit(clip, Lane::Speed, 600, 50, true).unwrap();
    edit.commit(clip, Lane::Speed, 1200, 100, true).unwrap();
    edit.insert_hard(clip, Lane::Speed, 900).unwrap();
    assert_eq!((edit.value_at(clip, Lane::Speed, 750), edit.value_at(clip, Lane::Speed, 1050)), (Some(63), Some(88)));
    // Over the hard key: nothing.
    assert_eq!(edit.commit(clip, Lane::Speed, 900, 10, false), Ok(Commit::Lost));
    // Segments cut on both sides: A writes the segment, no key.
    let mut edit = ReplayEdit::default();
    let clip = edit.add_clip(0, 1800).unwrap();
    edit.insert_hard(clip, Lane::Speed, 600).unwrap();
    assert!(edit.clips[clip].cut_both_sides(Lane::Speed, 750));
    assert_eq!(edit.commit(clip, Lane::Speed, 750, 50, false), Ok(Commit::Segment));
    assert_eq!((edit.value_at(clip, Lane::Speed, 599), edit.value_at(clip, Lane::Speed, 1700)), (Some(100), Some(50)));
    assert_eq!(edit.clips[clip].markers[Lane::Speed.index()].len(), 1);
    // Removing a hard key takes the soft keys around it up to the next hard key.
    edit.commit(clip, Lane::Speed, 1200, 70, true).unwrap();
    assert_eq!(edit.remove_key(clip, Lane::Speed, 1203), Some(true));
    edit.commit(clip, Lane::Speed, 1200, 70, true).unwrap();
    assert_eq!(edit.remove_key(clip, Lane::Speed, 600), Some(false));
    assert!(edit.clips[clip].markers[Lane::Speed.index()].is_empty());
    assert_eq!(edit.remove_key(clip, Lane::Speed, 600), None);
}

#[test]
fn replay_editor_encoded_length_follows_retail_formula() {
    let mut edit = ReplayEdit::default();
    // 30 s at 100 %: 30 s.
    let a = edit.add_clip(0, 900).unwrap();
    assert_eq!(edit.encoded_seconds(), 30);
    assert!(edit.can_share());
    // The same clip at 50 % plays 60 s: still shareable; a 10 frame second clip pushes it over.
    for _ in 0..5 {
        edit.adjust(a, Lane::Speed, -1).unwrap();
    }
    assert_eq!(edit.encoded_seconds(), 60);
    assert!(edit.can_share());
    edit.add_clip(900, 920).unwrap();
    assert_eq!(edit.encoded_seconds(), 61);
    assert!(!edit.can_share());
    assert_eq!(edit.add_clip(5, 5), Err(EditError::EmptySpan));
}

#[test]
fn replay_look_replaces_the_world_grade_terms_it_sets() {
    use crate::colour_matrix::{ColourMatrixRecord, apply, build};
    let mut edit = ReplayEdit::default();
    let clip = edit.add_clip(0, 1000).unwrap();
    let mut base = ColourMatrixRecord::default();
    base.near.add = [0.05; 3];
    base.far.saturation = 0.5;
    // Untouched lanes keep the world's grade.
    let look = edit.look_at(clip, 10, &base).unwrap();
    assert_eq!(look.record, base);
    assert_eq!(look.tint, [0.0; 4]);
    // Saturation 100 (x2, retail's SATURATE + 1.0) replaces both bands; brightness +40 is the add term; contrast 20 = x2.
    assert_eq!(edit.limits.lanes[Lane::Saturation.index()].default, 0);
    for _ in 0..10 {
        edit.adjust(clip, Lane::Saturation, 1).unwrap();
    }
    edit.adjust(clip, Lane::Brightness, 1).unwrap();
    edit.adjust(clip, Lane::Brightness, 1).unwrap();
    for _ in 0..5 {
        edit.adjust(clip, Lane::Contrast, 1).unwrap();
    }
    let look = edit.look_at(clip, 10, &base).unwrap();
    assert!((look.record.near.saturation - 2.0).abs() < 1e-6);
    assert!((look.record.far.saturation - 2.0).abs() < 1e-6);
    assert!((look.record.near.add[0] - 0.4).abs() < 1e-6 && (look.record.far.add[2] - 0.4).abs() < 1e-6);
    assert!((look.record.near.contrast - 2.0).abs() < 1e-6);
    // Mid-grey red at x2 saturation (contrast and brightness neutral): twice the distance from luma.
    let mut neutral = look.record.near;
    neutral.contrast = 1.0;
    neutral.add = [0.0; 3];
    let red = apply(&build(&neutral), [1.0, 0.0, 0.0]);
    assert!((red[0] - 1.7).abs() < 1e-5 && (red[1] + 0.3).abs() < 1e-5, "{red:?}");
    // At 0 the lane is the identity: retail's untouched default.
    let mut zero = ReplayEdit::default();
    let c0 = zero.add_clip(0, 1000).unwrap();
    zero.commit(c0, Lane::Saturation, 0, 0, true).unwrap();
    assert_eq!(zero.look_at(c0, 10, &ColourMatrixRecord::default()).unwrap().record.near.saturation, 1.0);
    // A tint key shows from its frame on.
    edit.commit(clip, Lane::TintA, 500, 250, true).unwrap();
    assert_eq!(edit.look_at(clip, 499, &base).unwrap().tint[3], 0.0);
    assert!((edit.look_at(clip, 500, &base).unwrap().tint[3] - 250.0 / 255.0).abs() < 1e-6);
}

#[test]
fn retail_trigger_scrub_eases_in_caps_and_is_frame_rate_independent() {
    let spec = ScrubSpec::default();
    // Full trigger for one second: eases in, then 0.005 of the recording per console frame.
    let mut a = ScrubFilter::default();
    let first = a.step(&spec, 1.0, 1.0 / 30.0);
    assert!((first - 0.00125).abs() < 1e-7, "{first}");
    let mut moved = first;
    for _ in 1..30 {
        moved += a.step(&spec, 1.0, 1.0 / 30.0);
    }
    assert!(moved > 0.12 && moved < 0.15, "{moved}");
    assert!((a.rate - 0.005).abs() < 1e-5);
    // The same second at 144 fps moves the same distance.
    let mut b = ScrubFilter::default();
    let fast: f32 = (0..144).map(|_| b.step(&spec, 1.0, 1.0 / 144.0)).sum();
    assert!((fast - moved).abs() < 1e-4, "{fast} vs {moved}");
    // Released: the rate decays into the dead band and stops.
    for _ in 0..60 {
        a.step(&spec, 0.0, 1.0 / 30.0);
    }
    assert_eq!(a.rate, 0.0);
    assert_eq!(a.step(&spec, 0.0, 1.0 / 30.0), 0.0);
}

#[test]
fn replay_editor_session_follows_retail_mode_transitions() {
    use super::session::{EditorAction, EditorMode, EditorSession, key};
    let mut s = EditorSession::instant();
    assert_eq!(s.mode, EditorMode::Instant);
    // Instant replay: D-pad jumps to the ends, A opens the timeline.
    assert_eq!(s.press(key::DPAD_LEFT_PRESSED), Some(EditorAction::JumpToStart));
    assert_eq!(s.press(key::B), None, "B is not bound in instant mode");
    assert_eq!(s.press(key::A), Some(EditorAction::OpenTimeline));
    assert_eq!(s.mode, EditorMode::Timeline);
    // LB held trims until it is released.
    assert_eq!(s.press(key::LB), Some(EditorAction::TrimStart));
    assert_eq!(s.mode, EditorMode::Trim);
    assert_eq!(s.press(key::LB_RELEASED), Some(EditorAction::TrimEnd));
    assert_eq!(s.mode, EditorMode::Timeline);
    // RB plays to the clip's end, then back to the timeline; B opens Finished.
    assert_eq!(s.press(key::RB), Some(EditorAction::Play));
    s.playback_ended();
    assert_eq!(s.mode, EditorMode::Timeline);
    assert_eq!(s.press(key::B), Some(EditorAction::Finished));
}

#[test]
fn look_lanes_ramp_between_markers_and_a_committed_neutral_value_still_overrides() {
    use crate::colour_matrix::ColourMatrixRecord;
    let mut edit = ReplayEdit::default();
    let clip = edit.add_clip(0, 1000).unwrap();
    let mut base = ColourMatrixRecord::default();
    base.near.add = [0.05; 3];
    // Brightness soft keys 0 at 200 and +100 at 600: ramps between them, holds after, unset before the first.
    edit.commit(clip, Lane::Brightness, 200, 0, true).unwrap();
    edit.commit(clip, Lane::Brightness, 600, 100, true).unwrap();
    assert_eq!(edit.look_at(clip, 100, &base).unwrap().record.near.add, [0.05; 3]);
    // At the first key the committed neutral 0 replaces the world's 0.05 (retail's OVERRIDE flag).
    assert_eq!(edit.look_at(clip, 200, &base).unwrap().record.near.add, [0.0; 3]);
    assert!((edit.look_at(clip, 400, &base).unwrap().record.near.add[0] - 0.5).abs() < 1e-6);
    assert!((edit.look_at(clip, 900, &base).unwrap().record.far.add[1] - 1.0).abs() < 1e-6);
    // Shake's level steps at its keys (b464).
    edit.commit(clip, Lane::Shake, 200, 1, true).unwrap();
    edit.commit(clip, Lane::Shake, 600, 3, true).unwrap();
    assert_eq!(edit.value_at(clip, Lane::Shake, 599), Some(1));
}

/// b460: a clip marker delete resets speed / shake over the merged span (lane default from the span's start, the
/// value in effect at its end kept), or with the film pack only the lane value in effect at the cursor.
#[test]
fn a_clip_delete_resets_the_merged_span_to_the_lane_defaults() {
    let mut edit = ReplayEdit::default();
    let clip = edit.add_clip(0, 300).unwrap();
    edit.clips[clip].markers[Lane::Speed.index()] = vec![LaneKey::hard(100, 50), LaneKey::hard(200, 70)];
    edit.reset_span(clip, Lane::Speed, 80, 250).unwrap();
    assert_eq!(edit.value_at(clip, Lane::Speed, 79), Some(100));
    assert_eq!(edit.value_at(clip, Lane::Speed, 80), Some(100));
    assert_eq!(edit.value_at(clip, Lane::Speed, 249), Some(100));
    assert_eq!(edit.value_at(clip, Lane::Speed, 250), Some(70), "the clip right of the span keeps its value");
    // From the clip start: the clip value takes the default, no marker at the start.
    edit.clips[clip].values[Lane::Shake.index()] = 2;
    edit.clips[clip].markers[Lane::Shake.index()] = vec![LaneKey::hard(120, 3)];
    edit.reset_span(clip, Lane::Shake, 0, 150).unwrap();
    assert_eq!(edit.clips[clip].values[Lane::Shake.index()], 0);
    assert_eq!(edit.clips[clip].markers[Lane::Shake.index()], vec![LaneKey::hard(150, 3)]);
    // To the clip end: nothing to restore.
    edit.reset_span(clip, Lane::Shake, 0, 300).unwrap();
    assert!(edit.clips[clip].markers[Lane::Shake.index()].is_empty());
    // The pack's form: the marker in effect at the frame takes the default, the others stay.
    edit.clips[clip].markers[Lane::Speed.index()] = vec![LaneKey::hard(100, 50), LaneKey::hard(200, 70)];
    edit.reset_at(clip, Lane::Speed, 150).unwrap();
    assert_eq!(edit.clips[clip].markers[Lane::Speed.index()], vec![LaneKey::hard(100, 100), LaneKey::hard(200, 70)]);
    edit.clips[clip].values[Lane::Speed.index()] = 40;
    edit.reset_at(clip, Lane::Speed, 50).unwrap();
    assert_eq!(edit.clips[clip].values[Lane::Speed.index()], 100);
    // The default is data (a mod's limits).
    let mut limits = EditLimits::default();
    limits.lanes[Lane::Speed.index()].default = 50;
    let mut modded = ReplayEdit::new(limits);
    let clip = modded.add_clip(0, 300).unwrap();
    modded.reset_span(clip, Lane::Speed, 0, 300).unwrap();
    assert_eq!(modded.value_at(clip, Lane::Speed, 10), Some(50));
}
/// b472: the encoded length counts each speed segment at its start value inside the heads, truncated per segment.
#[test]
fn encoded_length_counts_speed_segments_inside_the_heads() {
    let mut edit = ReplayEdit::default();
    // B: soft keys 100 -> 50 -> 100 over 0..1800 count at their start values: 600 + 1200 + 600 = 80 s.
    let clip = edit.add_clip(0, 1800).unwrap();
    edit.commit(clip, Lane::Speed, 600, 50, true).unwrap();
    edit.commit(clip, Lane::Speed, 1200, 100, true).unwrap();
    assert_eq!(edit.encoded_seconds(), 80);
    // C: a hard key at 900 with 50, heads 300..1200: 600 + 600 = 40 s; C2: heads 1000..1500 past the key: 33 s.
    let mut edit = ReplayEdit::default();
    let clip = edit.add_clip(0, 1800).unwrap();
    edit.insert_hard(clip, Lane::Speed, 900).unwrap();
    edit.commit(clip, Lane::Speed, 1000, 50, false).unwrap();
    edit.trim(clip, 300, 1200).unwrap();
    assert_eq!(edit.encoded_seconds(), 40);
    edit.trim(clip, 1000, 1500).unwrap();
    assert_eq!(edit.encoded_seconds(), 33);
    assert_eq!(edit.value_at(clip, Lane::Speed, 1100), Some(50), "the key before the in head still holds");
    // D: 34 segments of 10 frames at 30 % truncate to 33 frames each: 1122 frames = 37 s (a float sum gives 38).
    let mut edit = ReplayEdit::default();
    let clip = edit.add_clip(0, 340).unwrap();
    edit.clips[clip].values[Lane::Speed.index()] = 30;
    edit.clips[clip].markers[Lane::Speed.index()] = (1..34).map(|k| LaneKey::hard(10 * k, 30)).collect();
    assert_eq!(edit.encoded_seconds(), 37);
    // E: no clips.
    assert_eq!(ReplayEdit::default().encoded_seconds(), 0);
}
