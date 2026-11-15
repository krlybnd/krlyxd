use krly3d_brep_mcp::{serve_stdio, Server};
use std::ffi::OsStr;
use std::io::{self, Write};
use std::process::ExitCode;

const USAGE: &str = "Usage: krly3d-brep-mcp
       krly3d-brep-mcp --help
Serve the synchronous brep_planar tool over MCP stdio (2025-11-25).
No model or file arguments; every call builds a new document without file writes.
Incoming UTF-8 NDJSON frames are limited to 1 MiB before the newline.
Complete initialize and notifications/initialized before calling tools.
Building and output block all input. No in-flight request cancellation.
SIGINT (Ctrl-C) uses default OS handling. --help prints to stderr.";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report(&format!("krly3d-brep-mcp: {error}"));
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    match args.next() {
        Some(arg) if arg == OsStr::new("--help") && args.next().is_none() => {
            report(USAGE);
            Ok(())
        }
        Some(_) => Err(USAGE.to_owned()),
        None => serve_stdio(Server::new()).map_err(|error| error.to_string()),
    }
}

fn report(text: &str) {
    let _ = writeln!(io::stderr().lock(), "{text}");
}
