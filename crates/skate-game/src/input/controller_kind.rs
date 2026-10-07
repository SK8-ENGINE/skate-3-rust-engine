//! Identity of the controller in each slot: family, exact model (USB
//! vendor/product), capabilities and the driver that delivers it. Identity is
//! metadata only. Gameplay input is unchanged by it: `xbox::convert` still
//! receives the XInput-shaped state and capability subtype as before.
//!
//! Retail Skate 3 runs on the Xbox 360 and only ever sees Xbox 360 pads, so
//! nothing here changes gameplay. It is for troubleshooting (log, Esc menu),
//! mods (`sdk.input.controller`) and future per-pad presentation (prompt
//! glyphs, see `PromptStyle`).
use serde::Serialize;

/// Which backend produced the slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Backend {
    Sdl,
    #[cfg_attr(not(windows), allow(dead_code))]
    Xinput,
}

/// Coarse controller kind. Stable snake_case names are the mod-facing form.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Family {
    Xbox360,
    XboxOne,
    XboxElite,
    /// An XInput gamepad whose generation is unknown (no vendor/product).
    XinputGamepad,
    Playstation3,
    Playstation4,
    Playstation5,
    SwitchPro,
    JoyconLeft,
    JoyconRight,
    JoyconPair,
    /// Any other pad SDL maps as a standard gamepad.
    Standard,
    // XInput device subtypes other than a gamepad.
    Wheel,
    ArcadeStick,
    FlightStick,
    DancePad,
    Guitar,
    DrumKit,
    ArcadePad,
    Unknown,
}

impl Family {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Xbox360 => "Xbox 360",
            Self::XboxOne => "Xbox One / Series",
            Self::XboxElite => "Xbox Elite",
            Self::XinputGamepad => "XInput gamepad",
            Self::Playstation3 => "PlayStation 3",
            Self::Playstation4 => "PlayStation 4",
            Self::Playstation5 => "PlayStation 5",
            Self::SwitchPro => "Switch Pro",
            Self::JoyconLeft => "Joy-Con (L)",
            Self::JoyconRight => "Joy-Con (R)",
            Self::JoyconPair => "Joy-Con pair",
            Self::Standard => "Standard gamepad",
            Self::Wheel => "Wheel",
            Self::ArcadeStick => "Arcade stick",
            Self::FlightStick => "Flight stick",
            Self::DancePad => "Dance pad",
            Self::Guitar => "Guitar",
            Self::DrumKit => "Drum kit",
            Self::ArcadePad => "Arcade pad",
            Self::Unknown => "Unknown",
        }
    }

    /// Which face-button names the pad prints. Layout itself follows physical
    /// position (South = A slot) for every family: Nintendo pads are not
    /// remapped to their printed labels (decision 2026-09-30).
    pub(crate) fn prompt_style(self) -> PromptStyle {
        match self {
            Self::Playstation3 | Self::Playstation4 | Self::Playstation5 => PromptStyle::Playstation,
            Self::SwitchPro | Self::JoyconLeft | Self::JoyconRight | Self::JoyconPair => PromptStyle::Nintendo,
            _ => PromptStyle::Xbox,
        }
    }
}

/// Face-button glyph set for button prompts. Hook only: the retail HUD we
/// load has no button-prompt glyphs (Xbox 360 disc, Xbox art only).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PromptStyle {
    Xbox,
    Playstation,
    Nintendo,
}

impl PromptStyle {
    /// Printed names of the South, East, West, North buttons.
    pub(crate) fn face_labels(self) -> [&'static str; 4] {
        match self {
            Self::Xbox => ["A", "B", "X", "Y"],
            Self::Playstation => ["Cross", "Circle", "Square", "Triangle"],
            Self::Nintendo => ["B", "A", "Y", "X"],
        }
    }
}

/// Everything known about one slot's controller.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct ControllerKind {
    pub family: Family,
    /// Device name (SDL), the model name, or the family name.
    pub name: String,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub backend: Backend,
    /// SDL device path (e.g. "XInput#0" = SDL's XInput driver, "\\?\HID#..."
    /// = HIDAPI / RawInput), or "XInput" for the XInput fallback.
    pub driver: String,
    /// XInput capability subtype (XINPUT_DEVSUBTYPE_*); XInput backend only.
    pub xinput_subtype: Option<u8>,
    /// XINPUT_CAPS_WIRELESS; XInput backend only.
    pub wireless: Option<bool>,
    /// Back paddles the driver reports as buttons (usable in settings).
    pub paddles: u8,
    /// Back paddles the model has, from the model table (0 = none / unknown).
    pub hardware_paddles: u8,
    pub touchpad: bool,
    /// A misc button (Share / Capture / Mute) is reported.
    pub misc_button: bool,
    pub prompt_style: PromptStyle,
}

impl ControllerKind {
    /// One line for the log and the Esc menu.
    pub(crate) fn summary(&self) -> String {
        let mut text = format!("{} [{}]", self.name, self.family.name());
        if let (Some(vendor), Some(product)) = (self.vendor_id, self.product_id) {
            text += &format!(" {vendor:04x}:{product:04x}");
        }
        text += &match self.backend {
            Backend::Sdl => format!(", SDL via {}", if self.driver.is_empty() { "?" } else { &self.driver }),
            Backend::Xinput => format!(
                ", XInput subtype {}{}",
                self.xinput_subtype.unwrap_or(0),
                if self.wireless == Some(true) { " wireless" } else { "" }
            ),
        };
        if self.hardware_paddles > 0 || self.paddles > 0 {
            text += &format!(", paddles {} of {}", self.paddles, self.hardware_paddles.max(self.paddles));
        }
        text
    }
}

/// A known model: exact vendor/product → name, family, back paddles.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Model {
    pub vendor: u16,
    pub product: u16,
    pub name: String,
    pub family: Option<Family>,
    pub paddles: u8,
}

const MICROSOFT: u16 = 0x045e;
const SONY: u16 = 0x054c;
const NINTENDO: u16 = 0x057e;

/// Default model table (public USB IDs). `settings/controller.json` `models`
/// entries are looked up first, so users can name pads this table lacks.
const MODELS: &[(u16, u16, &str, Family, u8)] = &[
    (MICROSOFT, 0x028e, "Xbox 360 Controller", Family::Xbox360, 0),
    (MICROSOFT, 0x028f, "Xbox 360 Wireless Controller", Family::Xbox360, 0),
    (MICROSOFT, 0x0719, "Xbox 360 Wireless Receiver", Family::Xbox360, 0),
    (MICROSOFT, 0x02d1, "Xbox One Controller", Family::XboxOne, 0),
    (MICROSOFT, 0x02dd, "Xbox One Controller", Family::XboxOne, 0),
    (MICROSOFT, 0x02ea, "Xbox One S Controller", Family::XboxOne, 0),
    (MICROSOFT, 0x02e0, "Xbox One S Controller (Bluetooth)", Family::XboxOne, 0),
    (MICROSOFT, 0x02fd, "Xbox One S Controller (Bluetooth)", Family::XboxOne, 0),
    (MICROSOFT, 0x0b20, "Xbox One S Controller (Bluetooth)", Family::XboxOne, 0),
    (MICROSOFT, 0x0b12, "Xbox Series X|S Controller", Family::XboxOne, 0),
    (MICROSOFT, 0x0b13, "Xbox Series X|S Controller (Bluetooth)", Family::XboxOne, 0),
    (MICROSOFT, 0x02e3, "Xbox Elite Controller", Family::XboxElite, 4),
    (MICROSOFT, 0x0b00, "Xbox Elite Series 2", Family::XboxElite, 4),
    (MICROSOFT, 0x0b05, "Xbox Elite Series 2 (Bluetooth)", Family::XboxElite, 4),
    (MICROSOFT, 0x0b22, "Xbox Elite Series 2 (Bluetooth LE)", Family::XboxElite, 4),
    (SONY, 0x0268, "DualShock 3", Family::Playstation3, 0),
    (SONY, 0x05c4, "DualShock 4", Family::Playstation4, 0),
    (SONY, 0x09cc, "DualShock 4", Family::Playstation4, 0),
    (SONY, 0x0ba0, "DualShock 4 Wireless Adapter", Family::Playstation4, 0),
    (SONY, 0x0ce6, "DualSense", Family::Playstation5, 0),
    (SONY, 0x0df2, "DualSense Edge", Family::Playstation5, 2),
    (NINTENDO, 0x2009, "Switch Pro Controller", Family::SwitchPro, 0),
    (NINTENDO, 0x2006, "Joy-Con (L)", Family::JoyconLeft, 0),
    (NINTENDO, 0x2007, "Joy-Con (R)", Family::JoyconRight, 0),
];

/// Model lookup: user entries first, then the built-in table.
pub(crate) fn model(user: &[Model], vendor: u16, product: u16) -> Option<Model> {
    user.iter().find(|m| m.vendor == vendor && m.product == product).cloned().or_else(|| {
        MODELS.iter().find(|m| m.0 == vendor && m.1 == product).map(|&(vendor, product, name, family, paddles)| {
            Model { vendor, product, name: name.into(), family: Some(family), paddles }
        })
    })
}

/// SDL_GamepadType → family. The model table refines Xbox One pads into Elite.
pub(crate) fn family_from_sdl(kind: sdl3::gamepad::GamepadType) -> Family {
    use sdl3::gamepad::GamepadType as T;
    match kind {
        T::Xbox360 => Family::Xbox360,
        T::XboxOne => Family::XboxOne,
        T::PS3 => Family::Playstation3,
        T::PS4 => Family::Playstation4,
        T::PS5 => Family::Playstation5,
        T::NintendoSwitchPro => Family::SwitchPro,
        T::NintendoSwitchJoyconLeft => Family::JoyconLeft,
        T::NintendoSwitchJoyconRight => Family::JoyconRight,
        T::NintendoSwitchJoyconPair => Family::JoyconPair,
        T::Standard => Family::Standard,
        T::Unknown => Family::Unknown,
    }
}

/// XINPUT_DEVSUBTYPE_* → family (XInput fallback without vendor/product).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn family_from_xinput_subtype(subtype: u8) -> Family {
    match subtype {
        0x01 => Family::XinputGamepad,
        0x02 => Family::Wheel,
        0x03 => Family::ArcadeStick,
        0x04 => Family::FlightStick,
        0x05 => Family::DancePad,
        0x06 | 0x07 | 0x0b => Family::Guitar,
        0x08 => Family::DrumKit,
        0x13 => Family::ArcadePad,
        _ => Family::Unknown,
    }
}

/// What an SDL gamepad reports when it is opened.
pub(crate) struct SdlReport {
    pub name: String,
    pub gamepad_type: sdl3::gamepad::GamepadType,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub path: String,
    pub paddles: u8,
    pub touchpad: bool,
    pub misc_button: bool,
}

pub(crate) fn from_sdl(report: SdlReport, user: &[Model]) -> ControllerKind {
    let mut family = family_from_sdl(report.gamepad_type);
    let model = report.vendor_id.zip(report.product_id).and_then(|(v, p)| model(user, v, p));
    // The model only refines the family SDL already reports (Xbox One →
    // Elite), unless SDL knows nothing about the pad.
    if let Some(model_family) = model.as_ref().and_then(|m| m.family) {
        if matches!(family, Family::Unknown | Family::Standard)
            || (family == Family::XboxOne && model_family == Family::XboxElite)
        {
            family = model_family;
        }
    }
    let name = if report.name.is_empty() {
        model.as_ref().map_or_else(|| family.name().to_string(), |m| m.name.clone())
    } else {
        report.name
    };
    ControllerKind {
        family,
        name,
        vendor_id: report.vendor_id,
        product_id: report.product_id,
        backend: Backend::Sdl,
        driver: report.path,
        xinput_subtype: None,
        wireless: None,
        paddles: report.paddles,
        hardware_paddles: model.map_or(0, |m| m.paddles),
        touchpad: report.touchpad,
        misc_button: report.misc_button,
        prompt_style: family.prompt_style(),
    }
}

/// XInput capability data: XInputGetCapabilities, plus vendor/product from
/// XInputGetCapabilitiesEx when xinput1_4 provides it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct XinputCaps {
    pub subtype: u8,
    pub flags: u16,
    pub vendor_product: Option<(u16, u16)>,
}

#[cfg_attr(not(windows), allow(dead_code))]
const XINPUT_CAPS_WIRELESS: u16 = 0x0002;

/// Identity of an XInput slot; built only by the Windows XInput transport.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn from_xinput(caps: XinputCaps, user: &[Model]) -> ControllerKind {
    let coarse = family_from_xinput_subtype(caps.subtype);
    let model = caps.vendor_product.and_then(|(v, p)| model(user, v, p));
    // A gamepad subtype takes the model's family; other subtypes (wheel,
    // guitar...) keep theirs: the model table lists gamepads only.
    let family = match (coarse, model.as_ref().and_then(|m| m.family)) {
        (Family::XinputGamepad, Some(family)) => family,
        _ => coarse,
    };
    ControllerKind {
        family,
        name: model.as_ref().map_or_else(|| family.name().to_string(), |m| m.name.clone()),
        vendor_id: caps.vendor_product.map(|v| v.0),
        product_id: caps.vendor_product.map(|v| v.1),
        backend: Backend::Xinput,
        driver: "XInput".into(),
        xinput_subtype: Some(caps.subtype),
        wireless: Some(caps.flags & XINPUT_CAPS_WIRELESS != 0),
        // XInput has no paddle inputs.
        paddles: 0,
        hardware_paddles: model.map_or(0, |m| m.paddles),
        touchpad: false,
        misc_button: false,
        prompt_style: family.prompt_style(),
    }
}

/// User model entries (settings/controller.json `models`), shared with the
/// device thread. Set once at startup before the backend starts.
static USER_MODELS: std::sync::OnceLock<Vec<Model>> = std::sync::OnceLock::new();

pub(crate) fn set_user_models(models: Vec<Model>) {
    let _ = USER_MODELS.set(models);
}

pub(crate) fn user_models() -> &'static [Model] {
    USER_MODELS.get().map_or(&[], Vec::as_slice)
}

/// `"045e"` / `"0x045e"` / `1118` → 0x045e, for settings files.
pub(crate) fn parse_id(value: &serde_json::Value) -> Option<u16> {
    match value {
        serde_json::Value::Number(n) => n.as_u64().and_then(|v| u16::try_from(v).ok()),
        serde_json::Value::String(s) => {
            let s = s.trim();
            u16::from_str_radix(s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s), 16).ok()
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "tests/controller_kind.rs"]
mod tests;
