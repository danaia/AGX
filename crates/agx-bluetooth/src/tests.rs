use super::*;

#[test]
fn addresses_are_validated_and_normalized() {
    assert_eq!(normalize_address(" aa:bb:0c:0d:0e:0F "), Some("AA:BB:0C:0D:0E:0F".into()));
    for bad in ["", "AA:BB:CC:DD:EE", "AA:BB:CC:DD:EE:FF:00", "AA-BB-CC-DD-EE-FF", "GG:BB:CC:DD:EE:FF", "A:BB:CC:DD:EE:FF", "AA:BB:CC:DD:EE:F\n"] {
        assert_eq!(normalize_address(bad), None, "{bad:?}");
    }
}

#[test]
fn pins_are_short_printable_ascii() {
    assert!(valid_pin("0000"));
    assert!(valid_pin("123456"));
    for bad in ["", "12345678901234567", "12\n34", "pin\u{e9}"] {
        assert!(!valid_pin(bad), "{bad:?}");
    }
}

#[test]
fn categories_come_from_icon_class_and_appearance() {
    assert_eq!(Category::from_icon("audio-headset"), Some(Category::Headset));
    assert_eq!(Category::from_icon("input-keyboard"), Some(Category::Keyboard));
    assert_eq!(Category::from_icon("something-new"), None);
    // 0x000540 keyboard, 0x000580 pointing device, 0x240404 wearable headset, 0x5a020c phone.
    assert_eq!(Category::from_class(0x0540), Some(Category::Keyboard));
    assert_eq!(Category::from_class(0x0580), Some(Category::Mouse));
    assert_eq!(Category::from_class(0x24_0404), Some(Category::Headset));
    assert_eq!(Category::from_class(0x24_0414), Some(Category::Audio));
    assert_eq!(Category::from_class(0x5a_020c), Some(Category::Phone));
    assert_eq!(Category::from_class(0), None);
    // HID appearance 0x03C1 keyboard, 0x03C2 mouse; 0x0340 watch.
    assert_eq!(Category::from_appearance(0x03c1), Some(Category::Keyboard));
    assert_eq!(Category::from_appearance(0x03c2), Some(Category::Mouse));
    assert_eq!(Category::from_appearance(0x00c0), Some(Category::Watch));
    assert_eq!(Category::from_appearance(0), None);
}

#[test]
fn requests_use_a_tagged_json_form_and_reject_unknown_commands() {
    let json = serde_json::to_string(&Request::Trust { address: "AA:BB:CC:DD:EE:FF".into(), trusted: true }).unwrap();
    assert_eq!(json, r#"{"cmd":"trust","address":"AA:BB:CC:DD:EE:FF","trusted":true}"#);
    assert!(matches!(
        serde_json::from_str::<Request>(r#"{"cmd":"scan","on":true}"#),
        Ok(Request::Scan { on: true })
    ));
    assert!(serde_json::from_str::<Request>(r#"{"cmd":"format-disk"}"#).is_err());
    assert!(serde_json::from_str::<Request>(r#"{"cmd":"pair"}"#).is_err());
}
