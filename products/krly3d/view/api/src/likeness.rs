//! Native station evidence for one top-view snapshot and a supplied raster outline.
//!
//! This first contract fixes the world plane to XY, the local frame to identity,
//! precision to contour intersections and endpoint tolerance to one source pixel.
//! Polygons use even-odd fill. Stations compare sampled outer extents only: holes,
//! topology and continuous agreement are not certified. IoU remains raster sampled.
//! No kernel values, peer protocol or serialized geometry cross this boundary.

use crate::{
    diagnostic, ApiVersion, Diagnostic, Id, Length, Quality, RequestId, SnapshotIdentity, API_VERSION,
};

pub const REPORT_VERSION: &str = "krly3d.likeness.stations/1";
pub const ENGINE_REVISION: &str = "krly3d.view.likeness.snapshot/1";
/// Total vertices across all loops, including repeated closing vertices. Reusing
/// the planar contract also bounds the number of valid loops to at most 85.
pub const MAX_EVIDENCE_VERTICES: usize = krly3d_brep::planar::MAX_VERTICES;
pub const MAX_ABS_COORDINATE: f64 = krly3d_brep::planar::MAX_ABS_COORDINATE;
/// Same 1e9 domain as raster evidence producers, without a vision dependency.
pub const MAX_PIXEL_UNIT: f64 = krly3d_brep::planar::MAX_ABS_COORDINATE;
pub const MAX_SOURCE_BYTES: usize = krly3d_brep::planar::MAX_SOURCE_BYTES;
pub const MAX_REQUEST_ID_BYTES: usize = krly3d_brep::planar::MAX_REQUEST_ID_BYTES;
pub const MAX_NODE_BYTES: usize = 4096;

/// A pixel-derived outline already placed in XY model coordinates. Decimal contour
/// coordinates do not turn raster measurements into vector evidence.
#[derive(Clone, Debug, PartialEq)]
pub struct RasterEvidence {
    pub polygons: Vec<Vec<[f64; 2]>>,
    /// Positive finite model length per source image pixel, at most MAX_PIXEL_UNIT.
    pub pixel_unit: f64,
    /// Nonempty caller-supplied image/digest identity claim, copied verbatim. The
    /// native comparison neither reads the image nor authenticates this claim.
    pub source: String,
}

impl RasterEvidence {
    /// Structural checks only. Projection, representable grid resolution and the
    /// existing four-million-cell limit remain the station provider's responsibility.
    pub fn validate(&self) -> Result<(), Diagnostic> {
        if self.source.len() > MAX_SOURCE_BYTES || self.source.trim().is_empty() {
            return Err(diagnostic(
                "input",
                "raster evidence source identity claim must be nonempty and at most 1024 UTF-8 bytes",
            ));
        }
        if !self.pixel_unit.is_finite() || self.pixel_unit <= 0.0 || self.pixel_unit > MAX_PIXEL_UNIT {
            return Err(diagnostic(
                "input",
                "raster evidence pixel_unit must be positive, finite and at most 1e9 model units",
            ));
        }
        if self.polygons.is_empty() {
            return Err(diagnostic("input", "raster evidence needs at least one polygon"));
        }
        // Check counts before walking coordinates or cloning any evidence. Since
        // valid loops need three vertices, this also bounds per-loop overhead.
        if self.polygons.len() > MAX_EVIDENCE_VERTICES / 3 {
            return Err(diagnostic(
                "resource_limit",
                "raster evidence exceeds 256 total vertices or 85 polygons",
            ));
        }
        let mut remaining = MAX_EVIDENCE_VERTICES;
        for polygon in &self.polygons {
            remaining = remaining
                .checked_sub(polygon.len())
                .ok_or_else(|| diagnostic("resource_limit", "raster evidence exceeds 256 total vertices"))?;
            if polygon.len() < 3 || polygon.iter().flatten().any(|value| !value.is_finite()) {
                return Err(diagnostic(
                    "input",
                    "raster evidence needs polygons of at least three finite points",
                ));
            }
            if polygon.iter().flatten().any(|value| value.abs() > MAX_ABS_COORDINATE) {
                return Err(diagnostic(
                    "input",
                    "raster evidence coordinates exceed the native provider bound of +/-1e9 model units",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LikenessCall {
    pub request_id: RequestId,
    pub api: ApiVersion,
    /// One node, optionally qualified by a version. Execution checks this against
    /// the supplied snapshot, then retains its normalized explicit version.
    pub node: String,
    pub evidence: RasterEvidence,
}

impl LikenessCall {
    pub fn new(request_id: RequestId, node: impl Into<String>, evidence: RasterEvidence) -> Self {
        Self { request_id, api: API_VERSION, node: node.into(), evidence }
    }

    pub fn validate(&self) -> Result<(), Diagnostic> {
        if self.api != API_VERSION {
            return Err(diagnostic("unsupported_version", "likeness requires view API 1.0"));
        }
        if self.request_id.as_str().len() > MAX_REQUEST_ID_BYTES || self.request_id.as_str().trim().is_empty()
        {
            return Err(diagnostic(
                "input",
                "likeness request_id must be nonempty and at most 256 UTF-8 bytes",
            ));
        }
        if self.node.len() > MAX_NODE_BYTES || self.node.trim().is_empty() {
            return Err(diagnostic("input", "likeness node must be nonempty and at most 4096 UTF-8 bytes"));
        }
        self.evidence.validate()
    }
}

/// An extent across one sampled local axis. With the fixed identity frame, along
/// 0 is world X (local u), and along 1 is world Y (local v).
#[derive(Clone, Debug, PartialEq)]
pub struct Station {
    pub along: usize,
    pub at: f64,
    pub model: Option<[f64; 2]>,
    pub drawing: Option<[f64; 2]>,
    /// Provider-computed maximum endpoint difference, or the occupied extent's
    /// width when the other side is missing. A missing side still always fails.
    pub off: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StationDifference {
    /// Report-local u:index or v:index; scoped to retained evidence and grid.
    pub id: String,
    pub index: usize,
    pub station: Station,
    /// Signed model-minus-drawing endpoints; unknown if a side is missing.
    pub delta: Option<[f64; 2]>,
    pub tolerance: Option<f64>,
    pub matches: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StationFrame {
    pub origin: [f64; 2],
    pub u: [f64; 2],
    pub v: [f64; 2],
    pub world_axes: [usize; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StationGrid {
    pub low: [f64; 2],
    /// Effective cell, which can exceed the source pixel to bound provider work.
    /// The provider refuses a one-pixel verdict below this floor.
    pub cell: f64,
    pub size: [usize; 2],
    pub samples: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StationPrecision {
    Contour,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComparisonScope {
    SampledOuterExtents,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StationPolicy {
    pub precision: StationPrecision,
    /// Exactly one source pixel, with no additional stair allowance.
    pub tolerance: f64,
    pub stair_allowance: f64,
    pub raster_precision_floor: f64,
    pub scope: ComparisonScope,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LikenessProvenance {
    pub source: SnapshotIdentity,
    pub node: String,
    /// The complete supplied coordinate snapshot and its unverified source claim.
    pub evidence: RasterEvidence,
    pub engine_revision: Id,
    pub mesh_tolerance_requested: Length,
    /// Unknown: requested tessellation tolerance is not measured achieved error.
    pub mesh_achieved_error: Option<Length>,
    /// Unknown: pixel size and contour interpolation do not measure uncertainty.
    pub drawing_uncertainty: Option<Length>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LikenessResult {
    pub request_id: RequestId,
    pub report_version: String,
    pub iou: f64,
    pub model_area: f64,
    pub drawing_area: f64,
    /// The legacy raster resemblance's worst list, retained independently of the
    /// contour station ranking below.
    pub raster_worst: Vec<Station>,
    /// Every occupied station, in local-axis then grid-line order.
    pub stations: Vec<StationDifference>,
    /// Up to four separated worst stations per axis, including matching ones.
    pub worst: Vec<StationDifference>,
    pub compared: [usize; 2],
    pub failed: [usize; 2],
    pub matches: Option<bool>,
    pub frame: StationFrame,
    pub grid: StationGrid,
    pub policy: StationPolicy,
    pub quality: Quality,
    pub provenance: LikenessProvenance,
}

/// Explicit CLI output boundary. Native callers consume `LikenessResult` directly.
/// Unknown quantities/absent extents encode as null, never as invented accuracy.
pub fn encode_result(result: &LikenessResult) -> Result<String, Diagnostic> {
    use crate::Exactness;
    use serde_json::json;

    validate_result(result)?;
    let quality = &result.quality;
    let exactness = match quality.exactness {
        Exactness::Exact => json!({"kind":"exact"}),
        Exactness::Numeric { tolerance } => json!({"kind":"numeric","tolerance":tolerance}),
        Exactness::Sampled { samples } => json!({"kind":"sampled","samples":samples}),
    };
    let provenance = &result.provenance;
    let source = &provenance.source;
    let evidence = &provenance.evidence;
    Ok(json!({
        "api":{"major":API_VERSION.major,"minor":API_VERSION.minor},
        "request_id":result.request_id.as_str(),"report_version":result.report_version,
        "iou":result.iou,"model_area":result.model_area,"drawing_area":result.drawing_area,
        "raster_worst":result.raster_worst.iter().map(station_json).collect::<Vec<_>>(),
        "stations":result.stations.iter().map(difference_json).collect::<Vec<_>>(),
        "worst":result.worst.iter().map(difference_json).collect::<Vec<_>>(),
        "compared":result.compared,"failed":result.failed,"matches":result.matches,
        "frame":{"origin":result.frame.origin,"u":result.frame.u,"v":result.frame.v,
            "world_axes":["x","y"]},
        "grid":{"low":result.grid.low,"cell":result.grid.cell,
            "size":result.grid.size,"samples":result.grid.samples},
        "policy":{"precision":"contour","tolerance":result.policy.tolerance,"stairs":"none",
            "stair_allowance":result.policy.stair_allowance,
            "raster_precision_floor":result.policy.raster_precision_floor,
            "coordinate_method":"linear-contour-intersections",
            "scope":"sampled outer extents only; missing side always fails; holes and continuous agreement not tested",
            "subpixel_accuracy":"not certified; raster evidence retains effective pixel floor"},
        "quality":{"exactness":exactness,
            "requested_tolerance":quality.requested_tolerance.map(length_json),
            "positional_uncertainty":quality.positional_uncertainty.map(length_json),
            "confidence":quality.confidence},
        "provenance":{"source":{"source":source.source.as_str(),"session":source.session.as_str(),
            "node":source.node,"build_key":source.build_key},"node":provenance.node,
            "engine_revision":provenance.engine_revision.as_str(),"method":"raster",
            "drawing_evidence":"raster","drawing_unit":evidence.pixel_unit,
            "drawing_source":evidence.source,
            "drawing_uncertainty":provenance.drawing_uncertainty.map(length_json),
            "reference":{"polygons":evidence.polygons,"pixel_unit":evidence.pixel_unit,
                "source":evidence.source,"axes":["x","y"]},
            "source_claim":"caller-supplied image/digest identity; not authenticated",
            "mesh_tolerance_requested":length_json(provenance.mesh_tolerance_requested),
            "mesh_achieved_error":provenance.mesh_achieved_error.map(length_json),
            "accuracy":"sampled nominal differences; interpolation is not an accuracy guarantee"}
    }).to_string())
}

fn length_json(length: Length) -> serde_json::Value {
    use crate::LengthUnit;
    serde_json::json!({"value":length.value,"unit":match length.unit {
        LengthUnit::Model => "model", LengthUnit::Metre => "metre"
    }})
}

fn station_json(station: &Station) -> serde_json::Value {
    serde_json::json!({"along":(["u","v"][station.along]),"at":station.at,
        "model":station.model,"drawing":station.drawing,"off":station.off})
}

fn difference_json(difference: &StationDifference) -> serde_json::Value {
    let mut value = station_json(&difference.station);
    value["id"] = serde_json::json!(difference.id);
    value["index"] = serde_json::json!(difference.index);
    value["delta"] = serde_json::json!(difference.delta);
    value["tolerance"] = serde_json::json!(difference.tolerance);
    value["matches"] = serde_json::json!(difference.matches);
    value
}

fn validate_result(result: &LikenessResult) -> Result<(), Diagnostic> {
    use crate::{Exactness, LengthUnit};
    let bad = || diagnostic("output", "invalid likeness result metadata");
    let finite_pair = |pair: [f64; 2]| pair.iter().all(|value| value.is_finite());
    let nonnegative = |value: f64| value.is_finite() && value >= 0.0;
    let valid_station = |station: &Station| {
        station.along < 2
            && station.at.is_finite()
            && nonnegative(station.off)
            && [station.model, station.drawing]
                .into_iter()
                .flatten()
                .all(|extent| finite_pair(extent) && extent[0] <= extent[1])
    };
    let provenance = &result.provenance;
    provenance.evidence.validate().map_err(|_| bad())?;
    if result.request_id.as_str().trim().is_empty()
        || result.report_version != REPORT_VERSION
        || !result.iou.is_finite()
        || !(0.0..=1.0).contains(&result.iou)
        || !nonnegative(result.model_area)
        || !nonnegative(result.drawing_area)
        || !finite_pair(result.grid.low)
        || !result.grid.cell.is_finite()
        || result.grid.cell <= 0.0
        || result.grid.size.contains(&0)
        || result.grid.size[0].checked_mul(result.grid.size[1]) != Some(result.grid.samples as usize)
        || result.grid.samples == 0
        || result.grid.samples > 4_000_000
        || result.frame
            != (StationFrame { origin: [0.0, 0.0], u: [1.0, 0.0], v: [0.0, 1.0], world_axes: [0, 1] })
        || result.policy.tolerance != provenance.evidence.pixel_unit
        || result.policy.stair_allowance != 0.0
        || result.policy.raster_precision_floor != result.grid.cell
        || result.policy.tolerance < result.grid.cell
        || provenance.node != provenance.source.node
        || provenance.node.is_empty()
        || provenance.source.source.as_str().is_empty()
        || provenance.source.session.as_str().is_empty()
        || provenance.source.build_key.is_empty()
        || provenance.engine_revision.as_str() != ENGINE_REVISION
        || provenance.mesh_tolerance_requested
            != (Length { value: provenance.evidence.pixel_unit, unit: LengthUnit::Model })
        || provenance.mesh_achieved_error.is_some()
        || provenance.drawing_uncertainty.is_some()
        || result.quality.positional_uncertainty.is_some()
        || result.quality.confidence.is_some()
        || result.quality.requested_tolerance != Some(provenance.mesh_tolerance_requested)
        || result.quality.exactness != (Exactness::Sampled { samples: result.grid.samples })
        || result.failed.iter().zip(result.compared).any(|(failed, compared)| *failed > compared)
        || !result.raster_worst.iter().all(valid_station)
    {
        return Err(bad());
    }
    for difference in result.stations.iter().chain(&result.worst) {
        if !valid_station(&difference.station)
            || difference.index >= result.grid.size[difference.station.along]
            || difference.id != format!("{}:{}", ["u", "v"][difference.station.along], difference.index)
            || difference.delta.is_some_and(|delta| !finite_pair(delta))
            || difference.tolerance != Some(result.policy.tolerance)
            || difference.matches.is_none()
        {
            return Err(bad());
        }
    }
    Ok(())
}
