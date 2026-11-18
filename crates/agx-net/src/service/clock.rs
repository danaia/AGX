//! Keep the hardware clock usable for TLS at the next boot.
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn save_synchronized_time() {
    // Devices without an RTC can still use network time. Never save an unset clock.
    const MIN_EPOCH: u64 = 1_704_067_200; // 2024-01-01
    if !Path::new("/dev/rtc0").exists()
        || !SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .is_ok_and(|time| time.as_secs() >= MIN_EPOCH)
    {
        return;
    }
    let result = Command::new("/sbin/hwclock")
        .args(["-u", "-w", "-f", "/dev/rtc0"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status();
    if !result.as_ref().is_ok_and(|status| status.success()) {
        eprintln!("agx-netd: could not save synchronized time to RTC: {result:?}");
    }
}
