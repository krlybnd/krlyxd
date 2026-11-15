//! Synchronous view v1 and bounded operation v2 CLI. Rendering belongs to ViewEngine.

mod args;
mod input;

use krly3d_view_api::wire::{decode_call, descriptor, encode_result};
use krly3d_view_api::{diagnostic, Diagnostic};
use krly3d_view_renderer::ViewEngine;
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
        args::Invocation::DescribeOperations => krly3d_view_api::operation::descriptor().to_string(),
        args::Invocation::Render { model, request, operations } => {
            let request = input::read(request.as_deref(), "request", input::REQUEST_LIMIT)?;
            if operations {
                let call = krly3d_view_api::operation::decode_operation_call(&request)?;
                let model = input::read(Some(&model), "model", input::MODEL_LIMIT)?;
                let engine = ViewEngine::from_k3d(&model)?;
                let runner = krly3d_view_renderer::runner::ViewRunner::standard();
                let id = runner.start(&engine, call)?;
                let mut output = io::stdout().lock();
                while let Some(event) = runner.wait_next(&id)? {
                    let text = krly3d_view_api::operation::encode_event(&event)?;
                    writeln!(output, "{text}")
                        .and_then(|()| output.flush())
                        .map_err(|e| diagnostic("io", e.to_string()))?;
                    if matches!(
                        event.body,
                        krly3d_view_api::operation::RenderEventBody::Failed(_)
                            | krly3d_view_api::operation::RenderEventBody::Cancelled
                    ) {
                        return Err(diagnostic(
                            "operation_failed",
                            "view operation did not complete; see terminal event",
                        ));
                    }
                }
                return Ok(());
            }
            let call = decode_call(&request)?;
            let model = input::read(Some(&model), "model", input::MODEL_LIMIT)?;
            let engine = ViewEngine::from_k3d(&model)?;
            encode_result(&engine.call(call)?)?
        }
    };
    // Finish rendering and encoding before writing the single result. No event
    // stream is used on this v1 path; v2 returns above. SIGINT keeps OS handling.
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(output.as_bytes())
        .and_then(|()| stdout.write_all(b"\n"))
        .and_then(|()| stdout.flush())
        .map_err(|error| diagnostic("io", format!("cannot write stdout: {error}")))
}
