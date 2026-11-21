//! System snapshot presentation, units, clock and optional preview palette.
use crate::{AppWindow, HwRow, NetRow, Sys, Theme};
use agx_core::Snapshot;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::fs;
pub(super) fn apply(ui: &AppWindow, s: &Snapshot, temperature: Option<f64>) {
    let sys = ui.global::<Sys>();
    sys.set_hostname(s.hostname.clone().into());
    sys.set_board(s.board.clone().into());
    sys.set_kernel(s.kernel.clone().into());
    sys.set_uptime(duration(s.uptime_secs).into());
    sys.set_cpu_percent(s.cpu_usage_percent.unwrap_or(0.0) as f32);
    sys.set_cpu_detail(format!("{} cores", s.cpu_count).into());
    let temperature = temperature
        .map(|value| format!("{value:.1} °C"))
        .unwrap_or_else(|| "Unavailable".into());
    sys.set_cpu_temperature(temperature.into());
    sys.set_load_text(
        s.load_average
            .iter()
            .map(|v| format!("{v:.2}"))
            .collect::<Vec<_>>()
            .join("  ")
            .into(),
    );
    let mem_used = s
        .memory
        .total_bytes
        .saturating_sub(s.memory.available_bytes);
    sys.set_mem_percent(percent(mem_used, s.memory.total_bytes));
    sys.set_mem_detail(format!("{} / {}", bytes(mem_used), bytes(s.memory.total_bytes)).into());
    match &s.root_storage {
        Some(d) => {
            let used = d.total_bytes.saturating_sub(d.available_bytes);
            sys.set_disk_percent(percent(used, d.total_bytes));
            sys.set_disk_detail(format!("{} / {}", bytes(used), bytes(d.total_bytes)).into());
        }
        None => sys.set_disk_detail("unavailable".into()),
    }
    let primary = s
        .networks
        .iter()
        .filter(|n| n.name != "lo")
        .find_map(|n| n.ipv4.first().cloned())
        .unwrap_or_else(|| "no network".into());
    sys.set_primary_ip(primary.into());
    let nets: Vec<NetRow> = s
        .networks
        .iter()
        .map(|n| NetRow {
            name: n.name.clone().into(),
            state: n.state.clone().into(),
            mac: n.mac.clone().into(),
            ipv4: if n.ipv4.is_empty() {
                "-".into()
            } else {
                n.ipv4.join(", ").into()
            },
        })
        .collect();
    sys.set_networks(ModelRc::new(VecModel::from(nets)));
    let hw: Vec<HwRow> = s
        .hardware
        .iter()
        .map(|h| HwRow {
            kind: h.kind.clone().into(),
            name: h.name.clone().into(),
            state: h.state.clone().into(),
            detail: h.detail.clone().into(),
        })
        .collect();
    sys.set_hardware(ModelRc::new(VecModel::from(hw)));
    sys.set_warnings(s.warnings.join("; ").into());
}

pub(super) fn apply_network_safety(sys: &Sys, profile: crate::network_safety::Profile) {
    sys.set_network_risk_score(profile.score);
    sys.set_network_risk_reason(profile.reason.into());
}

fn percent(part: u64, whole: u64) -> f32 {
    if whole == 0 {
        0.0
    } else {
        (part as f64 * 100.0 / whole as f64) as f32
    }
}

pub(super) fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn duration(secs: u64) -> String {
    let (d, h, m) = (secs / 86_400, secs % 86_400 / 3600, secs % 3600 / 60);
    if d > 0 {
        format!("{d}d {h}h {m}m")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m {}s", secs % 60)
    }
}

pub(super) fn local_time() -> String {
    // SAFETY: localtime_r writes only into the zeroed tm we pass.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return String::new();
        }
        format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
    }
}

/// Follow Omarchy's active theme when its palette file is present, so the
/// GUI matches a desktop it is previewed on. The appliance falls back to the
/// built-in Tokyo Night defaults.
pub(super) fn apply_omarchy_theme(ui: &AppWindow) {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let path = std::path::Path::new(&home).join(".config/omarchy/current/theme/colors.toml");
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    let color = |key: &str| -> Option<slint::Color> {
        let line = text
            .lines()
            .find(|l| l.split('=').next().map(str::trim) == Some(key))?;
        let hex = line
            .split('=')
            .nth(1)?
            .trim()
            .trim_matches('"')
            .trim_start_matches('#');
        let n = u32::from_str_radix(hex, 16)
            .ok()
            .filter(|_| hex.len() == 6)?;
        Some(slint::Color::from_rgb_u8(
            (n >> 16) as u8,
            (n >> 8) as u8,
            n as u8,
        ))
    };
    let t = ui.global::<Theme>();
    macro_rules! set {
        ($setter:ident, $key:expr) => {
            if let Some(c) = color($key) {
                t.$setter(c);
            }
        };
    }
    set!(set_bg, "background");
    set!(set_bg_dark, "dark_background");
    set!(set_bg_light, "lighter_background");
    set!(set_fg, "foreground");
    set!(set_fg_bright, "bright_foreground");
    set!(set_fg_dim, "dark_foreground");
    set!(set_muted, "muted");
    set!(set_accent, "accent");
    set!(set_green, "green");
    set!(set_yellow, "yellow");
    set!(set_red, "red");
    set!(set_cyan, "bright_cyan");
    set!(set_magenta, "bright_magenta");
}
