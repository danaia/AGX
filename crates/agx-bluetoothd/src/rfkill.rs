//! Lift a software block on Bluetooth. A freshly booted image has no service restoring rfkill
//! state, and BlueZ refuses to power an adapter that is blocked.
use std::{fs::OpenOptions, io::Write};

const OP_CHANGE_ALL: u8 = 3;
const TYPE_BLUETOOTH: u8 = 2;

/// The kernel's `struct rfkill_event`: idx, type, op, soft, hard.
fn event(soft: bool) -> [u8; 8] {
    let mut bytes = [0u8; 8];
    bytes[4] = TYPE_BLUETOOTH;
    bytes[5] = OP_CHANGE_ALL;
    bytes[6] = soft as u8;
    bytes
}

pub fn unblock_bluetooth() -> std::io::Result<()> {
    OpenOptions::new()
        .write(true)
        .open("/dev/rfkill")?
        .write_all(&event(false))
}

#[cfg(test)]
mod tests {
    #[test]
    fn event_matches_the_kernel_layout() {
        assert_eq!(super::event(false), [0, 0, 0, 0, 2, 3, 0, 0]);
        assert_eq!(super::event(true)[6], 1);
    }
}
