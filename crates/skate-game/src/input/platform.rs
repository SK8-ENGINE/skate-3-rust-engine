//! Device transport. Windows reads raw XInput; elsewhere Bevy's gilrs backend
//! (default filters off, change thresholds zeroed) is quantized back to XInput
//! ranges. Both reach the TU3 converter without deadzones or remapping.
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use skate_core::input::xbox::XboxState;

pub(crate) struct DevicePacket {
    pub number: u32,
    pub state: XboxState,
    pub subtype: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeviceError {
    Disconnected,
    #[cfg(windows)]
    State(u32),
    Capabilities(u32),
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

#[cfg(not(windows))]
mod gamepad {
    use super::*;
    use bevy::input::gamepad::{
        AxisSettings, ButtonAxisSettings, GamepadAxis, GamepadButton, GamepadSettings,
    };

    /// XINPUT_DEVSUBTYPE_GAMEPAD: gilrs exposes no guitar/wheel subtype.
    const SUBTYPE: u8 = 1;
    /// XINPUT_GAMEPAD_* bits consumed by `skate_core::input::xbox::convert`.
    const BUTTONS: [(GamepadButton, u16); 14] = [
        (GamepadButton::DPadUp, 0x0001),
        (GamepadButton::DPadDown, 0x0002),
        (GamepadButton::DPadLeft, 0x0004),
        (GamepadButton::DPadRight, 0x0008),
        (GamepadButton::Start, 0x0010),
        (GamepadButton::Select, 0x0020),
        (GamepadButton::LeftThumb, 0x0040),
        (GamepadButton::RightThumb, 0x0080),
        (GamepadButton::LeftTrigger, 0x0100),
        (GamepadButton::RightTrigger, 0x0200),
        (GamepadButton::South, 0x1000),
        (GamepadButton::East, 0x2000),
        (GamepadButton::West, 0x4000),
        (GamepadButton::North, 0x8000),
    ];

    #[derive(Clone, Copy)]
    struct Slot {
        entity: Entity,
        state: XboxState,
        number: u32,
    }

    /// Per-frame snapshot, sampled once in `PreUpdate` after Bevy input.
    #[derive(Resource, Default)]
    pub(crate) struct Slots([Option<Slot>; 4]);

    impl Slots {
        pub(super) fn poll(&self, index: usize) -> Result<DevicePacket, DeviceError> {
            let slot = self.0[index].ok_or(DeviceError::Disconnected)?;
            Ok(DevicePacket {
                number: slot.number,
                state: slot.state,
                subtype: SUBTYPE,
            })
        }

        /// Like XInput user indices, a connected device keeps its slot; new
        /// devices take the lowest free slot and devices beyond four are ignored.
        /// The packet number advances only when the sampled state changes.
        pub(super) fn update(&mut self, connected: &[(Entity, XboxState)]) {
            for entry in &mut self.0 {
                let Some(slot) = entry else { continue };
                match connected.iter().find(|(entity, _)| *entity == slot.entity) {
                    None => *entry = None,
                    Some(&(_, state)) if state != slot.state => {
                        slot.state = state;
                        slot.number = slot.number.wrapping_add(1);
                    }
                    Some(_) => {}
                }
            }
            for &(entity, state) in connected {
                if self.0.iter().flatten().any(|slot| slot.entity == entity) {
                    continue;
                }
                let Some(free) = self.0.iter_mut().find(|entry| entry.is_none()) else {
                    break;
                };
                *free = Some(Slot {
                    entity,
                    state,
                    number: 0,
                });
            }
        }
    }

    /// `Gamepad::analog` stores unfiltered values; a zero threshold makes Bevy
    /// record every change instead of dropping moves below 1%.
    fn raw_settings() -> GamepadSettings {
        GamepadSettings {
            default_axis_settings: AxisSettings::new(-1.0, 0.0, 0.0, 1.0, 0.0)
                .expect("valid axis settings"),
            default_button_axis_settings: ButtonAxisSettings {
                high: 1.0,
                low: 0.0,
                threshold: 0.0,
            },
            ..default()
        }
    }

    pub(super) fn xbox_state(pad: &Gamepad) -> XboxState {
        let buttons = BUTTONS
            .iter()
            .filter(|(button, _)| pad.pressed(*button))
            .fold(0, |bits, (_, bit)| bits | bit);
        let trigger =
            |button| (pad.get(button).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0).round() as u8;
        // XInput is signed 16-bit with +Y up, matching gilrs; the converter
        // scales by 1/32768, so full deflection maps to -32768/32767.
        let axis = |axis| {
            (pad.get(axis).unwrap_or(0.0) * 32768.0)
                .round()
                .clamp(-32768.0, 32767.0) as i16
        };
        XboxState {
            buttons,
            triggers: [
                trigger(GamepadButton::LeftTrigger2),
                trigger(GamepadButton::RightTrigger2),
            ],
            left: [axis(GamepadAxis::LeftStickX), axis(GamepadAxis::LeftStickY)],
            right: [
                axis(GamepadAxis::RightStickX),
                axis(GamepadAxis::RightStickY),
            ],
        }
    }

    pub(super) fn sample(
        mut slots: ResMut<Slots>,
        pads: Query<(Entity, &Gamepad)>,
        mut connected: Query<&mut GamepadSettings, Added<Gamepad>>,
    ) {
        for mut settings in &mut connected {
            *settings = raw_settings();
        }
        let states: Vec<_> = pads
            .iter()
            .map(|(entity, pad)| (entity, xbox_state(pad)))
            .collect();
        slots.update(&states);
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn state(buttons: u16) -> XboxState {
            XboxState {
                buttons,
                triggers: [0; 2],
                left: [0; 2],
                right: [0; 2],
            }
        }

        #[test]
        fn buttons_map_to_xinput_bits_and_analog_quantizes_to_xinput_ranges() {
            let mut pad = Gamepad::default();
            for button in [
                GamepadButton::South,
                GamepadButton::RightTrigger,
                GamepadButton::DPadLeft,
                GamepadButton::Select,
            ] {
                pad.digital_mut().press(button);
            }
            pad.analog_mut().set(GamepadButton::LeftTrigger2, 1.0);
            pad.analog_mut().set(GamepadButton::RightTrigger2, 0.5);
            pad.analog_mut().set(GamepadAxis::LeftStickX, -1.0);
            pad.analog_mut().set(GamepadAxis::LeftStickY, 1.0);
            pad.analog_mut().set(GamepadAxis::RightStickX, 0.25);
            pad.analog_mut().set(GamepadAxis::RightStickY, -0.5);
            assert_eq!(
                xbox_state(&pad),
                XboxState {
                    buttons: 0x1000 | 0x0200 | 0x0004 | 0x0020,
                    triggers: [255, 128],
                    left: [-32768, 32767],
                    right: [8192, -16384],
                }
            );
        }

        #[test]
        fn slots_are_stable_across_disconnects_and_capped_at_four() {
            let mut world = World::new();
            let pads: Vec<Entity> = (0..6).map(|_| world.spawn_empty().id()).collect();
            let mut slots = Slots::default();
            slots.update(&[(pads[0], state(0)), (pads[1], state(1))]);
            // Slot 0 unplugs: slot 1 must not shift down.
            slots.update(&[(pads[1], state(0))]);
            assert_eq!(slots.poll(0).err(), Some(DeviceError::Disconnected));
            assert!(slots.poll(1).is_ok());
            // New devices fill the lowest free slot; a fifth is ignored.
            // Each pad's state carries its position as a tag.
            let all: Vec<_> = (1..6).map(|n| (pads[n], state(n as u16))).collect();
            slots.update(&all);
            let owners: Vec<u16> = (0..4)
                .map(|i| slots.poll(i).unwrap().state.buttons)
                .collect();
            assert_eq!(owners, [2, 1, 3, 4]);
        }

        #[test]
        fn packet_number_advances_only_on_state_change() {
            let mut world = World::new();
            let pad = world.spawn_empty().id();
            let mut slots = Slots::default();
            slots.update(&[(pad, state(0))]);
            let first = slots.poll(0).unwrap().number;
            slots.update(&[(pad, state(0))]);
            assert_eq!(slots.poll(0).unwrap().number, first);
            slots.update(&[(pad, state(0x1000))]);
            let packet = slots.poll(0).unwrap();
            assert_eq!(
                (packet.number, packet.state.buttons, packet.subtype),
                (first + 1, 0x1000, 1)
            );
        }

        #[test]
        fn sampling_zeroes_change_thresholds_and_fills_a_slot_on_connect() {
            use bevy::ecs::system::RunSystemOnce;
            let mut world = World::new();
            world.init_resource::<Slots>();
            let pad = world.spawn(Gamepad::default()).id();
            world.run_system_once(sample).unwrap();
            // Bevy's default 0.01 threshold drops small stick and trigger moves.
            let settings = world.get::<GamepadSettings>(pad).unwrap();
            assert_eq!(
                settings
                    .get_axis_settings(GamepadAxis::LeftStickX)
                    .threshold(),
                0.0
            );
            assert_eq!(
                settings
                    .get_button_axis_settings(GamepadButton::LeftTrigger2)
                    .threshold,
                0.0
            );
            assert!(world.resource::<Slots>().poll(0).is_ok());
        }
    }
}

/// Name used in controller status logs.
#[cfg(windows)]
pub(crate) const BACKEND: &str = "raw XInput";
#[cfg(not(windows))]
pub(crate) const BACKEND: &str = "gilrs";

/// Consumers of device polls order themselves `.after(DeviceSet)`.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DeviceSet;

pub(crate) fn install(app: &mut App) {
    #[cfg(not(windows))]
    app.init_resource::<gamepad::Slots>()
        .configure_sets(PreUpdate, DeviceSet.after(bevy::input::InputSystems))
        .add_systems(PreUpdate, gamepad::sample.in_set(DeviceSet));
    #[cfg(windows)]
    let _ = app;
}

/// Platform device access for systems. Windows polls XInput at call time;
/// elsewhere this reads the snapshot taken in `DeviceSet`.
#[derive(SystemParam)]
pub(crate) struct Devices<'w> {
    #[cfg(not(windows))]
    slots: Res<'w, gamepad::Slots>,
    #[cfg(windows)]
    _xinput: std::marker::PhantomData<&'w ()>,
}

impl Devices<'_> {
    pub(crate) fn poll_cached(
        &self,
        index: usize,
        cache: &mut CapabilityCache,
    ) -> Result<DevicePacket, DeviceError> {
        assert!(index < 4);
        #[cfg(windows)]
        return windows::poll(index as u32, cache);
        #[cfg(not(windows))]
        {
            // Subtype is constant here, so there is no capability query to cache.
            let _ = cache;
            self.slots.poll(index)
        }
    }

    // Preserve the uncached API for menu-only polling.
    pub(crate) fn poll(&self, index: usize) -> Result<DevicePacket, DeviceError> {
        self.poll_cached(index, &mut CapabilityCache::default())
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
