use super::*;

#[test]
#[ignore = "requires the user's decoded stock animation assets"]
fn stock_pose_commands_sample_complete_bones_and_preserve_tree_order() {
    let root = std::env::var_os("SKATE3_ASSET_ROOT").expect("Set SKATE3_ASSET_ROOT");
    let evaluator = PoseEvaluator::load(Path::new(&root)).unwrap();
    assert_eq!(evaluator.frames.parents.len(), 36);
    assert_eq!(evaluator.frames.clip_count(), 2672 + 652);
    for name in evaluator.frames.clip_names() {
        let clip = evaluator.frames.clip(name).unwrap();
        let end = (clip.frames.len() - 1) as f32 / f32::from_bits(clip.fps_bits);
        let commands = [
            PoseCommand::Clip {
                name: clip.name.clone(),
                time: 0.0,
                previous_time: 0.0,
                loops: 0,
            },
            PoseCommand::Clip {
                name: clip.name.clone(),
                time: end * 0.5,
                previous_time: 0.0,
                loops: 0,
            },
            PoseCommand::Blend { weight: 0.375 },
            PoseCommand::Clip {
                name: clip.name.clone(),
                time: end,
                previous_time: end * 0.5,
                loops: 1,
            },
            PoseCommand::ChannelBlend {
                weight: 0.25,
                use_channels_from_weights: false,
            },
        ];
        let pose = evaluator.evaluate(&commands).unwrap();
        assert_eq!(pose.len(), 36, "{}", clip.name);
        assert!(
            pose.iter().all(|s| s
                .scale
                .iter()
                .chain(&s.rotation)
                .chain(&s.translation)
                .all(|v| v.is_finite())),
            "{}",
            clip.name
        );
        // Regular stance takes every native AddBindPose branch. Include the
        // trajectory-relative mode used by the source's one-shot mirror flag.
        let mut wrapped = commands.to_vec();
        wrapped.extend([
            PoseCommand::Pose {
                name: "RIG_TPOSE".into(),
            },
            PoseCommand::Add { motion_is_a: true },
            PoseCommand::Pose {
                name: "BOARD_BACKWARDS".into(),
            },
            PoseCommand::Add { motion_is_a: false },
            PoseCommand::Pose {
                name: "BOARD_BACKWARDS_IK".into(),
            },
            PoseCommand::Add { motion_is_a: true },
            PoseCommand::Mirror { trajectory_mode: 2 },
            PoseCommand::Mirror { trajectory_mode: 1 },
        ]);
        let pose = evaluator.evaluate(&wrapped).unwrap();
        assert!(
            pose.iter().all(|s| s
                .scale
                .iter()
                .chain(&s.rotation)
                .chain(&s.translation)
                .all(|v| v.is_finite())),
            "{}",
            clip.name
        );
        let hierarchy = evaluator.hierarchy(&pose).unwrap();
        assert!(
            hierarchy.iter().flatten().flatten().all(|v| v.is_finite()),
            "{}",
            clip.name
        );
    }
    // Native SetDBContent82D1B45C overwrites equal keys; last RIG_TPOSE wins.
    assert_eq!(
        evaluator
            .frames
            .reference_pose(0xA46D20)
            .unwrap()
            .samples
            .len(),
        36
    );
    assert_eq!(
        evaluator
            .frames
            .named_pose("RIG_TPOSE")
            .unwrap()
            .source_offset,
        0xA46D20
    );
    assert_eq!(
        evaluator
            .frames
            .named_pose("BOARD_BACKWARDS")
            .unwrap()
            .source_offset,
        0xA473D0
    );
    assert_eq!(evaluator.frames.bone_names[0], "TRAJECTORY");
    assert_eq!(evaluator.frames.bone_names[25], "SKATEBOARD_ROOT");
}

#[test]
fn additive_pose_keeps_motion_channel_and_reference_rotation_order() {
    let motion = Sqt {
        scale: [1.0; 4],
        rotation: [1.0, 0.0, 0.0, 0.0],
        translation: [2.0, 3.0, 4.0, 0.25],
    };
    let reference = Sqt {
        scale: [2.0; 4],
        rotation: [0.0, 1.0, 0.0, 0.0],
        translation: [7.0, 11.0, 13.0, 0.75],
    };
    let result = pose_add::add(motion, reference, true);
    assert_eq!(result.rotation, [0.0, 0.0, -1.0, 0.0]);
    assert_eq!(result.translation, [5.0, 14.0, 9.0, 0.25]);
    assert_eq!(pose_add::add(motion, reference, false).translation[3], 0.75);
}
