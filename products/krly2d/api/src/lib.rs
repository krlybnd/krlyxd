//! Bounded, immutable planar geometry in explicit model units or metres.
//!
//! Validation establishes representation structure only. It does not establish
//! topology (including self-intersection, touching, winding or hole containment),
//! which remains the responsibility of existing BRep providers. Retaining curve
//! parameters does not establish physical accuracy or measurement uncertainty.
//! This crate performs no curve evaluation, flattening, repair or unit conversion.
//! Coordinates and controls need only be finite; positive arc radii need only be
//! finite. Numerical-operation magnitude limits belong to consuming providers.

use krlyxd::{ApiVersion, ContractMetadata, Coordinates, Diagnostic, Id, LengthUnit, PackageMetadata};

pub type Point2 = [f64; 2];

/// Native representation revision, independent of Cargo package SemVer.
pub const API_VERSION: ApiVersion = ApiVersion { major: 1, minor: 0 };
/// Inclusive bound on absolute ellipse-axis rotation in radians; never normalized.
pub const MAX_ABS_ANGLE: f64 = 1.0e6;
/// Maximum explicit segments per path and across all paths of a region.
/// Implicit closing edges are not stored or counted; their count is bounded by
/// the maximum number of loops, [`MAX_HOLES`] + 1.
pub const MAX_SEGMENTS: usize = 65_536;
pub const MAX_HOLES: usize = 1_024;

/// Shared metadata for native API revision 1. The common `engine_revision` slot
/// identifies this representation contract, not an executing geometry engine.
/// `Model` is the coordinate default, not a physical scale; each region retains
/// its explicitly supplied [`LengthUnit`].
pub fn metadata() -> ContractMetadata<'static> {
    ContractMetadata {
        package: PackageMetadata { name: env!("CARGO_PKG_NAME"), version: env!("CARGO_PKG_VERSION") },
        protocol: API_VERSION,
        api: API_VERSION,
        engine_revision: "krly2d.native/1",
        coordinates: Coordinates::RIGHT_HANDED_Z_UP,
        length_unit: LengthUnit::Model,
    }
}

/// A segment starts at its path's start or the preceding segment's endpoint.
/// Values are validated when admitted to a [`Path`]; an isolated enum value is
/// only a construction value and carries no validation guarantee.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    Line {
        to: Point2,
    },
    QuadraticBezier {
        control: Point2,
        to: Point2,
    },
    CubicBezier {
        control1: Point2,
        control2: Point2,
        to: Point2,
    },
    /// Endpoint-form ellipse arc. `radii` are positive semi-axis lengths;
    /// `x_axis_rotation` is in radians counterclockwise from +X toward +Y.
    /// `large_arc` selects the span greater than pi; `sweep` selects increasing
    /// ellipse parameter (counterclockwise before axis rotation) when true.
    ///
    /// No radii correction or angle normalization is performed. Endpoint/ellipse
    /// compatibility and degenerate or coincident endpoints belong to providers;
    /// coincident endpoints do not specify a full ellipse in this representation.
    EllipticArc {
        radii: Point2,
        x_axis_rotation: f64,
        large_arc: bool,
        sweep: bool,
        to: Point2,
    },
}

impl Segment {
    pub fn end(&self) -> Point2 {
        match *self {
            Self::Line { to }
            | Self::QuadraticBezier { to, .. }
            | Self::CubicBezier { to, .. }
            | Self::EllipticArc { to, .. } => to,
        }
    }

    fn validate(&self) -> Result<(), Diagnostic> {
        point(self.end())?;
        match *self {
            Self::Line { .. } => Ok(()),
            Self::QuadraticBezier { control, .. } => point(control),
            Self::CubicBezier { control1, control2, .. } => {
                point(control1)?;
                point(control2)
            }
            Self::EllipticArc { radii, x_axis_rotation, .. } => {
                if radii.iter().any(|r| !r.is_finite() || *r <= 0.0) {
                    return Err(diagnostic("input", "arc radii must be positive and finite"));
                }
                if !x_axis_rotation.is_finite() || x_axis_rotation.abs() > MAX_ABS_ANGLE {
                    return Err(diagnostic("input", "arc rotation must be finite and within +/-1e6 radians"));
                }
                Ok(())
            }
        }
    }
}

/// An owned, immutable path with exact connectivity through implicit starts.
/// If closed, a straight closing edge back to `start` is implicit. Explicit
/// repeated endpoints are retained, even when they yield zero-length edges.
/// Paths have no independent physical scale; their consumer supplies the units.
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    start: Point2,
    segments: Box<[Segment]>,
    closed: bool,
}

impl Path {
    /// Validate finite coordinates/controls, arc parameters and the segment budget.
    /// Coordinates have no magnitude or degeneracy threshold. Numerical operation
    /// bounds belong to consumers, not to this lossless representation.
    /// Open paths may have zero segments. Closed line-only paths require at least
    /// three vertex entries (the start and two endpoints), without checking
    /// distinctness, area or topology. Curved loops have no linear vertex minimum.
    /// Spare input capacity is discarded rather than retained in the contract.
    pub fn new(start: Point2, segments: Vec<Segment>, closed: bool) -> Result<Self, Diagnostic> {
        if segments.len() > MAX_SEGMENTS {
            return Err(diagnostic("resource_limit", "path exceeds the explicit segment budget"));
        }
        point(start)?;
        for segment in &segments {
            segment.validate()?;
        }
        if closed && segments.len() < 2 && segments.iter().all(|s| matches!(s, Segment::Line { .. })) {
            return Err(diagnostic("input", "closed linear paths require at least three vertex entries"));
        }
        Ok(Self { start, segments: segments.into_boxed_slice(), closed })
    }

    pub fn start(&self) -> Point2 {
        self.start
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    pub fn closed(&self) -> bool {
        self.closed
    }
}

/// One closed outer path and ordered closed hole paths in explicit length units.
/// Naming a path a hole does not establish containment or other valid topology.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    outer: Path,
    holes: Box<[Path]>,
    unit: LengthUnit,
}

impl Region {
    /// Validate closure and aggregate resource limits before retaining any data.
    /// Path constructors already establish the remaining structural invariants.
    pub fn new(outer: Path, holes: Vec<Path>, unit: LengthUnit) -> Result<Self, Diagnostic> {
        if holes.len() > MAX_HOLES {
            return Err(diagnostic("resource_limit", "region exceeds the hole budget"));
        }
        let mut remaining = MAX_SEGMENTS;
        for path in std::iter::once(&outer).chain(&holes) {
            if !path.closed() {
                return Err(diagnostic("input", "region outer and hole paths must be closed"));
            }
            take_segments(&mut remaining, path.segments().len())?;
        }
        Ok(Self { outer, holes: holes.into_boxed_slice(), unit })
    }

    /// The first polygon is the outer loop; all following polygons are holes in
    /// supplied order. Preflight all counts before allocating converted paths.
    /// Each loop needs at least three vertex entries. Coordinates, winding and
    /// explicit repeated closing vertices are retained exactly, without repair.
    pub fn from_polygons(polygons: Vec<Vec<Point2>>, unit: LengthUnit) -> Result<Self, Diagnostic> {
        if polygons.is_empty() {
            return Err(diagnostic("input", "polygons must contain an outer loop"));
        }
        if polygons.len() - 1 > MAX_HOLES {
            return Err(diagnostic("resource_limit", "polygons exceed the hole budget"));
        }
        let mut remaining = MAX_SEGMENTS;
        for polygon in &polygons {
            if polygon.len() < 3 {
                return Err(diagnostic("input", "each polygon requires at least three vertex entries"));
            }
            take_segments(&mut remaining, polygon.len() - 1)?;
        }
        let mut paths = polygons.into_iter().map(|polygon| {
            let mut points = polygon.into_iter();
            let start = points.next().ok_or_else(|| diagnostic("input", "polygon has no start"))?;
            let segments = points.map(|to| Segment::Line { to }).collect();
            Path::new(start, segments, true)
        });
        let outer = paths.next().ok_or_else(|| diagnostic("input", "polygons have no outer loop"))??;
        let holes = paths.collect::<Result<Vec<_>, _>>()?;
        Self::new(outer, holes, unit)
    }

    /// Return independent polygon copies, preserving each stored vertex exactly.
    /// Any curve returns `unsupported` before allocating output polygons; curves
    /// are never silently flattened or replaced with their endpoint chords.
    pub fn polygons(&self) -> Result<Vec<Vec<Point2>>, Diagnostic> {
        let paths = || std::iter::once(&self.outer).chain(self.holes.iter());
        if paths().any(|path| path.segments().iter().any(|s| !matches!(s, Segment::Line { .. }))) {
            return Err(diagnostic("unsupported", "polygon conversion requires line-only paths"));
        }
        Ok(paths()
            .map(|path| {
                std::iter::once(path.start()).chain(path.segments().iter().map(Segment::end)).collect()
            })
            .collect())
    }

    pub fn outer(&self) -> &Path {
        &self.outer
    }

    pub fn holes(&self) -> &[Path] {
        &self.holes
    }

    pub fn length_unit(&self) -> LengthUnit {
        self.unit
    }
}

fn point(value: Point2) -> Result<(), Diagnostic> {
    if value.iter().any(|v| !v.is_finite()) {
        return Err(diagnostic("input", "coordinates must be finite"));
    }
    Ok(())
}

fn take_segments(remaining: &mut usize, count: usize) -> Result<(), Diagnostic> {
    *remaining = remaining
        .checked_sub(count)
        .ok_or_else(|| diagnostic("resource_limit", "region exceeds the total explicit segment budget"))?;
    Ok(())
}

fn diagnostic(code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic { code: Id::new(code), message: message.into() }
}
