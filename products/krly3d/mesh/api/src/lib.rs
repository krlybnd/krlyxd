//! Mesh contract, independent of geometry engines and transports.
//!
//! Version 1.0 is synchronous: no operation handles, events, cancellation or deadlines.
//! Adding controlled execution requires an explicit descriptor/version decision.

pub mod codec;
mod descriptor;
pub mod operation;
pub mod preview;
pub mod print;

pub use descriptor::{descriptor, Descriptor};
pub use krlyxd::{ApiVersion, ArtifactId, Diagnostic, Exactness, Id, Length, LengthUnit, RequestId};

pub const PROTOCOL_VERSION: ApiVersion = ApiVersion { major: 1, minor: 0 };
pub const API_VERSION: ApiVersion = ApiVersion { major: 1, minor: 0 };
pub const DEFAULT_TOLERANCE: f64 = 0.05;
pub const DEFAULT_TRIANGLES: bool = false;
pub const ARTIFACT_FORMAT: &str = "krly3d.mesh.triangles/1";
/// Bump when the bridge/solver contract changes; not a portable build digest.
pub const ENGINE_REVISION: &str = "krly3d.mesh.brep/2";
pub const MAX_SAFE_INTEGER: u64 = (1_u64 << 53) - 1;

#[derive(Clone, Debug, PartialEq)]
pub struct MeshInput {
    pub node: String,
    pub tolerance: Length,
    pub triangles: bool,
}

impl MeshInput {
    pub fn new(node: impl Into<String>) -> Self {
        Self {
            node: node.into(),
            tolerance: Length { value: DEFAULT_TOLERANCE, unit: LengthUnit::Model },
            triangles: DEFAULT_TRIANGLES,
        }
    }

    /// Engine-independent validation. The bridge also checks its model's accuracy.
    pub fn validate(&self) -> Result<(), Diagnostic> {
        if self.tolerance.unit != LengthUnit::Model {
            return Err(diagnostic("input", "mesh tolerance only supports model units"));
        }
        if !self.tolerance.value.is_finite() || self.tolerance.value <= 0.0 {
            return Err(diagnostic(
                "input",
                format!("a mesh tolerance must be above the length tolerance, got {}", self.tolerance.value),
            ));
        }
        if self.node.is_empty() {
            return Err(diagnostic("input", "mesh needs a node"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeshCall {
    pub request_id: RequestId,
    pub protocol: ApiVersion,
    pub api: ApiVersion,
    pub timeout_ms: Option<u64>,
    pub input: MeshInput,
}

impl MeshCall {
    pub fn new(request_id: RequestId, input: MeshInput) -> Self {
        Self { request_id, protocol: PROTOCOL_VERSION, api: API_VERSION, timeout_ms: None, input }
    }

    pub fn validate(&self) -> Result<(), Diagnostic> {
        if self.protocol != PROTOCOL_VERSION || self.api != API_VERSION {
            return Err(diagnostic("unsupported_version", "mesh requires protocol 1.0 and mesh API 1.0"));
        }
        if self.timeout_ms.is_some_and(|timeout| timeout > MAX_SAFE_INTEGER) {
            return Err(diagnostic("input", "timeout_ms exceeds the JSON-safe integer range"));
        }
        if self.timeout_ms.is_some() {
            return Err(diagnostic("unsupported", "mesh timeouts require controlled execution"));
        }
        if self.request_id.as_str().is_empty() {
            return Err(diagnostic("input", "mesh needs a request_id"));
        }
        self.input.validate()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeshData {
    /// Right-handed, Z-up world coordinates in model units.
    pub points: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Quality {
    pub exactness: Exactness,
    /// Requested tolerance does not establish achieved error.
    pub positional_uncertainty: Option<Length>,
    pub confidence: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Provenance {
    pub package_version: String,
    pub engine_revision: Id,
    /// Session identity, resolved node version and build key; not a durable digest.
    pub source: Id,
    pub input: MeshInput,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeshArtifact {
    pub id: ArtifactId,
    pub revision: u32,
    pub data: MeshData,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeshResult {
    pub points: u64,
    pub triangles: u64,
    /// Model units cubed.
    pub volume: f64,
    /// Model units squared.
    pub area: f64,
    /// Edge-balance statistic, not a manifold or self-intersection proof.
    pub open: u64,
    pub quality: Quality,
    pub provenance: Provenance,
    pub mesh: Option<MeshArtifact>,
}

/// Neutral diagnostics shared by validation and the explicit legacy bridge.
pub fn diagnostic(code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic { code: Id::new(code), message: message.into() }
}
