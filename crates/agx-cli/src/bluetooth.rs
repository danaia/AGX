//! `agx bluetooth`: discovery, pairing and connections through agx-bluetoothd.
use agx_bluetooth::{Device, PROMPT_SECONDS, Reply, Request, SCAN_SECONDS, Status, normalize_address};
use clap::Subcommand;
use std::{error::Error, thread::sleep, time::{Duration, Instant}};

type Outcome = Result<(), Box<dyn Error>>;

#[derive(Subcommand)]
pub enum Action {
    /// Show the adapter and what is connected (default)
    Status,
    /// List known and nearby devices
    Devices {
        /// Only devices already paired
        #[arg(long)]
        paired: bool,
    },
    /// Turn the radio on
    On,
    /// Turn the radio off (only from the desktop's Settings)
    Off,
    /// Search for nearby devices for a few seconds, then list them
    Scan {
        /// Seconds to search (1-45)
        #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u32).range(1..=SCAN_SECONDS as i64))]
        seconds: u32,
    },
    /// Pair with a device, trust it and connect. The person at the ANGI desktop must allow it
    Pair {
        address: String,
        /// Return once the request is queued instead of waiting for the outcome
        #[arg(long)]
        no_wait: bool,
    },
    /// Connect to a device (a new device needs the person's approval at the desktop)
    Connect {
        address: String,
        #[arg(long)]
        no_wait: bool,
    },
    Disconnect {
        address: String,
    },
    /// Reconnect to this device automatically (needs approval at the desktop)
    Trust {
        address: String,
        #[arg(long)]
        no_wait: bool,
    },
    Untrust {
        address: String,
    },
    /// Remove a device and its pairing (needs approval at the desktop)
    Forget {
        address: String,
        #[arg(long)]
        no_wait: bool,
    },
}

fn call(request: Request) -> Result<Reply, Box<dyn Error>> {
    Ok(agx_bluetooth::request(&request)?)
}

fn state(device: &Device) -> String {
    let mut parts = Vec::new();
    for (on, label) in [(device.connected, "connected"), (device.paired, "paired"), (device.trusted, "trusted")] {
        if on {
            parts.push(label);
        }
    }
    if device.busy {
        parts.push("working");
    }
    if parts.is_empty() { "nearby".into() } else { parts.join(",") }
}

fn print_devices(devices: &[&Device]) {
    println!("{:<18} {:<28} {:<11} {:<22} SIGNAL", "ADDRESS", "NAME", "TYPE", "STATE");
    for device in devices {
        let signal = device.rssi.map_or_else(|| "-".into(), |rssi| format!("{rssi} dBm"));
        println!("{:<18} {:<28} {:<11} {:<22} {signal}", device.address, device.name.chars().take(28).collect::<String>(), device.category.label(), state(device));
    }
}

fn print_status(status: &Status) {
    match &status.adapter {
        None => println!("No Bluetooth adapter found"),
        Some(adapter) => println!(
            "{} ({}): {}{}",
            adapter.name,
            adapter.address,
            if adapter.powered { "on" } else { "off" },
            if adapter.scanning { ", searching" } else { "" }
        ),
    }
    let connected: Vec<_> = status.devices.iter().filter(|d| d.connected).collect();
    if connected.is_empty() {
        println!("Nothing connected");
    } else {
        print_devices(&connected);
    }
    for approval in &status.approvals {
        println!("Waiting for approval at the desktop: {} {}", approval.action.label(), approval.name);
    }
}

fn finish(reply: &Reply, json: bool) -> Outcome {
    if json {
        println!("{}", serde_json::to_string(&reply.status)?);
    } else if reply.ok {
        println!("{}", reply.message);
    }
    if reply.ok { Ok(()) } else { Err(reply.message.clone().into()) }
}

/// What a request should have achieved once the person allowed it and it finished.
fn achieved(request: &Request, device: Option<&Device>) -> bool {
    match request {
        Request::Pair { .. } => device.is_some_and(|d| d.paired),
        Request::Connect { .. } => device.is_some_and(|d| d.connected),
        Request::Trust { .. } => device.is_some_and(|d| d.trusted),
        Request::Forget { .. } => device.is_none(),
        _ => true,
    }
}

/// A request that needs the person's approval returns at once; wait for them to answer and for
/// the resulting operation to finish, so scripts and agents get a definite result.
fn settle(first: Reply, request: &Request, address: &str, json: bool) -> Outcome {
    if !first.ok || !first.message.starts_with("Waiting for approval") {
        return finish(&first, json);
    }
    if !json {
        eprintln!("{} (waiting up to {PROMPT_SECONDS}s)", first.message);
    }
    let deadline = Instant::now() + Duration::from_secs(u64::from(PROMPT_SECONDS) + 45);
    let mut idle = 0;
    loop {
        sleep(Duration::from_secs(1));
        let reply = call(Request::Status)?;
        let status = &reply.status;
        let device = status.devices.iter().find(|d| d.address == address);
        if achieved(request, device) && !device.is_some_and(|d| d.busy) {
            let message = if status.message.is_empty() { "Done".into() } else { status.message.clone() };
            return finish(&Reply { ok: true, message, status: status.clone() }, json);
        }
        let waiting = status.approvals.iter().any(|a| a.address == address) || device.is_some_and(|d| d.busy);
        // Trusting and forgetting finish just after the approval disappears; two quiet polls in a
        // row mean the request was declined, expired or failed.
        idle = if waiting { 0 } else { idle + 1 };
        if idle >= 2 {
            let message = status.error.clone().unwrap_or_else(|| "The request was declined or expired at the desktop.".into());
            return finish(&Reply { ok: false, message, status: status.clone() }, json);
        }
        if Instant::now() > deadline {
            return Err("Timed out waiting for the Bluetooth request".into());
        }
    }
}

pub fn run(action: Option<Action>, json: bool) -> Outcome {
    match action.unwrap_or(Action::Status) {
        Action::Status => {
            let reply = call(Request::Status)?;
            if json { println!("{}", serde_json::to_string(&reply.status)?) } else { print_status(&reply.status) }
        }
        Action::Devices { paired } => {
            let reply = call(Request::Status)?;
            let devices: Vec<_> = reply.status.devices.iter().filter(|d| !paired || d.paired).collect();
            if json { println!("{}", serde_json::to_string(&devices)?) } else { print_devices(&devices) }
        }
        Action::On => finish(&call(Request::Power { on: true })?, json)?,
        Action::Off => finish(&call(Request::Power { on: false })?, json)?,
        Action::Scan { seconds } => {
            let reply = call(Request::Scan { on: true })?;
            if !reply.ok {
                return finish(&reply, json);
            }
            sleep(Duration::from_secs(seconds.into()));
            // The daemon also stops a search by itself, so an interrupted CLI cannot leave it running.
            let status = call(Request::Scan { on: false })?.status;
            let devices: Vec<_> = status.devices.iter().filter(|d| !d.paired || d.rssi.is_some()).collect();
            if json { println!("{}", serde_json::to_string(&devices)?) } else { print_devices(&devices) }
        }
        Action::Pair { address, no_wait } => gated(Request::Pair { address: address.clone() }, &address, no_wait, json)?,
        Action::Connect { address, no_wait } => gated(Request::Connect { address: address.clone() }, &address, no_wait, json)?,
        Action::Trust { address, no_wait } => gated(Request::Trust { address: address.clone(), trusted: true }, &address, no_wait, json)?,
        Action::Forget { address, no_wait } => gated(Request::Forget { address: address.clone() }, &address, no_wait, json)?,
        Action::Disconnect { address } => finish(&call(Request::Disconnect { address })?, json)?,
        Action::Untrust { address } => finish(&call(Request::Trust { address, trusted: false })?, json)?,
    }
    Ok(())
}

fn gated(request: Request, address: &str, no_wait: bool, json: bool) -> Outcome {
    let address = normalize_address(address).ok_or("That is not a Bluetooth address (expected AA:BB:CC:DD:EE:FF)")?;
    let reply = call(request.clone())?;
    if no_wait { finish(&reply, json) } else { settle(reply, &request, &address, json) }
}
