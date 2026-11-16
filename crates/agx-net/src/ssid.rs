//! Byte-preserving SSID encoding and scan result parsing.
use crate::{AccessPoint, Security};

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
