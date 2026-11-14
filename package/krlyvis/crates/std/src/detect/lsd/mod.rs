//! LSD, the line segment detector of Grompone von Gioi, Jakubowicz, Morel and Randall
//! (IPOL 2012), written from the article.
//!
//! A segment is a band of pixels whose level lines (the directions square to the
//! gradient) agree within a tolerance. A band is kept only when that much agreement
//! would be expected less than once in an image of noise of the same size (the number
//! of false alarms), so the detector needs no threshold tuned per image.

mod field;
mod nfa;
mod rect;
mod region;
mod sampling;

use field::Field;
use krly3d_core::{Attempt, Error, Id};
use krlyvis_kernel::ports::detect::{Detect, Segment};
use krlyvis_kernel::{Context, Gray, Pixel, Plugin};
use rect::Rect;
use std::sync::Arc;

pub struct Lsd {
    /// The image is first scaled by this factor through a Gaussian filter, which undoes
    /// the staircase of aliased edges.
    pub scale: f64,
    /// The deviation of that filter, in pixels of the scaled image.
    pub sigma_scale: f64,
    /// The quantisation error of the grey levels, which bounds the gradient trusted.
    pub quant: f64,
    /// How far two level lines may differ and still agree, in degrees.
    pub angle_tolerance: f64,
    /// The decimal logarithm of the number of false segments allowed per image.
    pub log_eps: f64,
    /// The least share of its rectangle a band must fill.
    pub density: f64,
}

impl Default for Lsd {
    /// The values of the article, which it shows to suit any natural image.
    fn default() -> Self {
        Self { scale: 0.8, sigma_scale: 0.6, quant: 2.0, angle_tolerance: 22.5, log_eps: 0.0, density: 0.7 }
    }
}

impl Plugin<Detect> for Lsd {
    fn id(&self) -> Id {
        Id::new("lsd")
    }

    fn call(&self, _: &Context, image: &Arc<Gray>) -> Attempt<Vec<Segment>> {
        Attempt::from(self.run(image))
    }
}

/// What the search over one image keeps fixed.
struct Search<'a> {
    field: &'a Field,
    /// The angle tolerance in radians.
    prec: f64,
    /// The chance that a pixel of noise agrees with a given direction.
    p: f64,
    /// The decimal logarithm of the number of rectangles that could be tested.
    log_nt: f64,
    /// The fewest pixels a band can have and still be significant.
    min_size: usize,
    density: f64,
    log_eps: f64,
}

impl Search<'_> {
    /// The segment grown from one seed pixel, with its significance, if it is kept.
    fn grow_from(&self, used: &mut [bool], seed: usize) -> Option<(Rect, f64)> {
        let grown = region::grow(self.field, used, seed, self.prec);
        if grown.pixels.len() < self.min_size {
            return None;
        }
        let rect = Rect::of(&grown, self.field, self.prec, self.p);
        let (_, rect) = region::refine(self.field, used, grown, rect, self.density)?;
        let (rect, log_nfa) = rect::improve(rect, self.field, self.log_nt, self.log_eps);
        (log_nfa > self.log_eps).then_some((rect, log_nfa))
    }
}

impl Lsd {
    fn run(&self, image: &Gray) -> Result<Vec<Segment>, Error> {
        if !(self.scale > 0.0 && self.scale <= 1.0) {
            return Err(Error::input(format!("lsd scale {} is not in (0, 1]", self.scale)));
        }
        let scaled = if self.scale < 1.0 {
            sampling::gaussian(image, self.scale, self.sigma_scale)?
        } else {
            image.clone()
        };
        let prec = self.angle_tolerance.to_radians();
        let p = self.angle_tolerance / 180.0;
        let field = Field::new(&scaled, self.quant / prec.sin());
        let log_nt =
            5.0 * ((field.width() as f64).log10() + (field.height() as f64).log10()) / 2.0 + 11f64.log10();
        let search = Search {
            field: &field,
            prec,
            p,
            log_nt,
            min_size: (-log_nt / p.log10()) as usize,
            density: self.density,
            log_eps: self.log_eps,
        };
        let mut used = vec![false; field.len()];
        let mut found = Vec::new();
        for &seed in field.order() {
            if !used[seed] {
                if let Some(kept) = search.grow_from(&mut used, seed) {
                    found.push(self.segment(kept));
                }
            }
        }
        Ok(found)
    }

    /// A rectangle of the scaled field as a segment of the original image. The gradient
    /// of a pixel is taken from it and its neighbours to the right and below, so it
    /// belongs to the corner the four share: one pixel on from the pixel's own corner.
    fn segment(&self, (rect, log_nfa): (Rect, f64)) -> Segment {
        let to_image = |x: f64, y: f64| Pixel::new((x + 1.0) / self.scale, (y + 1.0) / self.scale);
        Segment {
            from: to_image(rect.x1, rect.y1),
            to: to_image(rect.x2, rect.y2),
            width: rect.width / self.scale,
            significance: log_nfa,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Lsd;
    use krlyvis_kernel::ports::detect::contract;
    use krlyvis_kernel::ports::detect::Detect;
    use krlyvis_kernel::VisionBuilder;

    #[test]
    fn lsd_passes_the_contract_of_detectors() {
        let mut builder = VisionBuilder::new();
        builder.plugin::<Detect>(Lsd::default());

        contract::check(&builder.context(), "lsd").unwrap();
    }

    #[test]
    fn lsd_without_scaling_passes_the_contract_too() {
        let mut builder = VisionBuilder::new();
        builder.plugin::<Detect>(Lsd { scale: 1.0, ..Lsd::default() });

        contract::check(&builder.context(), "lsd").unwrap();
    }
}
