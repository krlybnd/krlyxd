//! Lengths in a plane by lifting each pixel onto it.
//!
//! With `n` the plane's normal in the camera frame, the plane at unit distance is
//! `n·X = ±1`, and the pixel with line of sight `r` lies on it at `X = r / (±n·r)`. The
//! sign is the side the plane is seen from, taken at the reference segment.

use krly3d_core::{Attempt, Error, Id};
use krlyvis_kernel::ports::measure::{Measure, MeasureRequest, Measured};
use krlyvis_kernel::{Context, Pixel, Plugin};
use nalgebra::{Vector2, Vector3};

pub struct Plane;

impl Plugin<Measure> for Plane {
    fn id(&self) -> Id {
        Id::new("plane")
    }

    fn call(&self, _: &Context, request: &MeasureRequest) -> Attempt<Vec<Measured>> {
        Attempt::from(measure(request))
    }
}

/// Lines of sight closer than this to the plane's direction (the cosine with the plane)
/// meet it too far away to be trusted: they run to its horizon.
const GRAZING: f64 = 1e-6;

fn measure(request: &MeasureRequest) -> Result<Vec<Measured>, Error> {
    let camera = &request.camera;
    let normal = camera.axis(request.normal);
    let [first, second] = request.normal.others().map(|axis| camera.axis(axis));
    let segment = |id: usize| {
        request.segments.get(id).ok_or_else(|| Error::input(format!("there is no segment s{id}")))
    };
    let reference = match (request.known, request.which.first()) {
        (Some(known), _) => known.segment,
        (None, Some(&first)) => first,
        (None, None) => return Ok(Vec::new()),
    };
    let side = normal.dot(&camera.ray(segment(reference)?.midpoint())).signum();
    let lift = |pixel: Pixel| {
        let ray = camera.ray(pixel);
        let facing = side * normal.dot(&ray);
        (facing > GRAZING * ray.norm()).then(|| ray / facing)
    };
    let unit = match request.known {
        Some(known) => {
            let ends = segment(known.segment)?;
            let (Some(from), Some(to)) = (lift(ends.from), lift(ends.to)) else {
                return Err(Error::input(format!(
                    "the known segment s{} does not lie on the plane",
                    known.segment
                )));
            };
            let span = (to - from).norm();
            if !(known.length > 0.0 && span > 0.0) {
                return Err(Error::input(format!("s{} cannot set the unit", known.segment)));
            }
            known.length / span
        }
        None => 1.0,
    };
    let in_plane = |point: Vector3<f64>| Vector2::new(point.dot(&first), point.dot(&second)) * unit;
    let mut measured = Vec::with_capacity(request.which.len());
    for &id in &request.which {
        let ends = segment(id)?;
        if let (Some(from), Some(to)) = (lift(ends.from), lift(ends.to)) {
            measured.push(Measured {
                segment: id,
                length: (to - from).norm() * unit,
                from: in_plane(from),
                to: in_plane(to),
            });
        }
    }
    Ok(measured)
}

#[cfg(test)]
mod tests {
    use super::Plane;
    use krlyvis_kernel::ports::measure::{contract, Measure};
    use krlyvis_kernel::VisionBuilder;

    #[test]
    fn plane_passes_the_contract_of_measuring_methods() {
        let mut builder = VisionBuilder::new();
        builder.plugin::<Measure>(Plane);

        contract::check(&builder.context(), "plane").unwrap();
    }
}
