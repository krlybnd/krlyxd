//! Render through the typed API; a usage example, not a standalone wire adapter.
use krly3d_view_api::{Backend, RenderCall, RenderInput, RequestId};
use krly3d_view_renderer::ViewEngine;
use std::error::Error;
use std::io;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let model = args.next().ok_or("usage: render_snapshot MODEL.k3d NODE OUTPUT.png [raycast|mesh]")?;
    let node = args.next().ok_or("missing node")?;
    let output = args.next().ok_or("missing PNG output path")?;
    let backend = match args.next().as_deref() {
        None | Some("raycast") => Backend::Raycast,
        Some("mesh") => Backend::Mesh,
        Some(_) => return Err("backend must be raycast or mesh".into()),
    };
    if args.next().is_some() {
        return Err("unexpected extra argument".into());
    }
    let engine = ViewEngine::from_k3d(&std::fs::read_to_string(&model)?)
        .map_err(|error| io::Error::other(format!("{}: {}", error.code, error.message)))?;
    let mut input = RenderInput::new(node);
    input.width = 256;
    input.height = 192;
    input.backend = backend;
    let result = engine
        .call(RenderCall::new(RequestId::new("example"), input))
        .map_err(|error| io::Error::other(format!("{}: {}", error.code, error.message)))?;
    // Output creation is explicit and does not overwrite an existing file.
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(output)?;
    file.write_all(&result.png)?;
    println!(
        "{} {}x{} source={} render_ms={:.3} encoding_ms={:.3} faces={} achieved_error=unknown",
        result.provenance.input.node,
        result.width,
        result.height,
        result.provenance.source.source,
        result.timing.render_ms,
        result.timing.encoding_ms,
        result.faces.len()
    );
    Ok(())
}
