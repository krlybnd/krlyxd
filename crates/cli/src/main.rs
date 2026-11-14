//! The `krly3d` program: reads its command line, builds a session and hands it to a
//! transport.

use krly3d_cli::args::{self, Action, Invocation};
use krly3d_cli::endpoint::Endpoint;
use krly3d_cli::peer::{Joined, Peer, PeerAdapter, SharedPeer};
use krly3d_cli::{dialect, listen, shell};
use krly3d_core::Accuracy;
use krly3d_kernel::schema::Params;
use krly3d_kernel::Session;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

/// The call was made and did not do what was asked.
const FAILED: u8 = 1;
/// The program could not be run as asked.
const MISUSED: u8 = 2;

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(message) => {
            // If standard error is closed too, there is nowhere left to say so.
            let _ = writeln!(io::stderr(), "krly3d: {message}");
            ExitCode::from(MISUSED)
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let words = std::env::args_os()
        .skip(1)
        .map(|word| word.into_string().map_err(|word| format!("the argument {word:?} is not text")))
        .collect::<Result<Vec<String>, String>>()?;
    let invocation = args::parse(&words)?;
    match &invocation.action {
        Action::Help => say(&args::usage(), true),
        Action::Serve { dialect } => match &invocation.listen {
            Some(socket) => listen(socket, dialect, endpoint(&invocation)?.as_mut()),
            None => serve(dialect, endpoint(&invocation)?.as_mut()),
        },
        Action::Send { socket } => {
            listen::send(Path::new(socket), io::stdin().lock(), io::stdout().lock())?;
            Ok(ExitCode::SUCCESS)
        }
        Action::Call { method, arguments } => {
            let mut endpoint = endpoint(&invocation)?;
            let params = shell::params(&endpoint.methods(), method, arguments)?;
            let reply = endpoint.call(method, &params);
            say(&reply.text, reply.ok)
        }
    }
}

/// The session, joined with the peers that were named. The peers are started first, so
/// that the kernel can ask them too.
fn endpoint(invocation: &Invocation) -> Result<Box<dyn Endpoint>, String> {
    let peers: Vec<SharedPeer> = invocation
        .peers
        .iter()
        .map(|(name, program)| Peer::spawn(name, program).map(Peer::shared))
        .collect::<Result<_, _>>()?;
    let session = session(invocation, &peers)?;
    if peers.is_empty() {
        return Ok(Box::new(session));
    }
    let mut joined = Joined::new(session);
    for peer in peers {
        joined.join(peer)?;
    }
    Ok(Box::new(joined))
}

/// A standard kernel behind its one door, able to ask `peers`, with the model file loaded
/// if one was named.
fn session(invocation: &Invocation, peers: &[SharedPeer]) -> Result<Session, String> {
    let mut builder = krly3d_std::standard();
    krly3d_view::register(&mut builder);
    krly3d_exchange::register(&mut builder);
    for peer in peers {
        builder.plugin::<krly3d_kernel::ports::peer::Peer>(PeerAdapter::new(peer));
    }
    builder.accuracy(Accuracy::for_scale(invocation.scale).map_err(|error| error.to_string())?);
    let mut session = builder.session();
    if let Some(path) = &invocation.open {
        let text = std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
        let mut args = Params::new().with("text", text);
        if let Some(format) = Path::new(path).extension().and_then(|extension| extension.to_str()) {
            args.set("format", format);
        }
        let reply = session.call("open", &args);
        if !reply.ok {
            return Err(format!("{path}: {}", reply.text.trim_end()));
        }
    }
    Ok(session)
}

fn serve(name: &str, session: &mut dyn Endpoint) -> Result<ExitCode, String> {
    let mut dialect = dialect::named(name).ok_or_else(|| format!("there is no dialect `{name}`"))?;
    match dialect::serve(dialect.as_mut(), session, io::stdin().lock(), io::stdout().lock()) {
        Ok(()) => Ok(ExitCode::SUCCESS),
        // A client that hangs up has ended the connection, not broken it.
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(ExitCode::SUCCESS),
        Err(error) => Err(format!("the connection failed: {error}")),
    }
}

/// Serves the session on the socket at `socket` until the program is stopped.
fn listen(socket: &str, name: &str, session: &mut dyn Endpoint) -> Result<ExitCode, String> {
    let mut dialect = dialect::named(name).ok_or_else(|| format!("there is no dialect `{name}`"))?;
    let listener = listen::bind(Path::new(socket))?;
    listen::serve(&listener, dialect.as_mut(), session)?;
    Ok(ExitCode::SUCCESS)
}

/// Prints a text as whole lines: to standard output when all went well, else to
/// standard error with the exit code of a failed call.
fn say(text: &str, ok: bool) -> Result<ExitCode, String> {
    let ending = if text.ends_with('\n') { "" } else { "\n" };
    let written = if ok {
        write!(io::stdout().lock(), "{text}{ending}")
    } else {
        write!(io::stderr().lock(), "{text}{ending}")
    };
    let code = if ok { ExitCode::SUCCESS } else { ExitCode::from(FAILED) };
    match written {
        Ok(()) => Ok(code),
        // A reader that has stopped listening does not change what came of the call.
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(code),
        Err(error) => Err(format!("the reply could not be written: {error}")),
    }
}
