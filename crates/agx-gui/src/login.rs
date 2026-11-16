//! Root greeter only. A successful login execs a fresh, unprivileged session.
use crate::{ComponentHandle, LoginWindow, Timer, prepare_display};
use std::{
    cell::RefCell, ffi::CString, io, os::unix::process::CommandExt, process::Command, rc::Rc,
    time::Duration,
};
use zeroize::Zeroizing;

pub fn run(preview: bool) -> Result<(), Box<dyn std::error::Error>> {
    if !preview && unsafe { libc::geteuid() } != 0 {
        return Err("The login screen must be started by agx-init".into());
    }
    prepare_display();
    let ui = LoginWindow::new()?;
    ui.set_draw_cursor(cfg!(feature = "kms"));
    let authenticated = Rc::new(RefCell::new(None::<String>));
    let result = authenticated.clone();
    let weak = ui.as_weak();
    ui.on_sign_in(move |username, password| {
        let Some(ui) = weak.upgrade() else { return };
        if ui.get_busy() {
            return;
        }
        ui.set_busy(true);
        ui.set_error_text("".into());
        let username = username.to_string();
        let password = Zeroizing::new(password.to_string());
        let weak = ui.as_weak();
        // Hashing must not block rendering or keyboard/mouse input. Only the result
        // crosses back to the UI; never log the password or put it in argv/env.
        std::thread::spawn(move || {
            let ok = !preview && agx_profile::service::authenticate(&username, &password).is_ok();
            drop(password);
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = weak.upgrade() else { return };
                if ok {
                    // Store the verified username in a UI property-free callback.
                    ui.invoke_authenticated(username.into());
                } else {
                    ui.set_error_text(
                        "Username or password is incorrect, or the account is unavailable.".into(),
                    );
                    let weak = ui.as_weak();
                    Timer::single_shot(Duration::from_secs(2), move || {
                        if let Some(ui) = weak.upgrade() {
                            ui.set_busy(false);
                        }
                    });
                }
            });
        });
    });
    ui.on_authenticated(move |username| {
        *result.borrow_mut() = Some(username.to_string());
        let _ = slint::quit_event_loop();
    });
    ui.run()?;
    drop(ui);
    if let Some(username) = authenticated.borrow_mut().take() {
        return Err(exec_session(&username).into());
    }
    Ok(())
}

fn exec_session(username: &str) -> io::Error {
    let Ok(name) = CString::new(username) else {
        return io::Error::other("Invalid account");
    };
    // Copy libc's account data before forking. No allocation in pre_exec.
    let entry = unsafe { libc::getpwnam(name.as_ptr()) };
    if entry.is_null() {
        return io::Error::other("Account disappeared");
    }
    let (uid, gid, home) = unsafe {
        (
            (*entry).pw_uid,
            (*entry).pw_gid,
            std::ffi::CStr::from_ptr((*entry).pw_dir)
                .to_string_lossy()
                .into_owned(),
        )
    };
    if uid < 1000 {
        return io::Error::other("An AGX administrator account is required");
    }
    let mut session = Command::new("/usr/bin/agx");
    session
        .arg("desktop")
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("HOME", &home)
        .env("USER", username)
        .env("LOGNAME", username)
        .env("SHELL", "/bin/sh")
        .env("TERM", "linux")
        .current_dir(&home);
    unsafe {
        session.pre_exec(move || {
            // The software framebuffer backend needs access to the active VT.
            if libc::chown(c"/dev/tty1".as_ptr(), uid, gid) != 0
                || libc::initgroups(name.as_ptr(), gid) != 0
                || libc::setgid(gid) != 0
                || libc::setuid(uid) != 0
            {
                return Err(io::Error::last_os_error());
            }
            // Do not leak the root greeter's display/input descriptors into the session.
            if libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 4u32) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    session.exec()
}
