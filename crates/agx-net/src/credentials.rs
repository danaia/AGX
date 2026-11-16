//! Credential validation and WPA PSK derivation; secrets stay zeroized.
use crate::{Security, hex, unhex};
use zeroize::Zeroizing;

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
