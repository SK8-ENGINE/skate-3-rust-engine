use super::*;
#[test]
fn crouch_preserves_zero_presence_and_uses_maximum_trigger() {
    let mut words = [0u32; 26];
    words[8] = 1.0f32.to_bits();
    let values = produce(
        &DerivedControllerInput::from_words(words),
        1,
        PushPreferences::default(),
    );
    assert_eq!(
        values.iter().find(|i| i.name == "Crouch").unwrap().value,
        0.0
    );
    words[11] = 0.4f32.to_bits();
    words[12] = 0.7f32.to_bits();
    words[7] = 0.5f32.to_bits();
    let values = produce(
        &DerivedControllerInput::from_words(words),
        0,
        PushPreferences::default(),
    );
    assert_eq!(
        values.iter().find(|i| i.name == "Crouch").unwrap().value,
        0.7
    );
    assert_eq!(
        values.iter().find(|i| i.name == "KickTurn").unwrap().value,
        0.5
    );
    assert_ne!(values.iter().find(|i| i.name == "Turn").unwrap().value, 0.5);
}
fn snapshot(previous: u32, current: u32, held: f32) -> DerivedControllerInput {
    let mut words = [0; 26];
    words[6] = previous;
    words[13] = current;
    words[20] = held.to_bits();
    words[22] = held.to_bits();
    DerivedControllerInput::from_words(words)
}
#[test]
fn held_push_continues_after_new_push_allowance_expires() {
    let button = 1 << 21;
    let first = produce(&snapshot(0, button, 0.5), 0, PushPreferences::default());
    assert!(first.iter().any(|i| i.name == "NewPush"));
    let held = produce(
        &snapshot(button, button, 0.5),
        0,
        PushPreferences::default(),
    );
    assert_eq!(
        held.iter().map(|i| i.name).collect::<Vec<_>>(),
        ["RightPush", "Pushing"]
    );
    assert!(
        produce(
            &snapshot(0, button, 0.0),
            1 << 11,
            PushPreferences::default()
        )
        .is_empty()
    );
}
#[test]
fn dual_push_gate_requires_both_feet_and_brake_uses_actor_mode() {
    assert!(
        !produce(
            &snapshot(0, 1 << 21, 0.0),
            1 << 6,
            PushPreferences::default()
        )
        .is_empty()
    );
    assert!(
        produce(
            &snapshot(0, (1 << 21) | (1 << 23), 0.0),
            1 << 6,
            PushPreferences::default()
        )
        .is_empty()
    );
    let brake = snapshot(0, 1 << 20, 0.0);
    assert_eq!(
        produce(&brake, 0, PushPreferences::default())[0].name,
        "Brake"
    );
    assert!(produce(&brake, 1 << 7, PushPreferences::default()).is_empty());
}
