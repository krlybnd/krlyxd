//! Single-shot planar adapter. The API owns the wire format; the engine owns geometry.

mod args;
mod input;

use krly3d_brep::planar::{decode_call, descriptor, encode_result};
use krly3d_brep::{Diagnostic, Id};
use krly3d_brep_engine::build_planar;
use serde_json::json;
use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let message = json!({"code": error.code.as_str(), "message": error.message});
            let _ = writeln!(io::stderr().lock(), "{message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Diagnostic> {
    let output = match args::parse(std::env::args_os().skip(1))? {
        args::Invocation::Help => args::USAGE.trim_end().to_owned(),
        args::Invocation::Describe => descriptor().to_string(),
        args::Invocation::Build { request } => {
            let text = input::read(request.as_deref())?;
            encode_result(&build_planar(decode_call(&text)?)?)?
        }
    };
    // Complete decoding, building, and encoding before emitting the sole result.
    // Execution is synchronous; SIGINT retains the default OS behavior.
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(output.as_bytes())
        .and_then(|()| stdout.write_all(b"\n"))
        .and_then(|()| stdout.flush())
        .map_err(|error| diagnostic("io", format!("cannot write stdout: {error}")))
}

fn diagnostic(code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic { code: Id::new(code), message: message.into() }
}
