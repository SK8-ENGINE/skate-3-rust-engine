//! Controller rumble on the pads at retail's 60 Hz, gated by Vibration; mods: `sdk.input.rumble`.
//! Sources and gaps: docs/hails-additions/23-input-controllers.md#rumble-on-pads
use bevy::prelude::*;
use skate_core::rumble::{motor_speed, PadRumble, Rumble, RumbleTuning, TICK_HZ};

const VIBRATION: &str = "ID_GAMESETTINGS_VIBRATION_TOGGLE";
/// Retail's settings test rumble (`sub_8260DF40`): channel 0, 0.5 / 0.5 at 0x8209975C, rate 5.0 at 0x821F1790.
const TEST: Rumble = Rumble { low: 0.5, high: 0.5, rate: 5.0 };

#[derive(Resource)]
pub(crate) struct RumbleOutput {
    pads: [PadRumble; 4],
    pub tuning: RumbleTuning,
    /// Real seconds not yet spent on 60 Hz ticks.
    clock: f32,
    levels: [[f32; 2]; 4],
    /// The Vibration setting as last seen (None before the first frame).
    vibration: Option<bool>,
    /// Channels each mod started (pad, internal channel), stopped when the mod goes.
    owners: std::collections::BTreeMap<String, std::collections::BTreeSet<(usize, u32)>>,
}

/// Mod channels live above retail's: bit 31 set, the mod id's FNV-1a hash in bits 10..31, its own channel (0..1023)
/// below, so two mods never share a channel.
fn mod_channel(owner: &str, channel: u32) -> u32 {
    let hash = owner.bytes().fold(0x811c_9dc5u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193));
    0x8000_0000 | ((hash & 0x1f_ffff) << 10) | (channel & 0x3ff)
}

impl Default for RumbleOutput {
    fn default() -> Self {
        Self {
            pads: std::array::from_fn(|_| PadRumble::new(true)),
            tuning: RumbleTuning::RETAIL,
            clock: 0.0,
            levels: [[0.0; 2]; 4],
            vibration: None,
            owners: Default::default(),
        }
    }
}

impl RumbleOutput {
    /// Start (or restart) a channel on a pad; ignored while Vibration is off.
    pub(crate) fn start(&mut self, pad: usize, channel: u32, rumble: Rumble) {
        if let Some(p) = self.pads.get_mut(pad) {
            p.start(channel, rumble, &self.tuning);
        }
    }

    /// `sdk.input.rumble`: a mod's channel on a pad (None = the active pad).
    pub(crate) fn start_mod(&mut self, owner: &str, pad: usize, channel: u32, rumble: Rumble) {
        info!("RUMBLE mod {owner} pad {pad} channel {channel}: low {} high {} rate {}{}", rumble.low, rumble.high, rumble.rate,
            if self.vibration == Some(false) { " (Vibration is off: not started)" } else { "" });
        let internal = mod_channel(owner, channel);
        self.owners.entry(owner.to_owned()).or_default().insert((pad, internal));
        self.start(pad, internal, rumble);
    }

    /// `sdk.input.stop_rumble`: a mod's channels stop (one pad, or all with None).
    pub(crate) fn stop_mod(&mut self, owner: &str, pad: Option<usize>) {
        let Some(set) = self.owners.get_mut(owner) else { return };
        if !set.is_empty() {
            info!("RUMBLE mod {owner} stopped (pad {pad:?})");
        }
        set.retain(|&(p, channel)| {
            let hit = pad.is_none_or(|q| q == p);
            if hit {
                self.pads[p].stop(channel);
            }
            !hit
        });
    }

    /// Advance by `seconds` of real time at the 60 Hz retail tick; Vibration On / Off as given. Returns whether
    /// the setting just switched on (the caller plays the test rumble).
    fn advance(&mut self, seconds: f32, vibration: bool) -> bool {
        let switched_on = self.vibration == Some(false) && vibration;
        if self.vibration != Some(vibration) {
            info!("RUMBLE Vibration {}", if vibration { "on" } else { "off" });
        }
        self.vibration = Some(vibration);
        for pad in &mut self.pads {
            pad.enabled = vibration;
        }
        self.clock = (self.clock + seconds).min(0.25);
        while self.clock >= 1.0 / TICK_HZ {
            self.clock -= 1.0 / TICK_HZ;
            for (pad, levels) in self.pads.iter_mut().zip(&mut self.levels) {
                *levels = pad.tick(&self.tuning);
            }
        }
        switched_on
    }
}

pub(crate) struct RumblePlugin;

impl Plugin for RumblePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RumbleOutput>().add_systems(Update, update);
    }
}

/// Retail's curve choice (`sub_8296D480`): the wireless curve only when the pad reports X_INPUT_CAPS_WIRELESS (SDL:
/// a wireless connection), the wired curve for every other pad, including one whose connection is unknown.
fn wireless_pad(kind: Option<&crate::input::controller_kind::ControllerKind>) -> bool {
    kind.is_some_and(|k| k.wireless == Some(true))
}

/// What goes to the pad: the device curve while a channel is live, else [0, 0]. NOT RETAIL (host): retail runs the
/// curve every tick, so a wireless pad's motor 1 gets its 0.05 floor even with no channel, and the console's driver
/// lets it lapse; a PC pad would buzz from game start, so an idle pad gets a stop instead.
fn pad_speeds(levels: [f32; 2], wireless: bool, tuning: &RumbleTuning) -> [u16; 2] {
    if levels == [0.0, 0.0] {
        return [0, 0];
    }
    std::array::from_fn(|m| motor_speed(m, levels[m], wireless, tuning))
}

fn update(
    time: Res<Time<Real>>,
    menus: Option<Res<crate::retail_menus::RetailMenus>>,
    controllers: Res<crate::input::ControllerInput>,
    mut out: ResMut<RumbleOutput>,
    mut running: Local<[bool; 4]>,
    mut curves: Local<[Option<bool>; 4]>,
) {
    let vibration = menus
        .as_deref()
        .and_then(|m| crate::retail_menus::setting(m, VIBRATION))
        // No setting found: off (NOT RETAIL, the menu_values default; todo rumble-starts-and-sticks).
        .is_some_and(|v| skate_core::menu_values::number(&v) != 0.0);
    if out.advance(time.delta_secs(), vibration) {
        match controllers.active_slot() {
            Some(pad) => {
                info!("RUMBLE settings test rumble on pad {pad} (low 0.5 high 0.5 rate 5)");
                out.start(pad, 0, TEST);
            }
            None => info!("RUMBLE settings test rumble: no controller connected"),
        }
    }
    for pad in 0..4 {
        if controllers.status[pad] != crate::input::ControllerStatus::Ready {
            curves[pad] = None;
            continue;
        }
        let kind = controllers.kind(pad);
        let wireless = wireless_pad(kind);
        if curves[pad] != Some(wireless) {
            curves[pad] = Some(wireless);
            info!(
                "RUMBLE pad {pad}: {} curve ({})",
                if wireless { "wireless" } else { "wired" },
                kind.map_or_else(|| "no identity".to_string(), |k| k.summary())
            );
        }
        let active = out.levels[pad] != [0.0, 0.0];
        let speeds = pad_speeds(out.levels[pad], wireless, &out.tuning);
        if active != running[pad] {
            running[pad] = active;
            info!("RUMBLE pad {pad} {} (speeds {} / {})", if active { "rumbling" } else { "idle" }, speeds[0], speeds[1]);
        }
        crate::input::platform::set_rumble(pad, speeds);
    }
}

/// A mod stopped, failed or reloaded: its rumble stops at once.
pub(crate) fn stop_owner(world: &mut World, owner: &str) {
    if let Some(mut out) = world.get_resource_mut::<RumbleOutput>() {
        out.stop_mod(owner, None);
        out.owners.remove(owner);
    }
}

/// Every mod went (reload all).
pub(crate) fn clear_mods(world: &mut World) {
    if let Some(mut out) = world.get_resource_mut::<RumbleOutput>() {
        let owners: Vec<String> = out.owners.keys().cloned().collect();
        for owner in owners {
            out.stop_mod(&owner, None);
        }
        out.owners.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_channels_are_separate_and_stop_with_their_mod() {
        let mut out = RumbleOutput::default();
        out.advance(0.0, true);
        out.start_mod("a", 0, 1, Rumble { low: 0.4, high: 0.0, rate: 0.0 });
        out.start_mod("b", 0, 1, Rumble { low: 0.4, high: 0.0, rate: 0.0 });
        assert_ne!(mod_channel("a", 1), mod_channel("b", 1));
        out.advance(1.0 / 60.0, true);
        assert!(out.levels[0][0] > 0.4, "both mods' channels mix");
        out.stop_mod("a", None);
        out.advance(1.0 / 60.0, true);
        assert!((out.levels[0][0] - 0.4).abs() < 1e-6, "only b's channel is left");
        out.stop_mod("b", Some(0));
        out.advance(1.0 / 60.0, true);
        assert_eq!(out.levels[0][0], 0.0);
    }

    #[test]
    fn ticks_at_60_hz_whatever_the_frame_rate() {
        let at = |fps: f32| {
            let mut out = RumbleOutput::default();
            out.advance(0.0, true);
            out.start(0, 0, TEST);
            for _ in 0..(fps as usize / 2) {
                out.advance(1.0 / fps, true);
            }
            out.levels[0][0]
        };
        // Half a second = 30 ticks at any frame rate (to float rounding of the clock).
        let (a, b) = (at(30.0), at(240.0));
        assert!((a - b).abs() < 0.02, "{a} vs {b}");
        assert!(a > 0.0 && a < 0.5);
    }

    #[test]
    fn an_idle_pad_gets_a_stop_and_a_live_one_the_retail_curve() {
        let t = RumbleTuning::RETAIL;
        assert_eq!(pad_speeds([0.0, 0.0], true, &t), [0, 0], "no wireless floor without a live channel");
        assert_eq!(pad_speeds([0.0, 0.0], false, &t), [0, 0]);
        assert_eq!(pad_speeds([0.5, 0.0], true, &t), [motor_speed(0, 0.5, true, &t), 3276], "floor while live");
        assert_eq!(pad_speeds([0.5, 0.5], false, &t), [motor_speed(0, 0.5, false, &t), motor_speed(1, 0.5, false, &t)]);
    }

    #[test]
    fn only_a_wireless_pad_takes_the_wireless_curve() {
        use crate::input::controller_kind::{from_sdl, SdlReport};
        let pad = |wireless| {
            from_sdl(
                SdlReport {
                    name: "Xbox Elite".into(),
                    gamepad_type: sdl3::gamepad::GamepadType::XboxOne,
                    vendor_id: Some(0x045e),
                    product_id: Some(0x0b00),
                    path: String::new(),
                    paddles: 4,
                    touchpad: false,
                    misc_button: false,
                    wireless,
                    face_labels: None,
                },
                &[],
            )
        };
        assert!(wireless_pad(Some(&pad(Some(true)))));
        assert!(!wireless_pad(Some(&pad(Some(false)))), "a wired Xbox pad takes the wired curve, as in retail");
        assert!(!wireless_pad(Some(&pad(None))));
        assert!(!wireless_pad(None));
    }

    #[test]
    fn switching_vibration_on_reports_it_and_off_blocks_starts() {
        let mut out = RumbleOutput::default();
        assert!(!out.advance(0.0, true), "the first frame is not a switch");
        assert!(!out.advance(0.0, false));
        out.start(0, 0, TEST);
        out.advance(1.0 / 60.0, false);
        assert_eq!(out.levels[0], [0.0, 0.0], "no start while off");
        assert!(out.advance(0.0, true), "off -> on plays the test rumble");
    }
}
