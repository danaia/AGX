//! The hardware/stack boundary. Everything above this trait (policy, prompts, the socket
//! protocol) is stack-independent, so BlueZ could be replaced by a native Rust stack.
use agx_bluetooth::{Adapter, Device};
use std::{fmt, future::Future};

/// A failure already worded for the person at the desktop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(pub String);
impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
pub type Result<T> = std::result::Result<T, Error>;

/// Futures are `Send` so the daemon can run operations on separate tasks.
pub trait Backend: Send + Sync + 'static {
    /// The first adapter, or `None` when the machine has no Bluetooth radio (yet).
    fn adapter(&self) -> impl Future<Output = Result<Option<Adapter>>> + Send;
    /// Known devices with live state. Includes cached devices that are out of range.
    fn devices(&self) -> impl Future<Output = Result<Vec<Device>>> + Send;
    fn set_powered(&self, on: bool) -> impl Future<Output = Result<()>> + Send;
    /// Begin discovery; it continues until `stop_scan`.
    fn start_scan(&self) -> impl Future<Output = Result<()>> + Send;
    fn stop_scan(&self) -> impl Future<Output = Result<()>> + Send;
    /// Pairing may ask the person questions through the prompt broker, so it can take a while.
    /// Dropping the future cancels it.
    fn pair(&self, address: &str) -> impl Future<Output = Result<()>> + Send;
    fn connect(&self, address: &str) -> impl Future<Output = Result<()>> + Send;
    fn disconnect(&self, address: &str) -> impl Future<Output = Result<()>> + Send;
    fn set_trusted(&self, address: &str, trusted: bool) -> impl Future<Output = Result<()>> + Send;
    /// Unpair and drop the stored keys.
    fn forget(&self, address: &str) -> impl Future<Output = Result<()>> + Send;
}
