//! The Unix-socket service: access control, bounded connections and shutdown.
use crate::{backend::Backend, bluez::Bluez, core::Core, peer, prompts::Prompter};
use agx_bluetooth::{MAX_REQUEST, Reply, Request, SOCKET, socket_path};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::{
        fs::{OpenOptionsExt, PermissionsExt},
        io::AsRawFd,
    },
    path::Path,
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::Semaphore,
};

const IO_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_CONNECTIONS: usize = 16;

fn err(message: &str) -> io::Error {
    io::Error::other(message.to_owned())
}

/// The system socket needs root and the ANGI administration group; an `AGX_BLUETOOTH_SOCKET`
/// override (desktop previews) stays private to the current user.
fn bind() -> io::Result<(UnixListener, std::fs::File)> {
    let path = socket_path();
    let system = path == Path::new(SOCKET);
    if system {
        if unsafe { libc::geteuid() } != 0 {
            return Err(err("Bluetooth service requires root"));
        }
        let run = Path::new("/run/agx");
        fs::create_dir_all(run)?;
        fs::set_permissions(run, fs::Permissions::from_mode(0o750))?;
    }
    unsafe { libc::umask(0o077) };
    let mut lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path.with_extension("pid"))?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(err("Bluetooth service is already running"));
    }
    lock.set_len(0)?;
    writeln!(lock, "{}", std::process::id())?;
    if path.exists() {
        fs::remove_file(&path)?;
    }
    let listener = std::os::unix::net::UnixListener::bind(&path)?;
    listener.set_nonblocking(true)?;
    if system {
        agx_profile::service::prepare_access(SOCKET)?;
    }
    Ok((UnixListener::from_std(listener)?, lock))
}

pub fn run() -> io::Result<()> {
    // One thread is plenty for a handful of local clients and one radio.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let (listener, _lock) = bind()?;
        let prompter = Arc::new(Prompter::default());
        let backend = Bluez::start(prompter.clone())
            .await
            .map_err(|error| err(&error.0))?;
        let core = Core::new(backend, prompter);
        tokio::spawn(Arc::clone(&core).run_background());
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        let result = tokio::select! {
            result = accept(listener, core) => result,
            _ = terminate.recv() => Ok(()),
            _ = tokio::signal::ctrl_c() => Ok(()),
        };
        let _ = fs::remove_file(socket_path());
        result
    })
}

pub async fn accept<B: Backend>(listener: UnixListener, core: Arc<Core<B>>) -> io::Result<()> {
    let limit = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        let (stream, _) = listener.accept().await?;
        // A flood of connections waits here instead of exhausting tasks.
        let Ok(permit) = limit.clone().acquire_owned().await else {
            continue;
        };
        let core = core.clone();
        tokio::spawn(async move {
            let _ = serve(&core, stream).await;
            drop(permit);
        });
    }
}

async fn serve<B: Backend>(core: &Arc<Core<B>>, mut stream: UnixStream) -> io::Result<()> {
    let who = peer::of_pid(
        stream
            .peer_cred()
            .ok()
            .and_then(|credentials| credentials.pid()),
    );
    let mut line = Vec::new();
    let mut reader = BufReader::new((&mut stream).take(MAX_REQUEST as u64 + 1));
    tokio::time::timeout(IO_TIMEOUT, reader.read_until(b'\n', &mut line))
        .await
        .map_err(|_| err("request timed out"))??;
    let reply = if line.len() > MAX_REQUEST {
        failure(core, "Bluetooth request is too large")
    } else {
        match serde_json::from_slice::<Request>(&line) {
            Ok(request) => core.handle(request, who).await,
            Err(_) => failure(core, "Invalid Bluetooth request"),
        }
    };
    let mut bytes = serde_json::to_vec(&reply)?;
    bytes.push(b'\n');
    tokio::time::timeout(IO_TIMEOUT, stream.write_all(&bytes))
        .await
        .map_err(|_| err("reply timed out"))?
}

fn failure<B: Backend>(core: &Core<B>, message: &str) -> Reply {
    Reply {
        ok: false,
        message: message.into(),
        status: core.status(),
    }
}
