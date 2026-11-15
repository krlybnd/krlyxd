//! Additive station evidence contract. The legacy `Drawing`, request and response stay
//! usable as struct literals. Matches concern sampled silhouette extents, not topology,
//! interior holes, or a certified continuous Hausdorff bound.
//!
//! The active legacy facade exposes this as `likeness against=vis:top report=stations
//! tolerance=1 precision=contour`. Optional `frame=ox,oy,ux,uy,vx,vy` defines a rotated
//! local basis; `stairs=pixel-diagonal` adds sqrt(2) effective raster cells to the base
//! threshold (raster mode only). Omitting tolerance requests evidence without a verdict.
//! `report=legacy` remains the default, with its original text/data and IoU claims.
//!
//! Response `report_version=krly3d.likeness.stations/1` contains `stations` (all occupied
//! rows/columns), `worst` (up to four separated maxima per local axis), `matches`,
//! `compared` and `failed` (counts in u/v order), `frame`, `grid`, `policy`, `provenance`.
//! Each station has `id` (u:line or v:line), `along`, `index`, `at`, `model`, `drawing`,
//! signed endpoint `delta`, absolute maximum `off`, effective `tolerance`, and `matches`.
//! Missing extents/deltas are empty arrays; absent tolerance is "unknown" and verdicts
//! are "not-requested". The frame maps these local world-length coordinates back into
//! `world_axes`. Provenance retains pinned model node versions, the complete peer reply
//! coordinate snapshot (including its resolved node when supplied), evidence source,
//! declared drawing uncertainty and requested mesh tolerance. Achieved mesh error is
//! unknown. Station IDs are scoped to that retained report/reference and grid.
//!
//! Contour coordinates are linear intersections, not additional measurements. A peer
//! must explicitly supply an evidence source. Pixel-derived contours retain at least
//! the effective cell as their tolerance floor, even in contour mode. Only a declared
//! vector source with finite nonnegative uncertainty permits subpixel thresholds;
//! thresholds below that uncertainty are refused. Matches are nominal sampled
//! comparisons and never certify continuous agreement or unknown measurement error.

use super::{Drawing, LikenessRequest, Resemblance, Station};
use crate::plugin::Family;
use crate::schema::Value;
use crate::Context;
use krly3d_core::{Attempt, Error, Id};

/// An orthonormal frame inside the drawing's world-axis plane. A local point maps to
/// `origin + u * x + v * y`; coordinates and tolerance remain in world length units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawingFrame {
    pub origin: [f64; 2],
    pub u: [f64; 2],
    pub v: [f64; 2],
}

impl Default for DrawingFrame {
    fn default() -> Self {
        Self { origin: [0.0; 2], u: [1.0, 0.0], v: [0.0, 1.0] }
    }
}

impl DrawingFrame {
    pub fn validate(&self) -> Result<(), Error> {
        let dot = |a: [f64; 2], b: [f64; 2]| a[0] * b[0] + a[1] * b[1];
        if self.origin.into_iter().chain(self.u).chain(self.v).any(|n| !n.is_finite())
            || (dot(self.u, self.u) - 1.0).abs() > 1e-10
            || (dot(self.v, self.v) - 1.0).abs() > 1e-10
            || dot(self.u, self.v).abs() > 1e-10
        {
            return Err(Error::input("likeness frame needs finite origin and orthonormal u/v directions"));
        }
        Ok(())
    }

    pub fn local(&self, world: [f64; 2]) -> [f64; 2] {
        let p = [world[0] - self.origin[0], world[1] - self.origin[1]];
        [p[0] * self.u[0] + p[1] * self.u[1], p[0] * self.v[0] + p[1] * self.v[1]]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StationPrecision {
    #[default]
    Raster,
    /// Linear intersections of supplied vector contours and the projected mesh.
    /// Raster contours can be interpolated, but retain the effective pixel floor.
    /// Subpixel coordinates do not establish subpixel measurement or model accuracy.
    Contour,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StairPolicy {
    #[default]
    None,
    /// An explicitly chosen allowance of one effective pixel diagonal, added to the
    /// threshold at every station. It is not a bound on shallow-edge scanline error.
    PixelDiagonal,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawingEvidence {
    /// True only when the producer explicitly declares vector evidence. A polygon
    /// traced from pixels remains raster evidence, regardless of decimal coordinates.
    pub vector: bool,
    pub source: Option<String>,
    pub uncertainty: Option<f64>,
}

impl DrawingEvidence {
    /// Optional peer declaration: `evidence: {kind: "vector"|"raster", source,
    /// uncertainty}`. Missing metadata means raster with unknown uncertainty.
    pub fn from_value(value: &Value) -> Result<Self, Error> {
        let Value::Map(data) = value else { return Err(Error::format("drawing must be a map")) };
        let Some(evidence) = data.get("evidence") else { return Ok(Self::default()) };
        let Value::Map(fields) = evidence else {
            return Err(Error::format("drawing evidence must be a map"));
        };
        let vector = match fields.get("kind") {
            Some(Value::Text(kind)) if kind == "vector" => true,
            Some(Value::Text(kind)) if kind == "raster" => false,
            _ => return Err(Error::format("drawing evidence kind must be vector or raster")),
        };
        let source = match fields.get("source") {
            Some(Value::Text(source)) if !source.trim().is_empty() => Some(source.clone()),
            None => None,
            _ => return Err(Error::format("drawing evidence source must be nonempty text")),
        };
        let uncertainty = match fields.get("uncertainty") {
            Some(Value::Number(n)) if n.is_finite() && *n >= 0.0 => Some(*n),
            None => None,
            Some(Value::Text(s)) if s == "unknown" => None,
            _ => {
                return Err(Error::format(
                    "drawing evidence uncertainty must be finite and nonnegative or unknown",
                ))
            }
        };
        Ok(Self { vector, source, uncertainty })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StationOptions {
    pub frame: DrawingFrame,
    pub precision: StationPrecision,
    /// Base allowed endpoint difference. None requests evidence without a verdict.
    pub tolerance: Option<f64>,
    pub stairs: StairPolicy,
}

impl StationOptions {
    pub fn validate(&self, drawing: &Drawing, evidence: &DrawingEvidence) -> Result<(), Error> {
        drawing.validate()?;
        self.frame.validate()?;
        if self.tolerance.is_some_and(|n| !n.is_finite() || n < 0.0) {
            return Err(Error::input("likeness tolerance must be finite and nonnegative"));
        }
        if evidence.uncertainty.is_some_and(|n| !n.is_finite() || n < 0.0) {
            return Err(Error::input("drawing evidence uncertainty must be finite and nonnegative"));
        }
        if self.precision == StationPrecision::Contour
            && (evidence.source.as_ref().is_none_or(|s| s.trim().is_empty())
                || (evidence.vector && evidence.uncertainty.is_none()))
        {
            return Err(Error::input(
                "contour precision requires explicit evidence source; vector evidence also needs uncertainty; raster evidence retains the pixel precision floor",
            ));
        }
        if self.precision == StationPrecision::Contour && self.stairs != StairPolicy::None {
            return Err(Error::input("pixel-diagonal stairs apply only to raster precision"));
        }
        if self.tolerance.zip(evidence.uncertainty).is_some_and(|(t, u)| t < u) {
            return Err(Error::input("likeness tolerance is below the declared drawing uncertainty"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct StationRequest {
    pub comparison: LikenessRequest,
    pub options: StationOptions,
    pub evidence: DrawingEvidence,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StationDifference {
    /// Stable within this report: axis and grid line index identify a station.
    pub index: usize,
    pub station: Station,
    /// Signed model minus drawing low/high endpoints; absent when either side is missing.
    pub delta: Option<[f64; 2]>,
    /// Base threshold plus the chosen stair allowance, in world length units.
    pub tolerance: Option<f64>,
    /// A missing side always fails, even if its extent is smaller than the threshold.
    pub matches: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StationReport {
    /// Raster IoU/area and the legacy raster worst list; `worst` below follows the
    /// chosen station precision. IoU is always sampled, including in contour mode.
    pub resemblance: Resemblance,
    /// Every occupied row and column, ordered by local axis then line index.
    pub stations: Vec<StationDifference>,
    /// Up to four separated worst stations per local axis, including matching stations.
    pub worst: Vec<StationDifference>,
    pub compared: [usize; 2],
    pub failed: [usize; 2],
    pub matches: Option<bool>,
    pub grid_low: [f64; 2],
    pub cell: f64,
    pub size: [usize; 2],
    pub options: StationOptions,
    pub evidence: DrawingEvidence,
    pub samples: u32,
}

pub struct StationLikeness;

impl Family for StationLikeness {
    const NAME: &'static str = "likeness-stations";
    type Request = StationRequest;
    type Response = StationReport;
    type Context = Context;
}

pub fn station_likeness(cx: &Context, method: &str, request: &StationRequest) -> Attempt<StationReport> {
    cx.call::<StationLikeness>(&Id::new(method), request)
}
