//! One session served over a Unix domain socket, so that it outlives every client.
//!
//! Served on standard input, a session lasts as long as the one client that started it.
//! Behind a socket it lasts as long as the program: clients come and go, one connection at
//! a time, and each finds the model and the peers as the last one left them. Only the
//! user may connect: the socket is made readable and writable by its owner alone.

use crate::dialect::{self, Dialect};
use crate::endpoint::Endpoint;
use std::io::{self, BufReader, Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

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
    dialect::serve(dialect, session, reading, &stream)
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
    use super::{bind, connection};
    use crate::dialect::plain::Plain;
    use crate::dialect::testing::session;
    use std::io::{Read, Write};
    use std::net::Shutdown;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;

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
