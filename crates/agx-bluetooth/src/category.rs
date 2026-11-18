//! A short device category for icons and sorting, derived from what BlueZ reports.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Audio,
    Headset,
    Keyboard,
    Mouse,
    Gamepad,
    Phone,
    Computer,
    Watch,
    Display,
    Printer,
    Sensor,
    #[default]
    Other,
}

impl Category {
    /// From BlueZ's `Icon` property (freedesktop icon names such as `audio-headset`).
    pub fn from_icon(icon: &str) -> Option<Self> {
        Some(match icon {
            "audio-headset" | "audio-headphones" => Self::Headset,
            "audio-card" | "audio-speakers" | "audio-input-microphone" => Self::Audio,
            "input-keyboard" => Self::Keyboard,
            "input-mouse" | "input-tablet" => Self::Mouse,
            "input-gaming" => Self::Gamepad,
            "phone" | "multimedia-player" => Self::Phone,
            "computer" => Self::Computer,
            "video-display" => Self::Display,
            "printer" | "scanner" => Self::Printer,
            _ => return None,
        })
    }

    /// From the major/minor fields of a Bluetooth class of device (major = bits 8-12, minor = bits 2-7).
    pub fn from_class(class: u32) -> Option<Self> {
        let minor = (class >> 2) & 0x3f;
        Some(match (class >> 8) & 0x1f {
            1 => Self::Computer,
            2 => Self::Phone,
            4 => match minor {
                1 | 2 | 6 => Self::Headset,
                _ => Self::Audio,
            },
            // Peripheral: CoD bit 6 flags a keyboard and bit 7 a pointing device, so a combo
            // keyboard/trackpad reads as a keyboard. Bits 2-5 name joysticks and gamepads.
            5 if minor & 0x10 != 0 => Self::Keyboard,
            5 if minor & 0x20 != 0 => Self::Mouse,
            5 if matches!(minor & 0xf, 1 | 2) => Self::Gamepad,
            5 => Self::Other,
            // Imaging: CoD bit 4 is a display, bits 6 and 7 a scanner or printer.
            6 if minor & 0x04 != 0 => Self::Display,
            6 if minor & 0x30 != 0 => Self::Printer,
            7 => Self::Watch,
            _ => return None,
        })
    }

    /// From a GATT appearance value (category = value >> 6, subcategory = low six bits).
    pub fn from_appearance(appearance: u16) -> Option<Self> {
        Some(match (appearance >> 6, appearance & 0x3f) {
            (1, _) => Self::Phone,
            (2, _) => Self::Computer,
            (3, _) => Self::Watch,
            (5, _) => Self::Display,
            (12..=14 | 16..=18, _) => Self::Sensor,
            (15, 1) => Self::Keyboard,
            (15, 2) => Self::Mouse,
            (15, 3 | 4) => Self::Gamepad,
            _ => return None,
        })
    }

    /// Short human label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Audio => "Speaker",
            Self::Headset => "Headphones",
            Self::Keyboard => "Keyboard",
            Self::Mouse => "Mouse",
            Self::Gamepad => "Controller",
            Self::Phone => "Phone",
            Self::Computer => "Computer",
            Self::Watch => "Watch",
            Self::Display => "Display",
            Self::Printer => "Printer",
            Self::Sensor => "Sensor",
            Self::Other => "Device",
        }
    }
}
