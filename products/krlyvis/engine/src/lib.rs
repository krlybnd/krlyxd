//! Native drawing evidence composed only from the existing image/outline providers.
//! [`DrawingSource`] retains a decoded image for repeated component selection;
//! [`read_shared`] returns one immutable snapshot and [`read_drawing`] adapts it
//! to the wire result. No legacy CLI import, native document, global node, or session.

mod shared;
mod source;
mod value;

pub use shared::SharedDrawing;
pub use source::DrawingSource;

use krly3d_core::{CallControl, Error};
use krlyvis_api::{Diagnostic, DrawingCall, DrawingResult, Id};
use std::sync::Arc;

/// Read native shared evidence, then adapt it to this call's wire result.
/// `top@1` identifies only this returned snapshot.
pub fn read_drawing(call: DrawingCall) -> Result<DrawingResult, Diagnostic> {
    let request_id = call.request_id.clone();
    read_shared(call)?.to_result(&request_id)
}

/// Read one image, select a component with its holes, and place its exact pixel
/// corners in the top plane using the existing providers.
///
/// Typed validation and evidence stay native throughout this path. The returned
/// snapshot retains its datum by Arc and does not retain the image or request ID;
/// [`SharedDrawing::to_result`] is the explicit wire adapter.
pub fn read_shared(call: DrawingCall) -> Result<SharedDrawing, Diagnostic> {
    read_shared_controlled(call, Arc::new(CallControl::new()))
}

/// Read and select with one cooperative control, retaining it in neither owner.
/// External codec decode is uninterruptible inside the decoder; checks surround it.
pub fn read_shared_controlled(
    call: DrawingCall,
    control: Arc<CallControl>,
) -> Result<SharedDrawing, Diagnostic> {
    let seed = call.input.seed;
    DrawingSource::read_controlled(call, Arc::clone(&control))?.select_controlled(seed, control)
}

fn check_control(control: &CallControl) -> Result<(), Diagnostic> {
    control.check().map_err(|stop| provider_error(Error::from(stop)))
}

fn provider_error(error: Error) -> Diagnostic {
    diagnostic(error.kind.label(), error.message)
}

fn diagnostic(code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic { code: Id::new(code), message: message.into() }
}
