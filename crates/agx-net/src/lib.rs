//! Shared Wi-Fi protocol. Passwords travel only over a root-only Unix socket.
use serde::{Deserialize, Serialize};
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    time::Duration,
};
use zeroize::Zeroizing;

pub mod service;
pub const SOCKET: &str = "/run/agx/network.sock";
pub const MAX_REQUEST: usize = 16384;

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
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Reply {
    pub ok: bool,
    pub message: String,
    pub status: Status,
    #[serde(default)]
    pub profile: Option<agx_profile::Status>,
}

pub fn request(command: &Request) -> io::Result<Reply> {
    let mut connection = UnixStream::connect(SOCKET).map_err(|e| {
        io::Error::new(
            e.kind(),
            "Wi-Fi service unavailable. Check that agx-netd is installed and running as root.",
        )
    })?;
    connection.set_read_timeout(Some(Duration::from_secs(8)))?;
    connection.set_write_timeout(Some(Duration::from_secs(3)))?;
    let data = Zeroizing::new(serde_json::to_vec(command)?);
    connection.write_all(&data)?;
    connection.write_all(b"\n")?;
    let mut response = Vec::new();
    BufReader::new(connection)
        .take(1024 * 1024)
        .read_until(b'\n', &mut response)?;
    serde_json::from_slice(&response)
        .map_err(|_| io::Error::other("Invalid Wi-Fi service response"))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
pub fn unhex(value: &str) -> Result<Vec<u8>, String> {
    if value.is_empty() || value.len() > 64 || !value.len().is_multiple_of(2) {
        return Err("SSID must contain 1–32 bytes".into());
    }
    value
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            let digit = |b: u8| {
                (b as char)
                    .to_digit(16)
                    .map(|v| v as u8)
                    .ok_or_else(|| "Invalid SSID encoding".to_owned())
            };
            Ok(digit(pair[0])? * 16 + digit(pair[1])?)
        })
        .collect()
}
pub fn decode_ssid(value: &str) -> Vec<u8> {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            if bytes[i + 1] == b'x'
                && i + 3 < bytes.len()
                && let Some(decoded) = std::str::from_utf8(&bytes[i + 2..i + 4])
                    .ok()
                    .and_then(|s| unhex(s).ok())
            {
                out.extend(decoded);
                i += 4;
                continue;
            }
            out.push(match bytes[i + 1] {
                b'n' => b'\n',
                b'r' => b'\r',
                b't' => b'\t',
                b'e' => 27,
                other => other,
            });
            i += 2;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}
pub fn display_ssid(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .map(|c| if c.is_control() { '�' } else { c })
        .collect()
}
pub fn parse_scan(text: &str) -> Vec<AccessPoint> {
    let mut points = Vec::new();
    for line in text.lines().skip(1) {
        let fields: Vec<_> = line.splitn(5, '\t').collect();
        if fields.len() != 5 {
            continue;
        }
        let ssid = decode_ssid(fields[4]);
        if ssid.is_empty() || ssid.len() > 32 {
            continue;
        }
        let flags = fields[3];
        let security = if flags.contains("EAP") || flags.contains("WEP") {
            Security::Unsupported
        } else if flags.contains("PSK") {
            Security::Wpa2
        } else if flags.contains("SAE") {
            Security::Wpa3
        } else if flags.contains("OWE") {
            Security::Owe
        } else if flags.contains("WPA") || flags.contains("RSN") {
            Security::Unsupported
        } else {
            Security::Open
        };
        if let (Ok(frequency), Ok(signal_dbm)) = (fields[1].parse(), fields[2].parse()) {
            points.push(AccessPoint {
                ssid: display_ssid(&ssid),
                ssid_hex: hex(&ssid),
                bssid: fields[0].into(),
                frequency,
                signal_dbm,
                security,
            });
        }
    }
    points.sort_by_key(|point| std::cmp::Reverse(point.signal_dbm));
    let mut seen = std::collections::HashSet::new();
    points.retain(|p| seen.insert((p.ssid_hex.clone(), p.security as u8)));
    points
}
pub fn validate_credentials(
    ssid_hex: &str,
    security: Security,
    password: &str,
) -> Result<(), String> {
    unhex(ssid_hex)?;
    match security {
        Security::Unsupported => Err(
            "Enterprise and WEP networks are not supported by this personal-network setup.".into(),
        ),
        Security::Wpa2
            if !(password.len() == 64 && password.bytes().all(|b| b.is_ascii_hexdigit()))
                && !(password.len() >= 8
                    && password.len() <= 63
                    && password.bytes().all(|b| (32..=126).contains(&b))) =>
        {
            Err(
                "WPA/WPA2 needs 8–63 printable ASCII characters, or a 64-digit hexadecimal PSK."
                    .into(),
            )
        }
        Security::Wpa3
            if password.is_empty()
                || password.len() > 63
                || password.chars().any(char::is_control) =>
        {
            Err("WPA3 needs a password of 1–63 bytes without control characters.".into())
        }
        _ => Ok(()),
    }
}
pub fn psk(password: &str, ssid: &[u8]) -> Zeroizing<String> {
    if password.len() == 64 && password.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Zeroizing::new(password.into());
    }
    let mut output = Zeroizing::new([0u8; 32]);
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password.as_bytes(), ssid, 4096, &mut *output);
    Zeroizing::new(hex(&*output))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scan_parsing_preserves_bytes_and_selects_strongest() {
        let data = "bssid / frequency / signal level / flags / ssid\naa\t2412\t-70\t[WPA2-PSK-CCMP][ESS]\tCoffee\\x20shop\nbb\t5180\t-40\t[WPA2-PSK-CCMP][ESS]\tCoffee shop\ncc\t2412\t-30\t[RSN-SAE-CCMP][ESS]\tSecure\ndd\t2412\t-25\t[WPA2-EAP-CCMP]\tOffice\nee\t2412\t-20\t[ESS]\t\n";
        let scan = parse_scan(data);
        assert_eq!(scan.len(), 3);
        assert_eq!(scan[2].ssid, "Coffee shop");
        assert_eq!(scan[2].bssid, "bb");
        assert_eq!(scan[0].security, Security::Unsupported);
        assert_eq!(unhex(&hex(b"a\x00\xff")).unwrap(), b"a\x00\xff");
    }
    #[test]
    fn credentials_validate_without_echoing_passwords() {
        let ssid = hex(b"IEEE");
        assert!(validate_credentials(&ssid, Security::Wpa2, "short").is_err());
        assert!(validate_credentials(&ssid, Security::Wpa2, "password").is_ok());
        assert_eq!(
            &*psk("password", b"IEEE"),
            "f42c6fc52df0ebef9ebb4b90b38a5f902e83fe1b135a70e23aed762e9710a12e"
        );
        assert!(validate_credentials("00zz", Security::Open, "").is_err());
        assert!(validate_credentials(&ssid, Security::Wpa3, "password\n").is_err());
    }
}
