//! State transitions of the app library, and the worker against a stand-in for `angi-update`.
use super::{worker, Availability, Job, Library, State};
use serde_json::json;
use std::{
    os::unix::fs::PermissionsExt,
    sync::{mpsc, Arc, Mutex},
};

fn running(fraction: f64, message: &str) -> serde_json::Value {
    json!({"state": "running", "fraction": fraction, "message": message})
}

#[test]
fn availability_follows_jobs_then_the_installed_tag_and_newest_known_release() {
    let mut state = State::default();
    assert_eq!(state.availability("rg"), Availability::Available);
    state.installed.insert("rg".into(), "15.1.0".into());
    assert_eq!(state.availability("rg"), Availability::Installed { tag: "15.1.0".into(), update: None });
    state.latest.insert("rg".into(), "".into());
    assert_eq!(state.availability("rg"), Availability::Installed { tag: "15.1.0".into(), update: None }, "an unknown latest is not an update");
    state.latest.insert("rg".into(), "15.1.0".into());
    assert_eq!(state.availability("rg"), Availability::Installed { tag: "15.1.0".into(), update: None });
    state.latest.insert("rg".into(), "15.2.0".into());
    assert_eq!(state.availability("rg"), Availability::Installed { tag: "15.1.0".into(), update: Some("15.2.0".into()) });
    state.jobs.insert("rg".into(), Job::Queued { removing: false });
    assert_eq!(state.availability("rg"), Availability::Queued { removing: false }, "a job outranks the installed state");
}

#[test]
fn installer_events_drive_a_job_to_installed_with_a_notice() {
    let mut state = State::default();
    assert!(worker::apply_event(&mut state, "rg", false, &running(0.30, "Downloading ripgrep")).is_none());
    assert_eq!(
        state.availability("rg"),
        Availability::Working { fraction: 0.30, message: "Downloading ripgrep".into(), removing: false }
    );
    worker::apply_event(&mut state, "rg", false, &running(7.0, "x"));
    assert!(matches!(state.availability("rg"), Availability::Working { fraction, .. } if fraction == 1.0), "progress is clamped");
    let outcome = worker::apply_event(&mut state, "rg", false, &json!({"state": "succeeded", "message": "ripgrep 15.2.0 is installed. Run rg in Terminal."}));
    assert_eq!(outcome, Some(Ok("ripgrep 15.2.0 is installed. Run rg in Terminal.".into())));
    worker::finish(&mut state, "rg", outcome.unwrap());
    assert_eq!(state.availability("rg"), Availability::Available, "the job is gone; the ledger refresh marks it installed");
    assert_eq!(state.notices, ["ripgrep 15.2.0 is installed. Run rg in Terminal."]);
}

#[test]
fn a_failed_install_keeps_its_reason_until_retried() {
    let mut state = State::default();
    let outcome = worker::apply_event(&mut state, "rg", false, &json!({"state": "failed", "message": "Cannot reach GitHub."}));
    worker::finish(&mut state, "rg", outcome.unwrap());
    assert_eq!(state.availability("rg"), Availability::Failed("Cannot reach GitHub.".into()));
    assert_eq!(state.notices, ["Cannot reach GitHub."]);
    assert!(worker::apply_event(&mut state, "rg", false, &json!({"state": "mystery"})).is_none());
    assert!(worker::apply_event(&mut state, "rg", false, &json!({"no": "state"})).is_none());
}

#[test]
fn the_installed_list_and_newest_releases_come_from_the_listing() {
    let mut state = State::default();
    worker::apply_listing(
        &mut state,
        &json!({"apps": {
            "rg": {"tag": "v15.1.0", "version": "15.1.0", "latest": "v15.2.0", "update": "15.2.0"},
            "fd": {"tag": "v10", "version": "10", "latest": "v10", "update": ""},
            "jq": {"tag": "jq-1.8.2"},
        }}),
    );
    assert_eq!(state.installed.len(), 3);
    assert_eq!(state.availability("rg"), Availability::Installed { tag: "15.1.0".into(), update: Some("15.2.0".into()) });
    assert_eq!(state.availability("fd"), Availability::Installed { tag: "10".into(), update: None });
    assert_eq!(state.availability("jq"), Availability::Installed { tag: "jq-1.8.2".into(), update: None }, "an older listing without display fields still works");
    let generation = state.generation;
    worker::apply_listing(&mut state, &json!({"unexpected": true}));
    assert_eq!(state.generation, generation, "garbage leaves what is shown untouched");
    assert_eq!(state.installed.len(), 3);
}

fn detached() -> (Library, mpsc::Receiver<super::Request>) {
    let (requests, receiver) = mpsc::channel();
    (Library { state: Arc::new(Mutex::new(State::default())), requests }, receiver)
}

#[test]
fn one_app_cannot_have_two_jobs_and_requests_queue_in_order() {
    let (library, requests) = detached();
    assert!(library.install("rg"));
    assert!(!library.install("rg"), "a second install while one is queued is ignored");
    assert!(!library.remove("rg"));
    assert!(library.install("fd"));
    assert_eq!(library.availability("rg"), Availability::Queued { removing: false });
    let order: Vec<_> = requests
        .try_iter()
        .map(|request| match request {
            super::Request::Install(id) => id,
            _ => "other".into(),
        })
        .collect();
    assert_eq!(order, ["rg", "fd"]);
    library.lock().jobs.insert("rg".into(), Job::Failed("x".into()));
    assert!(library.install("rg"), "a failed app can be retried");
}

#[test]
fn updates_are_only_checked_every_ten_minutes() {
    let (library, requests) = detached();
    library.check_if_stale();
    library.check_if_stale();
    assert_eq!(requests.try_iter().count(), 1);
}

#[test]
fn terminal_path_puts_installed_apps_first_and_keeps_the_rest() {
    let path = super::terminal_path();
    let (first, rest) = path.split_once(':').unwrap();
    assert!(first.ends_with("/apps/bin") && first.starts_with("/var/lib/agx-data/agx-opencode-users/"));
    assert!(rest.contains("/bin"), "the system PATH survives: {rest}");
}

/// A shell script standing in for `angi-update`: prints the JSON events the real installer prints.
fn script(body: &str) -> (tempdir::Dir, String) {
    let dir = tempdir::Dir::new();
    let path = dir.0.join("angi-update");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = path.to_string_lossy().into_owned();
    (dir, path)
}

mod tempdir {
    pub struct Dir(pub std::path::PathBuf);
    impl Dir {
        pub fn new() -> Self {
            let path = std::env::temp_dir().join(format!("angi-library-test-{}-{:?}", std::process::id(), std::thread::current().id()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[test]
fn the_worker_streams_a_real_process_to_a_finished_install() {
    let (_dir, program) = script(
        r#"echo '{"state":"running","fraction":0.1,"message":"Finding the latest ripgrep release…"}'
echo 'not json is ignored'
echo '{"state":"running","fraction":0.5,"message":"Downloading ripgrep"}'
echo '{"state":"succeeded","fraction":1.0,"message":"ripgrep 15.2.0 is installed. Run rg in Terminal."}'"#,
    );
    let state = Mutex::new(State::default());
    worker::job_with(&program, &state, "rg", false);
    let state = state.lock().unwrap();
    assert_eq!(state.availability("rg"), Availability::Available);
    assert_eq!(state.notices, ["ripgrep 15.2.0 is installed. Run rg in Terminal."]);
}

#[test]
fn a_process_that_dies_without_a_verdict_reports_its_own_error_not_success() {
    let (_dir, program) = script("echo 'angi-update: no space left on device' >&2\nexit 1");
    let state = Mutex::new(State::default());
    worker::job_with(&program, &state, "rg", false);
    assert_eq!(
        state.lock().unwrap().availability("rg"),
        Availability::Failed("angi-update: no space left on device".into())
    );
    let (_dir, program) = script("echo '{\"state\":\"running\",\"fraction\":0.4,\"message\":\"Downloading\"}'\nexit 0");
    let state = Mutex::new(State::default());
    worker::job_with(&program, &state, "fd", false);
    assert!(
        matches!(state.lock().unwrap().availability("fd"), Availability::Failed(message) if message.contains("stopped before finishing")),
        "a run that never reports success is never shown as installed"
    );
    let state = Mutex::new(State::default());
    worker::job_with("/nonexistent/angi-update", &state, "fd", false);
    assert!(matches!(state.lock().unwrap().availability("fd"), Availability::Failed(message) if message.contains("could not start")));
}
