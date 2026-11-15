use crate::Server;
use serde_json::Value;
use std::io::{self, BufRead, Write};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::{Duration, Instant};

/// Maximum bytes before the newline of one UTF-8 JSON-RPC message.
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

const OUTPUT_QUEUE: usize = 8;
const SHUTDOWN_GRACE: Duration = Duration::from_secs(1);
const CONNECTION_POLL: Duration = Duration::from_millis(20);

enum Event {
    Frame(Vec<u8>),
    InputClosed(io::Result<()>),
    Written(io::Result<()>),
}

/// Consume a connection and cancel its runner on EOF, read/write failure, or
/// output saturation. OS reads and writes run on separate, bounded pumps: a full
/// stdout pipe never prevents the coordinator from observing EOF or write errors.
///
/// EOF cancels work immediately, then allows up to one second to flush queued
/// responses. A blocked OS pump is detached, never joined. The binary exits when
/// this function returns. At most eight responses can queue behind the writer;
/// exceeding that backpressure limit terminates the connection and cancels work.
pub fn serve(
    server: Server,
    input: impl BufRead + Send + 'static,
    output: impl Write + Send + 'static,
) -> io::Result<()> {
    serve_with_check(server, input, output, || Ok(()))
}

/// Real stdio additionally observes Unix stdout hangup/error while stdin is idle.
pub fn serve_stdio(server: Server) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::fd::AsFd;
        // Never let a detached writer own Rust's global stdout lock: runtime
        // cleanup would otherwise wait for that lock even after serve returns.
        let descriptor = io::stdout().as_fd().try_clone_to_owned()?;
        serve_with_check(
            server,
            io::BufReader::new(io::stdin()),
            std::fs::File::from(descriptor),
            output_connected,
        )
    }
    #[cfg(not(unix))]
    serve_with_check(server, io::BufReader::new(io::stdin()), io::stdout(), output_connected)
}

fn serve_with_check(
    server: Server,
    input: impl BufRead + Send + 'static,
    output: impl Write + Send + 'static,
    check: impl Fn() -> io::Result<()>,
) -> io::Result<()> {
    let (events, receive) = mpsc::sync_channel(4);
    let (write, responses) = mpsc::sync_channel(OUTPUT_QUEUE);
    let read_events = events.clone();
    std::thread::Builder::new().name("mesh-mcp-input".into()).spawn(move || read(input, read_events))?;
    std::thread::Builder::new()
        .name("mesh-mcp-output".into())
        .spawn(move || write_responses(output, responses, events))?;
    coordinate(server, receive, write, check)
}

fn coordinate(
    server: Server,
    receive: Receiver<Event>,
    write: SyncSender<Value>,
    check: impl Fn() -> io::Result<()>,
) -> io::Result<()> {
    let mut server = Some(server);
    let mut outstanding = 0;
    let mut shutdown: Option<Instant> = None;
    loop {
        check()?;
        if shutdown.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "MCP output did not drain after EOF"));
        }
        let event = match receive.recv_timeout(CONNECTION_POLL) {
            Ok(event) => event,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => return Err(io::Error::new(io::ErrorKind::BrokenPipe, "MCP pumps stopped")),
        };
        match event {
            Event::Frame(frame) => {
                let Some(server) = &mut server else { continue };
                let response = match std::str::from_utf8(&frame) {
                    Ok(text) => server.handle(text),
                    Err(_) => {
                        Some(crate::json::Failure::new(-32700, "Invalid UTF-8 JSON").response(Value::Null))
                    }
                };
                if let Some(response) = response {
                    write.try_send(response).map_err(|_| {
                        io::Error::new(io::ErrorKind::WouldBlock, "MCP output queue saturated")
                    })?;
                    outstanding += 1;
                }
            }
            Event::InputClosed(result) => {
                // Drop MeshRunner now, before waiting for output to drain.
                drop(server.take());
                result?;
                shutdown = Some(Instant::now() + SHUTDOWN_GRACE);
                if outstanding == 0 {
                    return Ok(());
                }
            }
            Event::Written(result) => {
                result?;
                outstanding -= 1;
                if shutdown.is_some() && outstanding == 0 {
                    return Ok(());
                }
            }
        }
    }
}

#[cfg(unix)]
fn output_connected() -> io::Result<()> {
    let mut descriptor = libc::pollfd { fd: libc::STDOUT_FILENO, events: 0, revents: 0 };
    // SAFETY: poll observes one initialized descriptor for this call and neither
    // closes stdout nor changes its flags. No pointer outlives this stack value.
    let result = unsafe { libc::poll(&mut descriptor, 1, 0) };
    if result < 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    } else if descriptor.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
        return Err(io::Error::new(io::ErrorKind::BrokenPipe, "MCP stdout disconnected"));
    }
    Ok(())
}

#[cfg(not(unix))]
fn output_connected() -> io::Result<()> {
    Ok(())
}

fn read(mut input: impl BufRead, events: SyncSender<Event>) {
    loop {
        match frame(&mut input) {
            Ok(Some(frame)) => {
                if events.send(Event::Frame(frame)).is_err() {
                    return;
                }
            }
            result => {
                let _ = events.send(Event::InputClosed(result.map(|_| ())));
                return;
            }
        }
    }
}

fn write_responses(output: impl Write, responses: Receiver<Value>, events: SyncSender<Event>) {
    let mut output = io::BufWriter::with_capacity(64 * 1024, output);
    for response in responses {
        let result = (|| {
            serde_json::to_writer(&mut output, &response)?;
            output.write_all(b"\n")?;
            output.flush()
        })();
        let failed = result.is_err();
        if events.send(Event::Written(result)).is_err() || failed {
            return;
        }
    }
}

fn frame(input: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut frame = Vec::new();
    loop {
        let bytes = input.fill_buf()?;
        if bytes.is_empty() {
            return if frame.is_empty() {
                Ok(None)
            } else {
                Err(io::Error::new(io::ErrorKind::InvalidData, "unterminated MCP message"))
            };
        }
        let end = bytes.iter().position(|byte| *byte == b'\n');
        let count = end.unwrap_or(bytes.len());
        if count > MAX_MESSAGE_BYTES - frame.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "MCP message exceeds 1 MiB"));
        }
        frame.extend_from_slice(&bytes[..count]);
        input.consume(count + usize::from(end.is_some()));
        if end.is_some() {
            return Ok(Some(frame));
        }
    }
}
