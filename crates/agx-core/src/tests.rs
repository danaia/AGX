//! CPU and memory parser regressions.

use super::*;
#[test]
fn cpu_excludes_guest_and_handles_reset() {
    let first = parse_cpu("cpu  10 0 10 70 10 0 0 0 20 0\n").unwrap();
    let second = parse_cpu("cpu  30 0 20 120 20 0 0 0 40 0\n").unwrap();
    assert_eq!(first.total, 100);
    assert!((cpu_usage(first, second).unwrap() - 100.0 / 3.0).abs() < 0.001);
    assert!(cpu_usage(second, first).is_none());
    assert!(cpu_usage(first, first).is_none());
    assert!(parse_cpu("cpu 1 invalid 3 4").is_err());
}
#[test]
fn memory_uses_available_and_rejects_bad_samples() {
    let memory = parse_memory("MemTotal: 1000 kB\nMemFree: 1 kB\nMemAvailable: 400 kB\n").unwrap();
    assert_eq!(memory.available_bytes, 409600);
    assert!(parse_memory("MemTotal: 1000 kB").is_err());
    assert!(parse_memory("MemTotal: 1 kB\nMemAvailable: 2 kB").is_err());
    assert!(parse_memory("MemTotal: 1 MB\nMemAvailable: 1 MB").is_err());
}
