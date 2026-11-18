//! Validation of untrusted device addresses and PINs before they reach BlueZ.

/// Canonical upper-case `AA:BB:CC:DD:EE:FF`, or `None` for anything else.
pub fn normalize_address(text: &str) -> Option<String> {
    let parts: Vec<&str> = text.trim().split(':').collect();
    if parts.len() != 6
        || parts
            .iter()
            .any(|part| part.len() != 2 || !part.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return None;
    }
    Some(text.trim().to_ascii_uppercase())
}

/// Legacy PINs are 1–16 characters and passkeys are 6 digits; accept printable ASCII only.
pub fn valid_pin(text: &str) -> bool {
    (1..=16).contains(&text.len()) && text.bytes().all(|b| (0x20..0x7f).contains(&b))
}
