//! Wi-Fi service and local clients, with first-boot setup over the USB gadget.
mod client;
mod credentials;
mod protocol;
pub mod service;
mod setup;
mod ssid;

pub const SOCKET: &str = "/run/agx/network.sock";
pub const MAX_REQUEST: usize = 16384;
pub use client::request;
pub use credentials::{psk, validate_credentials};
pub use protocol::{AccessPoint, Reply, Request, SavedNetwork, Security, Status};
pub use ssid::{decode_ssid, display_ssid, hex, parse_scan, unhex};

#[cfg(test)]
mod tests;
