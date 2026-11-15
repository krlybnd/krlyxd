//! File/JSON output adapter for the typed local host; no subprocess orchestration.

mod io_control;
mod lifecycle;
mod options;
mod output;
mod parts;
mod sheet;
mod stream;

use krly3d_reconstruction::Host;
use output::diag;
use std::sync::Arc;
use std::time::Instant;

fn main() {
    if let Err(message) = run() {
        io_control::report(&message);
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let Some(options) = options::Options::parse(std::env::args().skip(1))? else {
        println!("{}", options::HELP);
        return Ok(());
    };
    let started = Instant::now();
    let lifecycle = lifecycle::Lifecycle::new(options.timeout)?;
    let control = Arc::clone(&lifecycle.control);
    lifecycle::check(&control)?;
    let mut stdout =
        io_control::ControlledWriter::stdout(Arc::clone(&control)).map_err(|error| error.to_string())?;
    if options.sheet.is_some() {
        return sheet::run(options, started, control, stdout);
    }
    let call = options.read_call(&control)?;
    if options.stream {
        return stream::run(call, options.settings, &options.output, started, control, stdout);
    }
    let mut host = Host::default();
    let job = host.replace_drawing_controlled(call, options.settings, Arc::clone(&control)).map_err(diag)?;
    let completed = job.run().map_err(diag)?;
    let result = host.publish(completed).map_err(diag)?;
    let computation_ms = started.elapsed().as_secs_f64() * 1000.0;
    output::export(
        &result,
        &host.events_after(0).map_err(diag)?,
        &options.output,
        started,
        computation_ms,
        &mut stdout,
        &control,
    )
}
