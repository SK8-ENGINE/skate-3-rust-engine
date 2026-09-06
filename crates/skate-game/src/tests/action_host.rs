use crate::graph_host::action::*;
use crate::graph_host::action_nodes::Parameter;
use crate::graph_host::action_nodes::{ActionInstance, ActionInstances, ActionOperation};
use crate::graph_runtime::OperationRemap;
use skate_core::graph::controller::{Frame, Host};

fn parameter(mg_intent: &str, value: f32) -> Parameter {
    Parameter {
        name: None,
        mg_intent: Some(mg_intent.into()),
        ag_intent: None,
        text: None,
        float_bits: Some(value.to_bits()),
        boolean_byte: None,
        default_value: None,
        scale: None,
        on_update: false,
        filters: [0; 4],
    }
}

#[test]
fn gesture_behavior_publishes_and_releases_the_stock_tre_flip_intent() {
    let mut config = parameter("", 0.0);
    config.mg_intent = None;
    let instance = ActionInstance {
        operation: ActionOperation::CreateTrickIntentFromGesture {
            group: crate::input::gesture_catalog::Group::Square,
            override_name: None,
        },
        config,
        parameters: Vec::new(),
    };
    let mut host = ActionHost::new(ActionInstances::new(vec![instance]), OperationRemap {
        behaviors: vec![0], conditions: Vec::new(), hooks: Vec::new(),
    });
    let frame = Frame { dt: 1.0 / 60.0, current: None, last: None, state_times: Vec::new() };
    for (mirrored, expected) in [(false, "360Flip"), (true, "Laserflip")] {
        host.stance = Some((false, mirrored));
        host.action_intents.insert("360Flip", 1.0);
        Host::allocate(&mut host, 0, &frame);
        Host::begin(&mut host, 0, [0; 6], &frame);
        Host::update(&mut host, 0, [0; 6], &frame);
        assert_eq!(host.motion_intents.get(expected), Some(&0.0));
        Host::end(&mut host, 0, [0; 6], &frame);
        assert!(host.motion_intents.is_empty());
    }
    assert!(host.errors.is_empty(), "{:?}", host.errors);
}

#[test]
fn const_callbacks_publish_then_remove_motion_intent() {
    let instance = ActionInstance {
        operation: ActionOperation::CreateConstMgIntent,
        config: parameter("Crouch", 1.0),
        parameters: Vec::new(),
    };
    // Conditions are interleaved with behaviors in the source binding. A
    // compact behavior ID must select its mapped operation and configuration.
    let mut earlier = instance.clone();
    earlier.config = parameter("WrongSourceOperation", 9.0);
    let mut host = ActionHost::new(
        ActionInstances::new(vec![earlier, instance]),
        OperationRemap {
            behaviors: vec![1],
            conditions: vec![0],
            hooks: vec![],
        },
    );
    Host::begin(
        &mut host,
        0,
        [0; 6],
        &Frame {
            dt: 0.0,
            current: None,
            last: None,
            state_times: Vec::new(),
        },
    );
    assert_eq!(host.motion_intents.get("Crouch"), Some(&1.0));
    assert!(!host.motion_intents.contains_key("WrongSourceOperation"));
    Host::end(
        &mut host,
        0,
        [0; 6],
        &Frame {
            dt: 0.0,
            current: None,
            last: None,
            state_times: Vec::new(),
        },
    );
    assert!(!host.motion_intents.contains_key("Crouch"));
}

#[test]
fn pushing_clock_starts_fresh_when_graph_reenters_behavior() {
    let mut config = parameter("Pushing", 0.0);
    config.ag_intent = Some("Pushing".into());
    let instance = ActionInstance {
        operation: ActionOperation::CreateMgTimeIntent,
        config,
        parameters: Vec::new(),
    };
    let mut host = ActionHost::new(
        ActionInstances::new(vec![instance]),
        OperationRemap {
            behaviors: vec![0],
            conditions: vec![],
            hooks: vec![],
        },
    );
    let frame = Frame {
        dt: 0.125,
        current: None,
        last: None,
        state_times: vec![],
    };
    host.action_intents.insert("Pushing", 1.0);
    host.allocate(0, &frame);
    for _ in 0..3 {
        host.update(0, [0; 6], &frame);
    }
    assert_eq!(host.motion_intents.get("Pushing"), Some(&0.375));
    host.end(0, [0; 6], &frame);
    assert!(!host.motion_intents.contains_key("Pushing"));
    host.allocate(0, &frame); // TU382BBCB50 initializes each instance to zero.
    host.update(0, [0; 6], &frame);
    assert_eq!(host.motion_intents.get("Pushing"), Some(&0.125));
}

#[test]
fn native_intent_aliases_share_action_motion_and_end_lifecycle() {
    use crate::graph_host::motion::MotionAnimation;
    use skate_core::animation::playback_parameters::ParameterInputs;
    use skate_core::graph::{
        activation::ConditionHost,
        conditions::{ActionCondition, Comparison, NumericCondition},
    };
    use skate_data::animation_metadata::AnimationMetadata;

    // These case variants both occur in the authored ActionGraph. No animation
    // tree is needed to exercise its real persistent MotionAnimation intent map.
    for pair in [
        ["Fingerflip", "FingerFlip"],
        ["OB_OBJECTMVX", "OB_ObjectMvX"],
    ] {
        for [first, second] in [pair, [pair[1], pair[0]]] {
            let create = |name: &str| {
                let mut config = parameter(name, 0.0);
                config.ag_intent = Some(name.into());
                ActionInstance {
                    operation: ActionOperation::CreateMgIntent,
                    config,
                    parameters: vec![],
                }
            };
            let condition = ActionInstance {
                operation: ActionOperation::Condition(ActionCondition::HasIntent {
                    name: first.into(),
                    numeric: NumericCondition {
                        comparison: Comparison::Equal,
                        threshold: 0.75,
                        absolute: false,
                    },
                }),
                config: parameter(first, 0.0),
                parameters: vec![],
            };
            let mut host = ActionHost::new(
                ActionInstances::new(vec![create(first), create(second), condition]),
                OperationRemap {
                    behaviors: vec![0, 1],
                    conditions: vec![2],
                    hooks: vec![],
                },
            );
            let mut motion = MotionAnimation::from_metadata(AnimationMetadata::parse(
                r#"{"version":1,"source_bank":"intent-test","source_sha256":"0000000000000000000000000000000000000000000000000000000000000000","source_bytes":48,"clips":[],"unsupported_trees":[]}"#
            ).unwrap());
            let frame = Frame {
                dt: 1.0 / 60.0,
                current: None,
                last: None,
                state_times: vec![],
            };
            host.action_intents.insert(first, 0.25);
            assert_eq!(host.action_intents.insert(second, 0.75), Some(0.25));
            assert_eq!(host.action_intents.len(), 1);
            assert_eq!(host.condition_activation(0, &frame), 1);
            host.begin(0, [0; 6], &frame);
            assert_eq!(host.motion_intents.get(second), Some(&0.75));
            host.action_intents.insert(first, 1.25);
            host.begin(1, [0; 6], &frame);
            assert_eq!(host.motion_intents.len(), 1);

            // Preserve the production coordinator's single-map handoff.
            std::mem::swap(&mut host.motion_intents, &mut motion.motion_intents);
            assert_eq!(motion.motion_intent(first), Some(1.25));
            assert_eq!(motion.motion_intent(second), Some(1.25));
            std::mem::swap(&mut host.motion_intents, &mut motion.motion_intents);
            host.end(0, [0; 6], &frame);
            std::mem::swap(&mut host.motion_intents, &mut motion.motion_intents);
            assert_eq!(motion.motion_intent(first), None);
            assert_eq!(motion.motion_intent(second), None);
            assert!(motion.motion_intents.is_empty());
            assert_eq!(host.action_intents.remove(first), Some(1.25));
            assert!(!host.action_intents.contains_key(second));
            assert!(host.errors.is_empty(), "{:?}", host.errors);
        }
    }
}
