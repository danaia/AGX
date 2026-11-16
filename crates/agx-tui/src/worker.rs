//! Bounded background sampling channel; UI rendering never collects system data.
use agx_core::{Sampler, Snapshot};
use std::{
    io,
    sync::mpsc::{self, Receiver, SyncSender},
    time::Duration,
};

pub(super) struct Worker {
    pub(super) results: Receiver<Result<Snapshot, String>>,
    pub(super) refresh: SyncSender<()>,
}
impl Worker {
    pub(super) fn start() -> io::Result<Self> {
        let (sender, results) = mpsc::sync_channel(1);
        let (refresh, control) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("agx-sampler".into())
            .spawn(move || {
                let mut sampler = Sampler::default();
                loop {
                    if let Err(mpsc::TrySendError::Disconnected(_)) =
                        sender.try_send(sampler.collect().map_err(|e| e.to_string()))
                    {
                        break;
                    }
                    if let Err(mpsc::RecvTimeoutError::Disconnected) =
                        control.recv_timeout(Duration::from_secs(1))
                    {
                        break;
                    }
                }
            })?;
        Ok(Self { results, refresh })
    }
}
