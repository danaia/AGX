//! Private, bounded WPE host protocol. All integers are big endian.
use crate::keymap::Mods;
use std::io::{self, Read, Write};
pub const WIDTH: u32 = 4096;
pub const HEIGHT: u32 = 2160;
pub const FRAME_BYTES: u64 = WIDTH as u64 * HEIGHT as u64 * 4;
pub const HOME: &str = "file:///usr/share/agx/web/start.html";

pub fn write(mut writer: impl Write, kind: u8, data: &[u8]) -> io::Result<()> {
    if data.len() >= 65536 {
        return Err(io::Error::other("oversized browser command"));
    }
    writer.write_all(&((data.len() + 1) as u32).to_be_bytes())?;
    writer.write_all(&[kind])?;
    writer.write_all(data)
}
pub fn read(mut reader: impl Read) -> io::Result<(u8, Vec<u8>)> {
    let mut size = [0; 4];
    reader.read_exact(&mut size)?;
    let size = u32::from_be_bytes(size) as usize;
    if !(1..=65536).contains(&size) {
        return Err(io::Error::other("invalid browser message"));
    }
    let mut data = vec![0; size];
    reader.read_exact(&mut data)?;
    Ok((data[0], data[1..].to_vec()))
}
pub fn modifiers(m: Mods) -> u32 {
    u32::from(m.ctrl)
        | (u32::from(m.shift) << 1)
        | (u32::from(m.alt) << 2)
        | (u32::from(m.meta) << 3)
}
pub fn address(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty()
        || text.len() > 8192
        || text.chars().any(char::is_control)
        || text.contains(' ')
    {
        return None;
    }
    if text == HOME {
        return Some(text.into());
    }
    if text.starts_with("https://") || text.starts_with("http://") {
        return (!text.split_once("://")?.1.is_empty()).then(|| text.into());
    }
    if text.contains("://")
        || text.starts_with("javascript:")
        || text.starts_with("data:")
        || text.starts_with("file:")
    {
        return None;
    }
    Some(format!("https://{text}"))
}
pub fn keysym(text: &str) -> Option<u32> {
    // Reuse the RFB/Slint mapping so navigation, editing and modifiers agree.
    for key in [
        0xff08, 0xff09, 0xfe20, 0xff0d, 0xff1b, 0xffff, 0xff63, 0xff50, 0xff57, 0xff55, 0xff56,
        0xff51, 0xff52, 0xff53, 0xff54, 0xffe1, 0xffe2, 0xffe3, 0xffe4, 0xffe5, 0xffeb, 0xffec,
        0xffe9, 0xffea,
    ] {
        if crate::remote::input::key_text(key).is_some_and(|s| s == text) {
            return Some(key);
        }
    }
    for key in 0xffbe..=0xffc9 {
        if crate::remote::input::key_text(key).is_some_and(|s| s == text) {
            return Some(key);
        }
    }
    let mut chars = text.chars();
    let c = chars.next()?;
    if chars.next().is_some() || c.is_control() {
        return None;
    }
    let n = c as u32;
    Some(if n <= 0xff { n } else { 0x0100_0000 | n })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn messages_roundtrip_and_reject_unbounded_lengths() {
        let mut bytes = vec![];
        write(&mut bytes, 2, b"https://example.org").unwrap();
        assert_eq!(
            read(bytes.as_slice()).unwrap(),
            (2, b"https://example.org".to_vec())
        );
        assert!(read(&[0, 1, 0, 1][..]).is_err());
        assert!(read(&[0, 0, 0, 0][..]).is_err());
    }
    #[test]
    fn addresses_and_editing_keys() {
        assert_eq!(
            address(" example.org ").as_deref(),
            Some("https://example.org")
        );
        assert_eq!(address(HOME).as_deref(), Some(HOME));
        for value in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,x",
            "",
            "https://",
            "a\nb",
        ] {
            assert!(address(value).is_none(), "{value}");
        }
        assert_eq!(
            keysym(&slint::SharedString::from(slint::platform::Key::Backspace)),
            Some(0xff08)
        );
        assert_eq!(keysym("é"), Some(0xe9));
        assert_eq!(keysym("€"), Some(0x0100_20ac));
    }
}

#[derive(Debug, PartialEq)]
pub struct OptionItem {
    pub label: String,
    pub enabled: bool,
    pub selected: bool,
}
pub fn options(bytes: &[u8]) -> io::Result<Vec<OptionItem>> {
    if bytes.len() < 2 {
        return Err(io::Error::other("invalid web selection"));
    }
    let count = u16::from_be_bytes(bytes[..2].try_into().unwrap()) as usize;
    if count == 0 || count > 256 {
        return Err(io::Error::other("invalid web selection count"));
    }
    let mut rest = &bytes[2..];
    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        if rest.len() < 3 {
            return Err(io::Error::other("truncated web selection"));
        }
        let flags = rest[0];
        let size = u16::from_be_bytes(rest[1..3].try_into().unwrap()) as usize;
        rest = &rest[3..];
        if flags & !3 != 0 || size > 200 || rest.len() < size {
            return Err(io::Error::other("invalid web selection item"));
        }
        let label = std::str::from_utf8(&rest[..size])
            .map_err(io::Error::other)?
            .to_owned();
        rest = &rest[size..];
        items.push(OptionItem {
            label,
            enabled: flags & 1 != 0,
            selected: flags & 2 != 0,
        });
    }
    if !rest.is_empty() {
        return Err(io::Error::other("trailing web selection data"));
    }
    Ok(items)
}
pub fn dialog(bytes: &[u8]) -> io::Result<(u8, String, String)> {
    if bytes.len() < 3 || bytes[0] > 3 {
        return Err(io::Error::other("invalid website dialog"));
    }
    let size = u16::from_be_bytes(bytes[1..3].try_into().unwrap()) as usize;
    if size > 8192 || bytes.len() < size + 3 || bytes.len() - size - 3 > 8192 {
        return Err(io::Error::other("invalid website dialog text"));
    }
    Ok((
        bytes[0],
        std::str::from_utf8(&bytes[3..3 + size])
            .map_err(io::Error::other)?
            .into(),
        std::str::from_utf8(&bytes[3 + size..])
            .map_err(io::Error::other)?
            .into(),
    ))
}

#[cfg(test)]
mod popup_tests {
    use super::*;
    #[test]
    fn option_lists_preserve_selection_and_reject_malformed_items() {
        let items = options(&[0, 2, 3, 0, 3, b'O', b'n', b'e', 0, 0, 3, b'T', b'w', b'o']).unwrap();
        assert_eq!(
            items,
            vec![
                OptionItem {
                    label: "One".into(),
                    enabled: true,
                    selected: true
                },
                OptionItem {
                    label: "Two".into(),
                    enabled: false,
                    selected: false
                }
            ]
        );
        for bytes in [
            &[0, 0][..],
            &[1, 1][..],
            &[0, 1, 1, 0, 3, b'x'][..],
            &[0, 1, 1, 0, 1, 255][..],
        ] {
            assert!(options(bytes).is_err());
        }
    }
    #[test]
    fn dialogs_validate_kind_lengths_and_unicode() {
        assert_eq!(
            dialog(&[2, 0, 2, b'H', b'i', b'O', b'K']).unwrap(),
            (2, "Hi".into(), "OK".into())
        );
        for bytes in [&[4, 0, 0][..], &[0, 0, 4, b'x'][..], &[0, 0, 1, 255][..]] {
            assert!(dialog(bytes).is_err());
        }
    }
}
