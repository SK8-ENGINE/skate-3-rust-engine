//! Device transport. SDL3 gamepads are the default on every native platform; each
//! state is re-expressed as the XInput-shaped `XboxState`, so raw signed
//! axes/trigger bytes reach the TU3 converter without Bevy/gilrs deadzones or
//! normalized-axis reconstruction. `SKATE3_INPUT=xinput` selects the original
//! Windows XInput transport, which is also the fallback if SDL cannot start.
//! The browser build reads the Gamepad API's standard layout into the same shape.
use super::controller_kind::{ControllerKind, XinputCaps};
use skate_core::input::xbox::XboxState;
use std::sync::{Arc, OnceLock};
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
    /// Who is in the slot. Metadata only: gameplay reads `state`/`subtype`.
    pub kind: Option<Arc<ControllerKind>>,
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

/// XInput capability identity of one slot: the subtype `xbox::convert` needs
/// and the controller kind built from the same capability read.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct XinputIdentity {
    pub subtype: u8,
    pub kind: Arc<ControllerKind>,
}

/// Device identity is metadata; raw input is still sampled every host frame.
/// Refresh periodically as well as after errors, so hot swaps cannot leave a
/// subtype cached indefinitely even if Windows never exposes a disconnect.
pub(crate) struct CapabilityCache<T = XinputIdentity> {
    value: Option<(T, bevy::platform::time::Instant)>,
}
impl<T> Default for CapabilityCache<T> {
    fn default() -> Self {
        Self { value: None }
    }
}
impl<T: Clone> CapabilityCache<T> {
    pub(crate) fn invalidate(&mut self) {
        self.value = None;
    }
    #[cfg_attr(not(windows), allow(dead_code))]
    fn get(
        &mut self,
        now: bevy::platform::time::Instant,
        read: impl FnOnce() -> Result<T, DeviceError>,
    ) -> Result<T, DeviceError> {
        if let Some((subtype, expires)) = &self.value {
            if now < *expires {
                return Ok(subtype.clone());
            }
        }
        self.value = None;
        let subtype = read()?;
        self.value = Some((subtype.clone(), now + std::time::Duration::from_secs(1)));
        Ok(subtype)
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use crate::input::controller_kind;
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
    /// XINPUT_CAPABILITIES_EX as SDL declares it (xinput1_4 ordinal 108).
    #[repr(C)]
    struct CapabilitiesEx {
        capabilities: Capabilities,
        vendor_id: u16,
        product_id: u16,
        product_version: u16,
        unknown1: u16,
        unknown2: u32,
    }
    const _: () = assert!(size_of::<CapabilitiesEx>() == 32);
    type GetCapabilitiesEx = unsafe extern "system" fn(u32, u32, u32, *mut CapabilitiesEx) -> u32;

    #[link(name = "xinput")]
    unsafe extern "system" {
        fn XInputGetState(index: u32, state: *mut State) -> u32;
        fn XInputGetCapabilities(index: u32, flags: u32, capabilities: *mut Capabilities) -> u32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut std::ffi::c_void;
        fn GetProcAddress(module: *mut std::ffi::c_void, name: *const u8) -> *mut std::ffi::c_void;
    }

    /// Undocumented XInputGetCapabilitiesEx (Windows 8+ xinput1_4, ordinal
    /// 108), which SDL also uses for vendor/product. None when absent.
    fn capabilities_ex() -> Option<GetCapabilitiesEx> {
        static FUNCTION: OnceLock<Option<GetCapabilitiesEx>> = OnceLock::new();
        *FUNCTION.get_or_init(|| {
            let name: Vec<u16> = "xinput1_4.dll\0".encode_utf16().collect();
            // SAFETY: NUL-terminated wide string; the module is never freed.
            let module = unsafe { LoadLibraryW(name.as_ptr()) };
            if module.is_null() {
                return None;
            }
            // SAFETY: ordinal lookup (MAKEINTRESOURCE(108)) in a loaded module.
            let function = unsafe { GetProcAddress(module, 108usize as *const u8) };
            // SAFETY: the export at ordinal 108 has SDL's declared signature.
            (!function.is_null()).then(|| unsafe {
                std::mem::transmute::<*mut std::ffi::c_void, GetCapabilitiesEx>(function)
            })
        })
    }

    /// Vendor/product of slot `index`, if XInputGetCapabilitiesEx exists and
    /// reports one. Never affects the subtype gameplay uses.
    fn vendor_product(index: u32) -> Option<(u16, u16)> {
        let function = capabilities_ex()?;
        let mut capabilities = MaybeUninit::<CapabilitiesEx>::zeroed();
        // SAFETY: SDL's call shape (1, index, 0, out); read only on success.
        let result = unsafe { function(1, index, 0, capabilities.as_mut_ptr()) };
        if result != 0 {
            return None;
        }
        // SAFETY: zero-initialised storage the successful call filled.
        let capabilities = unsafe { capabilities.assume_init() };
        (capabilities.vendor_id != 0).then_some((capabilities.vendor_id, capabilities.product_id))
    }

    fn kind(capabilities: &Capabilities, index: u32, user: &[controller_kind::Model]) -> ControllerKind {
        let caps = XinputCaps {
            subtype: capabilities.subtype,
            flags: capabilities.flags,
            vendor_product: vendor_product(index),
        };
        controller_kind::from_xinput(caps, user)
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
        let identity = cache.get(bevy::platform::time::Instant::now(), || {
            let mut capabilities = MaybeUninit::<Capabilities>::uninit();
            // SAFETY: writable storage with the SDK ABI; read only on success.
            let result = unsafe { XInputGetCapabilities(index, 1, capabilities.as_mut_ptr()) };
            if result != 0 {
                return Err(DeviceError::Capabilities(result));
            }
            let capabilities = unsafe { capabilities.assume_init() };
            let kind = kind(&capabilities, index, controller_kind::user_models());
            Ok(XinputIdentity { subtype: capabilities.subtype, kind: Arc::new(kind) })
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
            subtype: identity.subtype,
            kind: Some(identity.kind),
        })
    }

    /// Identity of a slot without touching gameplay state (hardware test).
    #[cfg(test)]
    pub(crate) fn identity(index: u32) -> Option<ControllerKind> {
        let mut capabilities = MaybeUninit::<Capabilities>::uninit();
        // SAFETY: as in `poll`.
        if unsafe { XInputGetCapabilities(index, 1, capabilities.as_mut_ptr()) } != 0 {
            return None;
        }
        let capabilities = unsafe { capabilities.assume_init() };
        Some(kind(&capabilities, index, &[]))
    }

    /// The undocumented export exists on this Windows and agrees with the
    /// documented call about which slots hold a device.
    #[test]
    fn capabilities_ex_resolves_and_agrees_with_documented_call() {
        let function = capabilities_ex().expect("xinput1_4 ordinal 108");
        for index in 0..4 {
            let mut ex = MaybeUninit::<CapabilitiesEx>::zeroed();
            let mut plain = MaybeUninit::<Capabilities>::zeroed();
            // SAFETY: as in `vendor_product` / `poll`.
            let ex_result = unsafe { function(1, index, 0, ex.as_mut_ptr()) };
            let plain_result = unsafe { XInputGetCapabilities(index, 0, plain.as_mut_ptr()) };
            assert_eq!(ex_result == 0, plain_result == 0, "slot {index}: {ex_result} vs {plain_result}");
            if ex_result == 0 {
                let (ex, plain) = unsafe { (ex.assume_init(), plain.assume_init()) };
                assert_eq!((ex.capabilities.device_type, ex.capabilities.subtype), (plain.device_type, plain.subtype));
            }
        }
    }
}

/// XInput identity of a slot, for the hardware test.
#[cfg(all(windows, test))]
pub(crate) fn xinput_identity(index: u32) -> Option<ControllerKind> {
    windows::identity(index)
}

#[cfg(not(target_arch = "wasm32"))]
mod sdl {
    use super::*;
    use crate::input::controller_kind;
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

    type Published = Option<(u32, XboxState, Arc<ControllerKind>)>;

    /// Latest (packet number, state, identity) per slot, like XInputGetState's snapshot.
    pub(super) struct Shared {
        slots: Mutex<[Published; 4]>,
    }

    struct Slot {
        pad: Gamepad,
        id: u32,
        number: u32,
        state: XboxState,
        kind: Arc<ControllerKind>,
    }

    /// SDL must be pumped on the thread that initialised it, while Bevy runs
    /// systems on a pool; a dedicated thread owns SDL and publishes snapshots.
    pub(super) fn start() -> Result<&'static Shared, String> {
        let shared: &'static Shared = Box::leak(Box::new(Shared { slots: Mutex::new(Default::default()) }));
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
            .as_ref()
            .map(|(number, state, kind)| DevicePacket {
                number: *number,
                state: *state,
                subtype: SUBTYPE_GAMEPAD,
                kind: Some(kind.clone()),
            })
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
            let mut published: [Published; 4] = Default::default();
            for (slot, output) in slots.iter_mut().zip(&mut published) {
                let Some(slot) = slot else { continue };
                let state = read(&slot.pad, paddles);
                // Mirrors XInput dwPacketNumber: advances only when state changes.
                if state != slot.state {
                    slot.number = slot.number.wrapping_add(1);
                    slot.state = state;
                }
                *output = Some((slot.number, state, slot.kind.clone()));
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
        let kind = Arc::new(controller_kind::from_sdl(
            controller_kind::SdlReport {
                name: pad.name().unwrap_or_default(),
                gamepad_type: pad.r#type(),
                vendor_id: pad.vendor_id(),
                product_id: pad.product_id(),
                path: pad.path().unwrap_or_default(),
                paddles: PADDLE_BUTTONS.iter().filter(|&&b| pad.has_button(b)).count() as u8,
                touchpad: pad.touchpads_count() > 0,
                misc_button: pad.has_button(Button::Misc1),
            },
            controller_kind::user_models(),
        ));
        slots[index] = Some(Slot { pad, id: id.raw(), number: 0, state: XboxState::default(), kind });
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

/// The W3C Gamepad API's "standard" layout in XInput's shape: what the browser build reads.
/// Pure, so it is tested natively.
#[cfg(any(target_arch = "wasm32", test))]
mod standard {
    use skate_core::input::xbox::XboxState;

    /// Standard-layout button index → XInput `wButtons` bit, by position like SDL's South/East/
    /// West/North. Buttons 6 and 7 are the analog triggers; 16, the guide button, has no XInput bit.
    pub(super) const BUTTONS: [(usize, u16); 14] = [
        (12, 0x0001), (13, 0x0002), (14, 0x0004), (15, 0x0008),
        (9, 0x0010), (8, 0x0020), (10, 0x0040), (11, 0x0080),
        (4, 0x0100), (5, 0x0200),
        (0, 0x1000), (1, 0x2000), (2, 0x4000), (3, 0x8000),
    ];
    pub(super) const LEFT_TRIGGER: usize = 6;
    pub(super) const RIGHT_TRIGGER: usize = 7;

    /// An axis in -1..=1 → XInput's -32768..=32767.
    pub(super) fn stick(value: f64) -> i16 {
        let value = if value.is_finite() { value.clamp(-1., 1.) } else { 0. };
        (if value < 0. { value * 32768. } else { value * 32767. }).round() as i16
    }

    /// A trigger in 0..=1 → XInput's byte.
    pub(super) fn trigger(value: f64) -> u8 {
        let value = if value.is_finite() { value.clamp(0., 1.) } else { 0. };
        (value * 255.).round() as u8
    }

    /// `pressed`/`value` per button index; `axes` are left X/Y then right X/Y, Y positive down
    /// (XInput's Y is positive up).
    pub(super) fn state(pressed: impl Fn(usize) -> bool, value: impl Fn(usize) -> f64, axes: [f64; 4]) -> XboxState {
        XboxState {
            buttons: BUTTONS.iter().filter(|&&(index, _)| pressed(index)).fold(0, |bits, &(_, bit)| bits | bit),
            triggers: [trigger(value(LEFT_TRIGGER)), trigger(value(RIGHT_TRIGGER))],
            left: [stick(axes[0]), stick(-axes[1])],
            right: [stick(axes[2]), stick(-axes[3])],
        }
    }
}

/// The browser's pads, through the Gamepad API. Browsers only expose a pad once a button on it
/// has been pressed while the page is open.
#[cfg(target_arch = "wasm32")]
mod gamepad_api {
    use super::*;
    use crate::input::controller_kind;
    use bevy::log::{info, warn};
    use std::{collections::BTreeSet, sync::Mutex};
    use wasm_bindgen::JsCast;
    use web_sys::{Gamepad, GamepadButton, GamepadMappingType};

    /// The Gamepad API reports gamepads only; this is XINPUT_DEVSUBTYPE_GAMEPAD.
    const SUBTYPE_GAMEPAD: u8 = 1;

    struct Slot {
        /// The browser's own index for the pad.
        index: u32,
        number: u32,
        state: XboxState,
        kind: Arc<ControllerKind>,
    }

    struct Slots {
        slots: [Option<Slot>; 4],
        /// Pads without the standard layout, already reported.
        ignored: BTreeSet<String>,
    }

    static SLOTS: Mutex<Slots> = Mutex::new(Slots { slots: [None, None, None, None], ignored: BTreeSet::new() });

    fn connected() -> Vec<Gamepad> {
        let Some(pads) = web_sys::window().and_then(|window| window.navigator().get_gamepads().ok()) else {
            return Vec::new();
        };
        pads.iter().filter_map(|pad| pad.dyn_into::<Gamepad>().ok()).filter(Gamepad::connected).collect()
    }

    fn read(pad: &Gamepad) -> XboxState {
        let buttons: Vec<GamepadButton> = pad.buttons().iter().filter_map(|b| b.dyn_into().ok()).collect();
        let axes: Vec<f64> = pad.axes().iter().map(|a| a.as_f64().unwrap_or(0.)).collect();
        standard::state(
            |i| buttons.get(i).is_some_and(GamepadButton::pressed),
            |i| buttons.get(i).map_or(0., GamepadButton::value),
            std::array::from_fn(|i| axes.get(i).copied().unwrap_or(0.)),
        )
    }

    /// Mirrors the SDL thread: pads keep their slot while connected, a new one takes its browser
    /// index if free or else the first free slot, and the packet number advances on change.
    fn refresh(shared: &mut Slots) {
        let pads = connected();
        for (index, slot) in shared.slots.iter_mut().enumerate() {
            if slot.as_ref().is_some_and(|s| !pads.iter().any(|pad| pad.index() == s.index)) {
                *slot = None;
                info!("Controller {index}: gamepad removed");
            }
        }
        for pad in &pads {
            if pad.mapping() != GamepadMappingType::Standard {
                if shared.ignored.insert(pad.id()) {
                    warn!("Gamepad {:?}: the browser does not map it to the standard layout, so it is not used", pad.id());
                }
                continue;
            }
            let slot = match shared.slots.iter().position(|s| s.as_ref().is_some_and(|s| s.index == pad.index())) {
                Some(slot) => slot,
                None => {
                    let preferred = usize::try_from(pad.index()).ok().filter(|&i| i < 4 && shared.slots[i].is_none());
                    let Some(slot) = preferred.or_else(|| shared.slots.iter().position(Option::is_none)) else {
                        if shared.ignored.insert(pad.id()) {
                            warn!("Gamepad {:?}: all four controller slots are in use", pad.id());
                        }
                        continue;
                    };
                    info!("Controller {slot}: gamepad {:?} (Gamepad API index {})", pad.id(), pad.index());
                    let kind = Arc::new(controller_kind::from_gamepad_api(&pad.id(), controller_kind::user_models()));
                    shared.slots[slot] = Some(Slot { index: pad.index(), number: 0, state: XboxState::default(), kind });
                    slot
                }
            };
            let state = read(pad);
            let slot = shared.slots[slot].as_mut().expect("slot just found or filled");
            if state != slot.state {
                slot.number = slot.number.wrapping_add(1);
                slot.state = state;
            }
        }
    }

    pub(super) fn poll(index: usize) -> Result<DevicePacket, DeviceError> {
        let mut shared = SLOTS.lock().unwrap_or_else(|e| e.into_inner());
        refresh(&mut shared);
        shared.slots[index]
            .as_ref()
            .map(|slot| DevicePacket {
                number: slot.number,
                state: slot.state,
                subtype: SUBTYPE_GAMEPAD,
                kind: Some(slot.kind.clone()),
            })
            .ok_or(DeviceError::Disconnected)
    }
}

enum Backend {
    #[cfg(not(target_arch = "wasm32"))]
    Sdl(&'static sdl::Shared),
    #[cfg(target_arch = "wasm32")]
    GamepadApi,
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
        start_sdl()
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn start_sdl() -> Backend {
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
}

/// The browser has no SDL: its pads arrive through the Gamepad API.
#[cfg(target_arch = "wasm32")]
fn start_sdl() -> Backend {
    bevy::log::info!("Controller input: Gamepad API (a pad appears once one of its buttons is pressed)");
    Backend::GamepadApi
}

pub(crate) fn poll_cached(
    index: usize,
    cache: &mut CapabilityCache,
) -> Result<DevicePacket, DeviceError> {
    assert!(index < 4);
    match backend() {
        #[cfg(not(target_arch = "wasm32"))]
        Backend::Sdl(shared) => {
            let _ = cache;
            sdl::poll(shared, index)
        }
        #[cfg(target_arch = "wasm32")]
        Backend::GamepadApi => {
            let _ = cache;
            gamepad_api::poll(index)
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

    #[test]
    fn standard_layout_buttons_land_on_the_xinput_bits_of_the_same_names() {
        // Standard-layout order: A B X Y, LB RB, LT RT, back start, LS RS, up down left right.
        let names = ["a", "b", "x", "y", "left_shoulder", "right_shoulder", "", "", "back", "start",
            "left_stick", "right_stick", "dpad_up", "dpad_down", "dpad_left", "dpad_right"];
        for (index, name) in names.iter().enumerate().filter(|(_, name)| !name.is_empty()) {
            let state = standard::state(|i| i == index, |_| 0., [0.; 4]);
            assert_eq!(Some(state.buttons), button_mask(name), "{name}");
        }
        let all = standard::state(|_| true, |_| 1., [0.; 4]);
        assert_eq!(all.buttons, BUTTON_NAMES.iter().fold(0, |bits, &(_, bit)| bits | bit), "every XInput button, no extra bits");
        assert_eq!(all.triggers, [255, 255]);
    }

    #[test]
    fn standard_layout_axes_and_triggers_reach_xinput_ranges() {
        // Y is positive down in the Gamepad API and positive up in XInput.
        let state = standard::state(|_| false, |i| [0., 0., 0., 0., 0., 0., 0.5, 0.][i.min(7)], [1., 1., -1., -1.]);
        assert_eq!(state.left, [i16::MAX, i16::MIN]);
        assert_eq!(state.right, [i16::MIN, i16::MAX]);
        assert_eq!(state.triggers, [128, 0]);
        assert_eq!(standard::stick(0.), 0);
        assert_eq!(standard::stick(f64::NAN), 0);
        assert_eq!(standard::stick(2.), i16::MAX);
        assert_eq!(standard::trigger(-0.5), 0);
        assert_eq!(standard::trigger(f64::INFINITY), 0);
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    #[test]
    fn capability_cache_refreshes_and_never_caches_errors() {
        let start = bevy::platform::time::Instant::now();
        let mut cache = CapabilityCache::<u8>::default();
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
