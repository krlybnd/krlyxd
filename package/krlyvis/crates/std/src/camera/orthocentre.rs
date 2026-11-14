//! The centre and the focal length from three finite vanishing points of square
//! directions: the centre is the orthocentre of their triangle, and
//! `f² = -(a - c)·(b - c)` for any two of them.

use super::{orient, plausible};
use krly3d_core::{Attempt, Id};
use krlyvis_kernel::ports::camera::{Calibrate, Camera, CameraRequest};
use krlyvis_kernel::ports::vanish::Vanishing;
use krlyvis_kernel::{Context, Pixel, Plugin};
use nalgebra::{Matrix2, Vector2};

pub struct Orthocentre;

impl Plugin<Calibrate> for Orthocentre {
    fn id(&self) -> Id {
        Id::new("orthocentre")
    }

    fn call(&self, _: &Context, request: &CameraRequest) -> Attempt<Camera> {
        let finite: Vec<Pixel> = request.vanishing.iter().take(3).filter_map(Vanishing::pixel).collect();
        let [a, b, c] = finite[..] else {
            return Attempt::declined(
                "the orthocentre needs three vanishing points none of which is at infinity",
            );
        };
        let Some(centre) = orthocentre(a, b, c) else {
            return Attempt::declined("the three vanishing points lie on one line");
        };
        let squared = -(a - centre).dot(&(b - centre));
        let focal = squared.sqrt();
        let (width, height) = request.size;
        if !(squared > 0.0 && plausible(focal, request.size)) {
            Attempt::declined("the vanishing points make a triangle no camera gives")
        } else if !(0.0..=width as f64).contains(&centre.x) || !(0.0..=height as f64).contains(&centre.y) {
            Attempt::declined(format!("the centre found, {centre}, lies outside the image"))
        } else {
            Attempt::from(orient(focal, centre, &request.vanishing, "orthocentre"))
        }
    }
}

/// The point where the altitudes of a triangle meet: `(h - a)·(b - c) = 0` and
/// `(h - b)·(a - c) = 0`.
fn orthocentre(a: Pixel, b: Pixel, c: Pixel) -> Option<Pixel> {
    let (bc, ac) = (b - c, a - c);
    let system = Matrix2::new(bc.x, bc.y, ac.x, ac.y);
    let solved = system.try_inverse()? * Vector2::new(bc.dot(&a.coords), ac.dot(&b.coords));
    Some(Pixel::from(solved))
}

#[cfg(test)]
mod tests {
    use super::{orthocentre, Orthocentre};
    use krlyvis_kernel::ports::camera::{contract, Calibrate};
    use krlyvis_kernel::{Pixel, VisionBuilder};

    #[test]
    fn orthocentre_passes_the_contract_of_calibrations() {
        let mut builder = VisionBuilder::new();
        builder.plugin::<Calibrate>(Orthocentre);

        contract::check(&builder.context(), "orthocentre", None).unwrap();
    }

    #[test]
    fn the_orthocentre_of_a_right_triangle_is_its_right_corner() {
        let found = orthocentre(Pixel::new(0.0, 0.0), Pixel::new(4.0, 0.0), Pixel::new(0.0, 3.0)).unwrap();

        assert!((found - Pixel::new(0.0, 0.0)).norm() < 1e-12);
    }
}
