//! Device access and libudev-zero notifications for the ANGI desktop.
#[path = "../device_events.rs"]
mod device_events;
fn main() -> std::process::ExitCode {
    match device_events::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("agx-display-devices: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
