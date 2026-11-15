//! Single-shot CLI control. Signal handlers only set an atomic flag; a short-lived
//! watcher propagates it to the native publication-aware cancellation token.
//! No signal handler takes the CallControl mutex or performs I/O.

use krlyxd::{CallControl, CallStop, Clock, SystemClock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;

pub(super) struct Lifecycle {
    pub(super) control: Arc<CallControl>,
    handlers: Vec<signal_hook::SigId>,
    shutdown: Option<mpsc::Sender<()>>,
    watcher: Option<JoinHandle<()>>,
}

impl Lifecycle {
    pub(super) fn new(timeout: Duration) -> Result<Self, String> {
        let clock = Arc::new(SystemClock::default());
        let deadline = clock.now().checked_add(timeout).ok_or("timeout deadline overflow")?;
        let control = Arc::new(CallControl::with_deadline(clock, deadline));
        let mut owner =
            Self { control: Arc::clone(&control), handlers: Vec::new(), shutdown: None, watcher: None };
        let interrupted = Arc::new(AtomicBool::new(false));
        for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
            owner.handlers.push(
                signal_hook::flag::register(signal, Arc::clone(&interrupted))
                    .map_err(|error| format!("cannot install signal handler: {error}"))?,
            );
        }
        let (send, receive) = mpsc::channel();
        owner.watcher = Some(
            std::thread::Builder::new()
                .name("reconstruction-control".into())
                .spawn(move || loop {
                    if interrupted.load(Ordering::Relaxed) {
                        control.cancel();
                        break;
                    }
                    match receive.recv_timeout(Duration::from_millis(5)) {
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        _ => break,
                    }
                })
                .map_err(|error| format!("cannot start signal watcher: {error}"))?,
        );
        owner.shutdown = Some(send);
        Ok(owner)
    }
}

impl Drop for Lifecycle {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(watcher) = self.watcher.take() {
            let _ = watcher.join();
        }
        for handler in self.handlers.drain(..) {
            signal_hook::low_level::unregister(handler);
        }
    }
}

pub(super) fn check(control: &CallControl) -> Result<(), String> {
    control.check().map_err(|reason| match reason {
        CallStop::Cancelled => "cancelled: reconstruction interrupted".into(),
        CallStop::DeadlineExceeded => "deadline_exceeded: reconstruction deadline exceeded".into(),
    })
}
