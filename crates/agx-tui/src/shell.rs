//! Borrow the existing console for a real foreground shell, then return it to the TUI.
use crate::terminal::terminal_closed;
use std::{
    io,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Shell,
    Tmux,
    OpenCode,
}
fn executable(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|path| path.join(name))
        .find(|path| executable_file(path))
}
fn executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.is_absolute()
        && path
            .metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}
fn shell_path() -> PathBuf {
    std::env::var_os("AGX_TERMINAL_SHELL")
        .map(PathBuf::from)
        .filter(|p| executable_file(p))
        .or_else(|| executable_file(Path::new("/bin/bash")).then(|| PathBuf::from("/bin/bash")))
        .or_else(|| {
            std::env::var_os("SHELL")
                .map(PathBuf::from)
                .filter(|p| executable_file(p))
        })
        .unwrap_or_else(|| PathBuf::from("/bin/sh"))
}
pub(crate) fn available_opencode() -> bool {
    executable("opencode").is_some()
}
pub(crate) fn available_tmux() -> bool {
    executable("tmux").is_some()
}
fn command(kind: Kind) -> io::Result<Command> {
    if kind == Kind::Tmux && std::env::var_os("TMUX").is_some() {
        return Err(io::Error::other(
            "Already inside tmux. Use its Ctrl+B shortcuts, or F4 for a regular shell.",
        ));
    }
    let shell = shell_path();
    let mut command = if kind == Kind::OpenCode {
        Command::new(
            executable("opencode").ok_or_else(|| io::Error::other("OpenCode is not installed."))?,
        )
    } else if kind == Kind::Tmux {
        let tmux = executable("tmux").ok_or_else(|| {
            io::Error::other("tmux is not installed. Install the ANGI Terminal CLI runtime first.")
        })?;
        let mut command = Command::new(tmux);
        command.args(["new-session", "-A", "-s", "angi"]);
        command
    } else {
        let mut command = Command::new(&shell);
        if shell.file_name().is_some_and(|name| name == "bash") {
            let rcfile = Path::new("/usr/share/agx/terminal/bashrc");
            if rcfile.is_file() {
                command.arg("--rcfile").arg(rcfile);
            }
            command.arg("-i");
        } else {
            command.arg("-i");
        }
        command
    };
    command
        .env("SHELL", shell)
        .env("TERM_PROGRAM", "ANGI")
        .env("AGX_TERMINAL_CONTEXT", "tui");
    // Keep the real TERM and color capabilities, including Linux and serial consoles.
    if Path::new("/usr/lib/locale/C.utf8/LC_CTYPE").is_file() {
        command.env("LC_CTYPE", "C.UTF-8");
    }
    command.process_group(0);
    unsafe {
        command.pre_exec(|| foreground(libc::getpgrp()));
    }
    Ok(command)
}
/// SIGTTOU otherwise stops a background process attempting to change tty ownership.
fn foreground(group: libc::pid_t) -> io::Result<()> {
    unsafe {
        let mut set = std::mem::zeroed();
        let mut old = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGTTOU);
        if libc::sigprocmask(libc::SIG_BLOCK, &set, &mut old) < 0 {
            return Err(io::Error::last_os_error());
        }
        let result = libc::tcsetpgrp(libc::STDIN_FILENO, group);
        let error = (result < 0).then(io::Error::last_os_error);
        let restored = libc::sigprocmask(libc::SIG_SETMASK, &old, std::ptr::null_mut());
        if let Some(error) = error {
            Err(error)
        } else if restored < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}
struct Foreground {
    group: libc::pid_t,
    mode: libc::termios,
}
impl Drop for Foreground {
    fn drop(&mut self) {
        let _ = foreground(self.group);
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.mode);
        }
    }
}
pub(crate) fn run(kind: Kind, stop: &AtomicBool) -> io::Result<ExitStatus> {
    let old = unsafe { libc::tcgetpgrp(libc::STDIN_FILENO) };
    if old < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut mode = unsafe { std::mem::zeroed() };
    if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut mode) } < 0 {
        return Err(io::Error::last_os_error());
    }
    let _restore = Foreground { group: old, mode };
    let mut child = command(kind)?.spawn()?;
    let group = child.id() as libc::pid_t;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        }
        if stop.load(Ordering::Relaxed) || terminal_closed().unwrap_or(true) {
            // This child has not been reaped, so its process group cannot be reused.
            let active = unsafe { libc::tcgetpgrp(libc::STDIN_FILENO) };
            unsafe {
                if active > 0 && active != old {
                    libc::kill(-active, libc::SIGHUP);
                }
                libc::kill(-group, libc::SIGHUP);
            }
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(1) {
                if let Some(status) = child.try_wait()? {
                    return Ok(status);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            unsafe {
                libc::kill(-group, libc::SIGKILL);
            }
            return child.wait();
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shell_configuration_preserves_the_callers_terminal() {
        let cmd = command(Kind::Shell).unwrap();
        let env = cmd.get_envs().collect::<Vec<_>>();
        assert!(
            env.iter()
                .any(|(k, v)| *k == "AGX_TERMINAL_CONTEXT"
                    && *v == Some(std::ffi::OsStr::new("tui")))
        );
        assert!(env.iter().all(|(k, _)| *k != "TERM" && *k != "COLORTERM"));
        assert!(cmd.get_args().any(|a| a == "-i"));
    }
}
