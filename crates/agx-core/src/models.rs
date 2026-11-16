//! Serializable system snapshots and CPU sample counters.
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Memory {
    pub total_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Storage {
    pub total_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Network {
    pub name: String,
    pub state: String,
    pub mac: String,
    pub ipv4: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Hardware {
    pub kind: String,
    pub name: String,
    pub state: String,
    pub detail: String,
}

/// Observed VNC sockets and the resident memory of the GUI hosting them.
#[derive(Clone, Debug, Default, Serialize)]
pub struct VncStatus {
    pub port: u16,
    pub listeners: Vec<String>,
    pub connections: usize,
    pub gui_rss_bytes: Option<u64>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub hostname: String,
    pub board: String,
    pub kernel: String,
    pub uptime_secs: u64,
    pub cpu_count: usize,
    pub cpu_usage_percent: Option<f64>,
    pub load_average: Vec<f64>,
    pub memory: Memory,
    pub root_storage: Option<Storage>,
    pub networks: Vec<Network>,
    pub hardware: Vec<Hardware>,
    pub vnc: VncStatus,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
pub struct CpuTicks {
    pub total: u64,
    pub idle: u64,
}
