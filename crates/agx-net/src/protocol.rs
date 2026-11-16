//! Wi-Fi wire protocol and state models. Requests deliberately omit Debug.
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum Security {
    Open,
    Wpa2,
    Wpa3,
    Owe,
    Unsupported,
}
impl Security {
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Wpa2 => "WPA/WPA2",
            Self::Wpa3 => "WPA3",
            Self::Owe => "Enhanced open",
            Self::Unsupported => "Enterprise/WEP",
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AccessPoint {
    pub ssid: String,
    pub ssid_hex: String,
    pub bssid: String,
    pub frequency: u32,
    pub signal_dbm: i32,
    pub security: Security,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SavedNetwork {
    pub id: i32,
    pub ssid: String,
    pub ssid_hex: String,
    pub current: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Status {
    pub country: String,
    pub interface_present: bool,
    pub phase: String,
    pub ssid: Option<String>,
    pub ipv4: Vec<String>,
    pub gateway: Option<String>,
    pub dns: Vec<String>,
    pub signal_dbm: Option<i32>,
    pub access_points: Vec<AccessPoint>,
    pub saved: Vec<SavedNetwork>,
    pub scanning: bool,
    pub busy: bool,
    pub message: String,
    pub error: Option<String>,
    pub diagnostics: String,
    pub time_synced: bool,
}
impl Default for Status {
    fn default() -> Self {
        Self {
            country: "US".into(),
            interface_present: false,
            phase: "Starting".into(),
            ssid: None,
            ipv4: Vec::new(),
            gateway: None,
            dns: Vec::new(),
            signal_dbm: None,
            access_points: Vec::new(),
            saved: Vec::new(),
            scanning: false,
            busy: false,
            message: "Starting Wi-Fi service".into(),
            error: None,
            diagnostics: "Not checked".into(),
            time_synced: false,
        }
    }
}
// Deliberately no Debug implementation: requests can contain a password.
#[derive(Deserialize, Serialize)]
pub enum Request {
    ProfileStatus,
    SetGitHubToken {
        token: Zeroizing<String>,
    },
    CheckGitHubRelease,
    SaveProfile(agx_profile::Save),
    Status,
    Scan,
    Country {
        country: String,
    },
    Connect {
        ssid_hex: String,
        security: Security,
        password: Zeroizing<String>,
        hidden: bool,
    },
    SelectSaved {
        id: i32,
    },
    Forget {
        id: i32,
    },
    Disconnect,
    Check,
    Reboot {
        confirmed: bool,
    },
    Shutdown {
        confirmed: bool,
    },
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Reply {
    pub ok: bool,
    pub message: String,
    pub status: Status,
    #[serde(default)]
    pub profile: Option<agx_profile::Status>,
}
