//! Settings → Bluetooth. All socket traffic to agx-bluetoothd runs on one worker thread; this
//! module mirrors the daemon's state into the UI and turns the page's callbacks into requests.
mod view;

use crate::{App, AppWindow, BluetoothSettings, BtApproval, BtDevice, wm::Kind};
use agx_bluetooth::{Reply, Request, Status};
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::mpsc,
    time::{Duration, Instant},
};

const QUIET_POLL: Duration = Duration::from_millis(2000);
const BUSY_POLL: Duration = Duration::from_millis(800);

type Outcome = Result<Reply, String>;

/// What the page last showed, so unchanged state does not rebuild the list under the pointer.
#[derive(Default)]
struct Shown {
    rows: Vec<BtDevice>,
    status: Option<Status>,
    selected: String,
    approvals: Vec<BtApproval>,
    prompt: Option<u32>,
}

fn show(ui: &AppWindow, shown: &mut Shown, outcome: Outcome) {
    let page = ui.global::<BluetoothSettings>();
    let reply = match outcome {
        Ok(reply) => reply,
        Err(message) => {
            page.set_available(false);
            page.set_adapter(false);
            page.set_powered(false);
            page.set_scanning(false);
            page.set_status(message.into());
            page.set_error("".into());
            page.set_devices(ModelRc::new(VecModel::from(Vec::<BtDevice>::new())));
            page.set_approvals(ModelRc::new(VecModel::from(Vec::new())));
            page.set_prompt_kind(0);
            *shown = Shown::default();
            return;
        }
    };
    let status = &reply.status;
    page.set_available(true);
    page.set_adapter(status.adapter.is_some());
    page.set_powered(status.adapter.as_ref().is_some_and(|adapter| adapter.powered));
    page.set_scanning(status.adapter.as_ref().is_some_and(|adapter| adapter.scanning));
    page.set_seconds_left(status.scan_seconds_left as i32);
    page.set_status(if status.message.is_empty() { "Find, pair and connect Bluetooth devices".into() } else { status.message.as_str().into() });
    // A rejected request's reason arrives in the reply rather than in the sticky state.
    let error = if reply.ok { status.error.clone() } else { Some(reply.message.clone()) };
    page.set_error(error.unwrap_or_default().into());

    let rows = view::rows(&status.devices);
    if rows != shown.rows {
        // Keep the selection on the same device when the list reorders.
        let index = rows.iter().position(|row| row.address.as_str() == shown.selected).map_or(-1, |index| index as i32);
        page.set_devices(ModelRc::new(VecModel::from(rows.clone())));
        page.set_selected(index);
        shown.rows = rows;
    }
    let approvals = view::approvals(status);
    if approvals != shown.approvals {
        page.set_approvals(ModelRc::new(VecModel::from(approvals.clone())));
        shown.approvals = approvals;
    }
    let id = status.prompt.as_ref().map(|prompt| prompt.id);
    if id != shown.prompt {
        let (kind, text, code) = view::prompt(status.prompt.as_ref());
        page.set_prompt_text(text.into());
        page.set_prompt_code(code.into());
        page.set_prompt_kind(kind);
        shown.prompt = id;
    }
    shown.status = Some(reply.status);
}

pub(super) fn install(ui: &AppWindow, app: &Rc<RefCell<App>>) -> Timer {
    let (jobs, work) = mpsc::channel::<Request>();
    let (results, outcomes) = mpsc::channel::<Outcome>();
    std::thread::spawn(move || {
        for request in work {
            let outcome = agx_bluetooth::request(&request).map_err(|error| error.to_string());
            if results.send(outcome).is_err() {
                break;
            }
        }
    });
    let shown = Rc::new(RefCell::new(Shown::default()));
    // Replies still on their way back; a poll is not queued behind another.
    let pending = Rc::new(Cell::new(0u32));
    let send = {
        let (jobs, pending) = (jobs.clone(), pending.clone());
        move |request: Request| {
            pending.set(pending.get() + 1);
            let _ = jobs.send(request);
        }
    };
    let page = ui.global::<BluetoothSettings>();
    {
        let send = send.clone();
        page.on_set_power(move |on| send(Request::Power { on }));
    }
    {
        let send = send.clone();
        page.on_search(move |on| send(Request::Scan { on }));
    }
    {
        let send = send.clone();
        page.on_act(move |address, action| {
            let address = address.to_string();
            send(match action {
                0 => Request::Pair { address },
                1 => Request::Connect { address },
                2 => Request::Disconnect { address },
                3 => Request::Forget { address },
                4 => Request::Trust { address, trusted: true },
                5 => Request::Trust { address, trusted: false },
                _ => return,
            });
        });
    }
    {
        let (weak, shown) = (ui.as_weak(), shown.clone());
        page.on_select(move |index| {
            let Some(ui) = weak.upgrade() else { return };
            let page = ui.global::<BluetoothSettings>();
            let mut shown = shown.borrow_mut();
            let Some(row) = usize::try_from(index).ok().and_then(|index| shown.rows.get(index)) else { return };
            shown.selected = row.address.to_string();
            page.set_selected(index);
            // Moving to another device cancels a pending removal confirmation.
            if page.get_confirm_forget() != shown.selected {
                page.set_confirm_forget("".into());
            }
        });
    }
    {
        let (send, shown) = (send.clone(), shown.clone());
        page.on_respond(move |accept, value| {
            let Some(id) = shown.borrow().status.as_ref().and_then(|status| status.prompt.as_ref().map(|prompt| prompt.id)) else { return };
            send(Request::Respond { id, accept, value: (!value.is_empty()).then(|| value.to_string()) });
        });
    }
    {
        let send = send.clone();
        page.on_decide(move |id, allow| send(Request::Decide { id: id.max(0) as u32, allow }));
    }

    let timer = Timer::default();
    let (weak, state) = (ui.as_weak(), app.clone());
    let mut last_poll = Instant::now() - QUIET_POLL;
    timer.start(TimerMode::Repeated, Duration::from_millis(150), move || {
        let Some(ui) = weak.upgrade() else { return };
        while let Ok(outcome) = outcomes.try_recv() {
            pending.set(pending.get().saturating_sub(1));
            show(&ui, &mut shown.borrow_mut(), outcome);
        }
        // Poll only while the Bluetooth page is on screen.
        let open = state.try_borrow().is_ok_and(|app| app.desk.is_open(Kind::Settings));
        let page = ui.global::<BluetoothSettings>();
        if !open || !page.get_active() || pending.get() > 0 {
            return;
        }
        let lively = page.get_scanning() || page.get_prompt_kind() != 0 || !shown.borrow().approvals.is_empty();
        if last_poll.elapsed() >= if lively { BUSY_POLL } else { QUIET_POLL } {
            last_poll = Instant::now();
            send(Request::Status);
        }
    });
    timer
}
