//! Validate and commit a single browser claim using the existing services.
use crate::{Reply, Request, Security, request, validate_credentials};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::Path,
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

pub const ENABLED: &str = "/etc/agx/setup-enabled";
const HOST_KEY: &str = "/etc/dropbear/dropbear_ed25519_host_key";
const KEYGEN: &str = "/usr/bin/dropbearkey";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Form {
    name: String,
    hostname: String,
    username: String,
    password: Zeroizing<String>,
    ssh_key: String,
    country: String,
    ssid: String,
    #[serde(default)]
    ssid_hex: Option<String>,
    security: Security,
    wifi_password: Zeroizing<String>,
    hidden: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scan {
    country: String,
}

#[derive(Serialize)]
pub struct NearbyNetwork {
    name: String,
    ssid_hex: String,
    security: Security,
}

pub fn unclaimed() -> io::Result<bool> {
    if agx_profile::service::setup_claimed()? {
        return Ok(false);
    }
    // Agent-prepared cards and existing administrator accounts already have a login path.
    match fs::metadata("/root/.ssh/authorized_keys") {
        Ok(metadata) if metadata.len() > 0 => return Ok(false),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(agx_profile::service::status()?.accounts.is_empty())
}

pub fn host_fingerprint() -> io::Result<String> {
    if !Path::new(HOST_KEY).try_exists()? {
        let status = Command::new(KEYGEN)
            .args(["-t", "ed25519", "-f", HOST_KEY])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if !status.success() && !Path::new(HOST_KEY).try_exists()? {
            return Err(io::Error::other("Could not generate the Pi's SSH host key"));
        }
    }
    let output = Command::new(KEYGEN).args(["-y", "-f", HOST_KEY]).output()?;
    if !output.status.success() {
        return Err(io::Error::other("Could not read the Pi's SSH fingerprint"));
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("Fingerprint: "))
        .map(str::to_owned)
        .ok_or_else(|| io::Error::other("SSH fingerprint was not reported"))
}

fn public_key(input: &str) -> Result<Option<String>, String> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(None);
    }
    let invalid = || "Enter one OpenSSH public key, such as an ssh-ed25519 key.".to_owned();
    if input.len() > 4096 || input.contains(['\n', '\r', '\0']) {
        return Err(invalid());
    }
    let mut fields = input.split_ascii_whitespace();
    let kind = fields.next().ok_or_else(invalid)?;
    if !matches!(
        kind,
        "ssh-ed25519"
            | "ssh-rsa"
            | "ecdsa-sha2-nistp256"
            | "ecdsa-sha2-nistp384"
            | "ecdsa-sha2-nistp521"
    ) {
        return Err(invalid());
    }
    let blob = fields.next().ok_or_else(invalid)?;
    let key = format!("{kind} {blob}\n");
    let mut validator = Command::new("/usr/bin/ssh-keygen")
        .args(["-l", "-f", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| invalid())?;
    let result = validator
        .stdin
        .take()
        .ok_or_else(invalid)?
        .write_all(key.as_bytes());
    let status = validator.wait().map_err(|_| invalid())?;
    if result.is_err() || !status.success() {
        return Err(invalid());
    }
    Ok(Some(key))
}

fn wifi(command: &Request) -> Result<Reply, String> {
    let reply = request(command).map_err(|error| error.to_string())?;
    if !reply.ok {
        return Err(reply.message);
    }
    Ok(reply)
}

pub fn scan(body: &[u8], stop: &AtomicBool) -> Result<Vec<NearbyNetwork>, String> {
    let Scan { country } =
        serde_json::from_slice(body).map_err(|_| "Invalid Wi-Fi scan request.".to_owned())?;
    wifi(&Request::Country { country })?;
    wifi(&Request::Scan)?;
    let deadline = Instant::now() + Duration::from_secs(16);
    while !stop.load(Ordering::Relaxed) && Instant::now() < deadline {
        let status = wifi(&Request::Status)?.status;
        if !status.scanning {
            return Ok(status
                .access_points
                .into_iter()
                .filter(|point| point.security != Security::Unsupported)
                .map(|point| NearbyNetwork {
                    name: point.ssid,
                    ssid_hex: point.ssid_hex,
                    security: point.security,
                })
                .collect());
        }
        thread::sleep(Duration::from_millis(350));
    }
    Err("The Wi-Fi scan timed out. Try scanning again or enter the network name yourself.".into())
}

fn wait_for_wifi(ssid_hex: &str, stop: &AtomicBool) -> Result<Vec<String>, String> {
    let deadline = Instant::now() + Duration::from_secs(65);
    while !stop.load(Ordering::Relaxed) && Instant::now() < deadline {
        let status = wifi(&Request::Status)?.status;
        if let Some(error) = status.error {
            return Err(error);
        }
        if status.phase == "Connected"
            && status
                .saved
                .iter()
                .any(|network| network.current && network.ssid_hex == ssid_hex)
        {
            return Ok(status.ipv4);
        }
        thread::sleep(Duration::from_millis(500));
    }
    Err("The Pi did not obtain a Wi-Fi address. Check the network settings and try again.".into())
}

pub fn submit(body: &[u8], fingerprint: &str, stop: &AtomicBool) -> Result<String, String> {
    let form: Form = serde_json::from_slice(body).map_err(|_| "Invalid setup form.".to_owned())?;
    if !unclaimed().map_err(|error| error.to_string())? {
        return Err("This ANGI system is already set up.".into());
    }
    let profile = agx_profile::Save {
        name: form.name,
        hostname: form.hostname,
        username: form.username,
        password: form.password,
        create: true,
    };
    agx_profile::validate(&profile)?;
    let key = public_key(&form.ssh_key)?;
    let ssid_hex = if let Some(hex) = form.ssid_hex {
        let bytes = crate::unhex(&hex)?;
        if crate::display_ssid(&bytes) != form.ssid {
            return Err("Choose the network again or enter its name yourself.".into());
        }
        crate::hex(&bytes)
    } else {
        crate::hex(form.ssid.as_bytes())
    };
    validate_credentials(&ssid_hex, form.security, &form.wifi_password)?;
    wifi(&Request::Country {
        country: form.country,
    })?;
    wifi(&Request::Connect {
        ssid_hex: ssid_hex.clone(),
        security: form.security,
        password: form.wifi_password,
        hidden: form.hidden,
    })?;
    let addresses = wait_for_wifi(&ssid_hex, stop)?;
    let internet = check_internet(stop);
    if stop.load(Ordering::Relaxed) {
        return Err("The network service is shutting down. Try setup again after boot.".into());
    }
    let username = profile.username.clone();
    let account = agx_profile::service::save_initial(profile, key.as_deref())
        .map_err(|error| error.to_string())?;
    Ok(format!(
        "{account}\nWi-Fi: {}\n{internet}\nUSB-C: ssh {username}@10.77.0.2\nPi SSH host key: {fingerprint}",
        addresses.join(", ")
    ))
}

fn check_internet(stop: &AtomicBool) -> String {
    // Wi-Fi can obtain an address before a Pi without a real-time clock has
    // synchronized. Give NTP a bounded head start, then check even if blocked.
    let sync_deadline = Instant::now() + Duration::from_secs(12);
    while !stop.load(Ordering::Relaxed) && Instant::now() < sync_deadline {
        match wifi(&Request::Status) {
            Ok(reply) if reply.status.time_synced => break,
            Ok(_) => thread::sleep(Duration::from_millis(500)),
            Err(error) => return error,
        }
    }
    if stop.load(Ordering::Relaxed) {
        return "Internet check interrupted.".into();
    }
    if let Err(error) = wifi(&Request::Check) {
        return error;
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while !stop.load(Ordering::Relaxed) && Instant::now() < deadline {
        match wifi(&Request::Status) {
            Ok(reply) if reply.status.diagnostics == "Checking DNS and HTTPS..." => {}
            Ok(reply) => return reply.status.diagnostics,
            Err(error) => return error,
        }
        thread::sleep(Duration::from_millis(500));
    }
    "Wi-Fi is connected. The Internet check timed out.".into()
}
