//! Pure presentation of daemon state: wording, grouping and icons for the Bluetooth page.
use crate::{BtApproval, BtDevice};
use agx_bluetooth::{Category, Device, Prompt, PromptKind, Status};

pub fn glyph(category: Category) -> &'static str {
    match category {
        Category::Audio => "\u{f04c3}",
        Category::Headset => "\u{f02cb}",
        Category::Keyboard => "\u{f030c}",
        Category::Mouse => "\u{f037d}",
        Category::Gamepad => "\u{f02b4}",
        Category::Phone => "\u{f011c}",
        Category::Computer => "\u{f0322}",
        Category::Watch => "\u{f0589}",
        Category::Display => "\u{f0379}",
        Category::Printer => "\u{f042a}",
        Category::Sensor => "\u{f050f}",
        Category::Other => "\u{f00af}",
    }
}

/// Signal strength as 0 (unknown) to 4 bars.
pub fn bars(rssi: Option<i16>) -> i32 {
    match rssi {
        None => 0,
        Some(r) if r >= -55 => 4,
        Some(r) if r >= -67 => 3,
        Some(r) if r >= -78 => 2,
        Some(_) => 1,
    }
}

fn detail(device: &Device) -> String {
    if device.busy {
        return "Working…".into();
    }
    let mut parts = vec![device.category.label().to_owned()];
    parts.push(
        match (device.connected, device.paired) {
            (true, _) => "Connected",
            (false, true) => "Not connected",
            (false, false) => match bars(device.rssi) {
                4 => "Very close",
                3 => "Nearby",
                _ => "In range",
            },
        }
        .to_owned(),
    );
    if let Some(battery) = device.battery.filter(|_| device.connected) {
        parts.push(format!("{battery}%"));
    }
    if device.paired && device.trusted {
        parts.push("Auto-connect".into());
    }
    parts.join(" · ")
}

/// Rows in daemon order (connected, paired, nearby), with a section title on the first of each.
pub fn rows(devices: &[Device]) -> Vec<BtDevice> {
    let mut previous: Option<bool> = None;
    devices
        .iter()
        .map(|device| {
            let mine = device.paired || device.connected;
            let header = if previous != Some(mine) { if mine { "MY DEVICES" } else { "NEARBY DEVICES" } } else { "" };
            previous = Some(mine);
            BtDevice {
                address: device.address.as_str().into(),
                name: device.name.as_str().into(),
                detail: detail(device).into(),
                glyph: glyph(device.category).into(),
                header: header.into(),
                paired: device.paired,
                connected: device.connected,
                trusted: device.trusted,
                busy: device.busy,
                bars: bars(device.rssi),
            }
        })
        .collect()
}

pub fn approvals(status: &Status) -> Vec<BtApproval> {
    status
        .approvals
        .iter()
        .map(|approval| BtApproval {
            id: approval.id as i32,
            text: format!("{} {}.", approval.action.label(), approval.name).into(),
        })
        .collect()
}

/// (kind, sentence, code) for the page's prompt card; kind 0 means no prompt.
pub fn prompt(prompt: Option<&Prompt>) -> (i32, String, String) {
    let Some(prompt) = prompt else {
        return (0, String::new(), String::new());
    };
    let name = &prompt.name;
    let (kind, text) = match prompt.kind {
        PromptKind::Confirm => (1, format!("Does {name} show this code? Pair only if both screens match.")),
        PromptKind::Display => (2, format!("Type this code on {name}, then press Enter on it.")),
        PromptKind::Pin => (3, format!("{name} needs a PIN. Many devices use 0000 or 1234.")),
        PromptKind::Passkey => (4, format!("Enter the six-digit passkey for {name}.")),
        PromptKind::Authorize => (5, format!("{name} wants to connect to this device.")),
    };
    (kind, text, prompt.code.clone().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agx_bluetooth::{Approval, ApprovalAction};

    fn device(name: &str, paired: bool, connected: bool, rssi: Option<i16>) -> Device {
        Device { address: "AA:BB:CC:DD:EE:01".into(), name: name.into(), paired, connected, rssi, ..Device::default() }
    }

    #[test]
    fn signal_strength_maps_to_bars() {
        assert_eq!([None, Some(-40), Some(-60), Some(-70), Some(-85)].map(bars), [0, 4, 3, 2, 1]);
    }

    #[test]
    fn rows_group_my_devices_before_nearby_ones() {
        let rows = rows(&[
            device("Speaker", true, true, None),
            device("Keyboard", true, false, None),
            device("Phone", false, false, Some(-50)),
            device("Watch", false, false, Some(-80)),
        ]);
        let headers: Vec<_> = rows.iter().map(|r| r.header.as_str()).collect();
        assert_eq!(headers, ["MY DEVICES", "", "NEARBY DEVICES", ""]);
    }

    #[test]
    fn detail_says_what_matters_for_each_state() {
        let mut speaker = device("Speaker", true, true, None);
        speaker.category = Category::Audio;
        speaker.battery = Some(80);
        speaker.trusted = true;
        assert_eq!(detail(&speaker), "Speaker · Connected · 80% · Auto-connect");
        // A stale battery reading is not shown for a device that is not connected.
        speaker.connected = false;
        assert_eq!(detail(&speaker), "Speaker · Not connected · Auto-connect");
        assert_eq!(detail(&device("Phone", false, false, Some(-50))), "Device · Very close");
        let mut busy = device("Phone", false, false, Some(-50));
        busy.busy = true;
        assert_eq!(detail(&busy), "Working…");
    }

    #[test]
    fn prompts_and_approvals_read_as_plain_sentences() {
        assert_eq!(prompt(None), (0, String::new(), String::new()));
        let confirm = Prompt { id: 1, kind: PromptKind::Confirm, address: "A".into(), name: "Keyboard".into(), code: Some("123456".into()) };
        let (kind, text, code) = prompt(Some(&confirm));
        assert_eq!((kind, code.as_str()), (1, "123456"));
        assert!(text.contains("Keyboard") && text.contains("match"));
        let status = Status {
            approvals: vec![Approval { id: 7, action: ApprovalAction::Forget, address: "A".into(), name: "Speaker".into() }],
            ..Status::default()
        };
        let approvals = approvals(&status);
        assert_eq!((approvals[0].id, approvals[0].text.as_str()), (7, "remove Speaker."));
    }
}
