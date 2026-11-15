use crate::{Failure, TICK};
use krly3d_mesh::diagnostic;
use std::fs::File;
use std::io::{self, Write};
use std::os::fd::AsFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

/// Diagnostics are best effort. Neither a full stderr pipe nor Rust's global
/// stderr lock may delay a terminal event or keep this process alive at exit.
pub(crate) fn report(error: &krly3d_mesh::Diagnostic) {
    let Ok(descriptor) = io::stderr().as_fd().try_clone_to_owned() else {
        return;
    };
    let message = format!("krly3d-mesh: {}: {}\n", error.code, error.message);
    let (sent, received) = mpsc::sync_channel(1);
    if std::thread::Builder::new()
        .name("mesh-cli-diagnostic".into())
        .spawn(move || {
            let _ = File::from(descriptor).write_all(message.as_bytes());
            let _ = sent.send(());
        })
        .is_ok()
    {
        let _ = received.recv_timeout(Duration::from_millis(20));
    }
}

/// Detect a departed reader even while the engine has no new event to write.
/// POLLOUT is deliberately omitted: ordinary backpressure is not a disconnect.
pub(crate) fn check_connection() -> Result<(), Failure> {
    let mut descriptor = libc::pollfd { fd: libc::STDOUT_FILENO, events: 0, revents: 0 };
    // SAFETY: descriptor points to one initialized pollfd for the duration of this
    // nonblocking call. poll observes stdout without closing it or changing flags.
    let result = unsafe { libc::poll(&mut descriptor, 1, 0) };
    if result < 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(diagnostic("io", format!("cannot poll stdout: {error}")).into());
        }
    } else if descriptor.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
        return Err(diagnostic("io", "stdout is closed or disconnected").into());
    }
    Ok(())
}

/// At most one bounded event is in flight. Keep control on this thread while a
/// slow reader blocks the writer; neither signal handling nor cancellation waits
/// on stdout's lock. A blocked writer is detached when this single-shot CLI exits.
pub(crate) fn line(
    mut text: String,
    interrupted: &AtomicBool,
    mut cancel: impl FnMut() -> Result<(), Failure>,
) -> Result<(), Failure> {
    check_connection()?;
    text.push('\n');
    // Use an owned descriptor rather than holding Rust's global stdout lock:
    // runtime stdout cleanup must not wait for a detached, blocked writer.
    let descriptor = io::stdout()
        .as_fd()
        .try_clone_to_owned()
        .map_err(|error| diagnostic("io", format!("cannot duplicate stdout: {error}")))?;
    let (sent, received) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("mesh-cli-output".into())
        .spawn(move || {
            let mut output = File::from(descriptor);
            let result = output.write_all(text.as_bytes()).and_then(|()| output.flush());
            let _ = sent.send(result);
        })
        .map_err(|error| diagnostic("resource_limit", format!("could not start output writer: {error}")))?;
    let mut stopped_at = None;
    loop {
        if interrupted.load(Ordering::Relaxed) {
            cancel()?;
            let started = stopped_at.get_or_insert_with(Instant::now);
            // Deliver canonical cancellation when the reader is responsive, but
            // do not keep a cancelled process alive behind a full stdout pipe.
            if started.elapsed() >= Duration::from_millis(100) {
                return Err(Failure::cancelled("stdout remained blocked after SIGINT"));
            }
        }
        check_connection()?;
        match received.recv_timeout(TICK) {
            Ok(result) => return result.map_err(|error| diagnostic("io", format!("stdout: {error}")).into()),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(diagnostic("internal", "output writer stopped without a result").into());
            }
        }
    }
}
