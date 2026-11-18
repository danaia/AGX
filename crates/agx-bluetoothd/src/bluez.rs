//! The BlueZ implementation of `Backend`, using BlueR over D-Bus.
//!
//! State is event-driven: one task follows the adapter and one follows each device, so reading
//! the device list costs no D-Bus traffic and the radio is never polled.
use crate::{
    backend::{Backend, Error, Result},
    prompts::Prompter,
    rfkill,
};
use agx_bluetooth::{Adapter, Category, Device, PromptKind};
use bluer::{
    Address, AdapterEvent, AdapterProperty, DeviceEvent, DeviceProperty, ErrorKind,
    agent::{Agent, AgentHandle, ReqError, ReqResult},
};
use futures_util::StreamExt;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::task::JoinHandle;

/// What BlueZ reports for one device, kept raw so a changed property can recompute the rest.
#[derive(Default)]
struct Entry {
    device: Device,
    alias: String,
    name: Option<String>,
    icon: Option<String>,
    class: Option<u32>,
    appearance: Option<u16>,
}
impl Entry {
    fn recompute(&mut self) {
        // BlueZ's alias of an unnamed device is its address with dashes; treat that as no name.
        let placeholder = self.device.address.replace(':', "-");
        self.device.name = [Some(&self.alias), self.name.as_ref()]
            .into_iter()
            .flatten()
            .find(|name| !name.is_empty() && **name != placeholder)
            .cloned()
            .unwrap_or_default();
        self.device.category = self
            .icon
            .as_deref()
            .and_then(Category::from_icon)
            .or_else(|| self.class.and_then(Category::from_class))
            .or_else(|| self.appearance.and_then(Category::from_appearance))
            .unwrap_or_default();
    }
    fn apply(&mut self, property: DeviceProperty) {
        match property {
            DeviceProperty::Alias(value) => self.alias = value,
            DeviceProperty::Name(value) => self.name = Some(value),
            DeviceProperty::Icon(value) => self.icon = Some(value),
            DeviceProperty::Class(value) => self.class = Some(value),
            DeviceProperty::Appearance(value) => self.appearance = Some(value),
            DeviceProperty::Paired(value) => self.device.paired = value,
            DeviceProperty::Connected(value) => self.device.connected = value,
            DeviceProperty::Trusted(value) => self.device.trusted = value,
            DeviceProperty::Rssi(value) => self.device.rssi = Some(value),
            DeviceProperty::BatteryPercentage(value) => self.device.battery = Some(value),
            _ => return,
        }
        self.recompute();
    }
}

#[derive(Default)]
struct Cache {
    name: Option<String>,
    adapter: Option<Adapter>,
    devices: HashMap<Address, Entry>,
}

struct Shared {
    session: bluer::Session,
    cache: Mutex<Cache>,
    scan: tokio::sync::Mutex<Option<JoinHandle<()>>>,
}

/// Aborts every per-device task when the adapter is lost or re-synchronized.
#[derive(Default)]
struct Tracked(HashMap<Address, JoinHandle<()>>);
impl Drop for Tracked {
    fn drop(&mut self) {
        self.0.values().for_each(JoinHandle::abort);
    }
}

pub struct Bluez {
    shared: Arc<Shared>,
    _agent: AgentHandle,
}

/// Word BlueZ's errors for the person using the desktop.
fn describe(error: &bluer::Error) -> Error {
    let text = match &error.kind {
        ErrorKind::AuthenticationFailed => "Authentication failed. Check the code or PIN and try again.".to_owned(),
        ErrorKind::AuthenticationRejected => "The pairing request was rejected.".to_owned(),
        ErrorKind::AuthenticationCanceled => "Pairing was canceled.".to_owned(),
        ErrorKind::AuthenticationTimeout => "Pairing timed out. Put the device in pairing mode and try again.".to_owned(),
        ErrorKind::ConnectionAttemptFailed => "Could not connect. Is the device on and in range?".to_owned(),
        ErrorKind::InProgress => "Bluetooth is busy with another operation. Try again in a moment.".to_owned(),
        ErrorKind::NotReady => "Bluetooth is not ready. Turn it on and try again.".to_owned(),
        ErrorKind::DoesNotExist | ErrorKind::NotFound => "That device is no longer known. Search again.".to_owned(),
        ErrorKind::AlreadyExists => "That device is already paired.".to_owned(),
        _ if error.message.to_lowercase().contains("rfkill") => "Bluetooth is blocked by a software or hardware switch.".to_owned(),
        _ if error.message.to_lowercase().contains("serviceunknown") || error.message.to_lowercase().contains("not provided") => {
            "Bluetooth service is not running.".to_owned()
        }
        _ if error.message.is_empty() => format!("Bluetooth error: {:?}", error.kind),
        _ => format!("Bluetooth error: {}", error.message),
    };
    Error(text)
}
fn lift<T>(result: bluer::Result<T>) -> Result<T> {
    result.map_err(|error| describe(&error))
}
fn address(text: &str) -> Result<Address> {
    text.parse().map_err(|_| Error::new("That is not a Bluetooth address."))
}

impl Shared {
    fn adapter(&self) -> Result<bluer::Adapter> {
        let name = self.cache.lock().unwrap().name.clone().ok_or_else(|| Error::new("No Bluetooth adapter found."))?;
        lift(self.session.adapter(&name))
    }
    fn name_of(&self, address: Address) -> String {
        let cache = self.cache.lock().unwrap();
        cache.devices.get(&address).map(|entry| entry.device.name.clone()).filter(|name| !name.is_empty()).unwrap_or_else(|| address.to_string())
    }
}

async fn read_device(device: &bluer::Device) -> bluer::Result<Entry> {
    let (alias, name, icon, class, appearance, paired, connected, trusted, rssi, battery) = tokio::join!(
        device.alias(),
        device.name(),
        device.icon(),
        device.class(),
        device.appearance(),
        device.is_paired(),
        device.is_connected(),
        device.is_trusted(),
        device.rssi(),
        device.battery_percentage(),
    );
    let mut entry = Entry {
        device: Device {
            address: device.address().to_string(),
            paired: paired?,
            connected: connected?,
            trusted: trusted?,
            rssi: rssi.ok().flatten(),
            battery: battery.ok().flatten(),
            ..Device::default()
        },
        alias: alias?,
        name: name.ok().flatten(),
        icon: icon.ok().flatten(),
        class: class.ok().flatten(),
        appearance: appearance.ok().flatten(),
    };
    entry.recompute();
    Ok(entry)
}

/// Follow one device until BlueZ drops it.
async fn track(shared: Arc<Shared>, adapter: bluer::Adapter, address: Address) {
    let Ok(device) = adapter.device(address) else { return };
    // Subscribe first so a change between the read and the subscription is not lost.
    let Ok(mut events) = device.events().await else { return };
    let Ok(entry) = read_device(&device).await else { return };
    shared.cache.lock().unwrap().devices.insert(address, entry);
    while let Some(DeviceEvent::PropertyChanged(property)) = events.next().await {
        if let Some(entry) = shared.cache.lock().unwrap().devices.get_mut(&address) {
            entry.apply(property);
        }
    }
    shared.cache.lock().unwrap().devices.remove(&address);
}

async fn adapter_info(adapter: &bluer::Adapter) -> bluer::Result<Adapter> {
    let (alias, address, powered, discovering) =
        tokio::join!(adapter.alias(), adapter.address(), adapter.is_powered(), adapter.is_discovering());
    Ok(Adapter { name: alias?, address: address?.to_string(), powered: powered?, scanning: discovering? })
}

/// Follow the first adapter until it goes away (unplugged, or bluetoothd restarted).
async fn follow(shared: &Arc<Shared>) -> bluer::Result<()> {
    let names = shared.session.adapter_names().await?;
    let Some(name) = names.into_iter().next() else {
        tokio::time::sleep(Duration::from_secs(3)).await;
        return Ok(());
    };
    let adapter = shared.session.adapter(&name)?;
    let mut events = adapter.events().await?;
    let info = adapter_info(&adapter).await?;
    {
        let mut cache = shared.cache.lock().unwrap();
        cache.name = Some(name);
        cache.adapter = Some(info);
    }
    let mut tracked = Tracked::default();
    for address in adapter.device_addresses().await? {
        tracked.0.insert(address, tokio::spawn(track(shared.clone(), adapter.clone(), address)));
    }
    let mut liveness = tokio::time::interval(Duration::from_secs(5));
    loop {
        tokio::select! {
            event = events.next() => match event {
                Some(AdapterEvent::DeviceAdded(address)) => {
                    tracked.0.entry(address).or_insert_with(|| tokio::spawn(track(shared.clone(), adapter.clone(), address)));
                }
                Some(AdapterEvent::DeviceRemoved(address)) => {
                    if let Some(task) = tracked.0.remove(&address) {
                        task.abort();
                    }
                    shared.cache.lock().unwrap().devices.remove(&address);
                }
                Some(AdapterEvent::PropertyChanged(property)) => {
                    if let Some(info) = shared.cache.lock().unwrap().adapter.as_mut() {
                        match property {
                            AdapterProperty::Powered(value) => info.powered = value,
                            AdapterProperty::Discovering(value) => info.scanning = value,
                            AdapterProperty::Alias(value) => info.name = value,
                            _ => {}
                        }
                    }
                }
                None => return Ok(()),
            },
            // The event stream can stay silent when bluetoothd dies; a cheap read notices.
            _ = liveness.tick() => { adapter.is_powered().await?; }
        }
    }
}

async fn watch(shared: Arc<Shared>) {
    loop {
        if let Err(error) = follow(&shared).await {
            eprintln!("agx-bluetoothd: lost the adapter: {error}");
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
        let mut cache = shared.cache.lock().unwrap();
        cache.adapter = None;
        cache.name = None;
        cache.devices.clear();
    }
}

fn agent(shared: Arc<Shared>, prompter: Arc<Prompter>) -> Agent {
    // Each callback asks the person at the desktop; silence or refusal rejects the attempt.
    let ask = |kind: PromptKind| {
        let (shared, prompter) = (shared.clone(), prompter.clone());
        move |device: Address, code: Option<String>| {
            let (shared, prompter) = (shared.clone(), prompter.clone());
            async move {
                let name = shared.name_of(device);
                prompter.ask(kind, &device.to_string(), &name, code).await
            }
        }
    };
    let (confirm, authorize, pin, passkey) =
        (ask(PromptKind::Confirm), ask(PromptKind::Authorize), ask(PromptKind::Pin), ask(PromptKind::Passkey));
    let (authorize_service, authorize_incoming) = (authorize.clone(), authorize);
    let (show_shared, show_prompter) = (shared.clone(), prompter.clone());
    let show = move |device: Address, code: String, ended: tokio::sync::oneshot::Receiver<()>| {
        let (shared, prompter) = (show_shared.clone(), show_prompter.clone());
        async move {
            let name = shared.name_of(device);
            prompter.show(PromptKind::Display, &device.to_string(), &name, code, async { let _ = ended.await; }).await;
        }
    };
    let show_pin = show.clone();
    Agent {
        request_default: true,
        request_confirmation: Some(Box::new(move |request| {
            let answer = confirm(request.device, Some(format!("{:06}", request.passkey)));
            Box::pin(async move { if answer.await.accept { Ok(()) } else { Err(ReqError::Rejected) } })
        })),
        request_authorization: Some(Box::new(move |request| {
            let answer = authorize_incoming(request.device, None);
            Box::pin(async move { if answer.await.accept { Ok(()) } else { Err(ReqError::Rejected) } })
        })),
        authorize_service: Some(Box::new(move |request| {
            let answer = authorize_service(request.device, None);
            Box::pin(async move { if answer.await.accept { Ok(()) } else { Err(ReqError::Rejected) } })
        })),
        request_pin_code: Some(Box::new(move |request| {
            let answer = pin(request.device, None);
            Box::pin(async move {
                let answer = answer.await;
                answer.value.filter(|_| answer.accept).ok_or(ReqError::Rejected)
            })
        })),
        request_passkey: Some(Box::new(move |request| {
            let answer = passkey(request.device, None);
            Box::pin(async move {
                let answer = answer.await;
                answer.value.filter(|_| answer.accept).and_then(|text| text.parse().ok()).ok_or(ReqError::Rejected)
            })
        })),
        display_pin_code: Some(Box::new(move |request| {
            let shown = show_pin(request.device, request.pincode, request.cancel);
            Box::pin(async move { shown.await; ReqResult::Ok(()) })
        })),
        display_passkey: Some(Box::new(move |request| {
            let shown = show(request.device, format!("{:06}", request.passkey), request.cancel);
            Box::pin(async move { shown.await; ReqResult::Ok(()) })
        })),
        ..Agent::default()
    }
}

impl Bluez {
    pub async fn start(prompter: Arc<Prompter>) -> Result<Self> {
        let session = bluer::Session::new().await.map_err(|error| {
            Error(format!("Cannot reach the system bus ({error}). Is dbus running?"))
        })?;
        let shared = Arc::new(Shared { session, cache: Mutex::default(), scan: tokio::sync::Mutex::new(None) });
        let handle = lift(shared.session.register_agent(agent(shared.clone(), prompter)).await)?;
        tokio::spawn(watch(shared.clone()));
        Ok(Self { shared, _agent: handle })
    }
}

impl Backend for Bluez {
    async fn adapter(&self) -> Result<Option<Adapter>> {
        Ok(self.shared.cache.lock().unwrap().adapter.clone())
    }
    async fn devices(&self) -> Result<Vec<Device>> {
        Ok(self.shared.cache.lock().unwrap().devices.values().map(|entry| entry.device.clone()).collect())
    }
    async fn set_powered(&self, on: bool) -> Result<()> {
        let adapter = self.shared.adapter()?;
        if on {
            if let Err(error) = adapter.set_powered(true).await {
                // Retry once after lifting an rfkill block; otherwise report what BlueZ said.
                if rfkill::unblock_bluetooth().is_err() {
                    return Err(describe(&error));
                }
                tokio::time::sleep(Duration::from_millis(300)).await;
                lift(adapter.set_powered(true).await)?;
            }
            Ok(())
        } else {
            lift(adapter.set_powered(false).await)
        }
    }
    async fn start_scan(&self) -> Result<()> {
        let adapter = self.shared.adapter()?;
        let mut scan = self.shared.scan.lock().await;
        if scan.as_ref().is_some_and(|task| !task.is_finished()) {
            return Ok(());
        }
        let stream = lift(adapter.discover_devices().await)?;
        // Holding the stream holds BlueZ's discovery session; aborting the task ends it.
        *scan = Some(tokio::spawn(async move {
            let mut stream = Box::pin(stream);
            while stream.next().await.is_some() {}
        }));
        Ok(())
    }
    async fn stop_scan(&self) -> Result<()> {
        if let Some(task) = self.shared.scan.lock().await.take() {
            task.abort();
        }
        Ok(())
    }
    async fn pair(&self, device: &str) -> Result<()> {
        let device = lift(self.shared.adapter()?.device(address(device)?))?;
        lift(device.pair().await)
    }
    async fn connect(&self, device: &str) -> Result<()> {
        let device = lift(self.shared.adapter()?.device(address(device)?))?;
        lift(device.connect().await)
    }
    async fn disconnect(&self, device: &str) -> Result<()> {
        let device = lift(self.shared.adapter()?.device(address(device)?))?;
        lift(device.disconnect().await)
    }
    async fn set_trusted(&self, device: &str, trusted: bool) -> Result<()> {
        let device = lift(self.shared.adapter()?.device(address(device)?))?;
        lift(device.set_trusted(trusted).await)
    }
    async fn forget(&self, device: &str) -> Result<()> {
        lift(self.shared.adapter()?.remove_device(address(device)?).await)
    }
}
