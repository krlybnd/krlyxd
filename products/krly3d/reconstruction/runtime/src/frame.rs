//! Deterministic CAD coordinate frame for top-view raster contours.

use krly2d_api::{Point2, Region};
use krlyxd::{Diagnostic, Id};
use std::sync::Arc;

/// Maps the source top-view XY region into a centered, right-handed Z-up UCS.
/// Source orientation is retained so the reconstructed top view matches its evidence.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConstructionFrame {
    pub source_center: Point2,
    pub source_bounds: [Point2; 2],
}

impl ConstructionFrame {
    pub fn model_point(self, source: Point2) -> Point2 {
        [source[0] - self.source_center[0], source[1] - self.source_center[1]]
    }
}

/// Reframe the exact source loops without changing scale, topology, or loop order.
pub fn centered_top_view(region: &Region) -> Result<(Arc<Region>, ConstructionFrame), Diagnostic> {
    let polygons = region.polygons()?;
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for point in polygons.iter().flatten() {
        for axis in 0..2 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
        }
    }
    if !min.iter().chain(&max).all(|value| value.is_finite()) || min[0] >= max[0] || min[1] >= max[1] {
        return Err(Diagnostic {
            code: Id::new("geometry"),
            message: "top-view region needs finite, nondegenerate bounds".into(),
        });
    }
    let frame = ConstructionFrame {
        source_center: [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5],
        source_bounds: [min, max],
    };
    let placed = polygons
        .into_iter()
        .map(|polygon| polygon.into_iter().map(|point| frame.model_point(point)).collect())
        .collect();
    Ok((Arc::new(Region::from_polygons(placed, region.length_unit())?), frame))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::centered_top_view;
    use krly2d_api::Region;
    use krlyxd::LengthUnit;

    #[test]
    fn centers_bounds_at_origin_and_preserves_source_orientation() {
        let region = Region::from_polygons(
            vec![vec![[10.0, 20.0], [14.0, 20.0], [14.0, 26.0], [10.0, 26.0]]],
            LengthUnit::Model,
        )
        .unwrap();
        let (placed, frame) = centered_top_view(&region).unwrap();
        assert_eq!(frame.source_center, [12.0, 23.0]);
        assert_eq!(
            placed.polygons().unwrap(),
            vec![vec![[-2.0, -3.0], [2.0, -3.0], [2.0, 3.0], [-2.0, 3.0]]]
        );
        assert_eq!(placed.length_unit(), LengthUnit::Model);
    }
}
