use krly3d_view_mcp::{serve_stdio, Server};
use krly3d_view_renderer::ViewEngine;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read, Write};
use std::process::ExitCode;

const MAX_MODEL_BYTES: u64 = 16 * 1024 * 1024;
const USAGE: &str = "Usage: krly3d-view-mcp --model PATH\nServe read-only view rendering and bounded background operations over MCP stdio (2025-11-25).\nModel input is limited to 16 MiB; incoming NDJSON messages to 1 MiB.\nLegacy view_render blocks input; view_start/next/cancel/release keep computation in background. EOF cancels operations. Stop with SIGINT (Ctrl-C).";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report(&format!("krly3d-view-mcp: {error}"));
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let first = args.next();
    if first.as_deref() == Some(OsStr::new("--help")) {
        if args.next().is_some() {
            return Err(USAGE.to_owned());
        }
        report(USAGE);
        return Ok(());
    }
    if first.as_deref() != Some(OsStr::new("--model")) {
        return Err(USAGE.to_owned());
    }
    let path = args.next().ok_or_else(|| USAGE.to_owned())?;
    if args.next().is_some() {
        return Err(USAGE.to_owned());
    }
    let file = File::open(path).map_err(|error| format!("cannot open model: {error}"))?;
    let mut bytes = Vec::new();
    file.take(MAX_MODEL_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read model: {error}"))?;
    if bytes.len() as u64 > MAX_MODEL_BYTES {
        return Err("model exceeds 16 MiB".to_owned());
    }
    let text = std::str::from_utf8(&bytes).map_err(|error| format!("model is not UTF-8: {error}"))?;
    let engine = ViewEngine::from_k3d(text).map_err(|error| format!("{}: {}", error.code, error.message))?;
    serve_stdio(Server::new(engine)).map_err(|error| error.to_string())
}

fn report(message: &str) {
    let _ = writeln!(io::stderr().lock(), "{message}");
}
