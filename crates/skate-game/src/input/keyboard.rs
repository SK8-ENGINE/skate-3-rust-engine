//! Padless fallback: synthesize TU3 pad packets from the keyboard.
//! Used only when slot 0 has no platform pad and no net session owns it;
//! real pads always win (see input::poll_controllers). Sticks are digital
//! (full deflection), so flicks work but analog finesse does not.
//!
//! Default layout (see docs/macos.md for the table):
//! left stick WASD, right stick arrows, Space/J/K/L as A/B/X/Y,
//! Q/E/Z/C as LB/RB/LT/RT, T/F/G/H as the DPad, Enter/Backspace as
//! Start/Back, R/V as stick clicks.
use super::platform::{
    DevicePacket, PAD_A, PAD_B, PAD_BACK, PAD_DPAD_DOWN, PAD_DPAD_LEFT, PAD_DPAD_RIGHT, PAD_DPAD_UP,
    PAD_LEFT_SHOULDER, PAD_LEFT_THUMB, PAD_RIGHT_SHOULDER, PAD_RIGHT_THUMB, PAD_START, PAD_X, PAD_Y,
};
use bevy::prelude::*;
use skate_core::input::xbox::XboxState;

const FULL: i16 = 32767;

#[derive(Default)]
pub(crate) struct KeyboardState {
    last: (u16, u8, u8, i16, i16, i16, i16),
    packet: u32,
}

/// Returns a pad packet when any mapped key is down, else None (the caller
/// keeps reporting the slot disconnected). Packet numbers follow the same
/// change-only bump as the other transports.
pub(crate) fn sample(keys: &ButtonInput<KeyCode>, state: &mut KeyboardState) -> Option<DevicePacket> {
    let down = |code| keys.pressed(code);
    let axis = |neg, pos| match (down(neg), down(pos)) {
        (true, false) => -FULL,
        (false, true) => FULL,
        _ => 0,
    };
    let mut buttons: u16 = 0;
    if down(KeyCode::Space) { buttons |= PAD_A; }
    if down(KeyCode::KeyJ) { buttons |= PAD_B; }
    if down(KeyCode::KeyK) { buttons |= PAD_X; }
    if down(KeyCode::KeyL) { buttons |= PAD_Y; }
    if down(KeyCode::KeyQ) { buttons |= PAD_LEFT_SHOULDER; }
    if down(KeyCode::KeyE) { buttons |= PAD_RIGHT_SHOULDER; }
    if down(KeyCode::KeyR) { buttons |= PAD_LEFT_THUMB; }
    if down(KeyCode::KeyV) { buttons |= PAD_RIGHT_THUMB; }
    if down(KeyCode::KeyT) { buttons |= PAD_DPAD_UP; }
    if down(KeyCode::KeyG) { buttons |= PAD_DPAD_DOWN; }
    if down(KeyCode::KeyF) { buttons |= PAD_DPAD_LEFT; }
    if down(KeyCode::KeyH) { buttons |= PAD_DPAD_RIGHT; }
    if down(KeyCode::Enter) { buttons |= PAD_START; }
    if down(KeyCode::Backspace) { buttons |= PAD_BACK; }
    let lt = if down(KeyCode::KeyZ) { 255 } else { 0 };
    let rt = if down(KeyCode::KeyC) { 255 } else { 0 };
    let current = (
        buttons,
        lt,
        rt,
        axis(KeyCode::KeyA, KeyCode::KeyD),
        axis(KeyCode::KeyS, KeyCode::KeyW),
        axis(KeyCode::ArrowLeft, KeyCode::ArrowRight),
        axis(KeyCode::ArrowDown, KeyCode::ArrowUp),
    );
    if current == (0, 0, 0, 0, 0, 0, 0) {
        return None;
    }
    if current != state.last {
        state.last = current;
        state.packet = state.packet.wrapping_add(1);
    }
    let (buttons, lt, rt, lx, ly, rx, ry) = current;
    Some(DevicePacket {
        number: state.packet,
        state: XboxState { buttons, triggers: [lt, rt], left: [lx, ly], right: [rx, ry] },
        subtype: 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keys_synthesize_buttons_sticks_and_change_only_packets() {
        let mut keys = ButtonInput::<KeyCode>::default();
        let mut state = KeyboardState::default();
        assert!(sample(&keys, &mut state).is_none());
        keys.press(KeyCode::Space);
        let first = sample(&keys, &mut state).expect("space must synthesize a pad");
        assert_eq!(first.state.buttons & PAD_A, PAD_A);
        assert_eq!(first.subtype, 1);
        let held = sample(&keys, &mut state).expect("held keys still synthesize");
        assert_eq!(held.number, first.number);
        keys.release(KeyCode::Space);
        keys.press(KeyCode::KeyW);
        let stick = sample(&keys, &mut state).expect("stick key must synthesize");
        assert_eq!(stick.state.left, [0, FULL]);
        assert_ne!(stick.number, first.number);
        keys.release(KeyCode::KeyW);
        assert!(sample(&keys, &mut state).is_none());
    }
}
