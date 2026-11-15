//! One session served over a Unix domain socket, so that it outlives every client.
//!
//! Served on standard input, a session lasts as long as the one client that started it.
//! Behind a socket it lasts as long as the program: clients come and go, one connection at
//! a time, and each finds the model and the peers as the last one left them. Only the
//! user may connect: the socket is made readable and writable by its owner alone.

use crate::dialect::{self, Dialect};
use crate::endpoint::{Controlled, Endpoint};
use krly3d_core::CallControl;
use std::io::{self, BufReader, Read, Write};
use std::net::Shutdown;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

/// The socket at `path`, made for this program. A socket left there by a server that has
/// ended is replaced; a live one, or any other file, is not.
pub fn bind(path: &Path) -> Result<UnixListener, String> {
    let shown = path.display();
    match std::fs::symlink_metadata(path) {
        Ok(found) if !found.file_type().is_socket() => {
            return Err(format!("{shown} exists and is not a socket"));
        }
        Ok(_) => match UnixStream::connect(path) {
            Ok(_) => return Err(format!("{shown}: another server is answering there")),
            Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
                std::fs::remove_file(path).map_err(|error| format!("{shown}: {error}"))?;
            }
            Err(error) => return Err(format!("{shown}: {error}")),
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("{shown}: {error}")),
    }
    let listener = UnixListener::bind(path).map_err(|error| format!("{shown}: {error}"))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("{shown}: {error}"))?;
    Ok(listener)
}

/// Serves the clients of `listener` one after the other, for as long as the program runs.
/// A connection that breaks ends that client, not the server.
pub fn serve(
    listener: &UnixListener,
    dialect: &mut dyn Dialect,
    session: &mut dyn Endpoint,
) -> Result<(), String> {
    for stream in listener.incoming() {
        let stream = stream.map_err(|error| format!("a client could not connect: {error}"))?;
        match connection(stream, dialect, session) {
            Ok(()) => {}
            Err(error)
                if matches!(error.kind(), io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset) => {}
            Err(error) => return Err(format!("serving a client failed: {error}")),
        }
    }
    Ok(())
}

/// Answers one client until it stops sending.
pub fn connection(
    stream: UnixStream,
    dialect: &mut dyn Dialect,
    session: &mut dyn Endpoint,
) -> io::Result<()> {
    let reading = BufReader::new(stream.try_clone()?);
    let control = Arc::new(CallControl::new());
    let monitor = DisconnectMonitor::start(stream.try_clone()?, Arc::clone(&control))?;
    let mut session = Controlled { endpoint: session, control };
    let result = dialect::serve(dialect, &mut session, reading, &stream);
    let stopped = monitor.stop();
    result.and(stopped)
}

/// Watches only full hangup/error, never readability or read EOF: clients normally
/// shut down their write half before receiving replies. A separate socket wakes the
/// blocking poll on every exit, including unwinding; the watcher is always joined.
struct DisconnectMonitor {
    stop: UnixStream,
    worker: Option<JoinHandle<io::Result<()>>>,
}

impl DisconnectMonitor {
    fn start(stream: UnixStream, control: Arc<CallControl>) -> io::Result<Self> {
        let (stop, wake) = UnixStream::pair()?;
        let worker = thread::Builder::new().name("socket-disconnect".into()).spawn(move || {
            let outcome = watch_disconnect(&stream, &wake);
            if !matches!(outcome, Ok(false)) {
                control.cancel();
                // Also release an idle reader or a writer whose send buffer is full.
                // This happens only on full disconnect or a failed monitor.
                let _ = stream.shutdown(Shutdown::Both);
            }
            outcome.map(|_| ())
        })?;
        Ok(Self { stop, worker: Some(worker) })
    }

    fn stop(mut self) -> io::Result<()> {
        self.join()
    }

    fn join(&mut self) -> io::Result<()> {
        // Closing the wake writer yields POLLHUP even if no bytes can be written.
        let _ = self.stop.shutdown(Shutdown::Both);
        match self.worker.take() {
            Some(worker) => worker.join().map_err(|_| io::Error::other("disconnect monitor panicked"))?,
            None => Ok(()),
        }
    }
}

impl Drop for DisconnectMonitor {
    fn drop(&mut self) {
        let _ = self.join();
    }
}

/// `poll` reports POLLHUP/POLLERR regardless of the requested events. Requesting
/// neither POLLIN nor POLLRDHUP avoids spinning on buffered input or write half-close.
fn watch_disconnect(stream: &UnixStream, stop: &UnixStream) -> io::Result<bool> {
    let mut fds = [
        libc::pollfd { fd: stream.as_raw_fd(), events: 0, revents: 0 },
        libc::pollfd { fd: stop.as_raw_fd(), events: libc::POLLIN, revents: 0 },
    ];
    loop {
        // SAFETY: both descriptors are owned by this thread for the duration of poll;
        // fds points to two initialized pollfd entries, and poll retains no pointers.
        if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) } < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if fds[1].revents != 0 {
            return Ok(false);
        }
        if fds[0].revents & libc::POLLNVAL != 0 {
            return Err(io::Error::other("disconnect monitor lost its socket"));
        }
        if fds[0].revents & (libc::POLLHUP | libc::POLLERR) != 0 {
            return Ok(true);
        }
    }
}

/// The client side: sends `input` to the server at `path`, says it has sent all, and
/// copies the answers to `output` until the server is done with them.
pub fn send(path: &Path, mut input: impl Read, mut output: impl Write) -> Result<(), String> {
    let shown = path.display();
    let mut stream = UnixStream::connect(path).map_err(|error| format!("{shown}: {error}"))?;
    io::copy(&mut input, &mut stream).map_err(|error| format!("{shown}: sending failed: {error}"))?;
    stream.shutdown(Shutdown::Write).map_err(|error| format!("{shown}: {error}"))?;
    io::copy(&mut stream, &mut output).map_err(|error| format!("{shown}: receiving failed: {error}"))?;
    output.flush().map_err(|error| format!("the answers could not be written: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{bind, connection, DisconnectMonitor};
    use crate::dialect::plain::Plain;
    use crate::dialect::testing::session;
    use crate::endpoint::Endpoint;
    use krly3d_core::{CallControl, Exactness};
    use krly3d_kernel::schema::{Params, Value};
    use krly3d_kernel::{Method, Reply};
    use std::io::{Read, Write};
    use std::net::Shutdown;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    /// A path of its own for each test, under the system's place for temporary files.
    fn place(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("krly3d-listen-{}-{name}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    /// Sends the lines over a fresh connection to the session and returns the answers.
    fn exchange(session: &mut krly3d_kernel::Session, lines: &str) -> String {
        let (mut client, server) = UnixStream::pair().unwrap();
        client.write_all(lines.as_bytes()).unwrap();
        client.shutdown(Shutdown::Write).unwrap();
        connection(server, &mut Plain, session).unwrap();
        let mut answers = String::new();
        client.read_to_string(&mut answers).unwrap();
        answers
    }

    #[test]
    fn the_model_outlives_the_connection_that_made_it() {
        let mut session = session();
        exchange(&mut session, "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"box\",\"params\":{\"id\":\"plate\",\"w\":6,\"d\":4,\"h\":1}}\n");

        let answers =
            exchange(&mut session, "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"save\",\"params\":{}}\n");

        assert!(answers.contains("plate = box d=4 h=1 w=6"), "{answers}");
    }

    #[test]
    fn stopping_the_monitor_joins_it_without_cancelling_an_open_client() {
        let (_client, server) = UnixStream::pair().unwrap();
        let control = Arc::new(CallControl::new());

        DisconnectMonitor::start(server, Arc::clone(&control)).unwrap().stop().unwrap();

        assert_eq!(Arc::strong_count(&control), 1, "the worker released its control before returning");
        assert_eq!(control.check(), Ok(()));
    }

    #[test]
    fn the_monitor_is_stopped_and_joined_when_its_owner_unwinds() {
        let (_client, server) = UnixStream::pair().unwrap();
        let control = Arc::new(CallControl::new());
        let owner = Arc::clone(&control);

        let unwound = std::thread::spawn(move || {
            let _monitor = DisconnectMonitor::start(server, owner).unwrap();
            panic!("exercise the monitor's unwinding cleanup");
        })
        .join();

        assert!(unwound.is_err());
        assert_eq!(Arc::strong_count(&control), 1);
        assert_eq!(control.check(), Ok(()));
    }

    struct LargeReply;

    impl Endpoint for LargeReply {
        fn name(&self) -> &str {
            "large"
        }

        fn instructions(&self) -> String {
            String::new()
        }

        fn methods(&self) -> Vec<Method> {
            vec![]
        }

        fn call(&mut self, _: &str, _: &Params) -> Reply {
            Reply {
                ok: true,
                text: "x".repeat(4 * 1024 * 1024),
                data: Value::map([]),
                exact: Exactness::Exact,
            }
        }
    }

    #[test]
    fn full_disconnect_releases_a_response_writer_with_a_full_socket_buffer() {
        let (mut client, server) = UnixStream::pair().unwrap();
        client.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let (finished, done) = mpsc::channel();
        let serving = std::thread::spawn(move || {
            let result = connection(server, &mut Plain, &mut LargeReply);
            finished.send(result.map_err(|error| error.kind())).unwrap();
        });
        client.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"large\"}\n").unwrap();
        client.shutdown(Shutdown::Write).unwrap();
        // Reading one byte proves response serialization started. The remaining
        // multi-megabyte response cannot fit in the socket's send buffer.
        client.read_exact(&mut [0_u8]).unwrap();
        drop(client);

        let result = done.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(result.is_err_and(|kind| matches!(
            kind,
            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
        )));
        serving.join().unwrap();
    }

    #[test]
    fn the_socket_is_for_its_owner_alone() {
        let path = place("owner");

        let _listener = bind(&path).unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_socket_left_by_an_ended_server_is_replaced_and_a_live_one_is_not() {
        let path = place("stale");
        drop(UnixListener::bind(&path).unwrap());

        let listener = bind(&path).unwrap();
        let refused = bind(&path);

        assert!(refused.is_err_and(|reason| reason.contains("another server")));
        drop(listener);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_file_that_is_not_a_socket_is_left_alone() {
        let path = place("file");
        std::fs::write(&path, "keep").unwrap();

        let refused = bind(&path);

        assert!(refused.is_err_and(|reason| reason.contains("is not a socket")));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep");
        std::fs::remove_file(&path).unwrap();
    }
}
