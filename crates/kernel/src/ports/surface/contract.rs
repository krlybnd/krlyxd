//! What every surface must satisfy. The tests of a surface plugin call `check` on
//! instances of it.

use super::Surface;
use crate::ports::sample;
use krly3d_core::math::{Point3, Uv, Vec3};
use krly3d_core::Accuracy;

/// `offset` is a distance by which the test steps off the surface along the normal; it
/// must be smaller than the smallest radius of curvature at the sampled points.
pub fn check(surface: &dyn Surface, samples: &[Uv], offset: f64, accuracy: &Accuracy) -> Result<(), String> {
    let by = sample::similarity();
    let moved = surface.transformed(&by);
    let mirror = sample::mirror()?;
    let image = surface.mirrored(&mirror);
    if moved.classes() != surface.classes() {
        return Err(format!(
            "moving changed the classes from {:?} to {:?}",
            surface.classes(),
            moved.classes()
        ));
    }
    if surface.summary().is_empty() {
        return Err("the surface has no summary".into());
    }
    for pair in samples.windows(2) {
        path(surface, pair[0], pair[1], accuracy)?;
    }
    for direction in [Vec3::x(), Vec3::new(0.3, -0.5, 0.8)] {
        if let Some(stray) = surface
            .extremes(&direction)
            .iter()
            .find(|point| !accuracy.is_zero_length(surface.distance(point)))
        {
            return Err(format!("the extreme point {stray:?} along {direction:?} is not on the surface"));
        }
    }
    for &uv in samples {
        parameterisation(surface, uv, accuracy)?;
        equation(surface, uv, offset, accuracy)?;
        let point = surface.point(uv);
        let moved_point = by * point;
        let moved_uv = moved.project(&moved_point);
        let gap = (moved.point(moved_uv) - moved_point).norm();
        if gap > accuracy.length() * by.scaling() {
            return Err(format!("the moved surface is {gap} away from the moved point of {uv:?}"));
        }
        let turned = by.isometry.rotation * surface.normal(uv);
        if (moved.normal(moved_uv) - turned).norm() > accuracy.angle().sqrt() {
            return Err(format!("the normal of the moved surface at {uv:?} is not the turned normal"));
        }
        if let Some(image) = &image {
            reflected(image.as_ref(), &mirror.point(&point), &mirror.vector(&surface.normal(uv)), accuracy)
                .map_err(|problem| format!("the mirrored surface at the image of {uv:?}: {problem}"))?;
        }
    }
    Ok(())
}

/// The image passes through the reflected point with the reflected normal, up to its sense.
fn reflected(image: &dyn Surface, point: &Point3, normal: &Vec3, accuracy: &Accuracy) -> Result<(), String> {
    let gap = image.distance(point);
    if !accuracy.is_zero_length(gap) {
        return Err(format!("it is {gap} away from the reflected point"));
    }
    let found = image.normal(image.project(point));
    if found.cross(normal).norm() > accuracy.angle().sqrt() {
        return Err(format!("its normal {found:?} is not along the reflected normal {normal:?}"));
    }
    Ok(())
}

/// A walk between two points starts at the first, ends at the second, has no gaps and
/// stays on the surface.
fn path(surface: &dyn Surface, from: Uv, to: Uv, accuracy: &Accuracy) -> Result<(), String> {
    let (start, end) = (surface.point(from), surface.point(to));
    let mut at = start;
    for stretch in surface.walk(&start, &end) {
        if !accuracy.same_point(&stretch.point(0.0), &at) {
            return Err(format!(
                "the walk from {from:?} to {to:?} jumps from {at:?} to {:?}",
                stretch.point(0.0)
            ));
        }
        for fraction in [0.25, 0.5, 0.75] {
            let gap = surface.distance(&stretch.point(fraction));
            if !accuracy.is_zero_length(gap) {
                return Err(format!("the walk from {from:?} to {to:?} leaves the surface by {gap}"));
            }
        }
        at = stretch.point(1.0);
    }
    if accuracy.same_point(&at, &end) {
        Ok(())
    } else {
        Err(format!("the walk from {from:?} to {to:?} ends at {at:?} instead of {end:?}"))
    }
}

/// Point, normal and projection agree with each other.
fn parameterisation(surface: &dyn Surface, uv: Uv, accuracy: &Accuracy) -> Result<(), String> {
    let step = sample::difference_step();
    let point = surface.point(uv);
    let gap = (surface.point(surface.project(&point)) - point).norm();
    if gap > accuracy.length() {
        return Err(format!("projecting the point at {uv:?} lands {gap} away from it"));
    }
    let normal = surface.normal(uv);
    if (normal.norm() - 1.0).abs() > accuracy.relative() {
        return Err(format!("the normal at {uv:?} has length {}", normal.norm()));
    }
    for (name, ahead, behind) in [
        ("u", Uv::new(uv.x + step, uv.y), Uv::new(uv.x - step, uv.y)),
        ("v", Uv::new(uv.x, uv.y + step), Uv::new(uv.x, uv.y - step)),
    ] {
        let partial = (surface.point(ahead) - surface.point(behind)) / (2.0 * step);
        if normal.dot(&partial).abs() > step * (1.0 + partial.norm()) {
            return Err(format!(
                "the normal at {uv:?} is not perpendicular to the {name} direction {partial:?}"
            ));
        }
    }
    Ok(())
}

/// Distance, line intersection and the quadric form agree with the parameterisation.
fn equation(surface: &dyn Surface, uv: Uv, offset: f64, accuracy: &Accuracy) -> Result<(), String> {
    let tolerance = accuracy.length();
    let (point, normal) = (surface.point(uv), surface.normal(uv));
    for signed in [offset, -offset, 0.0] {
        let distance = surface.distance(&(point + normal * signed));
        if (distance - signed).abs() > tolerance {
            return Err(format!("{signed} off the surface at {uv:?} the distance reads {distance}"));
        }
    }
    let origin = point + normal * offset;
    let mut hits = Vec::new();
    surface.line(&origin, &-normal, &mut hits);
    if !hits.iter().any(|t| (t - offset).abs() <= tolerance) {
        return Err(format!("a line dropped onto {uv:?} from {offset} away reports hits {hits:?}"));
    }
    if let Some(stray) = hits.iter().find(|t| surface.distance(&(origin - normal * **t)).abs() > tolerance) {
        return Err(format!("the line hit at t={stray} above {uv:?} is not on the surface"));
    }
    if let Some(quadric) = surface.quadric() {
        let gradient = quadric.gradient(&point);
        if quadric.value(&point).abs() > tolerance * gradient.norm() {
            return Err(format!("the quadric form does not vanish at {uv:?}: {}", quadric.value(&point)));
        }
        if gradient.normalize().dot(&normal) < 1.0 - accuracy.angle().sqrt() {
            return Err(format!("the quadric gradient at {uv:?} does not point along the normal"));
        }
    }
    Ok(())
}
