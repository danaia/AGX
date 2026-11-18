//! Policy tests run against a fake stack, so they need no radio.
use crate::{
    backend::{Backend, Error, Result},
    core::Core,
    peer::{Peer, classify},
    prompts::{Answer, Prompter},
};
use agx_bluetooth::{Adapter, ApprovalAction, Category, Device, PromptKind, Request};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

const KEYBOARD: &str = "AA:BB:CC:00:00:01";
const SPEAKER: &str = "AA:BB:CC:00:00:02";
const STRANGER: &str = "AA:BB:CC:00:00:03";

#[derive(Default)]
struct Inner {
    adapter: Option<Adapter>,
    devices: Vec<Device>,
    calls: Vec<String>,
    pair_error: Option<&'static str>,
    connect_error: Option<&'static str>,
}
#[derive(Clone, Default)]
struct Fake(Arc<Mutex<Inner>>);
impl Fake {
    fn calls(&self) -> Vec<String> {
        self.0.lock().unwrap().calls.clone()
    }
    fn record(&self, call: String) {
        self.0.lock().unwrap().calls.push(call);
    }
    fn update(&self, address: &str, change: impl FnOnce(&mut Device)) {
        let mut inner = self.0.lock().unwrap();
        change(
            inner
                .devices
                .iter_mut()
                .find(|d| d.address == address)
                .unwrap(),
        );
    }
}
impl Backend for Fake {
    async fn adapter(&self) -> Result<Option<Adapter>> {
        Ok(self.0.lock().unwrap().adapter.clone())
    }
    async fn devices(&self) -> Result<Vec<Device>> {
        Ok(self.0.lock().unwrap().devices.clone())
    }
    async fn set_powered(&self, on: bool) -> Result<()> {
        self.record(format!("power {on}"));
        self.0.lock().unwrap().adapter.as_mut().unwrap().powered = on;
        Ok(())
    }
    async fn start_scan(&self) -> Result<()> {
        self.record("scan on".into());
        self.0.lock().unwrap().adapter.as_mut().unwrap().scanning = true;
        Ok(())
    }
    async fn stop_scan(&self) -> Result<()> {
        self.record("scan off".into());
        if let Some(adapter) = self.0.lock().unwrap().adapter.as_mut() {
            adapter.scanning = false;
        }
        Ok(())
    }
    async fn pair(&self, address: &str) -> Result<()> {
        self.record(format!("pair {address}"));
        if let Some(error) = self.0.lock().unwrap().pair_error {
            return Err(Error::new(error));
        }
        self.update(address, |d| d.paired = true);
        Ok(())
    }
    async fn connect(&self, address: &str) -> Result<()> {
        self.record(format!("connect {address}"));
        if let Some(error) = self.0.lock().unwrap().connect_error {
            return Err(Error::new(error));
        }
        self.update(address, |d| d.connected = true);
        Ok(())
    }
    async fn disconnect(&self, address: &str) -> Result<()> {
        self.record(format!("disconnect {address}"));
        self.update(address, |d| d.connected = false);
        Ok(())
    }
    async fn set_trusted(&self, address: &str, trusted: bool) -> Result<()> {
        self.record(format!("trust {address} {trusted}"));
        self.update(address, |d| d.trusted = trusted);
        Ok(())
    }
    async fn forget(&self, address: &str) -> Result<()> {
        self.record(format!("forget {address}"));
        self.0
            .lock()
            .unwrap()
            .devices
            .retain(|d| d.address != address);
        Ok(())
    }
}

fn device(address: &str, name: &str, rssi: Option<i16>) -> Device {
    Device {
        address: address.into(),
        name: name.into(),
        rssi,
        ..Device::default()
    }
}
fn setup() -> (Arc<Core<Fake>>, Fake, Arc<Prompter>) {
    let fake = Fake::default();
    {
        let mut inner = fake.0.lock().unwrap();
        inner.adapter = Some(Adapter {
            name: "angi".into(),
            address: "00:11:22:33:44:55".into(),
            powered: true,
            scanning: false,
        });
        let mut speaker = device(SPEAKER, "Desk speaker", Some(-60));
        speaker.paired = true;
        speaker.category = Category::Audio;
        inner.devices = vec![
            device(KEYBOARD, "Keyboard", Some(-50)),
            speaker,
            device(STRANGER, "Stranger", Some(-80)),
        ];
    }
    let prompter = Arc::new(Prompter::default());
    (Core::new(fake.clone(), prompter.clone()), fake, prompter)
}
async fn settle() {
    tokio::time::sleep(Duration::from_millis(50)).await;
}
fn pair(address: &str) -> Request {
    Request::Pair {
        address: address.into(),
    }
}

#[tokio::test(start_paused = true)]
async fn desktop_pairing_pairs_trusts_then_connects() {
    let (core, fake, _) = setup();
    let reply = core.handle(pair(KEYBOARD), Peer::Desktop).await;
    assert!(reply.ok, "{}", reply.message);
    settle().await;
    assert_eq!(
        fake.calls(),
        [
            format!("pair {KEYBOARD}"),
            format!("trust {KEYBOARD} true"),
            format!("connect {KEYBOARD}")
        ]
    );
    let status = core.handle(Request::Status, Peer::Desktop).await.status;
    assert_eq!(status.message, "Paired and connected to Keyboard");
    assert!(status.error.is_none());
    let keyboard = status
        .devices
        .iter()
        .find(|d| d.address == KEYBOARD)
        .unwrap();
    assert!(keyboard.paired && keyboard.trusted && keyboard.connected && !keyboard.busy);
}

#[tokio::test(start_paused = true)]
async fn a_paired_device_that_will_not_connect_is_still_reported_as_paired() {
    let (core, fake, _) = setup();
    fake.0.lock().unwrap().connect_error = Some("Page timeout");
    core.handle(pair(KEYBOARD), Peer::Desktop).await;
    settle().await;
    let status = core.status();
    assert_eq!(
        status.message,
        "Paired with Keyboard, but it did not connect: Page timeout"
    );
    assert!(status.error.is_none());
}

#[tokio::test(start_paused = true)]
async fn a_failed_pairing_reports_the_error_and_never_trusts() {
    let (core, fake, _) = setup();
    fake.0.lock().unwrap().pair_error = Some("Authentication failed");
    core.handle(pair(KEYBOARD), Peer::Desktop).await;
    settle().await;
    assert_eq!(
        core.status().error.as_deref(),
        Some("Authentication failed")
    );
    assert_eq!(fake.calls(), [format!("pair {KEYBOARD}")]);
    assert!(core.status().devices.iter().all(|d| !d.busy));
}

#[tokio::test(start_paused = true)]
async fn agent_pairing_waits_for_the_person_and_runs_only_when_allowed() {
    let (core, fake, _) = setup();
    let reply = core.handle(pair(KEYBOARD), Peer::Other).await;
    assert!(
        reply.ok && reply.message.starts_with("Waiting for approval"),
        "{}",
        reply.message
    );
    assert!(fake.calls().is_empty(), "nothing happens before approval");
    let approvals = reply.status.approvals;
    assert_eq!(approvals.len(), 1);
    assert_eq!(
        (approvals[0].action, approvals[0].name.as_str()),
        (ApprovalAction::Pair, "Keyboard")
    );
    // Asking again does not stack duplicates.
    assert_eq!(
        core.handle(pair(KEYBOARD), Peer::Other)
            .await
            .status
            .approvals
            .len(),
        1
    );
    // The agent cannot approve its own request.
    let id = approvals[0].id;
    assert!(
        !core
            .handle(Request::Decide { id, allow: true }, Peer::Other)
            .await
            .ok
    );
    assert!(fake.calls().is_empty());
    // The desktop can.
    assert!(
        core.handle(Request::Decide { id, allow: true }, Peer::Desktop)
            .await
            .ok
    );
    settle().await;
    assert_eq!(fake.calls()[0], format!("pair {KEYBOARD}"));
    assert!(core.status().approvals.is_empty());
    // A used approval cannot be replayed.
    assert!(
        !core
            .handle(Request::Decide { id, allow: true }, Peer::Desktop)
            .await
            .ok
    );
}

#[tokio::test(start_paused = true)]
async fn declining_an_approval_does_nothing() {
    let (core, fake, _) = setup();
    let id = core
        .handle(
            Request::Forget {
                address: SPEAKER.into(),
            },
            Peer::Other,
        )
        .await
        .status
        .approvals[0]
        .id;
    let reply = core
        .handle(Request::Decide { id, allow: false }, Peer::Desktop)
        .await;
    assert!(reply.ok && reply.status.approvals.is_empty());
    assert!(fake.calls().is_empty());
}

#[tokio::test(start_paused = true)]
async fn approvals_expire_and_are_bounded() {
    let (core, _, _) = setup();
    core.handle(
        Request::Trust {
            address: KEYBOARD.into(),
            trusted: true,
        },
        Peer::Other,
    )
    .await;
    assert_eq!(core.status().approvals.len(), 1);
    tokio::time::sleep(Duration::from_secs(61)).await;
    core.refresh().await;
    assert!(
        core.status().approvals.is_empty(),
        "a stale request must not wait forever"
    );
}

#[tokio::test(start_paused = true)]
async fn agents_get_routine_actions_but_not_risky_ones() {
    let (core, fake, _) = setup();
    // Connecting to something already paired, disconnecting and revoking trust are routine.
    assert!(
        core.handle(
            Request::Connect {
                address: SPEAKER.into()
            },
            Peer::Other
        )
        .await
        .ok
    );
    settle().await;
    assert!(
        core.handle(
            Request::Disconnect {
                address: SPEAKER.into()
            },
            Peer::Other
        )
        .await
        .ok
    );
    assert!(
        core.handle(
            Request::Trust {
                address: SPEAKER.into(),
                trusted: false
            },
            Peer::Other
        )
        .await
        .ok
    );
    assert_eq!(
        fake.calls(),
        [
            format!("connect {SPEAKER}"),
            format!("disconnect {SPEAKER}"),
            format!("trust {SPEAKER} false")
        ]
    );
    // Reaching a new device, trusting, forgetting, powering off and answering prompts are not.
    let before = fake.calls().len();
    for request in [
        Request::Connect {
            address: STRANGER.into(),
        },
        Request::Trust {
            address: SPEAKER.into(),
            trusted: true,
        },
        Request::Forget {
            address: SPEAKER.into(),
        },
    ] {
        let reply = core.handle(request, Peer::Other).await;
        assert!(reply.ok && reply.message.starts_with("Waiting for approval"));
    }
    assert!(
        !core
            .handle(Request::Power { on: false }, Peer::Other)
            .await
            .ok
    );
    assert!(
        !core
            .handle(
                Request::Respond {
                    id: 1,
                    accept: true,
                    value: None
                },
                Peer::Other
            )
            .await
            .ok
    );
    assert_eq!(fake.calls().len(), before);
    assert_eq!(core.status().approvals.len(), 3);
}

#[tokio::test(start_paused = true)]
async fn requests_are_validated_before_reaching_the_stack() {
    let (core, fake, _) = setup();
    for address in ["", "not-an-address", "AA:BB:CC:00:00", "AA:BB:CC:00:00:99"] {
        let reply = core.handle(pair(address), Peer::Desktop).await;
        assert!(!reply.ok, "{address:?}");
    }
    assert!(fake.calls().is_empty());
}

#[tokio::test(start_paused = true)]
async fn searching_needs_power_and_stops_by_itself() {
    let (core, fake, _) = setup();
    fake.0.lock().unwrap().adapter.as_mut().unwrap().powered = false;
    assert!(
        !core
            .handle(Request::Scan { on: true }, Peer::Other)
            .await
            .ok
    );
    assert!(
        core.handle(Request::Power { on: true }, Peer::Other)
            .await
            .ok
    );
    let reply = core.handle(Request::Scan { on: true }, Peer::Other).await;
    assert!(reply.ok);
    assert_eq!(reply.status.scan_seconds_left, agx_bluetooth::SCAN_SECONDS);
    tokio::time::sleep(Duration::from_secs(agx_bluetooth::SCAN_SECONDS as u64 + 1)).await;
    core.refresh().await;
    assert_eq!(fake.calls().last().unwrap(), "scan off");
    assert_eq!(core.status().scan_seconds_left, 0);
    assert_eq!(core.status().message, "Search finished");
}

#[tokio::test(start_paused = true)]
async fn the_list_hides_strangers_and_orders_what_matters_first() {
    let (core, fake, _) = setup();
    {
        let mut inner = fake.0.lock().unwrap();
        inner
            .devices
            .push(device("AA:BB:CC:00:00:10", "", Some(-40))); // unnamed, nearby: hidden
        inner
            .devices
            .push(device("AA:BB:CC:00:00:11", "Gone", None)); // cached, out of range: hidden
        let mut remembered = device("AA:BB:CC:00:00:12", "", None); // paired but unnamed: shown by address
        remembered.paired = true;
        inner.devices.push(remembered);
        inner
            .devices
            .iter_mut()
            .find(|d| d.address == STRANGER)
            .unwrap()
            .connected = true;
    }
    let names: Vec<_> = core
        .handle(Request::Status, Peer::Other)
        .await
        .status
        .devices
        .into_iter()
        .map(|d| d.name)
        .collect();
    // Connected first, then paired (strongest signal first), then the rest.
    assert_eq!(
        names,
        ["Stranger", "Desk speaker", "AA:BB:CC:00:00:12", "Keyboard"]
    );
}

#[tokio::test(start_paused = true)]
async fn no_adapter_is_reported_not_fatal() {
    let (core, fake, _) = setup();
    fake.0.lock().unwrap().adapter = None;
    let reply = core.handle(Request::Status, Peer::Desktop).await;
    assert!(reply.ok && reply.status.adapter.is_none() && reply.status.devices.is_empty());
}

#[tokio::test(start_paused = true)]
async fn prompts_validate_pins_and_one_replaces_another() {
    let prompter = Arc::new(Prompter::default());
    let ask = {
        let prompter = prompter.clone();
        tokio::spawn(async move {
            prompter
                .ask(PromptKind::Pin, KEYBOARD, "Keyboard", None)
                .await
        })
    };
    settle().await;
    let id = prompter.current().unwrap().id;
    assert!(
        !prompter.respond(id + 1, true, Some("0000".into())),
        "stale id"
    );
    assert!(!prompter.respond(id, true, None), "a PIN is required");
    assert!(
        !prompter.respond(id, true, Some("12\n34".into())),
        "control characters"
    );
    assert!(
        prompter.current().is_some(),
        "an invalid answer leaves the prompt open"
    );
    assert!(prompter.respond(id, true, Some("0000".into())));
    assert_eq!(
        ask.await.unwrap(),
        Answer {
            accept: true,
            value: Some("0000".into())
        }
    );
    assert!(prompter.current().is_none());

    let first = {
        let prompter = prompter.clone();
        tokio::spawn(async move {
            prompter
                .ask(
                    PromptKind::Confirm,
                    KEYBOARD,
                    "Keyboard",
                    Some("123456".into()),
                )
                .await
        })
    };
    settle().await;
    let second = {
        let prompter = prompter.clone();
        tokio::spawn(async move {
            prompter
                .ask(PromptKind::Authorize, SPEAKER, "Desk speaker", None)
                .await
        })
    };
    settle().await;
    assert!(
        !first.await.unwrap().accept,
        "the replaced prompt is refused"
    );
    assert_eq!(prompter.current().unwrap().address, SPEAKER);
    second.abort();
}

#[tokio::test(start_paused = true)]
async fn passkeys_are_six_digits_and_silence_means_no() {
    let prompter = Arc::new(Prompter::default());
    let ask = {
        let prompter = prompter.clone();
        tokio::spawn(async move {
            prompter
                .ask(PromptKind::Passkey, KEYBOARD, "Keyboard", None)
                .await
        })
    };
    settle().await;
    let id = prompter.current().unwrap().id;
    assert!(!prompter.respond(id, true, Some("12345".into())));
    assert!(!prompter.respond(id, true, Some("12345a".into())));
    // Nobody answers: BlueZ gets a refusal once the wait is over.
    tokio::time::sleep(Duration::from_secs(
        agx_bluetooth::PROMPT_SECONDS as u64 + 1,
    ))
    .await;
    assert!(!ask.await.unwrap().accept);
    assert!(prompter.current().is_none());
}

#[tokio::test(start_paused = true)]
async fn a_displayed_code_stays_until_the_attempt_ends() {
    let prompter = Arc::new(Prompter::default());
    let (end, ended) = tokio::sync::oneshot::channel::<()>();
    let show = {
        let prompter = prompter.clone();
        tokio::spawn(async move {
            prompter
                .show(
                    PromptKind::Display,
                    KEYBOARD,
                    "Keyboard",
                    "123456".into(),
                    async {
                        let _ = ended.await;
                    },
                )
                .await
        })
    };
    settle().await;
    let prompt = prompter.current().unwrap();
    assert_eq!(
        (prompt.kind, prompt.code.as_deref()),
        (PromptKind::Display, Some("123456"))
    );
    end.send(()).unwrap();
    show.await.unwrap();
    assert!(prompter.current().is_none());
}

#[test]
fn only_the_desktop_binary_counts_as_the_desktop() {
    assert_eq!(classify("/usr/bin/agx-gui"), Peer::Desktop);
    // How the device actually installs it: a release file the /usr/bin link resolves to.
    assert_eq!(
        classify("/var/lib/agx-data/agx-gui/agx-gui.8b781f4dc70aa674"),
        Peer::Desktop
    );
    assert_eq!(
        classify("/var/lib/agx-data/agx-gui/agx-gui.8b781f4dc70aa674 (deleted)"),
        Peer::Desktop
    );
    assert_eq!(
        classify("/var/lib/agx-data/releases/7/agx-gui (deleted)"),
        Peer::Desktop
    );
    for exe in [
        "/usr/bin/agx",
        "/usr/bin/agx-gui-helper",
        "/tmp/agx-gui.evil",
        "/tmp/agx-gui.",
        "/tmp/agx-gui.8b78/../sh",
        "/tmp/agx-gui/evil",
        "/bin/sh",
        "",
    ] {
        assert_eq!(classify(exe), Peer::Other, "{exe}");
    }
}
