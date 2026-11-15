use crate::{diagnostic, value};
use krly2d_api::Region;
use krlyvis_api::{
    Diagnostic, DrawingProvenance, DrawingQuality, DrawingResult, DrawingTiming, LOCAL_NODE, MAX_VERTICES,
    PROTOCOL,
};
use krlyvis_kernel::{Datum, View};
use std::sync::Arc;

/// Immutable, validated drawing evidence for native consumers such as a runtime.
///
/// The retained datum is always a placed top-view outline, with pixel loops and
/// its provider frame. Clones share that same [`Arc<Datum>`] and the once-projected
/// krly2d region; retaining both representations preserves the legacy adapter.
/// No image, document, provider context, request ID, or JSON evidence is retained. Provenance, quality,
/// and timing describe the original read and are exposed only by shared reference.
/// [`Self::to_result`] is the explicit adapter to the wire representation.
#[derive(Clone, Debug)]
pub struct SharedDrawing {
    datum: Arc<Datum>,
    region: Arc<Region>,
    material_area: f64,
    provenance: DrawingProvenance,
    quality: DrawingQuality,
    timing: DrawingTiming,
}

impl SharedDrawing {
    pub(super) fn new(
        datum: Datum,
        provenance: DrawingProvenance,
        quality: DrawingQuality,
        timing: DrawingTiming,
    ) -> Result<Self, Diagnostic> {
        let Datum::Placed { shape, frame } = &datum else {
            return Err(diagnostic("output", "drawing requires a placed datum"));
        };
        provenance.validate()?;
        quality.validate()?;
        timing.validate()?;
        if frame.view != View::Top || frame.axes != [0, 1] {
            return Err(diagnostic("output", "invalid placed top evidence identity"));
        }
        if frame.unit() != provenance.pixel_unit {
            return Err(diagnostic("output", "evidence and provenance pixel units disagree"));
        }
        if !shape.threshold.is_finite() || !(0.0..=255.5).contains(&shape.threshold) {
            return Err(diagnostic("output", "invalid raster evidence metadata"));
        }
        if shape.polygons.is_empty() {
            return Err(diagnostic("output", "missing polygon loops"));
        }
        let mut remaining = MAX_VERTICES;
        for polygon in &shape.polygons {
            remaining = remaining.checked_sub(polygon.len()).ok_or_else(|| {
                diagnostic(
                    "resource_limit",
                    if provenance.simplify == 0.0 {
                        "drawing exceeds 256 output vertices; exact trace is not simplified"
                    } else {
                        "drawing exceeds 256 output vertices after the requested contour tolerance; no automatic increase is applied"
                    },
                )
            })?;
            if polygon.len() < 3 {
                return Err(diagnostic("output", "a polygon needs at least three points"));
            }
            if polygon.iter().any(|pixel| {
                !pixel.x.is_finite()
                    || !pixel.y.is_finite()
                    || frame.world(pixel).iter().any(|value| !value.is_finite())
            }) {
                return Err(diagnostic("output", "polygon coordinates must be finite pairs"));
            }
        }
        let material_area = datum
            .measure("area")
            .filter(|area| area.is_finite() && *area >= 0.0)
            .ok_or_else(|| diagnostic("output", "material_area must be finite and nonnegative"))?;
        let polygons = shape
            .polygons
            .iter()
            .map(|polygon| polygon.iter().map(|pixel| frame.world(pixel)).collect())
            .collect();
        let region = Arc::new(Region::from_polygons(polygons, krlyvis_api::metadata().length_unit)?);
        Ok(Self { datum: Arc::new(datum), region, material_area, provenance, quality, timing })
    }

    /// The original typed provider datum, shared without copying its geometry.
    ///
    /// This is always [`Datum::Placed`]. Cloning this Arc preserves identity;
    /// mutating an owned copy with `Arc::make_mut` leaves this snapshot unchanged.
    pub fn datum(&self) -> &Arc<Datum> {
        &self.datum
    }

    /// Format-independent immutable XY region, projected once at admission.
    /// Raster provenance and uncertainty remain on this evidence owner. The
    /// region describes the supplied boundary, not its physical interpretation.
    pub fn region(&self) -> &Arc<Region> {
        &self.region
    }

    /// Computed image identity and the original caller's unverified source claim.
    pub fn provenance(&self) -> &DrawingProvenance {
        &self.provenance
    }

    /// Raster assumptions and unknown measurement and positional uncertainty.
    pub fn quality(&self) -> &DrawingQuality {
        &self.quality
    }

    /// Timing of the original native read, excluding later adapter conversions.
    pub fn timing(&self) -> &DrawingTiming {
        &self.timing
    }

    /// World coordinates along x and y, outer loop first, followed by holes.
    ///
    /// Uses the retained provider's `Frame::world` projection on native pixels;
    /// loops are closed implicitly and no JSON conversion is performed.
    pub fn polygons(&self) -> Vec<Vec<[f64; 2]>> {
        match self.datum.as_ref() {
            Datum::Placed { shape, frame } => shape
                .polygons
                .iter()
                .map(|polygon| polygon.iter().map(|pixel| frame.world(pixel)).collect())
                .collect(),
            // Construction validates the placed-datum invariant.
            _ => Vec::new(),
        }
    }

    /// Finite, nonnegative world area, with holes subtracted by `Datum::measure`.
    /// The checked provider measurement is retained from the original read.
    pub fn material_area(&self) -> f64 {
        self.material_area
    }

    /// Convert this snapshot to wire evidence for one caller's correlation ID.
    ///
    /// This is the only native-to-JSON adapter. It uses `Datum::value` unchanged
    /// and adds the local result label. It neither rereads the image nor changes
    /// the retained datum, provenance, quality, or original timing.
    pub fn to_result(&self, request_id: &str) -> Result<DrawingResult, Diagnostic> {
        let mut evidence = value::to_json(self.datum.value())?;
        let fields =
            evidence.as_object_mut().ok_or_else(|| diagnostic("output", "placed datum is not an object"))?;
        fields.insert("node".into(), LOCAL_NODE.into());
        let result = DrawingResult {
            protocol: PROTOCOL.into(),
            request_id: request_id.into(),
            evidence,
            provenance: self.provenance.clone(),
            quality: self.quality.clone(),
            timing: self.timing.clone(),
        };
        result.validate()?;
        Ok(result)
    }
}
