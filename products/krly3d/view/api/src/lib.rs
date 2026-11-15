//! Typed view contract with a synchronous version 1.0 JSON boundary.

pub mod likeness;
pub mod operation;
pub mod wire;

pub use krly3d_brep::SnapshotIdentity;
pub use krlyxd::{ApiVersion, Diagnostic, Exactness, Id, Length, LengthUnit, RequestId};

pub const API_VERSION: ApiVersion = ApiVersion { major: 1, minor: 0 };
pub const ENGINE_REVISION: &str = "krly3d.view.snapshot/1";
pub const MAX_SIDE: u32 = 2048;
pub const DEFAULT_TOLERANCE: f64 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Raycast,
    Mesh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamedView {
    Iso,
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
}

impl NamedView {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Iso => "iso",
            Self::Front => "front",
            Self::Back => "back",
            Self::Left => "left",
            Self::Right => "right",
            Self::Top => "top",
            Self::Bottom => "bottom",
        }
    }
}

/// World coordinates are right-handed, Z-up, in model units. Pinhole focal length
/// and optional optical centre are pixels. Directions need not be normalized.
#[derive(Clone, Debug, PartialEq)]
pub enum Camera {
    Named(NamedView),
    Orthographic { toward: [f64; 3], up: [f64; 3] },
    Pinhole { eye: [f64; 3], toward: [f64; 3], up: [f64; 3], focal: f64, centre: Option<[f64; 2]> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderInput {
    /// One source node, optionally qualified with an explicit version.
    pub node: String,
    pub width: u32,
    pub height: u32,
    pub camera: Camera,
    pub backend: Backend,
    /// Requested approximation tolerance; used only by the mesh backend.
    pub tolerance: Length,
}

impl RenderInput {
    pub fn new(node: impl Into<String>) -> Self {
        Self {
            node: node.into(),
            width: 480,
            height: 360,
            camera: Camera::Named(NamedView::Iso),
            backend: Backend::Raycast,
            tolerance: Length { value: DEFAULT_TOLERANCE, unit: LengthUnit::Model },
        }
    }

    pub fn validate(&self) -> Result<(), Diagnostic> {
        if self.node.is_empty() {
            return Err(diagnostic("input", "render needs a node"));
        }
        if self.width == 0 || self.height == 0 || self.width > MAX_SIDE || self.height > MAX_SIDE {
            return Err(diagnostic("input", "render dimensions must be between 1 and 2048 pixels"));
        }
        if self.tolerance.unit != LengthUnit::Model
            || !self.tolerance.value.is_finite()
            || self.tolerance.value <= 0.0
        {
            return Err(diagnostic("input", "render tolerance must be positive finite model units"));
        }
        self.camera.validate()
    }
}

impl Camera {
    fn validate(&self) -> Result<(), Diagnostic> {
        match self {
            Self::Named(_) => Ok(()),
            Self::Orthographic { toward, up } => directions(toward, up),
            Self::Pinhole { eye, toward, up, focal, centre } => {
                if eye.iter().any(|x| !x.is_finite())
                    || !focal.is_finite()
                    || *focal <= 0.0
                    || !(1.0 / focal).is_finite()
                    || centre.is_some_and(|c| c.iter().any(|x| !x.is_finite()))
                {
                    return Err(diagnostic(
                        "input",
                        "pinhole values must be finite and focal length positive",
                    ));
                }
                directions(toward, up)
            }
        }
    }
}

fn directions(toward: &[f64; 3], up: &[f64; 3]) -> Result<(), Diagnostic> {
    // Structural validation only; the shared camera projection checks parallelism
    // and model-dependent accuracy.
    for vector in [toward, up] {
        if vector.iter().any(|x| !x.is_finite()) || vector.iter().all(|x| *x == 0.0) {
            return Err(diagnostic("input", "camera directions must be finite and nonzero"));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderCall {
    pub request_id: RequestId,
    pub api: ApiVersion,
    pub input: RenderInput,
}

impl RenderCall {
    pub fn new(request_id: RequestId, input: RenderInput) -> Self {
        Self { request_id, api: API_VERSION, input }
    }

    pub fn validate(&self) -> Result<(), Diagnostic> {
        if self.api != API_VERSION {
            return Err(diagnostic("unsupported_version", "render requires view API 1.0"));
        }
        if self.request_id.as_str().is_empty() {
            return Err(diagnostic("input", "render needs a request_id"));
        }
        self.input.validate()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Quality {
    pub exactness: Exactness,
    pub requested_tolerance: Option<Length>,
    /// Unknown unless measured; requested tolerance never substitutes for this.
    pub positional_uncertainty: Option<Length>,
    pub confidence: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Provenance {
    pub source: SnapshotIdentity,
    pub input: RenderInput,
    pub engine_revision: Id,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaceCoverage {
    pub name: String,
    pub pixels: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Timing {
    /// Bounds, source conversion and rendering; excludes model loading and PNG.
    pub render_ms: f64,
    pub encoding_ms: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderResult {
    pub request_id: RequestId,
    pub width: u32,
    pub height: u32,
    /// PNG bytes; no base64 or transport envelope at this layer.
    pub png: Vec<u8>,
    pub faces: Vec<FaceCoverage>,
    pub quality: Quality,
    pub provenance: Provenance,
    pub timing: Timing,
}

pub fn diagnostic(code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic { code: Id::new(code), message: message.into() }
}
