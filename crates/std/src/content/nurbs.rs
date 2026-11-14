//! Content of a NURBS face that is a band between two iso-curves of constant `v`, by
//! Gauss–Legendre quadrature of `|Su × Sv|` and of `S·(Su × Sv)` on every knot span. The
//! integrands are smooth within a span, so the result is accurate to round-off; it is
//! marked numeric all the same.

use super::side;
use crate::surface::Nurbs;
use krly3d_core::math::quadrature;
use krly3d_core::{Attempt, Exactness, Id};
use krly3d_kernel::ports::content::{Content, ContentRequest, FaceContent};
use krly3d_kernel::ports::surface::Surface;
use krly3d_kernel::{Context, Plugin};

/// Sub-intervals of a knot span in each direction.
const PIECES: usize = 4;
/// The relative accuracy claimed.
const RELATIVE: f64 = 1e-10;

pub struct NurbsContent;

impl Plugin<Content> for NurbsContent {
    fn id(&self) -> Id {
        Id::new("nurbs")
    }

    fn call(&self, _: &Context, request: &ContentRequest) -> Attempt<FaceContent> {
        let face = match request.target() {
            Ok(face) => face,
            Err(error) => return Attempt::Failed(error),
        };
        let Some(surface) = face.surface.as_any().downcast_ref::<Nurbs>() else {
            return Attempt::declined("the nurbs rule takes a face on a NURBS surface");
        };
        let Some((low, high)) = surface.band(&crate::trim::loop_points(&request.body, face)) else {
            return Attempt::declined(
                "a NURBS face has a content rule only between two iso-curves of constant v",
            );
        };
        if surface.periods()[0].is_none() {
            return Attempt::declined("a NURBS band has a content rule only when it closes around in u");
        }
        let data = surface.data();
        let knots = |basis: &krly3d_core::math::nurbs::Basis, from: f64, to: f64| {
            let mut breaks: Vec<f64> = vec![from];
            breaks.extend(basis.knots.iter().copied().filter(|knot| *knot > from && *knot < to));
            breaks.push(to);
            breaks.dedup();
            breaks
        };
        let (start, end) = data.u.domain();
        let pieces = |breaks: Vec<f64>| -> Vec<(f64, f64)> {
            breaks.windows(2).flat_map(|span| quadrature::nodes(span[0], span[1], PIECES)).collect()
        };
        let (across, along) = (pieces(knots(&data.u, start, end)), pieces(knots(&data.v, low, high)));
        let orientation = side(face.outward);
        let (mut area, mut flux) = (0.0, 0.0);
        for (u, wu) in &across {
            for (v, wv) in &along {
                let d = data.derivatives(*u, *v, 1);
                // Su × Sv is the surface's normal direction; the face takes it or its opposite.
                let normal = d[1][0].cross(&d[0][1]);
                let size = normal.norm();
                area += size * wu * wv;
                flux += orientation * d[0][0].dot(&normal) * wu * wv;
            }
        }
        Attempt::Done(FaceContent { area, flux, exact: Exactness::Numeric { tolerance: area * RELATIVE } })
    }
}
