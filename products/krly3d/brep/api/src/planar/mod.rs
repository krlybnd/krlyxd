//! A single synchronous, stateless extrusion of supplied planar polygon loops.
//!
//! The first loop is the outer boundary and subsequent loops are holes, independent
//! of winding. Coordinates are preserved in the XY plane and extruded from Z=0
//! toward +Z. Model units do not establish any physical scale or image accuracy.

mod codec;
mod descriptor;

pub use codec::{decode_call, encode_result};
pub use descriptor::descriptor;

use crate::{Diagnostic, Id, SnapshotIdentity};
use krlyxd::{Exactness, Length};

pub const PROTOCOL: &str = "krly3d.brep.planar/1";
/// Revision of this composition of the native profile/extrude and k3d providers.
pub const ENGINE_REVISION: &str = "krly3d.brep.planar/1";
pub const MAX_VERTICES: usize = 256;
pub const MAX_REQUEST_ID_BYTES: usize = 256;
pub const MAX_SOURCE_BYTES: usize = 1024;
pub const MAX_CALL_BYTES: usize = 1_048_576;
/// Conservative provider domain; prevents overflow in norms, areas and volumes.
pub const MAX_ABS_COORDINATE: f64 = 1.0e9;
pub const MAX_HEIGHT: f64 = 1.0e9;

/// Typed common metadata; the wire protocol retains its domain-specific string.
pub fn metadata() -> krlyxd::ContractMetadata<'static> {
    krlyxd::ContractMetadata {
        package: krlyxd::PackageMetadata { name: env!("CARGO_PKG_NAME"), version: env!("CARGO_PKG_VERSION") },
        protocol: krlyxd::ApiVersion { major: 1, minor: 0 },
        api: krlyxd::ApiVersion { major: 1, minor: 0 },
        engine_revision: ENGINE_REVISION,
        coordinates: krlyxd::Coordinates::RIGHT_HANDED_Z_UP,
        length_unit: krlyxd::LengthUnit::Model,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlanarCall {
    pub protocol: String,
    pub request_id: String,
    pub input: PlanarInput,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlanarInput {
    pub polygons: Vec<Vec<[f64; 2]>>,
    pub height: f64,
    pub length_unit: String,
    /// Caller-supplied provenance claim, never a verified image or content hash.
    pub source: String,
}

impl PlanarCall {
    /// Also required at the engine boundary for calls constructed directly in Rust.
    pub fn validate(&self) -> Result<(), Diagnostic> {
        if self.protocol != PROTOCOL {
            return Err(diagnostic("unsupported_version", format!("planar requires protocol {PROTOCOL}")));
        }
        bounded_text("request_id", &self.request_id, MAX_REQUEST_ID_BYTES)?;
        self.input.validate()
    }
}

impl PlanarInput {
    /// Structural validation only; native providers validate and build the geometry.
    pub fn validate(&self) -> Result<(), Diagnostic> {
        bounded_text("source", &self.source, MAX_SOURCE_BYTES)?;
        if self.length_unit != "model" {
            return Err(diagnostic("input", "length_unit must be model"));
        }
        if !self.height.is_finite() || self.height <= 0.0 {
            return Err(diagnostic("input", "height must be positive and finite"));
        }
        if self.height > MAX_HEIGHT {
            return Err(diagnostic("input", "height exceeds the native provider bound of 1e9 model units"));
        }
        if self.polygons.is_empty() {
            return Err(diagnostic("input", "polygons must contain an outer loop"));
        }
        let mut remaining = MAX_VERTICES;
        for polygon in &self.polygons {
            remaining = remaining
                .checked_sub(polygon.len())
                .ok_or_else(|| diagnostic("resource_limit", "polygons exceed 256 total vertices"))?;
            if polygon.len() < 3 {
                return Err(diagnostic("input", "each polygon must contain at least three vertices"));
            }
            if !polygon.iter().flatten().all(|number| number.is_finite()) {
                return Err(diagnostic("input", "polygon coordinates must be finite"));
            }
            if polygon.iter().flatten().any(|number| number.abs() > MAX_ABS_COORDINATE) {
                return Err(diagnostic(
                    "input",
                    "coordinates exceed the native provider bound of +/-1e9 model units",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlanarResult {
    pub protocol: String,
    pub request_id: String,
    /// Native k3d document written by the engine's document provider.
    pub model: String,
    pub node: String,
    /// The input source claim, copied without authentication or interpretation.
    pub source: String,
    pub provenance: SnapshotIdentity,
    pub quality: PlanarQuality,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlanarQuality {
    pub exactness: Exactness,
    /// Measurement uncertainty, unknown for an uncalibrated supplied contour.
    pub positional_uncertainty: Option<Length>,
    pub confidence: Option<f64>,
    pub assumptions: Vec<String>,
    pub accuracy_scope: String,
    pub source_claim: String,
}

impl Default for PlanarQuality {
    fn default() -> Self {
        Self {
            exactness: Exactness::Exact,
            positional_uncertainty: None,
            confidence: None,
            assumptions: vec![
                "The supplied planar contour is the intended boundary in model units".into(),
                "Uniform extrusion from z=0 along +Z with constant height and through holes".into(),
            ],
            accuracy_scope: "Exact analytic geometry from the supplied contour; not physical accuracy or verified reconstruction accuracy".into(),
            source_claim: "Caller-supplied provenance; not a verified image hash".into(),
        }
    }
}

pub(super) fn bounded_text(name: &str, text: &str, maximum: usize) -> Result<(), Diagnostic> {
    if text.trim().is_empty() || text.len() > maximum {
        return Err(diagnostic(
            "input",
            format!("{name} must be nonempty and at most {maximum} UTF-8 bytes"),
        ));
    }
    Ok(())
}

pub(super) fn diagnostic(code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic { code: Id::new(code), message: message.into() }
}
