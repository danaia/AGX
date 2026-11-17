//! Wait for exported native GPU fences off the UI thread; never access GL here.
use std::{
    io,
    os::fd::{AsRawFd, OwnedFd},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
};
pub(super) struct Job {
    pub fence: OwnedFd,
    pub ready: Arc<AtomicBool>,
    pub commands: SyncSender<(u8, Vec<u8>)>,
    pub sequence: u64,
    pub wake: slint::Weak<crate::AppWindow>,
}
pub(super) struct Worker(SyncSender<Job>);
impl Worker {
    pub fn new() -> io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel::<Job>(1);
        std::thread::Builder::new()
            .name("web-gpu-fence".into())
            .spawn(move || {
                while let Ok(job) = receiver.recv() {
                    let mut poll = libc::pollfd {
                        fd: job.fence.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    loop {
                        let status = unsafe { libc::poll(&mut poll, 1, -1) };
                        if status < 0
                            && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted
                        {
                            continue;
                        }
                        if status <= 0
                            || poll.revents & (libc::POLLERR | libc::POLLNVAL) != 0
                            || poll.revents & libc::POLLIN == 0
                        {
                            super::AVAILABLE.store(false, Ordering::Relaxed);
                            let _ = job.commands.try_send((12, Vec::new()));
                            eprintln!("agx-web: native completion fence could not be waited on");
                            break;
                        }
                        // The native fence has completed. Publish readiness before the
                        // ACK can cause WebKit to send the next GPU frame.
                        job.ready.store(true, Ordering::Release);
                        let _ = job.commands.send((11, job.sequence.to_be_bytes().to_vec()));
                        let _ = job.wake.upgrade_in_event_loop(|ui| {
                            use slint::ComponentHandle;
                            ui.window().request_redraw();
                        });
                        break;
                    }
                }
            })?;
        Ok(Self(sender))
    }
    pub fn submit(&self, job: Job) -> bool {
        self.0.try_send(job).is_ok()
    }
}
