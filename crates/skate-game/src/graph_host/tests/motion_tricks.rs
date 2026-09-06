use super::*;
use skate_core::animation::playback_tree::Evaluation;
#[test]
#[ignore = "requires private stock graphs and animation banks; no gameplay simulation"]
fn stock_360_crouch_takeoff_and_air_animation_pipeline() {
    use crate::{animation_pose::PoseEvaluator, graph_runtime::CompiledGraph};
    use skate_data::{
        animation_banks::AnimationBanks,
        state_graph::{StateGraph, binding::Binding},
    };
    let root = std::path::PathBuf::from(
        std::env::var_os("SKATE3_ASSET_ROOT").expect("Set SKATE3_ASSET_ROOT"),
    );
    let banks = AnimationBanks::load(&root).unwrap();
    let evaluator = PoseEvaluator::from_banks(&banks).unwrap();
    let source =
        StateGraph::load(&root.join("private/stock/data/state/MotionGraph_OnBoard.stategraph"))
            .unwrap();
    let binding = Binding::from_graph(&source).unwrap();
    let runtime = CompiledGraph::from_binding(&binding).unwrap();
    let graph = LoadedGraph {
        source,
        binding,
        runtime,
    };
    let collections = Collections::load(&root).unwrap();
    let mut host = MotionHost::from_graph(
        &graph,
        &collections,
        banks.metadata().unwrap(),
        PlaybackContext {
            is_switch: Some(false),
            is_mirrored: Some(false),
            board_available: Some(true),
            pro_skater: encode(b""),
            transition_override: None,
        },
    )
    .unwrap();
    host.animation
        .set_hierarchy(
            &evaluator.frames.bone_names,
            &evaluator.frames.mirror_indices,
        )
        .unwrap();

    let frame = Frame {
        dt: 1.0 / 60.0,
        current: None,
        last: None,
        state_times: Vec::new(),
    };
    let evaluation = Evaluation {
        cull_threshold: 0.01,
        update_history: true,
    };
    let find = |suffix: &str, operation: &str| -> usize {
        use skate_data::state_graph::binding::Node;
        graph
            .binding
            .operations
            .iter()
            .enumerate()
            .find_map(|(id, source)| {
                let Node::State(mut state) = source.parent else {
                    return None;
                };
                let mut names = Vec::new();
                loop {
                    names.push(graph.binding.states[state].name.clone());
                    let Some(parent) = graph.binding.states[state].parent else {
                        break;
                    };
                    state = parent;
                }
                names.reverse();
                if !names.join(".").ends_with(suffix) {
                    return None;
                }
                let runtime = host.remap.behaviors.iter().position(|&op| op == id)?;
                let matches = match (&host.operations[id], operation) {
                    (MotionOperation::Play(_), "PlayAnimation") => true,
                    (
                        MotionOperation::Trick(super::super::motion_tricks::Operation::Height {
                            ..
                        }),
                        "SetTrickHeight",
                    ) => true,
                    (
                        MotionOperation::Trick(super::super::motion_tricks::Operation::Attribute(
                            _,
                        )),
                        "SetTrickAttr",
                    ) => true,
                    (
                        MotionOperation::Trick(super::super::motion_tricks::Operation::Scoring(_)),
                        "ScoringTrick",
                    ) => true,
                    (
                        MotionOperation::Trick(
                            super::super::motion_tricks::Operation::MonitorUnderflip,
                        ),
                        "MonitorUnderflip",
                    ) => true,
                    _ => false,
                };
                matches.then_some(runtime)
            })
            .unwrap_or_else(|| panic!("Missing {suffix}/{operation}"))
    };
    let crouch = find("Anticipation.AnticInto.AnticTail", "PlayAnimation");
    let scoop = find(
        "Anticipation.AnticCyc.360PopShuvitAnticCycle",
        "PlayAnimation",
    );
    let ground = find("TailTrick.360Flip.Takeoff.FromAntic", "PlayAnimation");
    let height = find("TailTrick.360Flip.Takeoff.FromAntic", "SetTrickHeight");
    let air = find("TailTrick.360Flip.LeftGround", "PlayAnimation");
    let attr = find("TailTrick.360Flip", "SetTrickAttr");
    let score = find("TailTrick.360Flip", "ScoringTrick");
    let monitor = find("TailTrick.360Flip", "MonitorUnderflip");
    for mirrored in [false, true] {
        host.animation.reset_from_stock();
        host.animation.skater_animation_flags = Some(if mirrored { 0x40000000 } else { 0 });
        host.playback_context.is_mirrored = Some(mirrored);
        host.animation.filtered_intents.insert("Compression", 1.0);
        for (behavior, duration) in [(crouch, 0.25), (scoop, 0.5)] {
            host.execute(behavior, &frame, 0).unwrap();
            host.animation.apply_parameters().unwrap();
            host.animation.advance(duration, 0.0);
            host.animation.refresh_tree_attributes().unwrap();
            let pose = evaluator
                .evaluate(&host.animation.evaluate_pose(evaluation).unwrap())
                .unwrap();
            assert!(
                pose.iter()
                    .flat_map(|b| b.rotation.iter().chain(&b.translation))
                    .all(|v| v.is_finite())
            );
        }
        let antic = host
            .animation
            .last_attribute(encode(b"AnticStrength"))
            .unwrap()
            .expect("stock crouch must supply AnticStrength");
        assert!(f32::from_bits(antic.payload.0[0].unwrap()) > 0.0);
        host.animation.motion_intents.insert("GestureSpeed", 1.0);
        for behavior in [attr, score, monitor, ground, height] {
            host.execute(behavior, &frame, 0).unwrap();
        }
        host.execute(score, &frame, 1).unwrap();
        assert_eq!(host.score_packet.trick, Some(encode(b"360Flip")));
        assert!(
            host.animation
                .construction_values
                .contains(&(encode(b"Trick"), encode(b"360Flip")))
        );
        host.animation.apply_parameters().unwrap();
        host.animation.advance(0.1, 0.0);
        host.animation.refresh_tree_attributes().unwrap();
        assert!(
            host.animation
                .last_attribute(encode(b"TrickHeight"))
                .unwrap()
                .is_some()
        );
        let pose = evaluator
            .evaluate(&host.animation.evaluate_pose(evaluation).unwrap())
            .unwrap();
        assert!(
            pose.iter()
                .flat_map(|b| b.rotation.iter().chain(&b.translation))
                .all(|v| v.is_finite())
        );
        host.execute(air, &frame, 0).unwrap();
        host.animation.apply_parameters().unwrap();
        host.animation.advance(0.1, 0.0);
        host.animation.refresh_tree_attributes().unwrap();
        let pose = evaluator
            .evaluate(&host.animation.evaluate_pose(evaluation).unwrap())
            .unwrap();
        assert!(
            pose.iter()
                .flat_map(|b| b.rotation.iter().chain(&b.translation))
                .all(|v| v.is_finite())
        );
        host.execute(monitor, &frame, 1).unwrap();
        assert!(!host.trick_requests.dark_catch);
        host.execute(monitor, &frame, 2).unwrap();
    }
    println!(
        "Stock crouch, scoop, B_360FLIP_G -> B_360FLIP_A, attributes and finite poses checked in both stances"
    );
}
