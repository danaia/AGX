//! Request, reply and state models. Requests deliberately omit Debug: a response to a pairing
//! prompt can carry a PIN.
use crate::Category;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Adapter {
    pub name: String,
    pub address: String,
    pub powered: bool,
    pub scanning: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Device {
    pub address: String,
    pub name: String,
    pub category: Category,
    pub paired: bool,
    pub connected: bool,
    pub trusted: bool,
    /// A connect or pair attempt for this device is running.
    pub busy: bool,
    pub rssi: Option<i16>,
    pub battery: Option<u8>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromptKind {
    /// Numeric comparison: accept only if both screens show `code`.
    Confirm,
    /// Type `code` on the device being paired (a keyboard, for instance).
    Display,
    /// The device wants a legacy PIN typed here.
    Pin,
    /// The device wants a six-digit passkey typed here.
    Passkey,
    /// A device asks to pair or connect on its own.
    Authorize,
}

/// A decision BlueZ needs from the person at the desktop.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Prompt {
    pub id: u32,
    pub kind: PromptKind,
    pub address: String,
    pub name: String,
    pub code: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalAction {
    /// Pair, then trust the device so it reconnects by itself, then connect.
    Pair,
    Connect,
    Trust,
    Forget,
}
impl ApprovalAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pair => "pair with and always connect to",
            Self::Connect => "connect to",
            Self::Trust => "always connect to",
            Self::Forget => "remove",
        }
    }
}

/// Something other than the desktop (an agent or the CLI) asked for, held until the person allows it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Approval {
    pub id: u32,
    pub action: ApprovalAction,
    pub address: String,
    pub name: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Status {
    pub adapter: Option<Adapter>,
    pub devices: Vec<Device>,
    pub prompt: Option<Prompt>,
    pub approvals: Vec<Approval>,
    /// Seconds left in the current search; zero when not searching.
    pub scan_seconds_left: u32,
    pub message: String,
    pub error: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Status,
    Power {
        on: bool,
    },
    /// Start a bounded search, or stop the running one.
    Scan {
        on: bool,
    },
    Pair {
        address: String,
    },
    Connect {
        address: String,
    },
    Disconnect {
        address: String,
    },
    Trust {
        address: String,
        trusted: bool,
    },
    Forget {
        address: String,
    },
    /// Answer the pairing prompt. Only the desktop may do this.
    Respond {
        id: u32,
        accept: bool,
        value: Option<String>,
    },
    /// Allow or refuse an `Approval`. Only the desktop may do this.
    Decide {
        id: u32,
        allow: bool,
    },
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Reply {
    pub ok: bool,
    pub message: String,
    pub status: Status,
}
