//! Linux /proc, /sys and libc collection behind the shared Sampler API.
use crate::parsers::invalid;
use crate::{CpuTicks, Hardware, Network, Snapshot, Storage, cpu_usage, parse_cpu, parse_memory};
use std::{collections::HashMap, ffi::CStr, fs, io, net::Ipv4Addr, path::Path};

fn text(path: impl AsRef<Path>) -> io::Result<String> {
    Ok(fs::read_to_string(path)?
        .trim_matches(['\0', '\n', ' ', '\t'])
        .to_owned())
}
fn optional_text(path: impl AsRef<Path>, fallback: &str) -> String {
    text(path).unwrap_or_else(|_| fallback.to_owned())
}

fn root_storage() -> io::Result<Storage> {
    let mut value = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // The path is NUL-terminated and statvfs initializes value on success.
    if unsafe { libc::statvfs(c"/".as_ptr(), value.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let value = unsafe { value.assume_init() };
    Ok(Storage {
        total_bytes: value.f_blocks.saturating_mul(value.f_frsize),
        available_bytes: value.f_bavail.saturating_mul(value.f_frsize),
    })
}

fn addresses() -> io::Result<HashMap<String, Vec<String>>> {
    let mut head = std::ptr::null_mut();
    // getifaddrs owns this linked list until freeifaddrs; all references stay inside this scope.
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut found: HashMap<String, Vec<String>> = HashMap::new();
    let mut next = head;
    while !next.is_null() {
        let item = unsafe { &*next };
        if !item.ifa_name.is_null()
            && !item.ifa_addr.is_null()
            && unsafe { (*item.ifa_addr).sa_family as i32 } == libc::AF_INET
        {
            let name = unsafe { CStr::from_ptr(item.ifa_name) }
                .to_string_lossy()
                .into_owned();
            let address = unsafe { &*(item.ifa_addr as *const libc::sockaddr_in) };
            let ip = Ipv4Addr::from(address.sin_addr.s_addr.to_ne_bytes());
            found.entry(name).or_default().push(ip.to_string());
        }
        next = item.ifa_next;
    }
    unsafe { libc::freeifaddrs(head) };
    for values in found.values_mut() {
        values.sort();
        values.dedup();
    }
    Ok(found)
}

#[derive(Default)]
pub struct Sampler {
    previous_cpu: Option<CpuTicks>,
}
impl Sampler {
    pub fn collect(&mut self) -> io::Result<Snapshot> {
        let stat = fs::read_to_string("/proc/stat")?;
        let ticks = parse_cpu(&stat)?;
        let memory = parse_memory(&fs::read_to_string("/proc/meminfo")?)?;
        let uptime: f64 = text("/proc/uptime")?
            .split_whitespace()
            .next()
            .ok_or_else(|| invalid("Missing uptime"))?
            .parse()
            .map_err(|_| invalid("Invalid uptime"))?;
        let load_average = text("/proc/loadavg")?
            .split_whitespace()
            .take(3)
            .map(|v| v.parse().map_err(|_| invalid("Invalid load average")))
            .collect::<io::Result<Vec<f64>>>()?;
        let cpu_usage_percent = self
            .previous_cpu
            .and_then(|previous| cpu_usage(previous, ticks));
        self.previous_cpu = Some(ticks);
        let mut warnings = Vec::new();
        let root_storage = root_storage()
            .map_err(|e| warnings.push(format!("Root storage: {e}")))
            .ok();
        let ips = addresses().unwrap_or_else(|e| {
            warnings.push(format!("Network addresses: {e}"));
            HashMap::new()
        });
        let mut networks = Vec::new();
        match fs::read_dir("/sys/class/net") {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let path = entry.path();
                    networks.push(Network {
                        name: name.clone(),
                        state: optional_text(path.join("operstate"), "unknown"),
                        mac: optional_text(path.join("address"), "unknown"),
                        ipv4: ips.get(&name).cloned().unwrap_or_default(),
                    });
                }
            }
            Err(e) => warnings.push(format!("Network inventory: {e}")),
        }
        networks.sort_by(|a, b| a.name.cmp(&b.name));
        let mut hardware = Vec::new();
        match fs::read_dir("/sys/block") {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with("loop") || name.starts_with("ram") {
                        continue;
                    }
                    let path = entry.path();
                    let sectors = text(path.join("size"))
                        .ok()
                        .and_then(|s| s.parse::<u64>().ok());
                    let detail = sectors
                        .map(|n| format!("{:.2} GiB", n as f64 * 512.0 / 1073741824.0))
                        .unwrap_or_else(|| "size unavailable".into());
                    let model = optional_text(path.join("device/model"), "");
                    hardware.push(Hardware {
                        kind: "Storage".into(),
                        name,
                        state: "detected".into(),
                        detail: format!("{detail} {model}").trim().into(),
                    });
                }
            }
            Err(e) => warnings.push(format!("Storage inventory: {e}")),
        }
        for network in &networks {
            hardware.push(Hardware {
                kind: "Network".into(),
                name: network.name.clone(),
                state: network.state.clone(),
                detail: if network.ipv4.is_empty() {
                    format!("{} / no IPv4 address", network.mac)
                } else {
                    network.ipv4.join(", ")
                },
            });
        }
        match fs::read_to_string("/proc/modules") {
            Ok(modules) => {
                for line in modules.lines().take(192) {
                    let fields: Vec<_> = line.split_whitespace().collect();
                    if fields.len() >= 5 {
                        hardware.push(Hardware {
                            kind: "Module".into(),
                            name: fields[0].into(),
                            state: fields[4].into(),
                            detail: format!("{} bytes / {} users", fields[1], fields[2]),
                        });
                    }
                }
            }
            Err(e) => warnings.push(format!("Kernel modules: {e}")),
        }
        hardware.sort_by(|a, b| (&a.kind, &a.name).cmp(&(&b.kind, &b.name)));
        Ok(Snapshot {
            hostname: optional_text("/proc/sys/kernel/hostname", "unknown"),
            board: text("/proc/device-tree/model")
                .or_else(|_| text("/sys/class/dmi/id/product_name"))
                .unwrap_or_else(|_| "Board identity unavailable".into()),
            kernel: optional_text("/proc/sys/kernel/osrelease", "unknown"),
            uptime_secs: uptime.max(0.0) as u64,
            cpu_count: stat
                .lines()
                .filter(|line| {
                    line.strip_prefix("cpu")
                        .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
                })
                .count(),
            cpu_usage_percent,
            load_average,
            memory,
            root_storage,
            networks,
            hardware,
            vnc: crate::vnc::collect(),
            warnings,
        })
    }
}
