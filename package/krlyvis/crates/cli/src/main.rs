//! The `krlyvis` program: reads its command line and hands the vision session to a
//! transport of krly3d.

use krly3d_cli::endpoint::Endpoint;
use krly3d_cli::{dialect, shell};
use krlyvis_cli::args::{self, Action};
use krlyvis_cli::session::VisionSession;
use std::io::{self, Write};
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
            let _ = writeln!(io::stderr(), "krlyvis: {message}");
            ExitCode::from(MISUSED)
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let words = std::env::args_os()
        .skip(1)
        .map(|word| word.into_string().map_err(|word| format!("the argument {word:?} is not text")))
        .collect::<Result<Vec<String>, String>>()?;
    let mut session = VisionSession::standard();
    match args::parse(&words)? {
        Action::Help => say(&args::usage(), true),
        Action::Serve { dialect } => serve(&dialect, &mut session),
        Action::Call { method, arguments } => {
            let params = shell::params(&session.methods(), &method, &arguments)?;
            let reply = session.call(&method, &params);
            say(&reply.text, reply.ok)
        }
    }
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
