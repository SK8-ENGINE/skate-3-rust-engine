use super::*;

#[test]
fn removing_reference_roundtrips_native_add_including_scaled_reference() {
    let reference = Sqt {
        scale: [1.2, 0.8, 1.1, 1.0],
        rotation: Quat::from_rotation_y(0.8).to_array(),
        translation: [0.2, 0.9, -0.3, 1.0],
    };
    let motion = Sqt {
        scale: [0.9, 1.1, 1.0, 1.0],
        rotation: Quat::from_rotation_x(-0.3).to_array(),
        translation: [-0.4, 0.1, 0.2, 1.0],
    };
    let actual = remove_reference(matrix(pose_add::add(motion, reference, true)), reference);
    assert!(matrix(actual).abs_diff_eq(matrix(motion), 0.00001));
}

#[test]
fn replacement_rejects_wrong_skeleton_split_and_nonfinite_samples() {
    let names = vec!["HIPS".to_string()];
    let mut file = File {
        version: 1,
        bone_names: names.clone(),
        ground_last_frame: 1,
        air_first_frame: 2,
        frames: vec![vec![Mat4::IDENTITY.to_cols_array()]; 4],
    };
    assert!(file.validate(&names).is_ok());
    assert!(file.validate(&["HEAD".into()]).is_err());
    file.air_first_frame = 3;
    assert!(file.validate(&names).is_err());
    file.air_first_frame = 2;
    file.frames[1][0][12] = f32::NAN;
    assert!(file.validate(&names).is_err());
}

#[test]
#[ignore = "requires private stock banks and exported Kimodo body"]
fn kimodo_360flip_preserves_stock_board_timing_channels_and_other_tricks() {
    let root = std::env::var_os("SKATE3_ASSET_ROOT").expect("Set SKATE3_ASSET_ROOT");
    let root = Path::new(&root);
    let banks = AnimationBanks::load(root).unwrap();
    let stock = PoseEvaluator::from_banks(&banks).unwrap();
    let custom = PoseEvaluator::load(root).unwrap();
    assert_eq!(custom.tricks.0.len(), 12);
    let command = |name: &str, time| PoseCommand::Clip {
        name: name.into(),
        time,
        previous_time: 0.0,
        loops: 0,
    };
    for (name, replacement) in &custom.tricks.0 {
        let original = stock.frames.clip(name).unwrap();
        assert_eq!(replacement.fps_bits, original.fps_bits);
        assert_eq!(replacement.frames.len(), original.frames.len());
        assert_eq!(replacement.channel_weights, original.channel_weights);
        assert_eq!(replacement.loop_rotation_bits, original.loop_rotation_bits);
        assert_eq!(
            replacement.loop_translation_bits,
            original.loop_translation_bits
        );
        for (a, b) in original.frames.iter().zip(&replacement.frames) {
            for bone in std::iter::once(0).chain(25..32) {
                assert_eq!(a[bone], b[bone], "{name} changed board/trajectory {bone}");
            }
        }
        if name.ends_with("_G") {
            assert_eq!(original.frames[0], replacement.frames[0]);
        }
        if name.ends_with("_A") {
            assert_eq!(original.frames.last(), replacement.frames.last());
        }
        assert_ne!(
            original.frames[original.frames.len() / 2][5],
            replacement.frames[replacement.frames.len() / 2][5],
            "{name} did not replace the torso"
        );
        for fraction in [0.0, 0.125, 0.5, 0.875, 1.0] {
            let time =
                fraction * (original.frames.len() - 1) as f32 / f32::from_bits(original.fps_bits);
            let a = stock.evaluate(&[command(name, time)]).unwrap();
            let b = custom.evaluate(&[command(name, time)]).unwrap();
            for (a, b) in a.iter().zip(&b) {
                assert_eq!(a.translation[3], b.translation[3]);
            }
            for mirrored in [false, true] {
                let mut commands = vec![
                    command(name, time),
                    PoseCommand::Pose {
                        name: "RIG_TPOSE".into(),
                    },
                    PoseCommand::Add { motion_is_a: true },
                ];
                if mirrored {
                    commands.push(PoseCommand::Mirror { trajectory_mode: 0 });
                }
                let pose = custom.evaluate(&commands).unwrap();
                let hierarchy = custom.hierarchy(&pose).unwrap();
                assert!(hierarchy.iter().flatten().flatten().all(|v| v.is_finite()));
                if !mirrored && name.starts_with("360FLIP_D_HIGH") {
                    if let Some(folder) = std::env::var_os("SKATE3_CUSTOM_CAPTURE") {
                        let folder = Path::new(&folder);
                        std::fs::create_dir_all(folder).unwrap();
                        let value = serde_json::json!({"names":custom.frames.bone_names,
                            "pose":hierarchy,"root":Mat4::IDENTITY.to_cols_array_2d(),"board_state":1});
                        std::fs::write(
                            folder.join(format!("{name}_{}.json", (fraction * 1000.) as u32)),
                            serde_json::to_string(&value).unwrap(),
                        )
                        .unwrap();
                    }
                }
            }
        }
    }
    for name in ["N_360FLIP_HIGH_G", "N_360FLIP_HIGH_A", "R_IDLE_HCOM_000"] {
        assert_eq!(
            stock.evaluate(&[command(name, 0.1)]).unwrap(),
            custom.evaluate(&[command(name, 0.1)]).unwrap()
        );
    }
}
