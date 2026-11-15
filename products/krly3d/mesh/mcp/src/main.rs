use krly3d_mesh_engine::runner::{MeshRunner, RunnerLimits};
use krly3d_mesh_engine::MeshEngine;
use krly3d_mesh_mcp::{serve_stdio, Server};
use krlyxd::SystemClock;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::process::ExitCode;
use std::sync::Arc;

const MAX_MODEL_BYTES: u64 = 16 * 1024 * 1024;
const USAGE: &str = "Usage: krly3d-mesh-mcp --model PATH\nServe mesh operation tools over MCP stdio (2025-11-25). Model input is limited to 16 MiB.";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report(&format!("krly3d-mesh-mcp: {error}"));
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let first = args.next();
    if first.as_deref() == Some(std::ffi::OsStr::new("--help")) {
        if args.next().is_some() {
            return Err(USAGE.to_owned());
        }
        report(USAGE);
        return Ok(());
    }
    if first != Some(OsString::from("--model")) {
        return Err(USAGE.to_owned());
    }
    let path = args.next().ok_or_else(|| USAGE.to_owned())?;
    if args.next().is_some() {
        return Err(USAGE.to_owned());
    }
    let file = File::open(&path).map_err(|error| format!("cannot open model: {error}"))?;
    let mut bytes = Vec::new();
    file.take(MAX_MODEL_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read model: {error}"))?;
    if bytes.len() as u64 > MAX_MODEL_BYTES {
        return Err("model exceeds 16 MiB".to_owned());
    }
    let text = std::str::from_utf8(&bytes).map_err(|error| format!("model is not UTF-8: {error}"))?;
    let engine = MeshEngine::from_k3d(text).map_err(|error| format!("{}: {}", error.code, error.message))?;
    let runner = MeshRunner::new(RunnerLimits::default(), Arc::new(SystemClock::default()))
        .map_err(|error| error.message)?;
    let server = Server::new(engine, runner);
    serve_stdio(server).map_err(|error| error.to_string())
}

/// Diagnostics are best effort: a full stderr pipe cannot keep shutdown alive.
fn report(message: &str) {
    #[cfg(unix)]
    let output = {
        use std::os::fd::AsFd;
        let Ok(descriptor) = io::stderr().as_fd().try_clone_to_owned() else {
            return;
        };
        File::from(descriptor)
    };
    #[cfg(not(unix))]
    let output = io::stderr();
    let message: String = message.chars().take(4096).collect();
    let (sent, received) = std::sync::mpsc::sync_channel(1);
    if std::thread::Builder::new()
        .name("mesh-mcp-diagnostic".into())
        .spawn(move || {
            let mut output = output;
            let _ = writeln!(output, "{message}");
            let _ = sent.send(());
        })
        .is_ok()
    {
        let _ = received.recv_timeout(std::time::Duration::from_millis(20));
    }
}
