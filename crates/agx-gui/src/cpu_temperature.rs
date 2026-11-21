//! Read the Pi's CPU thermal sensor for the desktop.

use std::{fs, path::Path};

pub(super) fn temperature() -> Option<f64> {
    read_temperature(Path::new("/sys/class/thermal"))
}

fn read_temperature(thermal_dir: &Path) -> Option<f64> {
    fs::read_dir(thermal_dir)
        .ok()?
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("thermal_zone")
        })
        .filter_map(|entry| {
            let zone = entry.path();
            let kind = fs::read_to_string(zone.join("type")).ok()?;
            if !matches!(kind.trim(), "cpu-thermal" | "cpu_thermal") {
                return None;
            }
            let millidegrees: i32 = fs::read_to_string(zone.join("temp"))
                .ok()?
                .trim()
                .parse()
                .ok()?;
            Some(millidegrees)
        })
        .max()
        .map(|value| f64::from(value) / 1000.0)
}
