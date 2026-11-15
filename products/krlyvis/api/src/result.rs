use crate::{diagnostic, pixel_unit, seed, simplify, Diagnostic};
use crate::{ENGINE_REVISION, LOCAL_NODE, MAX_IMAGE_PIXELS, MAX_JPEG_SIDE, MAX_VERTICES, PROTOCOL};
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DrawingResult {
    pub protocol: String,
    pub request_id: String,
    /// Exactly the provider's placed datum value, plus the local `top@1` label.
    pub evidence: Value,
    pub provenance: DrawingProvenance,
    pub quality: DrawingQuality,
    pub timing: DrawingTiming,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DrawingProvenance {
    /// SHA-256 of actual image-file bytes after base64 decoding, before image decoding.
    pub image_sha256: String,
    /// Unverified caller claim; never substituted for the computed digest.
    pub source: String,
    pub image_format: String,
    pub image_size: [usize; 2],
    pub pixel_unit: f64,
    pub seed: Option<[f64; 2]>,
    pub method: String,
    /// Requested maximum distance in source pixels; safe fallback can remain exact.
    pub simplify: f64,
    pub engine_revision: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DrawingQuality {
    pub confidence: Option<f64>,
    pub positional_uncertainty: Option<f64>,
    pub assumptions: Vec<String>,
    pub accuracy_scope: String,
    pub source_claim: String,
}

impl Default for DrawingQuality {
    fn default() -> Self {
        Self {
            confidence: None,
            positional_uncertainty: None,
            assumptions: vec![
                "One connected subject selected by seed, or the largest component when seed is null".into(),
                "Automatic threshold against the image border; interior holes preserved".into(),
                "Uniform caller-supplied model units per pixel; no physical calibration".into(),
            ],
            accuracy_scope: "Pixel-derived polyline; measurement and positional uncertainty unknown".into(),
            source_claim:
                "source is an unverified caller claim; image_sha256 hashes the supplied image bytes".into(),
        }
    }
}

/// Wall-clock milliseconds for this invocation only; no remote lifecycle or job state.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DrawingTiming {
    pub read_ms: f64,
    pub outline_ms: f64,
    pub total_ms: f64,
}

impl DrawingResult {
    pub fn validate(&self) -> Result<(), Diagnostic> {
        if self.protocol != PROTOCOL {
            return Err(diagnostic("output", "invalid drawing result protocol"));
        }
        validate_evidence(&self.evidence)?;
        self.provenance.validate()?;
        let unit = self.evidence["unit"].as_f64();
        if unit != Some(self.provenance.pixel_unit) {
            return Err(diagnostic("output", "evidence and provenance pixel units disagree"));
        }
        self.quality.validate()?;
        self.timing.validate()
    }
}

impl DrawingQuality {
    /// Validate native quality metadata without constructing wire evidence.
    pub fn validate(&self) -> Result<(), Diagnostic> {
        if self.confidence.is_some() || self.positional_uncertainty.is_some() {
            return Err(diagnostic(
                "output",
                "drawing confidence and positional uncertainty must be unknown (null)",
            ));
        }
        if self.accuracy_scope.trim().is_empty()
            || self.source_claim.trim().is_empty()
            || self.assumptions.iter().any(|text| text.trim().is_empty())
        {
            return Err(diagnostic("output", "invalid drawing quality metadata"));
        }
        Ok(())
    }
}

impl DrawingTiming {
    /// Validate native invocation timing without constructing wire evidence.
    pub fn validate(&self) -> Result<(), Diagnostic> {
        if [self.read_ms, self.outline_ms, self.total_ms].iter().any(|time| !time.is_finite() || *time < 0.0)
            || self.total_ms < self.read_ms + self.outline_ms
        {
            return Err(diagnostic("output", "invalid drawing timing"));
        }
        Ok(())
    }
}

impl DrawingProvenance {
    /// Validate native provenance without constructing wire evidence.
    pub fn validate(&self) -> Result<(), Diagnostic> {
        if self.source.trim().is_empty()
            || self.image_sha256.len() != 64
            || !self.image_sha256.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || !matches!(self.image_format.as_str(), "png" | "jpeg" | "pnm")
            || self.method != "trace-holes"
            || self.engine_revision != ENGINE_REVISION
        {
            return Err(diagnostic("output", "invalid drawing provenance"));
        }
        simplify(self.simplify)
            .map_err(|_| diagnostic("output", "invalid drawing simplification provenance"))?;
        let [width, height] = self.image_size;
        if width == 0
            || height == 0
            || width.checked_mul(height).is_none_or(|pixels| pixels > MAX_IMAGE_PIXELS)
            || (self.image_format == "jpeg" && (width > MAX_JPEG_SIDE || height > MAX_JPEG_SIDE))
        {
            return Err(diagnostic("output", "image dimensions exceed provider limits"));
        }
        pixel_unit(self.pixel_unit)?;
        seed(self.seed)
    }
}

fn exact_fields<'a>(
    value: &'a Value,
    names: &[&str],
) -> Result<&'a serde_json::Map<String, Value>, Diagnostic> {
    let fields = value.as_object().ok_or_else(|| diagnostic("output", "expected evidence object"))?;
    if fields.len() != names.len() || names.iter().any(|name| !fields.contains_key(*name)) {
        return Err(diagnostic("output", "invalid evidence fields"));
    }
    Ok(fields)
}

fn validate_evidence(value: &Value) -> Result<(), Diagnostic> {
    exact_fields(
        value,
        &[
            "kind",
            "view",
            "looking",
            "axes",
            "unit",
            "material_area",
            "evidence",
            "polygons",
            "pixels",
            "node",
        ],
    )?;
    if value["kind"] != "placed"
        || value["view"] != "top"
        || value["node"] != LOCAL_NODE
        || value["axes"] != serde_json::json!(["x", "y"])
        || value["looking"].as_array().is_none_or(|v| {
            v.len() != 3 || v.iter().zip([0.0, 0.0, -1.0]).any(|(a, b)| a.as_f64() != Some(b))
        })
    {
        return Err(diagnostic("output", "invalid placed top evidence identity"));
    }
    let unit = value["unit"].as_f64().ok_or_else(|| diagnostic("output", "unit must be finite"))?;
    pixel_unit(unit)?;
    if value["material_area"].as_f64().is_none_or(|area| !area.is_finite() || area < 0.0) {
        return Err(diagnostic("output", "material_area must be finite and nonnegative"));
    }
    let raster = &value["evidence"];
    exact_fields(raster, &["kind", "source", "uncertainty", "pixel_unit", "threshold", "coordinates"])?;
    if raster["kind"] != "raster"
        || raster["source"] != "krlyvis.placed-pixel-outline/1"
        || raster["uncertainty"] != "unknown"
        || raster["pixel_unit"].as_f64() != Some(unit)
        || raster["threshold"].as_f64().is_none_or(|n| !n.is_finite() || !(0.0..=255.5).contains(&n))
        || raster["coordinates"]
            != "pixel-derived polyline; simplification and image measurement error unknown"
    {
        return Err(diagnostic("output", "invalid raster evidence metadata"));
    }
    let world = loops(&value["polygons"])?;
    let pixels = loops(&value["pixels"])?;
    if world.len() != pixels.len()
        || world.iter().zip(pixels).any(|(a, b)| a.as_array().map(Vec::len) != b.as_array().map(Vec::len))
    {
        return Err(diagnostic("output", "world and pixel loop dimensions disagree"));
    }
    Ok(())
}

fn loops(value: &Value) -> Result<&Vec<Value>, Diagnostic> {
    let polygons = value
        .as_array()
        .filter(|loops| !loops.is_empty())
        .ok_or_else(|| diagnostic("output", "missing polygon loops"))?;
    let mut remaining = MAX_VERTICES;
    for polygon in polygons {
        let points = polygon
            .as_array()
            .filter(|points| points.len() >= 3)
            .ok_or_else(|| diagnostic("output", "a polygon needs at least three points"))?;
        remaining = remaining
            .checked_sub(points.len())
            .ok_or_else(|| diagnostic("resource_limit", "drawing exceeds 256 output vertices"))?;
        for point in points {
            if point.as_array().is_none_or(|point| {
                point.len() != 2 || point.iter().any(|n| n.as_f64().is_none_or(|n| !n.is_finite()))
            }) {
                return Err(diagnostic("output", "polygon coordinates must be finite pairs"));
            }
        }
    }
    Ok(polygons)
}
