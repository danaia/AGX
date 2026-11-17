//! Xterm-compatible key encoding, without consuming ANGI's Super bindings.
use crate::keymap::Mods;
use alacritty_terminal::term::TermMode;
pub(super) fn encode(text: &str, mods: Mods, mode: TermMode) -> Option<Vec<u8>> {
    use slint::platform::Key;
    let is = |key: Key| text.chars().eq(std::iter::once(char::from(key)));
    // Slint encodes modifier keys as C0 characters (Shift is U+0010).
    // Sending them to a PTY would invoke Readline commands such as Ctrl+P.
    if [
        Key::Shift,
        Key::ShiftR,
        Key::Control,
        Key::ControlR,
        Key::Alt,
        Key::AltGr,
        Key::Meta,
        Key::MetaR,
        Key::CapsLock,
    ]
    .into_iter()
    .any(is)
    {
        return None;
    }
    let modifier = 1 + u8::from(mods.shift) + 2 * u8::from(mods.alt) + 4 * u8::from(mods.ctrl);
    let arrow = if is(Key::UpArrow) {
        Some('A')
    } else if is(Key::DownArrow) {
        Some('B')
    } else if is(Key::RightArrow) {
        Some('C')
    } else if is(Key::LeftArrow) {
        Some('D')
    } else if is(Key::Home) {
        Some('H')
    } else if is(Key::End) {
        Some('F')
    } else {
        None
    };
    if let Some(final_byte) = arrow {
        return Some(
            if modifier != 1 {
                format!("\x1b[1;{modifier}{final_byte}")
            } else if mode.contains(TermMode::APP_CURSOR) {
                format!("\x1bO{final_byte}")
            } else {
                format!("\x1b[{final_byte}")
            }
            .into_bytes(),
        );
    }
    let tilde = if is(Key::Insert) {
        Some(2)
    } else if is(Key::Delete) {
        Some(3)
    } else if is(Key::PageUp) {
        Some(5)
    } else if is(Key::PageDown) {
        Some(6)
    } else {
        None
    };
    if let Some(n) = tilde {
        return Some(
            if modifier == 1 {
                format!("\x1b[{n}~")
            } else {
                format!("\x1b[{n};{modifier}~")
            }
            .into_bytes(),
        );
    }
    for (key, n) in [
        (Key::F1, 11),
        (Key::F2, 12),
        (Key::F3, 13),
        (Key::F4, 14),
        (Key::F5, 15),
        (Key::F6, 17),
        (Key::F7, 18),
        (Key::F8, 19),
        (Key::F9, 20),
        (Key::F10, 21),
        (Key::F11, 23),
        (Key::F12, 24),
    ] {
        if is(key) {
            return Some(
                if n <= 14 {
                    let c = (b'P' + (n - 11) as u8) as char;
                    if modifier == 1 {
                        format!("\x1bO{c}")
                    } else {
                        format!("\x1b[1;{modifier}{c}")
                    }
                } else if modifier == 1 {
                    format!("\x1b[{n}~")
                } else {
                    format!("\x1b[{n};{modifier}~")
                }
                .into_bytes(),
            );
        }
    }
    let mut bytes = if is(Key::Return) {
        vec![b'\r']
    } else if is(Key::Backspace) {
        vec![if mods.ctrl { 8 } else { 127 }]
    } else if is(Key::Backtab) || (is(Key::Tab) && mods.shift) {
        return Some(b"\x1b[Z".to_vec());
    } else if is(Key::Tab) {
        vec![b'\t']
    } else if is(Key::Escape) {
        vec![27]
    } else if text.chars().any(|c| ('\u{e000}'..='\u{f8ff}').contains(&c)) {
        return None;
    } else if mods.ctrl {
        let c = text.chars().next()?;
        let n = c.to_ascii_uppercase() as u32;
        match n {
            64..=95 => vec![(n - 64) as u8],
            32 | 50 => vec![0],
            63 => vec![127],
            0..=31 => vec![n as u8],
            _ => return None,
        }
    } else {
        text.as_bytes().to_vec()
    };
    if mods.alt {
        bytes.insert(0, 27);
    }
    Some(bytes)
}
pub(super) fn paste(text: &str, bracketed: bool) -> Vec<u8> {
    // Prevent pasted escape/control sequences from terminating bracketed paste.
    let text: String = text
        .chars()
        .take(1024 * 1024 - 16)
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
        .collect();
    let text = text.replace("\r\n", "\n").replace('\n', "\r");
    if bracketed {
        format!("\x1b[200~{text}\x1b[201~").into_bytes()
    } else {
        text.into_bytes()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn control_keys_and_application_arrows() {
        assert_eq!(
            encode(
                "c",
                Mods {
                    ctrl: true,
                    ..Mods::default()
                },
                TermMode::empty()
            )
            .unwrap(),
            [3]
        );
        assert_eq!(
            encode(
                slint::SharedString::from(slint::platform::Key::UpArrow).as_str(),
                Mods::default(),
                TermMode::APP_CURSOR
            )
            .unwrap(),
            b"\x1bOA"
        );
        assert_eq!(
            encode("/", Mods::default(), TermMode::empty()).unwrap(),
            b"/"
        );
    }
    #[test]
    fn modifier_presses_never_reach_the_shell() {
        for key in [
            slint::platform::Key::Shift,
            slint::platform::Key::Control,
            slint::platform::Key::Alt,
            slint::platform::Key::Meta,
        ] {
            let text = slint::SharedString::from(key);
            assert!(encode(text.as_str(), Mods::default(), TermMode::empty()).is_none());
        }
        assert_eq!(
            encode(
                "p",
                Mods {
                    ctrl: true,
                    ..Mods::default()
                },
                TermMode::empty()
            )
            .unwrap(),
            [16]
        );
    }
    #[test]
    fn paste_cannot_inject_escape_sequences() {
        assert_eq!(paste("a\x1b[201~\n", true), b"\x1b[200~a[201~\r\x1b[201~");
    }
}
