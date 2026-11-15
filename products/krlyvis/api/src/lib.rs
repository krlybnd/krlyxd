//! One synchronous, stateless raster-to-planar-evidence operation.
//!
//! `top@1` is a local label for the returned coordinate snapshot. It is not a
//! retained document, a globally resolvable node, or a live engine handle.

mod codec;
mod descriptor;
mod result;

pub use codec::{decode_call, encode_result};
pub use descriptor::descriptor;
pub use krlyxd::{Diagnostic, Id};
pub use result::{DrawingProvenance, DrawingQuality, DrawingResult, DrawingTiming};

pub const PROTOCOL: &str = "krlyvis.drawing/1";
pub const ENGINE_REVISION: &str = "krlyvis.drawing/1";
pub const MAX_CALL_BYTES: usize = 24 * 1024 * 1024;
/// Bytes of the decoded base64 image file, before the image reader decompresses it.
pub const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_BASE64_BYTES: usize = MAX_IMAGE_BYTES.div_ceil(3) * 4;
/// The existing image providers enforce this before allocating image samples.
pub const MAX_IMAGE_PIXELS: usize = 1 << 28;
pub const MAX_JPEG_SIDE: usize = 1 << 15;
pub const MAX_VERTICES: usize = 256;
pub const MAX_PIXEL_UNIT: f64 = 1.0e9;
/// Largest explicit contour tolerance, measured in source-image pixels.
pub const MAX_SIMPLIFY_PIXELS: f64 = 1024.0;
pub const LOCAL_NODE: &str = "top@1";

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
pub struct DrawingCall {
    pub protocol: String,
    pub request_id: String,
    pub input: DrawingInput,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrawingInput {
    pub image: DrawingImage,
    pub pixel_unit: f64,
    /// Requested maximum contour simplification distance in source pixels.
    /// Zero preserves the exact trace. Unsafe reductions may retain exact loops.
    pub simplify: f64,
    /// Pixel coordinates, with x right and y down; the provider floors fractions.
    pub seed: Option<[f64; 2]>,
    /// An unverified caller claim, kept separate from the engine's image hash.
    pub source: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrawingImage {
    pub format: String,
    /// Actual image-file bytes; the wire boundary decodes canonical standard base64.
    pub data: Vec<u8>,
}

impl DrawingCall {
    /// Validate typed semantics, including image-file and scalar limits.
    ///
    /// Required again at the engine boundary for directly constructed Rust calls.
    /// This does not serialize the call or impose a JSON envelope size on native
    /// callers. [`decode_call`] separately bounds the actual wire request bytes.
    pub fn validate(&self) -> Result<(), Diagnostic> {
        if self.protocol != PROTOCOL {
            return Err(diagnostic("unsupported_version", format!("drawing requires protocol {PROTOCOL}")));
        }
        self.input.validate()
    }
}

impl DrawingInput {
    pub fn validate(&self) -> Result<(), Diagnostic> {
        if !matches!(self.image.format.as_str(), "png" | "jpeg" | "pnm") {
            return Err(diagnostic("input", "image.format must be png, jpeg or pnm"));
        }
        if self.image.data.len() > MAX_IMAGE_BYTES {
            return Err(diagnostic("resource_limit", "decoded image file exceeds 16 MiB"));
        }
        pixel_unit(self.pixel_unit)?;
        simplify(self.simplify)?;
        seed(self.seed)?;
        if self.source.trim().is_empty() {
            return Err(diagnostic("input", "source must be a nonempty caller provenance claim"));
        }
        Ok(())
    }
}

pub(crate) fn pixel_unit(value: f64) -> Result<(), Diagnostic> {
    if !value.is_finite() || value <= 0.0 || value > MAX_PIXEL_UNIT {
        return Err(diagnostic("input", "pixel_unit must be positive, finite and at most 1e9"));
    }
    Ok(())
}

pub(crate) fn seed(value: Option<[f64; 2]>) -> Result<(), Diagnostic> {
    if value.is_some_and(|point| point.iter().any(|value| !value.is_finite() || *value < 0.0)) {
        return Err(diagnostic("input", "seed must be null or two nonnegative finite pixel coordinates"));
    }
    Ok(())
}

pub(crate) fn simplify(value: f64) -> Result<(), Diagnostic> {
    if !value.is_finite() || !(0.0..=MAX_SIMPLIFY_PIXELS).contains(&value) {
        return Err(diagnostic(
            "input",
            "simplify must be finite and between 0 and 1024 source-image pixels",
        ));
    }
    Ok(())
}

pub(crate) fn diagnostic(code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic { code: Id::new(code), message: message.into() }
}
