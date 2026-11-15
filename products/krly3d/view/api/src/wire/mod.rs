//! Synchronous view wire v1. No operation handles, deadlines or event stream.
pub(crate) mod request;
mod schema;

pub use request::{decode_call, encode_call};
pub use schema::descriptor;

use crate::{
    diagnostic, Backend, Camera, Diagnostic, Exactness, Length, LengthUnit, RenderInput, RenderResult,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};

fn version() -> Value {
    json!({"major": 1, "minor": 0})
}

fn length(value: Length) -> Value {
    json!({"value": value.value, "unit": match value.unit {LengthUnit::Model => "model", LengthUnit::Metre => "metre"}})
}

pub(crate) fn input(input: &RenderInput) -> Value {
    let camera = match &input.camera {
        Camera::Named(name) => json!({"kind":"named","name":name.as_str()}),
        Camera::Orthographic { toward, up } => json!({"kind":"orthographic","toward":toward,"up":up}),
        Camera::Pinhole { eye, toward, up, focal, centre } => {
            let mut value = json!({"kind":"pinhole","eye":eye,"toward":toward,"up":up,"focal":focal});
            if let Some(centre) = centre {
                value["centre"] = json!(centre);
            }
            value
        }
    };
    json!({"node":input.node,"width":input.width,"height":input.height,
        "backend":match input.backend { Backend::Raycast => "raycast", Backend::Mesh => "mesh" },
        "camera":camera,"tolerance":length(input.tolerance)})
}

/// Encode a complete image, rejecting invalid numeric metadata before JSON could
/// silently turn a non-finite float into null. PNG remains an opaque image payload.
pub fn encode_result(result: &RenderResult) -> Result<String, Diagnostic> {
    validate_result(result)?;
    let quality = &result.quality;
    let exactness = match quality.exactness {
        Exactness::Exact => json!({"kind":"exact"}),
        Exactness::Numeric { tolerance } => json!({"kind":"numeric","tolerance":tolerance}),
        Exactness::Sampled { samples } => json!({"kind":"sampled","samples":samples}),
    };
    let source = &result.provenance.source;
    Ok(json!({"protocol":version(),"api":version(),"request_id":result.request_id.as_str(),
        "width":result.width,"height":result.height,
        "image":{"mime_type":"image/png","encoding":"base64","data":STANDARD.encode(&result.png)},
        "faces":result.faces.iter().map(|face| json!({"name":face.name,"pixels":face.pixels})).collect::<Vec<_>>(),
        "quality":{"exactness":exactness,"requested_tolerance":quality.requested_tolerance.map(length),
            "positional_uncertainty":quality.positional_uncertainty.map(length),"confidence":quality.confidence},
        "provenance":{"source":{"source":source.source.as_str(),"session":source.session.as_str(),
            "node":source.node,"build_key":source.build_key},"input":input(&result.provenance.input),
            "engine_revision":result.provenance.engine_revision.as_str()},
        "timing":{"render_ms":result.timing.render_ms,"encoding_ms":result.timing.encoding_ms}
    }).to_string())
}

fn validate_result(result: &RenderResult) -> Result<(), Diagnostic> {
    result.provenance.input.validate()?;
    let bad = || diagnostic("output", "invalid render result metadata");
    if result.request_id.as_str().is_empty()
        || result.width != result.provenance.input.width
        || result.height != result.provenance.input.height
        || !result.png.starts_with(b"\x89PNG\r\n\x1a\n")
        || [result.timing.render_ms, result.timing.encoding_ms].iter().any(|v| !v.is_finite() || *v < 0.0)
    {
        return Err(bad());
    }
    let source = &result.provenance.source;
    if source.source.as_str().is_empty()
        || source.session.as_str().is_empty()
        || source.node.is_empty()
        || result.provenance.engine_revision.as_str().is_empty()
    {
        return Err(bad());
    }
    let pixels = u64::from(result.width) * u64::from(result.height);
    let total = result.faces.iter().try_fold(0_u64, |total, face| total.checked_add(face.pixels));
    if total.is_none_or(|total| total > pixels) {
        return Err(bad());
    }
    for quantity in
        [result.quality.requested_tolerance, result.quality.positional_uncertainty].into_iter().flatten()
    {
        if krlyxd::PositionalUncertainty::measured(quantity).is_err() {
            return Err(bad());
        }
    }
    if result.quality.confidence.is_some_and(|v| krlyxd::Confidence::new(v).is_err()) {
        return Err(bad());
    }
    match result.quality.exactness {
        Exactness::Numeric { tolerance } if !tolerance.is_finite() || tolerance <= 0.0 => return Err(bad()),
        Exactness::Sampled { samples: 0 } => return Err(bad()),
        _ => {}
    }
    Ok(())
}

/// Typed common descriptor metadata, shared with other domain contracts.
pub fn metadata() -> krlyxd::ContractMetadata<'static> {
    krlyxd::ContractMetadata {
        package: krlyxd::PackageMetadata { name: env!("CARGO_PKG_NAME"), version: env!("CARGO_PKG_VERSION") },
        protocol: crate::API_VERSION,
        api: crate::API_VERSION,
        engine_revision: crate::ENGINE_REVISION,
        coordinates: krlyxd::Coordinates::RIGHT_HANDED_Z_UP,
        length_unit: krlyxd::LengthUnit::Model,
    }
}
