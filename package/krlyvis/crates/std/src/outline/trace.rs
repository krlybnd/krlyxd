//! The outline of the subject by threshold: the mask of the subject (`mask`), its boundary
//! followed along the edges of its pixels, and the corners kept that matter (`simplify`).
//!
//! The boundary runs along pixel edges, so its corners are corners of pixels and the
//! polygon encloses exactly the pixels of the subject.

use super::mask::{subject, Mask};
use super::simplify::simplified;
use krly3d_core::{Attempt, Error, Id};
use krlyvis_kernel::ports::outline::{Outline, OutlineRequest, Shape};
use krlyvis_kernel::{Context, Pixel, Plugin};

pub struct Trace;

impl Plugin<Outline> for Trace {
    fn id(&self) -> Id {
        Id::new("trace")
    }

    fn call(&self, _: &Context, request: &OutlineRequest) -> Attempt<Shape> {
        match outlined(request) {
            Ok(Some(shape)) => Attempt::Done(shape),
            Ok(None) => Attempt::declined("nothing in the picture stands apart from its background"),
            Err(error) => Attempt::Failed(error),
        }
    }
}

fn outlined(request: &OutlineRequest) -> Result<Option<Shape>, Error> {
    let (width, height) = request.image.size();
    let region = request.region.unwrap_or([0, 0, width, height]);
    let Some(mask) = subject(&request.image, region, request.seed, request.threshold)? else {
        return Ok(None);
    };
    let boundary = boundary(&mask).ok_or_else(|| Error::geometry("the subject has no boundary to follow"))?;
    Ok(Some(Shape {
        polygons: vec![simplified(&boundary, request.simplify)],
        pixels: mask.pixels,
        threshold: mask.threshold,
        dark: mask.dark,
    }))
}

/// Directions along pixel edges in the picture (y down): east, south, west, north.
const STEPS: [(i64, i64); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];

/// The outer boundary of the mask as the corners where it turns, walked with the subject
/// on the right hand, from the top-left corner of its top-left pixel.
fn boundary(mask: &Mask) -> Option<Vec<Pixel>> {
    let first = mask.inside.iter().position(|inside| *inside)?;
    let start = ((first % mask.width) as i64, (first / mask.width) as i64);
    let (mut at, mut heading) = (start, 0_usize);
    let mut corners = vec![Pixel::new(start.0 as f64, start.1 as f64)];
    // Each pixel edge is walked at most once; the bound only guards against a broken mask.
    for _ in 0..4 * (mask.pixels + 1) {
        at = (at.0 + STEPS[heading].0, at.1 + STEPS[heading].1);
        let (ahead_left, ahead_right) = beside(at, heading);
        let turned = if mask.at(ahead_left.0, ahead_left.1) {
            (heading + 3) % 4
        } else if mask.at(ahead_right.0, ahead_right.1) {
            heading
        } else {
            (heading + 1) % 4
        };
        if at == start && turned == 0 {
            return Some(corners);
        }
        if turned != heading {
            corners.push(Pixel::new(at.0 as f64, at.1 as f64));
            heading = turned;
        }
    }
    None
}

/// The pixels ahead of a corner on the left and on the right of the heading.
fn beside((x, y): (i64, i64), heading: usize) -> ((i64, i64), (i64, i64)) {
    match heading {
        0 => ((x, y - 1), (x, y)),
        1 => ((x, y), (x - 1, y)),
        2 => ((x - 1, y), (x - 1, y - 1)),
        _ => ((x - 1, y - 1), (x, y - 1)),
    }
}

#[cfg(test)]
mod tests {
    use super::Trace;
    use krlyvis_kernel::ports::outline::{contract, Outline};
    use krlyvis_kernel::VisionBuilder;

    #[test]
    fn satisfies_the_outline_contract() {
        let mut builder = VisionBuilder::new();
        builder.plugin::<Outline>(Trace);

        contract::check(&builder.context(), "trace").unwrap();
    }
}
