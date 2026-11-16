//! SSID parsing and secret-handling regressions.

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
