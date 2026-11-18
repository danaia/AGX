//! Bluetooth wire protocol shared by agx-bluetoothd, `agx bluetooth` and the desktop's Settings.
mod address;
mod category;
mod client;
mod protocol;

/// Socket served by agx-bluetoothd. `AGX_BLUETOOTH_SOCKET` redirects it for desktop previews.
pub const SOCKET: &str = "/run/agx/bluetooth.sock";
pub const SOCKET_ENV: &str = "AGX_BLUETOOTH_SOCKET";
pub const MAX_REQUEST: usize = 4096;
/// How long a device search runs before the daemon stops it, so a forgotten search cannot drain
/// the radio or the battery indefinitely.
pub const SCAN_SECONDS: u32 = 45;
/// How long a pairing prompt or agent approval waits for the person at the desktop.
pub const PROMPT_SECONDS: u32 = 60;

pub use address::{normalize_address, valid_pin};
pub use category::Category;
pub use client::{request, socket_path};
pub use protocol::{
    Adapter, Approval, ApprovalAction, Device, Prompt, PromptKind, Reply, Request, Status,
};

#[cfg(test)]
mod tests;
