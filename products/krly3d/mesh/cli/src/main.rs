//! Single-shot v2 adapter. Model construction and all geometry stay in the engine.

mod args;
mod input;
mod output;

use krly3d_mesh::operation::{self, decode_operation_call, encode_event, MeshEventBody, OperationCall};
use krly3d_mesh::{diagnostic, Diagnostic};
use krly3d_mesh_engine::runner::{MeshRunner, RunnerLimits};
use krly3d_mesh_engine::MeshEngine;
use krlyxd::SystemClock;
use serde_json::json;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const TICK: Duration = Duration::from_millis(10);

struct Failure {
    status: u8,
    diagnostic: Diagnostic,
}

impl Failure {
    fn cancelled(message: &str) -> Self {
        Self { status: 130, diagnostic: diagnostic("cancelled", message) }
    }
}

impl From<Diagnostic> for Failure {
    fn from(diagnostic: Diagnostic) -> Self {
        Self { status: 1, diagnostic }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(status) => ExitCode::from(status),
        Err(error) => {
            output::report(&error.diagnostic);
            ExitCode::from(error.status)
        }
    }
}

fn run() -> Result<u8, Failure> {
    let interrupted = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&interrupted))
        .map_err(|error| diagnostic("io", format!("cannot install SIGINT handler: {error}")))?;
    match args::parse(std::env::args_os().skip(1))? {
        args::Invocation::Help => {
            output::line(args::USAGE.trim_end().to_owned(), &interrupted, || Ok(()))?;
        }
        args::Invocation::Describe => {
            output::line(describe(), &interrupted, || Ok(()))?;
        }
        args::Invocation::Mesh { model, request } => {
            let model = input::read(Some(model), input::MODEL_LIMIT, &interrupted)?;
            let request = input::read(request, input::REQUEST_LIMIT, &interrupted)?;
            let call = decode_operation_call(&request)?;
            let engine = MeshEngine::from_k3d(&model)?;
            if interrupted.load(Ordering::Relaxed) {
                return Err(Failure::cancelled("interrupted before operation acceptance"));
            }
            return mesh(&engine, call, &interrupted);
        }
    }
    Ok(0)
}

fn mesh(engine: &MeshEngine, call: OperationCall, interrupted: &AtomicBool) -> Result<u8, Failure> {
    let limits = RunnerLimits { max_live_workers: 1, max_retained_operations: 1, ..RunnerLimits::default() };
    let runner = MeshRunner::new(limits, Arc::new(SystemClock::default()))?;
    let id = runner.start(engine, call)?;
    let stream = || -> Result<u8, Failure> {
        loop {
            if interrupted.load(Ordering::Relaxed) {
                runner.cancel(&id)?;
            }
            output::check_connection()?;
            let Some(event) = runner.next(&id)? else {
                std::thread::sleep(TICK);
                continue;
            };
            let status = match &event.body {
                MeshEventBody::Started(_) => None,
                MeshEventBody::Completed(_) => Some(0),
                MeshEventBody::Failed(_) => Some(1),
                MeshEventBody::Cancelled => Some(130),
            };
            output::line(encode_event(&event)?, interrupted, || runner.cancel(&id).map_err(Failure::from))?;
            if let MeshEventBody::Failed(error) = &event.body {
                output::report(error);
            }
            if let Some(status) = status {
                return Ok(status);
            }
        }
    };
    let result = stream();
    if result.is_err() {
        // Includes broken stdout during computation or a blocked event write.
        let _ = runner.cancel(&id);
    }
    result
}

fn describe() -> String {
    let descriptor = operation::descriptor();
    let version = |version: krly3d_mesh::ApiVersion| json!({"major": version.major, "minor": version.minor});
    json!({
        "package": descriptor.package.as_str(), "package_version": descriptor.package_version,
        "protocol": version(descriptor.protocol), "api": version(descriptor.api),
        "engine_revision": descriptor.engine_revision.as_str(), "operation": descriptor.operation,
        "synchronous": descriptor.synchronous, "read_only": descriptor.read_only,
        "cancellation": descriptor.cancellation, "timeouts": descriptor.timeouts,
        "supported_events": descriptor.supported_events, "coordinates": descriptor.coordinates,
        "length_unit": descriptor.length_unit, "area_unit": descriptor.area_unit,
        "volume_unit": descriptor.volume_unit, "artifact_format": descriptor.artifact_format,
        "call_schema": descriptor.call_schema, "input_schema": descriptor.input_schema,
        "output_schema": descriptor.output_schema,
    })
    .to_string()
}
