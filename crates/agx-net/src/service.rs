//! Root-only network service. Owns authentication, DHCP, saved profiles and reboot.
use crate::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    net::Ipv4Addr,
    os::{
        fd::AsRawFd,
        unix::{
            fs::{OpenOptionsExt, PermissionsExt},
            net::{UnixDatagram, UnixListener, UnixStream},
            process::CommandExt,
        },
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const ROOT: &str = "/etc/agx";
const CONFIG: &str = "/etc/agx/wpa_supplicant.conf";
const SETTINGS: &str = "/etc/agx/network.json";
const RUN: &str = "/run/agx";
const IFACE: &str = "wlan0";
static NEXT: AtomicU64 = AtomicU64::new(0);
fn err(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}
fn private_dir(path: &str) -> io::Result<()> {
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}
fn atomic_private(path: &Path, data: &[u8]) -> io::Result<()> {
    let temp = path.with_extension("agx-new");
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&temp)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    file.write_all(data)?;
    file.sync_all()?;
    fs::rename(&temp, path)?;
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}
fn command(program: &str, args: &[&str]) -> io::Result<()> {
    let status = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        return Err(err(format!("{program} failed")));
    }
    Ok(())
}

struct Control {
    socket: UnixDatagram,
    path: PathBuf,
}
impl Control {
    fn new() -> io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = PathBuf::from(format!(
            "{RUN}/c-{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let socket = UnixDatagram::bind(&path)?;
        let control = Self { socket, path };
        control
            .socket
            .set_read_timeout(Some(Duration::from_secs(1)))?;
        control
            .socket
            .set_write_timeout(Some(Duration::from_secs(1)))?;
        control.socket.connect(format!("{RUN}/wpa/{IFACE}"))?;
        Ok(control)
    }
    fn send(&self, value: &str) -> io::Result<String> {
        self.socket.send(value.as_bytes())?;
        let mut buffer = vec![0u8; 65536];
        let length = self.socket.recv(&mut buffer)?;
        Ok(String::from_utf8_lossy(&buffer[..length])
            .trim_end()
            .to_owned())
    }
}
impl Drop for Control {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
fn wpa(value: &str) -> io::Result<String> {
    Control::new()?.send(value)
}
fn ok(value: &str) -> io::Result<()> {
    if wpa(value)? != "OK" {
        return Err(err(
            "Wireless authentication service rejected the operation",
        ));
    }
    Ok(())
}
fn pairs(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|line| {
            line.split_once('=')
                .map(|(a, b)| (a.to_owned(), b.to_owned()))
        })
        .collect()
}

#[derive(Serialize, Deserialize)]
struct Settings {
    country: String,
}
struct Attempt {
    id: i32,
    previous: Option<i32>,
    temporary: bool,
    ssid_hex: String,
    started: Instant,
}
struct Diagnostic {
    child: Child,
}
struct Manager {
    state: Status,
    wpa_child: Option<Child>,
    dhcp: Option<Child>,
    ntp: Option<Child>,
    events: Option<Control>,
    attempt: Option<Attempt>,
    scan_started: Option<Instant>,
    diagnostic: Option<Diagnostic>,
    last_poll: Instant,
    last_start: Option<Instant>,
    last_dhcp: Option<Instant>,
    last_ntp: Option<Instant>,
    disconnected: bool,
}
impl Manager {
    fn new() -> io::Result<Self> {
        private_dir(ROOT)?;
        private_dir(&format!("{RUN}/wpa"))?;
        let settings = fs::read(SETTINGS)
            .ok()
            .and_then(|data| serde_json::from_slice::<Settings>(&data).ok())
            .unwrap_or(Settings {
                country: "US".into(),
            });
        let country = if valid_country(&settings.country) {
            settings.country
        } else {
            "US".into()
        };
        if !Path::new(CONFIG).exists() {
            atomic_private(
                Path::new(CONFIG),
                format!("ctrl_interface={RUN}/wpa\nupdate_config=1\ncountry={country}\n")
                    .as_bytes(),
            )?;
        }
        fs::set_permissions(CONFIG, fs::Permissions::from_mode(0o600))?;
        let state = Status {
            country,
            ..Status::default()
        };
        Ok(Self {
            state,
            wpa_child: None,
            dhcp: None,
            ntp: None,
            events: None,
            attempt: None,
            scan_started: None,
            diagnostic: None,
            last_poll: Instant::now() - Duration::from_secs(2),
            last_start: None,
            last_dhcp: None,
            last_ntp: None,
            disconnected: false,
        })
    }
    fn start_stack(&mut self) -> io::Result<()> {
        if self
            .last_start
            .is_some_and(|at| at.elapsed() < Duration::from_secs(10))
        {
            return Ok(());
        }
        self.last_start = Some(Instant::now());
        if !Path::new("/sys/class/net/wlan0").exists() {
            command("/sbin/modprobe", &["brcmfmac"])?;
        }
        self.state.interface_present = Path::new("/sys/class/net/wlan0").exists();
        if !self.state.interface_present {
            return Err(err(
                "Wi-Fi interface not detected; check Broadcom driver and firmware",
            ));
        }
        if let Ok(entries) = fs::read_dir("/sys/class/rfkill") {
            for entry in entries.flatten() {
                if fs::read_to_string(entry.path().join("type")).is_ok_and(|s| s.trim() == "wlan") {
                    let _ = fs::write(entry.path().join("soft"), b"0");
                }
            }
        }
        command("/sbin/ip", &["link", "set", IFACE, "up"])?;
        let _ = command("/usr/sbin/iw", &["reg", "set", &self.state.country]);
        self.wpa_child = Some(
            Command::new("/usr/sbin/wpa_supplicant")
                .args(["-i", IFACE, "-D", "nl80211", "-c", CONFIG])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?,
        );
        self.state.phase = "Starting".into();
        self.state.error = None;
        self.state.message = "Starting wireless authentication".into();
        Ok(())
    }
    fn saved(&self) -> Vec<SavedNetwork> {
        wpa("LIST_NETWORKS")
            .unwrap_or_default()
            .lines()
            .skip(1)
            .filter_map(|line| {
                let fields: Vec<_> = line.splitn(4, '\t').collect();
                if fields.len() < 4 {
                    return None;
                }
                let id = fields[0].parse().ok()?;
                if self
                    .attempt
                    .as_ref()
                    .is_some_and(|a| a.temporary && a.id == id)
                {
                    return None;
                }
                let bytes = decode_ssid(fields[1]);
                Some(SavedNetwork {
                    id,
                    ssid: display_ssid(&bytes),
                    ssid_hex: hex(&bytes),
                    current: fields[3].contains("CURRENT"),
                })
            })
            .collect()
    }
    fn fail_attempt(&mut self, message: &str) {
        if let Some(attempt) = self.attempt.take() {
            if attempt.temporary {
                let _ = ok(&format!("REMOVE_NETWORK {}", attempt.id));
            }
            if let Some(previous) = attempt.previous {
                let _ = ok(&format!("SELECT_NETWORK {previous}"));
            } else {
                let _ = ok("DISCONNECT");
                self.disconnected = true;
            }
        }
        self.state.busy = false;
        self.state.error = Some(message.into());
        self.state.message = message.into();
    }
    fn authenticate(
        &mut self,
        ssid_hex: String,
        security: Security,
        password: Zeroizing<String>,
        hidden: bool,
    ) -> io::Result<()> {
        validate_credentials(&ssid_hex, security, &password).map_err(err)?;
        if self.attempt.is_some() {
            return Err(err("A connection is already in progress"));
        }
        let previous = self.state.saved.iter().find(|n| n.current).map(|n| n.id);
        let id: i32 = wpa("ADD_NETWORK")?
            .trim()
            .parse()
            .map_err(|_| err("Unable to create a network profile"))?;
        let configure = (|| {
            ok(&format!("SET_NETWORK {id} ssid {ssid_hex}"))?;
            if hidden {
                ok(&format!("SET_NETWORK {id} scan_ssid 1"))?;
            }
            match security {
                Security::Wpa2 => {
                    ok(&format!("SET_NETWORK {id} key_mgmt WPA-PSK"))?;
                    let key = psk(&password, &unhex(&ssid_hex).map_err(err)?);
                    let request = Zeroizing::new(format!("SET_NETWORK {id} psk {}", *key));
                    ok(&request)?;
                    ok(&format!("SET_NETWORK {id} ieee80211w 1"))?;
                }
                Security::Wpa3 => {
                    ok(&format!("SET_NETWORK {id} key_mgmt SAE"))?;
                    let quoted = Zeroizing::new(serde_json::to_string(&*password)?);
                    let request =
                        Zeroizing::new(format!("SET_NETWORK {id} sae_password {}", *quoted));
                    ok(&request)?;
                    ok(&format!("SET_NETWORK {id} ieee80211w 2"))?;
                }
                Security::Owe => {
                    ok(&format!("SET_NETWORK {id} key_mgmt OWE"))?;
                    ok(&format!("SET_NETWORK {id} ieee80211w 2"))?;
                }
                Security::Open => ok(&format!("SET_NETWORK {id} key_mgmt NONE"))?,
                Security::Unsupported => return Err(err("Unsupported authentication method")),
            }
            ok(&format!("SET_NETWORK {id} priority 10"))?;
            ok(&format!("SELECT_NETWORK {id}"))
        })();
        if let Err(error) = configure {
            let _ = ok(&format!("REMOVE_NETWORK {id}"));
            return Err(error);
        }
        self.disconnected = false;
        self.state.error = None;
        self.state.busy = true;
        self.state.phase = "Authenticating".into();
        self.state.message =
            "Authenticating; credentials will be saved after successful association".into();
        self.attempt = Some(Attempt {
            id,
            previous,
            temporary: true,
            ssid_hex,
            started: Instant::now(),
        });
        Ok(())
    }
    fn stop_dhcp(&mut self) {
        if let Some(mut child) = self.dhcp.take() {
            // DHCP hooks share this dedicated group; stop them before clearing routes.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGTERM);
            }
            thread::sleep(Duration::from_millis(50));
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.wait();
        }
        let _ = command("/sbin/ip", &["addr", "flush", "dev", IFACE]);
        let _ = command("/sbin/ip", &["route", "flush", "dev", IFACE]);
        let _ = fs::remove_file(format!("{RUN}/lease.json"));
        let _ = update_dns(&[]);
    }
    fn handle(&mut self, request: Request) -> io::Result<String> {
        match request {
            Request::SetGitHubToken { token } => {
                if token.is_empty() {
                    fs::remove_file("/etc/agx/github-token").or_else(|e| {
                        if e.kind() == io::ErrorKind::NotFound {
                            Ok(())
                        } else {
                            Err(e)
                        }
                    })?;
                    return Ok("GitHub token removed.".into());
                }
                if token.len() < 20
                    || token.len() > 255
                    || !token
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                {
                    return Err(err("Invalid GitHub token format."));
                }
                atomic_private(Path::new("/etc/agx/github-token"), token.as_bytes())?;
                return Ok("GitHub token saved. Press C to check releases.".into());
            }
            Request::CheckGitHubRelease => return check_github_release(),
            Request::ProfileStatus => return Ok("Profile ready".into()),
            Request::SaveProfile(save) => return agx_profile::service::save(save),
            Request::Status => return Ok(self.state.message.clone()),
            Request::Reboot { confirmed } | Request::Shutdown { confirmed } => {
                let reboot = matches!(request, Request::Reboot { .. });
                if !confirmed {
                    return Err(err("Power operation requires explicit confirmation"));
                }
                if Path::new(agx_init::SOCKET).exists() {
                    let operation = if reboot {
                        agx_init::Request::Reboot { confirmed: true }
                    } else {
                        agx_init::Request::Shutdown { confirmed: true }
                    };
                    let reply = agx_init::request(&operation)?;
                    return if reply.ok {
                        Ok(reply.message)
                    } else {
                        Err(err(reply.message))
                    };
                }
                thread::spawn(move || {
                    thread::sleep(Duration::from_secs(2));
                    let _ = command(
                        if reboot {
                            "/sbin/reboot"
                        } else {
                            "/sbin/poweroff"
                        },
                        &[],
                    );
                });
                return Ok("Power operation requested. SSH sessions will close.".into());
            }
            Request::Country { country } => {
                let country = country.to_uppercase();
                if !valid_country(&country) {
                    return Err(err("Use a two-letter country code, such as US or GB"));
                }
                if self.state.busy {
                    return Err(err(
                        "Wait for the connection attempt before changing country",
                    ));
                }
                ok(&format!("SET country {country}"))?;
                command("/usr/sbin/iw", &["reg", "set", &country])?;
                ok("SAVE_CONFIG")?;
                atomic_private(
                    Path::new(SETTINGS),
                    &serde_json::to_vec(&Settings {
                        country: country.clone(),
                    })?,
                )?;
                self.state.country = country;
                self.state.message = "Radio region saved".into();
            }
            Request::Scan => {
                if self.state.busy {
                    return Err(err("Wait for authentication to finish before scanning"));
                }
                let reply = wpa("SCAN")?;
                if reply != "OK" && !reply.contains("BUSY") {
                    return Err(err("Scan could not start"));
                }
                self.scan_started = Some(Instant::now());
                self.state.scanning = true;
                self.state.message = "Scanning nearby networks".into();
            }
            Request::Connect {
                ssid_hex,
                security,
                password,
                hidden,
            } => {
                self.authenticate(ssid_hex, security, password, hidden)?;
                return Ok(self.state.message.clone());
            }
            Request::SelectSaved { id } => {
                let selected = self
                    .saved()
                    .into_iter()
                    .find(|n| n.id == id)
                    .ok_or_else(|| err("Saved network no longer exists"))?;
                if self.state.busy {
                    return Err(err("A connection is already in progress"));
                }
                let previous = self.state.saved.iter().find(|n| n.current).map(|n| n.id);
                ok(&format!("SELECT_NETWORK {id}"))?;
                self.attempt = Some(Attempt {
                    id,
                    previous,
                    temporary: false,
                    ssid_hex: selected.ssid_hex,
                    started: Instant::now(),
                });
                self.disconnected = false;
                self.state.busy = true;
                self.state.error = None;
                self.state.message = "Connecting using saved authentication".into();
            }
            Request::Forget { id } => {
                if self.state.busy {
                    return Err(err(
                        "Wait for authentication to finish before forgetting a profile",
                    ));
                }
                let network = self
                    .saved()
                    .into_iter()
                    .find(|n| n.id == id)
                    .ok_or_else(|| err("Saved network no longer exists"))?;
                ok(&format!("REMOVE_NETWORK {id}"))?;
                ok("SAVE_CONFIG")?;
                if network.current {
                    self.disconnected = true;
                    self.stop_dhcp();
                }
                self.state.message = "Saved network and its authentication removed".into();
            }
            Request::Disconnect => {
                if let Some(attempt) = self.attempt.take()
                    && attempt.temporary
                {
                    let _ = ok(&format!("REMOVE_NETWORK {}", attempt.id));
                }
                ok("DISCONNECT")?;
                self.stop_dhcp();
                self.disconnected = true;
                self.state.busy = false;
                self.state.message =
                    "Disconnected. Saved networks are retained for reconnect.".into();
            }
            Request::Check => {
                if self.state.ipv4.is_empty() {
                    return Err(err(
                        "Connect and obtain an IP address before checking Internet access",
                    ));
                }
                if self.diagnostic.is_some() {
                    return Err(err("An Internet check is already running"));
                }
                let child = Command::new("/usr/bin/curl")
                    .args([
                        "--silent",
                        "--output",
                        "/dev/null",
                        "--write-out",
                        "%{http_code}",
                        "--connect-timeout",
                        "5",
                        "--max-time",
                        "8",
                        "https://api.openai.com/v1/models",
                    ])
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()?;
                self.diagnostic = Some(Diagnostic { child });
                self.state.diagnostics = "Checking DNS and HTTPS...".into();
            }
        }
        self.state.error = None;
        Ok(self.state.message.clone())
    }
    fn tick(&mut self) {
        if let Some(child) = self.wpa_child.as_mut()
            && child.try_wait().is_ok_and(|exit| exit.is_some())
        {
            self.wpa_child = None;
            self.events = None;
            self.state.error = Some("Wireless authentication service exited; restarting".into());
        }
        if self.wpa_child.is_none() {
            if let Err(error) = self.start_stack() {
                self.state.phase = "Unavailable".into();
                self.state.error = Some(error.to_string());
            }
            return;
        }
        if self.last_poll.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last_poll = Instant::now();
        if self.events.is_none()
            && let Ok(control) = Control::new()
            && control.send("ATTACH").is_ok_and(|r| r == "OK")
        {
            let _ = control.socket.set_nonblocking(true);
            self.events = Some(control);
        }
        let mut messages = Vec::new();
        if let Some(events) = &self.events {
            for _ in 0..32 {
                let mut bytes = [0u8; 4096];
                match events.socket.recv(&mut bytes) {
                    Ok(n) => messages.push(String::from_utf8_lossy(&bytes[..n]).into_owned()),
                    Err(_) => break,
                }
            }
        }
        for message in messages {
            if message.contains("WRONG_KEY") || message.contains("AUTH-REJECT") {
                self.fail_attempt("Authentication failed. Check the password and try again.");
            }
            if message.contains("CTRL-EVENT-SCAN-RESULTS") {
                self.refresh_scan();
            }
        }
        if self
            .scan_started
            .is_some_and(|at| at.elapsed() > Duration::from_secs(12))
        {
            self.refresh_scan();
        }
        let current = match wpa("STATUS") {
            Ok(text) => pairs(&text),
            Err(_) => {
                self.state.phase = "Starting".into();
                return;
            }
        };
        let associated = current.get("wpa_state").is_some_and(|s| s == "COMPLETED");
        self.state.ssid = current.get("ssid").map(|s| display_ssid(&decode_ssid(s)));
        if associated {
            if self.state.message.starts_with("Starting") {
                self.state.message = "Connected using saved authentication".into();
            }
            if self.attempt.as_ref().is_some_and(|a| {
                current.get("id").and_then(|id| id.parse::<i32>().ok()) == Some(a.id)
            }) {
                let attempt = self.attempt.take().unwrap();
                if attempt.temporary {
                    for old in self.saved() {
                        if old.id != attempt.id && old.ssid_hex == attempt.ssid_hex {
                            let _ = ok(&format!("REMOVE_NETWORK {}", old.id));
                        }
                    }
                }
                let saved = ok("ENABLE_NETWORK all").and_then(|_| ok("SAVE_CONFIG"));
                match saved {
                    Ok(()) => {
                        self.state.message = "Authenticated; network saved for future boots".into()
                    }
                    Err(_) => {
                        self.state.error = Some("Connected, but saving the network failed".into())
                    }
                };
                self.state.busy = false;
            }
            if self
                .dhcp
                .as_mut()
                .is_some_and(|child| child.try_wait().is_ok_and(|exit| exit.is_some()))
            {
                self.dhcp = None;
            }
            if self.dhcp.is_none()
                && self
                    .last_dhcp
                    .is_none_or(|at| at.elapsed() > Duration::from_secs(10))
            {
                self.last_dhcp = Some(Instant::now());
                match Command::new("/sbin/udhcpc")
                    .process_group(0)
                    .args([
                        "-f",
                        "-i",
                        IFACE,
                        "-s",
                        "/usr/sbin/agx-dhcp",
                        "-p",
                        "/run/agx/udhcpc.pid",
                        "-t",
                        "3",
                        "-T",
                        "3",
                        "-A",
                        "10",
                    ])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    Ok(child) => self.dhcp = Some(child),
                    Err(_) => self.state.error = Some("DHCP client could not start".into()),
                }
            }
            self.state.signal_dbm = wpa("SIGNAL_POLL")
                .ok()
                .and_then(|text| pairs(&text).get("RSSI").and_then(|v| v.parse().ok()));
        } else if self.dhcp.is_some() {
            self.stop_dhcp();
        }
        if self
            .attempt
            .as_ref()
            .is_some_and(|a| a.started.elapsed() > Duration::from_secs(45))
        {
            self.fail_attempt(
                "Connection timed out. Check the password, network range, and radio region.",
            );
        }
        self.state.ipv4 = agx_core::Sampler::default()
            .collect()
            .ok()
            .and_then(|s| s.networks.into_iter().find(|n| n.name == IFACE))
            .map(|n| n.ipv4)
            .unwrap_or_default();
        let lease = fs::read(format!("{RUN}/lease.json"))
            .ok()
            .and_then(|data| serde_json::from_slice::<Lease>(&data).ok());
        self.state.gateway = lease.as_ref().and_then(|l| l.gateway.clone());
        self.state.dns = lease.map(|l| l.dns).unwrap_or_default();
        self.state.phase = if self.state.busy {
            "Authenticating"
        } else if associated && !self.state.ipv4.is_empty() {
            "Connected"
        } else if associated {
            "Obtaining IP"
        } else if self.disconnected {
            "Disconnected"
        } else {
            "Searching"
        }
        .into();
        self.state.saved = self.saved();
        if !self.state.ipv4.is_empty()
            && !self.state.time_synced
            && self.ntp.is_none()
            && self
                .last_ntp
                .is_none_or(|at| at.elapsed() > Duration::from_secs(30))
        {
            self.last_ntp = Some(Instant::now());
            self.ntp = Command::new("/usr/sbin/ntpd")
                .args(["-n", "-q", "-p", "pool.ntp.org"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .ok();
        }
        if let Some(ntp) = self.ntp.as_mut()
            && let Ok(Some(status)) = ntp.try_wait()
        {
            self.state.time_synced = status.success();
            self.ntp = None;
        }
        if let Some(diag) = self.diagnostic.as_mut()
            && let Ok(Some(exit)) = diag.child.try_wait()
        {
            let mut code = String::new();
            if let Some(mut output) = diag.child.stdout.take() {
                let _ = output.read_to_string(&mut code);
            }
            self.state.diagnostics = if exit.success() {
                format!("DNS and HTTPS reachable (HTTP {})", code.trim())
            } else {
                match exit.code() {
                    Some(6) => "DNS lookup failed".into(),
                    Some(60) => "TLS validation failed; check the clock and CA certificates".into(),
                    _ => "HTTPS connection failed or timed out".into(),
                }
            };
            self.diagnostic = None;
        }
    }
    fn refresh_scan(&mut self) {
        if let Ok(text) = wpa("SCAN_RESULTS") {
            self.state.access_points = parse_scan(&text);
        }
        if self.scan_started.is_some() && !self.state.busy {
            self.state.message = format!(
                "{} networks found. Choose a network to authenticate.",
                self.state.access_points.len()
            );
        }
        self.scan_started = None;
        self.state.scanning = false;
    }
}
impl Drop for Manager {
    fn drop(&mut self) {
        self.stop_dhcp();
        for child in [&mut self.dhcp, &mut self.ntp, &mut self.wpa_child]
            .into_iter()
            .flatten()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(diag) = self.diagnostic.as_mut() {
            let _ = diag.child.kill();
            let _ = diag.child.wait();
        }
    }
}
fn github_release_message(data: &[u8]) -> io::Result<String> {
    let releases: serde_json::Value =
        serde_json::from_slice(data).map_err(|_| err("GitHub returned invalid release data."))?;
    let releases = releases
        .as_array()
        .ok_or_else(|| err("GitHub returned invalid release data."))?;
    let release = releases
        .iter()
        .filter(|v| v["draft"] == false)
        .filter_map(|v| {
            let tag = v["tag_name"].as_str()?;
            if tag.len() > 64
                || !tag
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
            {
                return None;
            }
            let version = semver::Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()?;
            Some((version, v, tag))
        })
        .max_by(|a, b| a.0.cmp(&b.0));
    let Some((_, release, tag)) = release else {
        return Ok("No published versioned AGX releases were found.".into());
    };
    let asset = release["assets"].as_array().is_some_and(|assets| {
        assets.iter().any(|asset| {
            asset["name"] == "agx-system-update.tar.gz" && asset["state"] == "uploaded"
        })
    });
    let stage = if release["prerelease"] == true {
        "preview"
    } else {
        "release"
    };
    let package = if asset {
        "update package available"
    } else {
        "no AGX package attached"
    };
    Ok(format!(
        "GitHub: {tag} ({stage}); {package}. Installed system version is not tracked yet."
    ))
}
fn github_curl_config(token: Option<&str>) -> io::Result<Zeroizing<String>> {
    let Some(token) = token else {
        return Ok(Zeroizing::new(String::new()));
    };
    if token.len() < 20
        || token.len() > 255
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(err(
            "Stored GitHub token is invalid. Press G to replace or remove it.",
        ));
    }
    // curl reads the optional Authorization header through private stdin, never argv or env.
    Ok(Zeroizing::new(format!(
        "header = \"Authorization: Bearer {}\"\n",
        token
    )))
}
fn check_github_release() -> io::Result<String> {
    let token = match fs::read_to_string("/etc/agx/github-token") {
        Ok(token) => Some(Zeroizing::new(token)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(_) => {
            return Err(err(
                "Unable to read stored GitHub token. Press G to replace or remove it.",
            ));
        }
    };
    let config = github_curl_config(token.as_ref().map(|token| token.as_str()))?;
    let mut child = Command::new("/usr/bin/curl")
        .args([
            "--silent",
            "--show-error",
            "--max-time",
            "6",
            "--max-filesize",
            "1048576",
            "--user-agent",
            "AGX-OS",
            "--header",
            "Accept: application/vnd.github+json",
            "--header",
            "X-GitHub-Api-Version: 2022-11-28",
            "--write-out",
            "\n%{http_code}",
            "--config",
            "-",
            "https://api.github.com/repos/danaia/AGX/releases?per_page=100",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or_else(|| err("Unable to start GitHub check."))?
        .write_all(config.as_bytes())?;
    let output = child.wait_with_output()?;
    if !output.status.success() || output.stdout.len() > 1048576 {
        return Err(err(
            "GitHub check failed. Check Wi-Fi, DNS, clock, and HTTPS connectivity.",
        ));
    }
    let index = output
        .stdout
        .iter()
        .rposition(|b| *b == b'\n')
        .ok_or_else(|| err("Invalid response from GitHub."))?;
    let (body, code) = (&output.stdout[..index], &output.stdout[index + 1..]);
    match code {
        b"200" => github_release_message(body),
        b"401" => Err(err(
            "GitHub rejected the token. Press G to replace or remove it.",
        )),
        b"403" => Err(err("GitHub denied the request or rate limit was reached.")),
        b"404" => Err(err("GitHub repository or release endpoint was not found.")),
        _ => Err(err("GitHub release check failed; retry later.")),
    }
}

fn valid_country(country: &str) -> bool {
    country.len() == 2 && country.bytes().all(|b| b.is_ascii_uppercase())
}

pub fn run() -> io::Result<()> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(err("Network service requires root"));
    }
    unsafe { libc::umask(0o077) };
    private_dir(RUN)?;
    let mut lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(format!("{RUN}/network.pid"))?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(err("Network service is already running"));
    }
    lock.set_len(0)?;
    writeln!(lock, "{}", std::process::id())?;
    if Path::new(SOCKET).exists() {
        fs::remove_file(SOCKET)?;
    }
    let listener = UnixListener::bind(SOCKET)?;
    agx_profile::service::prepare_access(SOCKET)?;
    listener.set_nonblocking(true)?;
    let stop = Arc::new(AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
        signal_hook::flag::register(signal, Arc::clone(&stop))?;
    }
    let mut manager = Manager::new()?;
    while !stop.load(Ordering::Relaxed) {
        manager.tick();
        match listener.accept() {
            Ok((mut connection, _)) => {
                let _ = serve(&mut manager, &mut connection);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100))
            }
            Err(error) => return Err(error),
        }
    }
    let _ = fs::remove_file(SOCKET);
    Ok(())
}
fn serve(manager: &mut Manager, connection: &mut UnixStream) -> io::Result<()> {
    connection.set_read_timeout(Some(Duration::from_secs(2)))?;
    connection.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut bytes = Zeroizing::new(Vec::new());
    BufReader::new(&mut *connection)
        .take(MAX_REQUEST as u64 + 1)
        .read_until(b'\n', &mut bytes)?;
    let result = if bytes.len() > MAX_REQUEST {
        Err(err("Network request is too large"))
    } else {
        serde_json::from_slice::<Request>(&bytes)
            .map_err(|_| err("Invalid network request"))
            .and_then(|request| manager.handle(request))
    };
    let (ok, message) = match result {
        Ok(message) => (true, message),
        Err(error) => (false, error.to_string()),
    };
    let reply = Reply {
        ok,
        message,
        status: manager.state.clone(),
        profile: agx_profile::service::status().ok(),
    };
    serde_json::to_writer(&mut *connection, &reply)?;
    connection.write_all(b"\n")
}

#[derive(Serialize, Deserialize)]
struct Lease {
    ip: String,
    gateway: Option<String>,
    dns: Vec<String>,
}
fn update_dns(dns: &[String]) -> io::Result<()> {
    let previous = fs::read_to_string("/etc/resolv.conf").unwrap_or_default();
    let mut lines: Vec<String> = previous
        .lines()
        .filter(|line| !line.ends_with("# agx-wlan0"))
        .map(str::to_owned)
        .collect();
    for address in dns {
        lines.push(format!("nameserver {address} # agx-wlan0"));
    }
    fs::write("/etc/resolv.conf", format!("{}\n", lines.join("\n")))?;
    fs::set_permissions("/etc/resolv.conf", fs::Permissions::from_mode(0o644))
}
pub fn dhcp_hook() -> io::Result<()> {
    if std::env::var("interface").as_deref() != Ok(IFACE) {
        return Err(err("DHCP hook accepts wlan0 only"));
    }
    match std::env::args().nth(1).as_deref() {
        Some("deconfig") => {
            command("/sbin/ip", &["addr", "flush", "dev", IFACE])?;
            let _ = command("/sbin/ip", &["route", "flush", "dev", IFACE]);
            let _ = fs::remove_file(format!("{RUN}/lease.json"));
            update_dns(&[])?;
        }
        Some("bound" | "renew") => {
            let ip: Ipv4Addr = std::env::var("ip")
                .unwrap_or_default()
                .parse()
                .map_err(|_| err("Invalid DHCP address"))?;
            let mask: Ipv4Addr = std::env::var("subnet")
                .unwrap_or_else(|_| "255.255.255.0".into())
                .parse()
                .map_err(|_| err("Invalid DHCP netmask"))?;
            let mask = u32::from(mask);
            let prefix = mask.leading_ones();
            if mask != u32::MAX.checked_shl(32 - prefix).unwrap_or(0) {
                return Err(err("Non-contiguous DHCP netmask"));
            }
            let gateway = std::env::var("router")
                .unwrap_or_default()
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<Ipv4Addr>().ok())
                .map(|s| s.to_string());
            let dns = std::env::var("dns")
                .unwrap_or_default()
                .split_whitespace()
                .filter_map(|s| s.parse::<Ipv4Addr>().ok())
                .take(4)
                .map(|s| s.to_string())
                .collect::<Vec<_>>();
            command(
                "/sbin/ip",
                &["addr", "replace", &format!("{ip}/{prefix}"), "dev", IFACE],
            )?;
            if let Some(gateway) = &gateway {
                command(
                    "/sbin/ip",
                    &[
                        "route", "replace", "default", "via", gateway, "dev", IFACE, "metric",
                        "100",
                    ],
                )?;
            }
            update_dns(&dns)?;
            atomic_private(
                Path::new(&format!("{RUN}/lease.json")),
                &serde_json::to_vec(&Lease {
                    ip: ip.to_string(),
                    gateway,
                    dns,
                })?,
            )?;
        }
        _ => (),
    }
    Ok(())
}

#[cfg(test)]
mod github_tests {
    use super::{github_curl_config, github_release_message};
    #[test]
    fn public_checks_need_no_authorization_and_optional_tokens_are_validated() {
        assert!(github_curl_config(None).unwrap().is_empty());
        let token = "github_pat_example_read_only_token";
        assert_eq!(
            github_curl_config(Some(token)).unwrap().as_str(),
            format!("header = \"Authorization: Bearer {token}\"\n")
        );
        for invalid in [
            "",
            "short",
            "github_pat_example\nurl = https://example.com",
            "github_pat_example\"injected",
        ] {
            assert!(github_curl_config(Some(invalid)).is_err());
        }
    }

    #[test]
    fn reports_release_assets_and_rejects_untrusted_tags() {
        let data=br#"[{"draft":false,"prerelease":true,"tag_name":"v0.4.0-preview.1","assets":[{"name":"agx-system-update.tar.gz","state":"uploaded"}]}]"#;
        let message = github_release_message(data).unwrap();
        assert!(
            message.contains("v0.4.0-preview.1")
                && message.contains("preview")
                && message.contains("package available")
        );
        assert!(
            github_release_message(br#"[]"#)
                .unwrap()
                .contains("No published")
        );
        assert!(
            github_release_message(br#"[{"draft":false,"tag_name":"bad\u001b[2J","assets":[]}]"#)
                .unwrap()
                .contains("No published")
        );
        assert!(github_release_message(b"not json").is_err());
        let ordered = br#"[{"draft":false,"tag_name":"v0.4.0","assets":[]},{"draft":false,"tag_name":"v0.6.0","assets":[]},{"draft":false,"tag_name":"v0.5.0","assets":[]}]"#;
        assert!(github_release_message(ordered).unwrap().contains("v0.6.0"));
    }
}
