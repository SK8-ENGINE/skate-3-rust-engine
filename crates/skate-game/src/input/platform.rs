//! Device transport. SDL3 gamepads are the default on every platform; each
//! state is re-expressed as the XInput-shaped `XboxState`, so raw signed
//! axes/trigger bytes reach the TU3 converter without Bevy/gilrs deadzones or
//! normalized-axis reconstruction. `SKATE3_INPUT=xinput` selects the original
//! Windows XInput transport, which is also the fallback if SDL cannot start.
use skate_core::input::xbox::XboxState;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

/// XInput `wButtons` bits, the layout `xbox::convert` consumes.
pub(crate) const BUTTON_NAMES: [(&str, u16); 14] = [
    ("dpad_up", 0x0001), ("dpad_down", 0x0002), ("dpad_left", 0x0004), ("dpad_right", 0x0008),
    ("start", 0x0010), ("back", 0x0020), ("left_stick", 0x0040), ("right_stick", 0x0080),
    ("left_shoulder", 0x0100), ("right_shoulder", 0x0200),
    ("a", 0x1000), ("b", 0x2000), ("x", 0x4000), ("y", 0x8000),
];

pub(crate) fn button_mask(name: &str) -> Option<u16> {
    BUTTON_NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|&(_, bit)| bit)
}

/// XInput has no paddle inputs, so each paddle (right1, left1, right2, left2)
/// is folded into the XInput button mask chosen in settings/controller.json.
static PADDLES: AtomicU64 = AtomicU64::new(0);

pub(crate) fn set_paddles(masks: [u16; 4]) {
    let packed = masks.iter().enumerate().fold(0u64, |acc, (i, &m)| acc | u64::from(m) << (16 * i));
    PADDLES.store(packed, Ordering::Relaxed);
}

fn paddles() -> [u16; 4] {
    let packed = PADDLES.load(Ordering::Relaxed);
    std::array::from_fn(|i| (packed >> (16 * i)) as u16)
}

/// SDL trigger range 0..=32767 back to the XInput byte. SDL expands XInput's
/// byte as b*257 over the full axis, then rescales to 0..=32767, so rounding to
/// nearest returns the original byte exactly.
pub(crate) fn xinput_trigger(value: i16) -> u8 {
    ((u32::from(value.max(0) as u16) * 255 + 16383) / 32767) as u8
}

/// SDL reports stick Y as positive-down by storing `~y`; invert it the same way.
pub(crate) fn xinput_y(value: i16) -> i16 {
    !value
}

pub(crate) struct DevicePacket {
    pub number: u32,
    pub state: XboxState,
    pub subtype: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeviceError {
    Disconnected,
    State(u32),
    Capabilities(u32),
    /// No device backend could be started.
    #[cfg(not(windows))]
    Unavailable,
}

/// Device identity is metadata; raw input is still sampled every host frame.
/// Refresh periodically as well as after errors, so hot swaps cannot leave a
/// subtype cached indefinitely even if Windows never exposes a disconnect.
#[derive(Default)]
pub(crate) struct CapabilityCache {
    value: Option<(u8, std::time::Instant)>,
}
impl CapabilityCache {
    pub(crate) fn invalidate(&mut self) {
        self.value = None;
    }
    fn get(
        &mut self,
        now: std::time::Instant,
        read: impl FnOnce() -> Result<u8, DeviceError>,
    ) -> Result<u8, DeviceError> {
        if let Some((subtype, expires)) = self.value {
            if now < expires {
                return Ok(subtype);
            }
        }
        self.value = None;
        let subtype = read()?;
        self.value = Some((subtype, now + std::time::Duration::from_secs(1)));
        Ok(subtype)
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::mem::MaybeUninit;

    // ABI from the installed Windows SDK Xinput.h. No OS-owned pointers are
    // retained and only successful calls permit reading output storage.
    #[repr(C)]
    struct Gamepad {
        buttons: u16,
        left_trigger: u8,
        right_trigger: u8,
        left_x: i16,
        left_y: i16,
        right_x: i16,
        right_y: i16,
    }
    #[repr(C)]
    struct State {
        number: u32,
        gamepad: Gamepad,
    }
    #[repr(C)]
    struct Vibration {
        left: u16,
        right: u16,
    }
    #[repr(C)]
    struct Capabilities {
        device_type: u8,
        subtype: u8,
        flags: u16,
        gamepad: Gamepad,
        vibration: Vibration,
    }
    const _: () = assert!(size_of::<Gamepad>() == 12);
    const _: () = assert!(size_of::<State>() == 16);
    const _: () = assert!(size_of::<Capabilities>() == 20);

    #[link(name = "xinput")]
    unsafe extern "system" {
        fn XInputGetState(index: u32, state: *mut State) -> u32;
        fn XInputGetCapabilities(index: u32, flags: u32, capabilities: *mut Capabilities) -> u32;
    }

    pub(super) fn poll(
        index: u32,
        cache: &mut CapabilityCache,
    ) -> Result<DevicePacket, DeviceError> {
        let mut state = MaybeUninit::<State>::uninit();
        // SAFETY: properly aligned writable storage with the SDK's exact C ABI.
        let result = unsafe { XInputGetState(index, state.as_mut_ptr()) };
        if result != 0 {
            cache.invalidate();
        }
        if result == 1167 {
            return Err(DeviceError::Disconnected);
        }
        if result != 0 {
            return Err(DeviceError::State(result));
        }
        let subtype = cache.get(std::time::Instant::now(), || {
            let mut capabilities = MaybeUninit::<Capabilities>::uninit();
            // SAFETY: writable storage with the SDK ABI; read only on success.
            let result = unsafe { XInputGetCapabilities(index, 1, capabilities.as_mut_ptr()) };
            if result != 0 {
                return Err(DeviceError::Capabilities(result));
            }
            Ok(unsafe { capabilities.assume_init() }.subtype)
        })?;
        // SAFETY: successful XInputGetState initialized the complete structure.
        let state = unsafe { state.assume_init() };
        Ok(DevicePacket {
            number: state.number,
            state: XboxState {
                buttons: state.gamepad.buttons,
                triggers: [state.gamepad.left_trigger, state.gamepad.right_trigger],
                left: [state.gamepad.left_x, state.gamepad.left_y],
                right: [state.gamepad.right_x, state.gamepad.right_y],
            },
            subtype,
        })
    }
}

mod sdl {
    use super::*;
    use bevy::log::{info, warn};
    use sdl3::event::Event;
    use sdl3::gamepad::{Axis, Button, Gamepad};
    use std::sync::{Mutex, mpsc};

    /// Polling faster than any pad reports keeps added latency below 1 ms.
    const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(1);
    /// XINPUT_DEVSUBTYPE_GAMEPAD; SDL only opens devices it maps as gamepads.
    const SUBTYPE_GAMEPAD: u8 = 1;
    const BUTTONS: [(Button, u16); 14] = [
        (Button::DPadUp, 0x0001), (Button::DPadDown, 0x0002),
        (Button::DPadLeft, 0x0004), (Button::DPadRight, 0x0008),
        (Button::Start, 0x0010), (Button::Back, 0x0020),
        (Button::LeftStick, 0x0040), (Button::RightStick, 0x0080),
        (Button::LeftShoulder, 0x0100), (Button::RightShoulder, 0x0200),
        (Button::South, 0x1000), (Button::East, 0x2000),
        (Button::West, 0x4000), (Button::North, 0x8000),
    ];
    const PADDLE_BUTTONS: [Button; 4] =
        [Button::RightPaddle1, Button::LeftPaddle1, Button::RightPaddle2, Button::LeftPaddle2];

    /// Latest (packet number, state) per slot, like XInputGetState's snapshot.
    pub(super) struct Shared {
        slots: Mutex<[Option<(u32, XboxState)>; 4]>,
    }

    struct Slot {
        pad: Gamepad,
        id: u32,
        number: u32,
        state: XboxState,
    }

    /// SDL must be pumped on the thread that initialised it, while Bevy runs
    /// systems on a pool; a dedicated thread owns SDL and publishes snapshots.
    pub(super) fn start() -> Result<&'static Shared, String> {
        let shared: &'static Shared = Box::leak(Box::new(Shared { slots: Mutex::new([None; 4]) }));
        let (ready, started) = mpsc::channel();
        std::thread::Builder::new()
            .name("sdl-gamepad".into())
            .spawn(move || run(shared, ready))
            .map_err(|e| e.to_string())?;
        started.recv().map_err(|_| "SDL gamepad thread exited".to_string())??;
        Ok(shared)
    }

    pub(super) fn poll(shared: &Shared, index: usize) -> Result<DevicePacket, DeviceError> {
        let slots = shared.slots.lock().unwrap_or_else(|e| e.into_inner());
        slots[index]
            .map(|(number, state)| DevicePacket { number, state, subtype: SUBTYPE_GAMEPAD })
            .ok_or(DeviceError::Disconnected)
    }

    fn run(shared: &'static Shared, ready: mpsc::Sender<Result<(), String>>) {
        // The window belongs to winit, so SDL never sees focus; the game
        // decides itself when unfocused input is ignored (multiplayer).
        sdl3::hint::set("SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS", "1");
        let init = (|| {
            let context = sdl3::init()?;
            let gamepads = context.gamepad()?;
            let events = context.event_pump()?;
            Ok::<_, sdl3::Error>((context, gamepads, events))
        })();
        let (_context, gamepads, mut events) = match init {
            Ok(value) => value,
            Err(error) => {
                let _ = ready.send(Err(error.to_string()));
                return;
            }
        };
        let _ = ready.send(Ok(()));
        info!("Controller input: SDL {}", sdl3::version::version());
        let mut slots: [Option<Slot>; 4] = Default::default();
        // Devices present at startup are also announced as GamepadAdded.
        for id in gamepads.gamepads().unwrap_or_default() {
            open(&gamepads, &mut slots, id);
        }
        loop {
            for event in events.poll_iter() {
                match event {
                    Event::GamepadAdded { which, .. } => open(&gamepads, &mut slots, which),
                    Event::GamepadRemoved { which, .. } => {
                        for (index, slot) in slots.iter_mut().enumerate() {
                            if slot.as_ref().is_some_and(|s| s.id == which.raw()) {
                                *slot = None;
                                info!("Controller {index}: SDL gamepad removed");
                            }
                        }
                    }
                    _ => {}
                }
            }
            let paddles = paddles();
            let mut published = [None; 4];
            for (slot, output) in slots.iter_mut().zip(&mut published) {
                let Some(slot) = slot else { continue };
                let state = read(&slot.pad, paddles);
                // Mirrors XInput dwPacketNumber: advances only when state changes.
                if state != slot.state {
                    slot.number = slot.number.wrapping_add(1);
                    slot.state = state;
                }
                *output = Some((slot.number, state));
            }
            *shared.slots.lock().unwrap_or_else(|e| e.into_inner()) = published;
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    fn open(gamepads: &sdl3::GamepadSubsystem, slots: &mut [Option<Slot>; 4], id: sdl3::joystick::JoystickId) {
        if slots.iter().flatten().any(|s| s.id == id.raw()) {
            return;
        }
        let pad = match gamepads.open(id) {
            Ok(pad) => pad,
            Err(error) => return warn!("SDL gamepad {}: cannot open: {error}", id.raw()),
        };
        // XInput-backed pads report their XInput user index as player index;
        // keep that slot so the ring light matches, else take the first free one.
        let preferred = pad.player_index().map(usize::from).filter(|&i| i < 4 && slots[i].is_none());
        let Some(index) = preferred.or_else(|| slots.iter().position(Option::is_none)) else {
            return warn!("SDL gamepad {}: all four controller slots are in use", id.raw());
        };
        info!(
            "Controller {index}: SDL gamepad {:?} ({:?}, vendor {:04x} product {:04x}, paddles {}, path {:?})",
            pad.name().unwrap_or_default(),
            pad.r#type(),
            pad.vendor_id().unwrap_or(0),
            pad.product_id().unwrap_or(0),
            if pad.has_button(Button::RightPaddle1) { "available" } else { "not reported" },
            pad.path().unwrap_or_default(),
        );
        slots[index] = Some(Slot { pad, id: id.raw(), number: 0, state: XboxState::default() });
    }

    fn read(pad: &Gamepad, paddles: [u16; 4]) -> XboxState {
        let mut buttons = 0;
        for (button, bit) in BUTTONS {
            if pad.button(button) {
                buttons |= bit;
            }
        }
        for (button, mask) in PADDLE_BUTTONS.into_iter().zip(paddles) {
            if mask != 0 && pad.button(button) {
                buttons |= mask;
            }
        }
        XboxState {
            buttons,
            triggers: [xinput_trigger(pad.axis(Axis::TriggerLeft)), xinput_trigger(pad.axis(Axis::TriggerRight))],
            left: [pad.axis(Axis::LeftX), xinput_y(pad.axis(Axis::LeftY))],
            right: [pad.axis(Axis::RightX), xinput_y(pad.axis(Axis::RightY))],
        }
    }
}

enum Backend {
    Sdl(&'static sdl::Shared),
    #[cfg(windows)]
    XInput,
    #[cfg(not(windows))]
    Unavailable,
}

fn backend() -> &'static Backend {
    static BACKEND: OnceLock<Backend> = OnceLock::new();
    BACKEND.get_or_init(|| {
        #[cfg(windows)]
        if std::env::var("SKATE3_INPUT").is_ok_and(|v| v.eq_ignore_ascii_case("xinput")) {
            bevy::log::info!("Controller input: XInput (SKATE3_INPUT=xinput)");
            return Backend::XInput;
        }
        match sdl::start() {
            Ok(shared) => Backend::Sdl(shared),
            #[cfg(windows)]
            Err(error) => {
                bevy::log::warn!("SDL gamepad input unavailable ({error}); using XInput");
                Backend::XInput
            }
            #[cfg(not(windows))]
            Err(error) => {
                bevy::log::error!("SDL gamepad input unavailable: {error}");
                Backend::Unavailable
            }
        }
    })
}

pub(crate) fn poll_cached(
    index: usize,
    cache: &mut CapabilityCache,
) -> Result<DevicePacket, DeviceError> {
    assert!(index < 4);
    match backend() {
        Backend::Sdl(shared) => {
            let _ = cache;
            sdl::poll(shared, index)
        }
        #[cfg(windows)]
        Backend::XInput => windows::poll(index as u32, cache),
        #[cfg(not(windows))]
        Backend::Unavailable => Err(DeviceError::Unavailable),
    }
}

#[cfg(test)]
mod conversion_tests {
    use super::*;

    /// SDL's XInput expansion: byte*257-32768 on the full axis, then the
    /// gamepad layer rescales -32768..=32767 onto 0..=32767.
    fn sdl_trigger(byte: u8) -> i16 {
        let axis = i32::from(byte) * 257 - 32768;
        ((axis + 32768) * 32767 / 65535) as i16
    }

    #[test]
    fn every_trigger_byte_survives_the_sdl_round_trip() {
        for byte in 0..=255u8 {
            assert_eq!(xinput_trigger(sdl_trigger(byte)), byte);
        }
        assert_eq!(xinput_trigger(-1), 0);
    }

    #[test]
    fn stick_y_inversion_restores_xinput_values_without_overflow() {
        for y in [i16::MIN, -1, 0, 1, 12345, i16::MAX] {
            assert_eq!(xinput_y(!y), y);
        }
        assert_eq!(xinput_y(i16::MIN), i16::MAX);
    }

    #[test]
    fn paddle_masks_round_trip_and_names_use_xinput_bits() {
        set_paddles([0x1000, 0, 0x0100, 0x8000]);
        assert_eq!(paddles(), [0x1000, 0, 0x0100, 0x8000]);
        set_paddles([0; 4]);
        assert_eq!(button_mask("A"), Some(0x1000));
        assert_eq!(button_mask("left_shoulder"), Some(0x0100));
        assert_eq!(button_mask("guide"), None);
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    #[test]
    fn capability_cache_refreshes_and_never_caches_errors() {
        let start = std::time::Instant::now();
        let mut cache = CapabilityCache::default();
        assert_eq!(cache.get(start, || Ok(1)), Ok(1));
        assert_eq!(
            cache.get(start + std::time::Duration::from_millis(999), || panic!(
                "redundant capability query"
            )),
            Ok(1)
        );
        assert_eq!(
            cache.get(start + std::time::Duration::from_secs(1), || Ok(2)),
            Ok(2)
        );
        cache.invalidate();
        assert_eq!(
            cache.get(start, || Err(DeviceError::Capabilities(5))),
            Err(DeviceError::Capabilities(5))
        );
        assert_eq!(cache.get(start, || Ok(3)), Ok(3));
        cache.invalidate();
        assert_eq!(cache.get(start, || Ok(4)), Ok(4));
    }
}

// Preserve the uncached API for menu-only polling.
pub(crate) fn poll(index: usize) -> Result<DevicePacket, DeviceError> {
    poll_cached(index, &mut CapabilityCache::default())
}
