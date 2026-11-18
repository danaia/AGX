//! agx-bluetoothd: discovery, pairing and connections for the ANGI desktop, CLI and agents.
mod backend;
mod bluez;
mod core;
mod peer;
mod prompts;
mod rfkill;
mod server;
#[cfg(test)]
mod tests;

fn main() {
    if let Err(error) = server::run() {
        eprintln!("agx-bluetoothd: {error}");
        std::process::exit(1);
    }
}
