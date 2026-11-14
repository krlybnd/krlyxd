//! Segments standing on a flat ground.
//!
//! In the camera frame the ground is the plane `up·X = -1`, the camera's height being
//! the unit. A vertical segment's lower end, taken to touch the ground, lifts onto it as
//! `F = r / (-up·r)`. Its upper end lies on the vertical through the foot, `F + s·up`,
//! and on its own line of sight `t`, so `(F + s·up) × t = 0`, which gives the height in
//! closed form: `s = -(F × t)·(up × t) / |up × t|²`.

use krly3d_core::{Attempt, Error, Id};
use krlyvis_kernel::ports::camera::Camera;
use krlyvis_kernel::ports::depth::{Depth, DepthRequest, Depths, Standing, Unit};
use krlyvis_kernel::ports::detect::Segment;
use krlyvis_kernel::{Axis, Context, Plugin};
use nalgebra::{Vector2, Vector3};

pub struct Ground {
    /// How far, in degrees, a segment may point away from the vertical vanishing point and
    /// still count as vertical.
    pub tolerance: f64,
}

impl Default for Ground {
    fn default() -> Self {
        Self { tolerance: 2.0 }
    }
}

impl Plugin<Depth> for Ground {
    fn id(&self) -> Id {
        Id::new("ground")
    }

    fn call(&self, _: &Context, request: &DepthRequest) -> Attempt<Depths> {
        Attempt::from(self.run(request))
    }
}

/// Lines of sight closer than this to the horizon (the cosine with the ground) meet the
/// ground too far away to be trusted.
const GRAZING: f64 = 1e-6;

impl Ground {
    fn run(&self, request: &DepthRequest) -> Result<Depths, Error> {
        let camera = &request.camera;
        let up = camera.axis(Axis::Z);
        let vertical = camera.intrinsics() * up;
        let cos_tolerance = self.tolerance.to_radians().cos();
        let standing: Vec<Standing> = request
            .segments
            .iter()
            .enumerate()
            .filter(|(_, segment)| runs_to(segment, &vertical, cos_tolerance))
            .filter_map(|(id, segment)| stand(camera, id, segment))
            .collect();
        let (unit, scale) = match (request.eye, request.known) {
            (Some(eye), _) if eye.is_finite() && eye > 0.0 => (Unit::Eye(eye), eye),
            (Some(eye), _) => {
                return Err(Error::input(format!("the camera's height must be positive, not {eye}")))
            }
            (None, Some(known)) => match standing.iter().find(|found| found.segment == known.segment) {
                Some(found) if found.height > 0.0 => (Unit::Known(known), known.length / found.height),
                _ => (Unit::CameraHeight, 1.0),
            },
            (None, None) => (Unit::CameraHeight, 1.0),
        };
        let standing = standing
            .into_iter()
            .map(|found| Standing {
                height: found.height * scale,
                distance: found.distance * scale,
                foot: found.foot * scale,
                ..found
            })
            .collect();
        Ok(Depths { unit, standing })
    }
}

/// Whether a segment runs towards a homogeneous point.
fn runs_to(segment: &Segment, point: &Vector3<f64>, cos_tolerance: f64) -> bool {
    let towards = point.xy() - segment.midpoint().coords * point.z;
    let norm = towards.norm();
    norm > f64::EPSILON * point.norm() && towards.dot(&segment.direction()).abs() / norm >= cos_tolerance
}

/// A vertical segment standing on the ground with its lower end, in units of the
/// camera's height; `None` when that end is not below the horizon.
fn stand(camera: &Camera, id: usize, segment: &Segment) -> Option<Standing> {
    let up = camera.axis(Axis::Z);
    let (a, b) = (camera.ray(segment.from), camera.ray(segment.to));
    // The foot is the end whose line of sight points further down.
    let (foot_ray, top_ray) = if up.dot(&a.normalize()) < up.dot(&b.normalize()) { (a, b) } else { (b, a) };
    let falling = -up.dot(&foot_ray);
    if falling <= GRAZING * foot_ray.norm() {
        return None;
    }
    let foot = foot_ray / falling;
    let across = up.cross(&top_ray);
    let squared = across.norm_squared();
    if squared <= f64::EPSILON * top_ray.norm_squared() {
        return None;
    }
    let height = -foot.cross(&top_ray).dot(&across) / squared;
    // The point of the ground below the camera is -up, so the foot lies `foot + up` from it.
    let along_ground = foot + up;
    Some(Standing {
        segment: id,
        height,
        distance: along_ground.norm(),
        foot: Vector2::new(along_ground.dot(&camera.axis(Axis::X)), along_ground.dot(&camera.axis(Axis::Y))),
    })
}

#[cfg(test)]
mod tests {
    use super::Ground;
    use krlyvis_kernel::ports::depth::{contract, Depth};
    use krlyvis_kernel::VisionBuilder;

    #[test]
    fn ground_passes_the_contract_of_depth_methods() {
        let mut builder = VisionBuilder::new();
        builder.plugin::<Depth>(Ground::default());

        contract::check(&builder.context(), "ground").unwrap();
    }
}
