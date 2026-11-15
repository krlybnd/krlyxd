//! Unix pipe output without a blocking writer thread or unbounded event queue.
//! O_NONBLOCK affects the shared open-file description; original flags are
//! restored on drop. Ordinary filesystem I/O may still block in the OS.

use krlyxd::{CallControl, CallStop, SystemClock};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

pub(super) struct ControlledWriter {
    descriptor: OwnedFd,
    flags: libc::c_int,
    control: Arc<CallControl>,
}

impl ControlledWriter {
    pub(super) fn stdout(control: Arc<CallControl>) -> io::Result<Self> {
        Self::new(io::stdout().as_fd().try_clone_to_owned()?, control)
    }

    fn new(descriptor: OwnedFd, control: Arc<CallControl>) -> io::Result<Self> {
        let fd = descriptor.as_raw_fd();
        // SAFETY: descriptor owns a valid open fd; F_GETFL takes no extra argument.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: F_SETFL takes the valid fd and integer file-status flags.
        if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { descriptor, flags, control })
    }

    fn check(&self) -> io::Result<()> {
        self.control.check().map_err(|reason| match reason {
            // Write::write_all retries Interrupted indefinitely; cancellation is
            // deliberately Other so a stopped writer always terminates its call.
            CallStop::Cancelled => io::Error::other("cancelled: reconstruction interrupted"),
            CallStop::DeadlineExceeded => {
                io::Error::new(io::ErrorKind::TimedOut, "deadline_exceeded: stdout deadline exceeded")
            }
        })
    }
}

impl Write for ControlledWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        loop {
            self.check()?;
            if bytes.is_empty() {
                return Ok(0);
            }
            // SAFETY: bytes is readable for this length and the fd is owned/live.
            let written =
                unsafe { libc::write(self.descriptor.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) };
            if written >= 0 {
                return Ok(written as usize);
            }
            let error = io::Error::last_os_error();
            match error.kind() {
                io::ErrorKind::Interrupted => continue,
                io::ErrorKind::WouldBlock => {
                    let mut fd =
                        libc::pollfd { fd: self.descriptor.as_raw_fd(), events: libc::POLLOUT, revents: 0 };
                    // SAFETY: one initialized pollfd, borrowed for a bounded wait.
                    let polled = unsafe { libc::poll(&mut fd, 1, 10) };
                    if polled < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                        return Err(io::Error::last_os_error());
                    }
                    if fd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                        return Err(io::Error::new(io::ErrorKind::BrokenPipe, "stdout disconnected"));
                    }
                }
                _ => return Err(error),
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        // There is no userspace buffer. A final cancellation check may still
        // fail after bytes reached the reader; delivery is not transactional.
        self.check()
    }
}

impl Drop for ControlledWriter {
    fn drop(&mut self) {
        // SAFETY: the descriptor remains live until after this destructor.
        let _ = unsafe { libc::fcntl(self.descriptor.as_raw_fd(), libc::F_SETFL, self.flags) };
    }
}

/// Reject special files without waiting for a FIFO writer. Per-chunk checks
/// bound cooperative work, not time spent inside the filesystem/kernel itself.
pub(super) fn read_file(path: &Path, limit: usize, control: &CallControl) -> Result<Vec<u8>, String> {
    super::lifecycle::check(control)?;
    let mut file: File = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| error.to_string())?;
    if !file.metadata().map_err(|error| error.to_string())?.is_file() {
        return Err("input must be a regular file".into());
    }
    let mut bytes = Vec::new();
    let mut chunk = [0; 65_536];
    loop {
        super::lifecycle::check(control)?;
        let allowed = chunk.len().min(limit + 1 - bytes.len());
        let count = file.read(&mut chunk[..allowed]).map_err(|error| error.to_string())?;
        if count == 0 {
            super::lifecycle::check(control)?;
            return Ok(bytes);
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > limit {
            return Err(format!("input exceeds {limit} bytes"));
        }
    }
}

/// Diagnostics must not hold a failed/cancelled process behind a full stderr pipe.
pub(super) fn report(message: &str) {
    let control =
        Arc::new(CallControl::with_deadline(Arc::new(SystemClock::default()), Duration::from_millis(25)));
    if let Ok(fd) = io::stderr().as_fd().try_clone_to_owned() {
        if let Ok(mut writer) = ControlledWriter::new(fd, control) {
            let _ = writeln!(writer, "{message}");
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::ControlledWriter;
    use krlyxd::{CallControl, FakeClock};
    use std::io::{Read, Write};
    use std::os::fd::{AsFd, AsRawFd};
    use std::os::unix::net::UnixStream;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn writing_uses_the_owned_descriptor_and_restores_original_flags() {
        let (socket, mut reader) = UnixStream::pair().unwrap();
        // SAFETY: the socket owns this valid descriptor for the entire test.
        let before = unsafe { libc::fcntl(socket.as_raw_fd(), libc::F_GETFL) };
        {
            let fd = socket.as_fd().try_clone_to_owned().unwrap();
            let mut writer = ControlledWriter::new(fd, Arc::new(CallControl::new())).unwrap();
            writer.write_all(b"first\nsecond\n").unwrap();
            writer.flush().unwrap();
            // SAFETY: socket remains live; F_GETFL only observes its shared flags.
            let during = unsafe { libc::fcntl(socket.as_raw_fd(), libc::F_GETFL) };
            assert_ne!(during & libc::O_NONBLOCK, 0);
        }
        // SAFETY: same live descriptor, after the duplicate writer was dropped.
        assert_eq!(unsafe { libc::fcntl(socket.as_raw_fd(), libc::F_GETFL) }, before);
        drop(socket);
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"first\nsecond\n");
    }

    #[test]
    fn stopped_writes_return_non_retryable_errors_without_emitting_bytes() {
        for cancelled in [true, false] {
            let (socket, mut reader) = UnixStream::pair().unwrap();
            let clock = Arc::new(FakeClock::default());
            let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(1)));
            if cancelled {
                control.cancel();
            } else {
                clock.advance(Duration::from_secs(1));
            }
            let mut writer = ControlledWriter::new(socket.into(), control).unwrap();
            let error = writer.write_all(b"must not be sent").unwrap_err();
            assert_ne!(error.kind(), std::io::ErrorKind::Interrupted);
            assert!(error.to_string().contains(if cancelled { "cancelled" } else { "deadline_exceeded" }));
            drop(writer);
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).unwrap();
            assert!(bytes.is_empty());
        }
    }
}
