use super::*;
use sdl3::gamepad::GamepadType as T;

fn sdl(gamepad_type: T, vendor_product: Option<(u16, u16)>, paddles: u8) -> ControllerKind {
    from_sdl(
        SdlReport {
            name: "Test pad".into(),
            gamepad_type,
            vendor_id: vendor_product.map(|v| v.0),
            product_id: vendor_product.map(|v| v.1),
            path: "XInput#0".into(),
            paddles,
            touchpad: false,
            misc_button: false,
        },
        &[],
    )
}

#[test]
fn every_sdl_gamepad_type_has_a_family_and_prompt_style() {
    let cases = [
        (T::Unknown, Family::Unknown, PromptStyle::Xbox),
        (T::Standard, Family::Standard, PromptStyle::Xbox),
        (T::Xbox360, Family::Xbox360, PromptStyle::Xbox),
        (T::XboxOne, Family::XboxOne, PromptStyle::Xbox),
        (T::PS3, Family::Playstation3, PromptStyle::Playstation),
        (T::PS4, Family::Playstation4, PromptStyle::Playstation),
        (T::PS5, Family::Playstation5, PromptStyle::Playstation),
        (T::NintendoSwitchPro, Family::SwitchPro, PromptStyle::Nintendo),
        (T::NintendoSwitchJoyconLeft, Family::JoyconLeft, PromptStyle::Nintendo),
        (T::NintendoSwitchJoyconRight, Family::JoyconRight, PromptStyle::Nintendo),
        (T::NintendoSwitchJoyconPair, Family::JoyconPair, PromptStyle::Nintendo),
    ];
    for (sdl_type, family, style) in cases {
        let kind = sdl(sdl_type, None, 0);
        assert_eq!((kind.family, kind.prompt_style), (family, style), "{sdl_type:?}");
        assert_eq!(kind.backend, Backend::Sdl);
    }
}

#[test]
fn nintendo_prompts_name_buttons_by_print_but_layout_stays_positional() {
    // South/East/West/North: the A slot (South) prints "B" on a Nintendo pad.
    assert_eq!(PromptStyle::Nintendo.face_labels(), ["B", "A", "Y", "X"]);
    assert_eq!(PromptStyle::Xbox.face_labels(), ["A", "B", "X", "Y"]);
    assert_eq!(PromptStyle::Playstation.face_labels()[0], "Cross");
}

#[test]
fn vendor_product_refines_xbox_one_into_elite_with_hardware_paddles() {
    // The user's pad: Elite Series 2 over Bluetooth LE, SDL's XInput driver
    // (no paddles reported).
    let kind = sdl(T::XboxOne, Some((0x045e, 0x0b22)), 0);
    assert_eq!(kind.family, Family::XboxElite);
    assert_eq!((kind.paddles, kind.hardware_paddles), (0, 4));
    assert_eq!(kind.name, "Test pad");
    assert!(kind.summary().contains("045e:0b22") && kind.summary().contains("paddles 0 of 4"));
    // Paddles through HIDAPI.
    assert_eq!(sdl(T::XboxOne, Some((0x045e, 0x0b00)), 4).paddles, 4);
    // A plain Series pad stays Xbox One; an unknown pad keeps SDL's type.
    assert_eq!(sdl(T::XboxOne, Some((0x045e, 0x0b12)), 0).family, Family::XboxOne);
    assert_eq!(sdl(T::PS4, Some((0x1234, 0x5678)), 0).family, Family::Playstation4);
    // The table never overrides a specific SDL type with another family.
    assert_eq!(sdl(T::Xbox360, Some((0x054c, 0x0ce6)), 0).family, Family::Xbox360);
    // ...but names a pad SDL only knows as standard.
    assert_eq!(sdl(T::Standard, Some((0x054c, 0x0ce6)), 0).family, Family::Playstation5);
}

#[test]
fn known_vendor_product_ids() {
    for (vendor, product, family) in [
        (0x045e, 0x028e, Family::Xbox360),
        (0x045e, 0x02ea, Family::XboxOne),
        (0x045e, 0x0b13, Family::XboxOne),
        (0x045e, 0x02e3, Family::XboxElite),
        (0x045e, 0x0b05, Family::XboxElite),
        (0x054c, 0x0268, Family::Playstation3),
        (0x054c, 0x09cc, Family::Playstation4),
        (0x054c, 0x0df2, Family::Playstation5),
        (0x057e, 0x2009, Family::SwitchPro),
        (0x057e, 0x2006, Family::JoyconLeft),
        (0x057e, 0x2007, Family::JoyconRight),
    ] {
        assert_eq!(model(&[], vendor, product).and_then(|m| m.family), Some(family), "{vendor:04x}:{product:04x}");
    }
    assert!(model(&[], 0x045e, 0xffff).is_none());
    let user = [Model { vendor: 0x2dc8, product: 0x3106, name: "8BitDo Ultimate".into(), family: Some(Family::XboxOne), paddles: 2 }];
    assert_eq!(model(&user, 0x2dc8, 0x3106).map(|m| m.paddles), Some(2));
    // User entries win over the built-in table.
    let rename = [Model { vendor: 0x045e, product: 0x0b22, name: "My Elite".into(), family: None, paddles: 4 }];
    assert_eq!(model(&rename, 0x045e, 0x0b22).map(|m| m.name), Some("My Elite".into()));
}

#[test]
fn xinput_subtypes_map_to_coarse_kinds() {
    for (subtype, family) in [
        (0x00, Family::Unknown),
        (0x01, Family::XinputGamepad),
        (0x02, Family::Wheel),
        (0x03, Family::ArcadeStick),
        (0x04, Family::FlightStick),
        (0x05, Family::DancePad),
        (0x06, Family::Guitar),
        (0x07, Family::Guitar),
        (0x08, Family::DrumKit),
        (0x0b, Family::Guitar),
        (0x13, Family::ArcadePad),
        (0x42, Family::Unknown),
    ] {
        assert_eq!(family_from_xinput_subtype(subtype), family, "{subtype:#x}");
        let kind = from_xinput(XinputCaps { subtype, flags: 0, vendor_product: None }, &[]);
        assert_eq!((kind.family, kind.backend, kind.xinput_subtype), (family, Backend::Xinput, Some(subtype)));
        assert_eq!(kind.name, family.name());
    }
}

#[test]
fn xinput_fallback_reports_identity_from_capabilities_ex() {
    let kind = from_xinput(XinputCaps { subtype: 1, flags: 0x0002, vendor_product: Some((0x045e, 0x0b22)) }, &[]);
    assert_eq!(kind.family, Family::XboxElite);
    assert_eq!(kind.name, "Xbox Elite Series 2 (Bluetooth LE)");
    assert_eq!((kind.vendor_id, kind.product_id), (Some(0x045e), Some(0x0b22)));
    assert_eq!((kind.wireless, kind.paddles, kind.hardware_paddles), (Some(true), 0, 4));
    assert_eq!(kind.driver, "XInput");
    // A non-gamepad subtype keeps its own kind even for a known pad ID.
    let wheel = from_xinput(XinputCaps { subtype: 2, flags: 0, vendor_product: Some((0x045e, 0x028e)) }, &[]);
    assert_eq!(wheel.family, Family::Wheel);
}

#[test]
fn json_names_are_stable_for_mods() {
    let kind = sdl(T::PS5, Some((0x054c, 0x0ce6)), 0);
    let json = serde_json::to_value(&kind).unwrap();
    assert_eq!(json["family"], "playstation5");
    assert_eq!(json["backend"], "sdl");
    assert_eq!(json["prompt_style"], "playstation");
    assert_eq!(json["vendor_id"], 0x054c);
    assert_eq!(json["driver"], "XInput#0");
    assert_eq!(serde_json::to_value(Family::XboxElite).unwrap(), "xbox_elite");
    assert_eq!(serde_json::from_value::<Family>("switch_pro".into()).unwrap(), Family::SwitchPro);
}

#[test]
fn settings_ids_accept_hex_strings_and_numbers() {
    assert_eq!(parse_id(&"045e".into()), Some(0x045e));
    assert_eq!(parse_id(&"0x0B22".into()), Some(0x0b22));
    assert_eq!(parse_id(&serde_json::json!(1118)), Some(0x045e));
    assert_eq!(parse_id(&serde_json::json!(70000)), None);
    assert_eq!(parse_id(&"xyz".into()), None);
}

#[test]
fn gamepad_api_ids_give_name_and_usb_ids_in_chrome_and_firefox_forms() {
    assert_eq!(
        parse_gamepad_id("Xbox 360 Controller (STANDARD GAMEPAD Vendor: 045e Product: 028e)"),
        ("Xbox 360 Controller".into(), Some((0x045e, 0x028e)))
    );
    assert_eq!(parse_gamepad_id("Xbox 360 Controller (XInput STANDARD GAMEPAD)"), ("Xbox 360 Controller".into(), None));
    assert_eq!(parse_gamepad_id("054c-0ce6-DualSense Wireless Controller"), ("DualSense Wireless Controller".into(), Some((0x054c, 0x0ce6))));
    assert_eq!(parse_gamepad_id("Some Pad (Bluetooth)"), ("Some Pad (Bluetooth)".into(), None));
    assert_eq!(parse_gamepad_id(""), (String::new(), None));
}

#[test]
fn gamepad_api_pads_take_the_model_family() {
    let kind = from_gamepad_api("DualSense Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 0ce6)", &[]);
    assert_eq!((kind.family, kind.backend, kind.vendor_id), (Family::Playstation5, Backend::GamepadApi, Some(0x054c)));
    assert_eq!(kind.name, "DualSense Wireless Controller");
    assert_eq!(kind.prompt_style, PromptStyle::Playstation);
    let unknown = from_gamepad_api("Generic USB Joypad", &[]);
    assert_eq!((unknown.family, unknown.vendor_id), (Family::Standard, None));
    let elite = from_gamepad_api("045e-0b00-", &[]);
    assert_eq!((elite.family, elite.name.as_str(), elite.hardware_paddles), (Family::XboxElite, "Xbox Elite Series 2", 4));
    assert_eq!(serde_json::to_value(&kind).unwrap()["backend"], "gamepad_api");
}

/// Reads slot 0 through XInput (and XInputGetCapabilitiesEx) on this machine.
/// Needs a connected pad; prints what the fallback reports.
#[cfg(windows)]
#[test]
#[ignore]
fn xinput_identity_of_connected_pad() {
    let kinds: Vec<_> = (0..4).map(crate::input::platform::xinput_identity).collect();
    for (slot, kind) in kinds.iter().enumerate() {
        println!("XInput slot {slot}: {:?}", kind.as_ref().map(ControllerKind::summary));
    }
    assert!(kinds.iter().any(Option::is_some), "no XInput pad connected");
}
