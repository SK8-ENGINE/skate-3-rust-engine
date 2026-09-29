//! Non-Windows device transport via gilrs with its default filters disabled,
//! so evdev values reach the TU3 converter without deadzones.
//!
//! Range normalization: gilrs exposes stick axes as f32 in -1..1 and analog
//! triggers as 0..1, while the shared converter expects XInput ranges
//! (i16 sticks, u8 triggers). Sticks are scaled by 32767 and triggers by 255.
use super::*;
use gilrs::{Axis, Button, Gilrs};
use std::cell::{Cell, RefCell};

// Community controller DB (SDL_GameControllerDB): generic HID pads do not
// self-describe their layout, so mappings come from this VID/PID-keyed list,
// the same one used by SDL/Steam/gilrs upstream. Users can override or extend
// it with the standard SDL_GAMECONTROLLERCONFIG env var.
const GAMECONTROLLERDB: &str = include_str!("../../gamecontrollerdb.txt");

// Same layout as the upstream DB entry for the DragonRise 0079:0006 adapter,
// but with the exact GUID this unit reports (version field 1001 instead of
// 0107). Candidate for submission to SDL_GameControllerDB; remove once the
// DB covers this GUID variant.
const EXTRA_MAPPINGS: &str = "03000000790000000600000010010000,DragonRise Inc. Generic USB Joystick,platform:Linux,a:b2,b:b1,x:b3,y:b0,back:b8,start:b9,leftstick:b10,rightstick:b11,leftshoulder:b4,rightshoulder:b5,dpup:h0.1,dpdown:h0.4,dpleft:h0.8,dpright:h0.2,leftx:a0,lefty:a1,rightx:a2,righty:a3,lefttrigger:b6,righttrigger:b7,";

thread_local! {
    static GILRS: RefCell<Option<Gilrs>> = const { RefCell::new(None) };
    static PACKET: Cell<u32> = const { Cell::new(0) };
}

fn with_gilrs<R>(f: impl FnOnce(&mut Gilrs) -> R) -> Result<R, DeviceError> {
    GILRS.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            let gilrs = gilrs::GilrsBuilder::new()
                .with_default_filters(false)
                .add_env_mappings(true)
                .add_mappings(GAMECONTROLLERDB)
                .add_mappings(EXTRA_MAPPINGS)
                .build()
                .map_err(|_| DeviceError::State(1))?;
            *slot = Some(gilrs);
        }
        Ok(f(slot.as_mut().unwrap()))
    })
}

fn button_bits(gamepad: &gilrs::Gamepad) -> u16 {
    // XInput XUSB button bitmask, matching the Windows transport.
    let pressed = |button: Button| gamepad.is_pressed(button);
    let mut bits = 0u16;
    for (button, bit) in [
        (Button::DPadUp, 0x0001),
        (Button::DPadDown, 0x0002),
        (Button::DPadLeft, 0x0004),
        (Button::DPadRight, 0x0008),
        (Button::Start, 0x0010),
        (Button::Select, 0x0020),
        (Button::LeftThumb, 0x0040),
        (Button::RightThumb, 0x0080),
        (Button::LeftTrigger, 0x0100),
        (Button::RightTrigger, 0x0200),
        (Button::Mode, 0x0400),
        (Button::South, 0x1000),
        (Button::East, 0x2000),
        (Button::West, 0x4000),
        (Button::North, 0x8000),
    ] {
        if pressed(button) {
            bits |= bit;
        }
    }
    // Hats arrive as DPad axes, and the axis_dpad_to_button filter that would
    // turn them into button presses is a default filter — disabled above so
    // stick values stay raw. Derive the dpad bits from the axes directly.
    let dpad_x = gamepad.axis_data(Axis::DPadX).map_or(0.0, |data| data.value());
    let dpad_y = gamepad.axis_data(Axis::DPadY).map_or(0.0, |data| data.value());
    if dpad_y > 0.5 {
        bits |= 0x0001;
    }
    if dpad_y < -0.5 {
        bits |= 0x0002;
    }
    if dpad_x < -0.5 {
        bits |= 0x0004;
    }
    if dpad_x > 0.5 {
        bits |= 0x0008;
    }
    bits
}

fn axis(gamepad: &gilrs::Gamepad, axis: Axis) -> i16 {
    let value = gamepad.axis_data(axis).map_or(0.0, |data| data.value());
    (value.clamp(-1.0, 1.0) * 32767.0).round() as i16
}

fn trigger(gamepad: &gilrs::Gamepad, button: Button, axis: Axis) -> u8 {
    if let Some(data) = gamepad.button_data(button) {
        return (data.value().clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    // Fallback for pads reporting triggers as axes normalized to -1..1.
    let value = gamepad.axis_data(axis).map_or(-1.0, |data| data.value());
    ((value.clamp(-1.0, 1.0) + 1.0) * 0.5 * 255.0).round() as u8
}

pub(super) fn poll(index: u32, cache: &mut CapabilityCache) -> Result<DevicePacket, DeviceError> {
    with_gilrs(|gilrs| {
        while let Some(_event) = gilrs.next_event() {}
        let mut connected: Vec<_> = gilrs
            .gamepads()
            .filter(|(_, gamepad)| gamepad.is_connected())
            .map(|(id, _)| id)
            .collect();
        connected.sort_by_key(|id| usize::from(*id));
        let Some(&id) = connected.get(index as usize) else {
            cache.invalidate();
            return Err(DeviceError::Disconnected);
        };
        let subtype = cache.get(std::time::Instant::now(), || Ok(1))?;
        let gamepad = gilrs.gamepad(id);
        let number = PACKET.with(|packet| {
            let number = packet.get().wrapping_add(1);
            packet.set(number);
            number
        });
        Ok(DevicePacket {
            number,
            state: XboxState {
                buttons: button_bits(&gamepad),
                triggers: [
                    trigger(&gamepad, Button::LeftTrigger2, Axis::LeftZ),
                    trigger(&gamepad, Button::RightTrigger2, Axis::RightZ),
                ],
                left: [axis(&gamepad, Axis::LeftStickX), axis(&gamepad, Axis::LeftStickY)],
                right: [axis(&gamepad, Axis::RightStickX), axis(&gamepad, Axis::RightStickY)],
            },
            subtype,
        })
    })?
}
