//! Agent Angi swarms that are running, found from the process list.
//!
//! A swarm outlives the terminal tab that started it: closing the tab detaches Herdr but leaves its
//! server and agents running, holding memory with nothing on screen. This watches for them and gives
//! Agent Angi's page a button that opens Herdr again (`App::terminal_open_herdr`).
use crate::{AgentAngi, App, AppWindow};
use slint::ComponentHandle;
use std::{cell::RefCell, os::unix::fs::MetadataExt, rc::Rc, time::Duration};

/// What is running for this account.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Swarms {
    /// Orchestrator, builder and tester agents.
    pub agents: usize,
    /// Herdr sessions (servers). Herdr shows one at a time, so there can be more than it shows.
    pub swarms: usize,
}

/// The processes a swarm is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    /// `herdr --session angi-… server`
    Server,
    /// `opencode --agent angi-<role> …`
    Agent,
    /// `main.py _dispatch RUN`: delivers peer mail, polling its server every two seconds while it lives.
    Dispatcher,
}

const ROLES: [&str; 3] = ["angi-orchestrator", "angi-builder", "angi-tester"];

fn program<'a>(args: &[&'a str]) -> &'a str {
    args.first().and_then(|program| program.rsplit('/').next()).unwrap_or("")
}

/// What a command line is, if it belongs to a swarm. Other agents (OS updates) and programs are not.
pub(crate) fn classify(args: &[&str]) -> Option<Part> {
    let after = |flag: &str| args.iter().position(|arg| *arg == flag).and_then(|at| args.get(at + 1)).copied();
    match program(args) {
        "herdr" if args.last() == Some(&"server") && after("--session").is_some_and(|name| name.starts_with("angi-")) => {
            Some(Part::Server)
        }
        "opencode" if after("--agent").is_some_and(|role| ROLES.contains(&role)) => Some(Part::Agent),
        name if name.starts_with("python") && after("_dispatch").is_some() && args.iter().any(|arg| arg.ends_with("/main.py")) => {
            Some(Part::Dispatcher)
        }
        _ => None,
    }
}

pub(crate) fn count(parts: impl IntoIterator<Item = Part>) -> Swarms {
    parts.into_iter().fold(Swarms::default(), |mut found, part| {
        match part {
            Part::Server => found.swarms += 1,
            Part::Agent => found.agents += 1,
            Part::Dispatcher => {}
        }
        found
    })
}

/// The arguments in a `/proc/<pid>/cmdline`: NUL-terminated, so no empty argument follows the last.
fn arguments(cmdline: &str) -> Vec<&str> {
    cmdline.strip_suffix('\0').unwrap_or(cmdline).split('\0').collect()
}

/// This account's swarm processes, from `/proc`.
fn processes() -> Vec<(i32, Part)> {
    let me = unsafe { libc::geteuid() };
    let Ok(entries) = std::fs::read_dir("/proc") else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|entry| Some((entry.file_name().to_str()?.parse::<i32>().ok()?, entry)))
        .filter(|(pid, entry)| *pid > 1 && entry.metadata().is_ok_and(|meta| meta.uid() == me))
        .filter_map(|(pid, entry)| {
            let bytes = std::fs::read(entry.path().join("cmdline")).ok()?;
            let command = String::from_utf8_lossy(&bytes);
            classify(&arguments(&command)).map(|part| (pid, part))
        })
        .collect()
}

pub(crate) fn scan() -> Swarms {
    count(processes().into_iter().map(|(_, part)| part))
}

/// Ends every swarm of this account: Herdr servers, their agents and the delivery workers.
/// Asks first (SIGTERM), then insists on whatever is still there after `patience`.
/// Files the agents wrote stay; only the running sessions end.
pub(crate) fn stop(patience: Duration) {
    end(|| processes().into_iter().map(|(pid, _)| pid).collect(), patience);
}

fn end(find: impl Fn() -> Vec<i32>, patience: Duration) {
    let signal = |signal| {
        for pid in find() {
            unsafe { libc::kill(pid, signal) };
        }
    };
    signal(libc::SIGTERM);
    let deadline = std::time::Instant::now() + patience;
    while !find().is_empty() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    signal(libc::SIGKILL);
}

pub(super) fn install(ui: &AppWindow, app: &Rc<RefCell<App>>) {
    let (weak, state) = (ui.as_weak(), app.clone());
    ui.global::<AgentAngi>().on_open_herdr(move || {
        if let (Some(ui), Ok(mut app)) = (weak.upgrade(), state.try_borrow_mut()) {
            app.terminal_open_herdr(&ui);
        }
    });
    // The banner asks for confirmation before it calls this.
    let weak = ui.as_weak();
    ui.global::<AgentAngi>().on_stop_agents(move || {
        let Some(ui) = weak.upgrade() else { return };
        let global = ui.global::<AgentAngi>();
        if global.get_stopping() {
            return;
        }
        global.set_stopping(true);
        let weak = weak.clone();
        std::thread::spawn(move || {
            stop(Duration::from_secs(5));
            publish(&weak, scan(), true);
        });
    });
    let weak = ui.as_weak();
    std::thread::spawn(move || {
        let mut shown = None;
        loop {
            let now = scan();
            if shown != Some(now) {
                shown = Some(now);
                if !publish(&weak, now, false) {
                    break;
                }
            }
            std::thread::sleep(Duration::from_secs(3));
        }
    });
}

/// Hands the counts to the UI thread. False once the desktop is gone.
fn publish(weak: &slint::Weak<AppWindow>, now: Swarms, stopped: bool) -> bool {
    let weak = weak.clone();
    slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            let global = ui.global::<AgentAngi>();
            global.set_running_agents(now.agents as i32);
            global.set_running_swarms(now.swarms as i32);
            if stopped {
                global.set_stopping(false);
            }
        }
    })
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command};

    /// A command as `/proc/<pid>/cmdline` holds it: arguments separated, and ended, by NUL.
    fn cmdline(command: &str) -> String {
        command.split(' ').map(|arg| format!("{arg}\0")).collect()
    }
    fn parts(commands: &[&str]) -> Vec<Option<Part>> {
        commands.iter().map(|command| cmdline(command)).map(|command| classify(&arguments(&command))).collect()
    }

    #[test]
    fn recognises_swarm_servers_agents_and_delivery_workers_only() {
        let found = parts(&[
            "/usr/bin/herdr --session angi-20261118-010239-4390 server",
            "/usr/libexec/opencode/opencode --agent angi-builder --prompt Read",
            "/var/lib/agx-data/angi-sdk/tools/usr/bin/python3.11 /usr/libexec/agx-agent/main.py _dispatch /state/angi-1",
            // A client attached to a swarm is not a second swarm, and Herdr sessions of other tools are not ours.
            "herdr --session angi-20261118-010239-4390",
            "herdr --session maya-experiments server",
            // OS updates run one agent without Herdr; other agent commands and programs are not swarms.
            "/usr/libexec/opencode/opencode --agent angi-os --prompt update",
            "/usr/libexec/agx-agent/main.py provider-bridge",
            "/usr/bin/python3 /usr/libexec/agx-agent/main.py provider-bridge",
            "/usr/bin/opencode",
            "/usr/bin/agx-gui",
        ]);
        assert_eq!(
            found,
            [Some(Part::Server), Some(Part::Agent), Some(Part::Dispatcher), None, None, None, None, None, None, None]
        );
    }

    #[test]
    fn counts_agents_and_sessions_but_not_workers() {
        let all = [Part::Server, Part::Server, Part::Agent, Part::Agent, Part::Agent, Part::Dispatcher, Part::Dispatcher];
        assert_eq!(count(all), Swarms { agents: 3, swarms: 2 });
        assert_eq!(count([]), Swarms::default());
    }

    fn alive(pid: i32) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| stat.rsplit(") ").next().is_some_and(|rest| !rest.starts_with('Z')))
    }
    fn sleeper(ignore_terminate: bool) -> Child {
        let code = if ignore_terminate { "import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(60)" } else { "import time; time.sleep(60)" };
        Command::new("python3").arg0("/usr/bin/herdr").args(["-c", code, "--session", "angi-test", "server"]).spawn().unwrap()
    }

    #[test]
    fn stopping_ends_polite_and_stubborn_processes_alike() {
        let mut children = [sleeper(false), sleeper(true)];
        let pids: Vec<i32> = children.iter().map(|child| child.id() as i32).collect();
        std::thread::sleep(Duration::from_millis(400));
        // Only these test processes: stop() itself would end every real swarm of the account.
        end(|| pids.iter().copied().filter(|pid| alive(*pid)).collect(), Duration::from_millis(500));
        for child in &mut children {
            child.wait().unwrap();
        }
        assert!(pids.iter().all(|pid| !alive(*pid)));
    }
}
