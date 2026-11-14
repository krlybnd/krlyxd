//! What every curve must satisfy. The tests of a curve plugin call `check` on instances
//! of it; a plugin that passes can stand in wherever a curve is expected.

use super::Curve;
use crate::ports::sample;
use krly3d_core::math::{Point3, Vec3};
use krly3d_core::Accuracy;

pub fn check(curve: &dyn Curve, parameters: &[f64], accuracy: &Accuracy) -> Result<(), String> {
    let tolerance = accuracy.length();
    let step = sample::difference_step();
    let by = sample::similarity();
    let moved = curve.transformed(&by);
    let mirror = sample::mirror()?;
    if moved.classes() != curve.classes() {
        return Err(format!(
            "moving changed the classes from {:?} to {:?}",
            curve.classes(),
            moved.classes()
        ));
    }
    for &t in parameters {
        let point = curve.point(t);
        near(&curve.point(curve.project(&point)), &point, tolerance)
            .map_err(|gap| format!("projecting the point at t={t} lands {gap} away from it"))?;
        let tangent = curve.tangent(t);
        let secant = (curve.point(t + step) - curve.point(t - step)) / (2.0 * step);
        if (secant - tangent).norm() > step * (1.0 + tangent.norm()) {
            return Err(format!("tangent at t={t} is {tangent:?} but the curve moves along {secant:?}"));
        }
        if let Some(period) = curve.period() {
            near(&curve.point(t + period), &point, tolerance).map_err(|gap| {
                format!("the curve does not repeat after its period: off by {gap} at t={t}")
            })?;
        }
        near(&moved.point(t), &(by * point), tolerance * by.scaling())
            .map_err(|gap| format!("the moved curve is {gap} away from the moved point at t={t}"))?;
        if let Some(image) = curve.mirrored(&mirror) {
            near(&image.point(t), &mirror.point(&point), tolerance).map_err(|gap| {
                format!("the mirrored curve is {gap} away from the reflected point at t={t}")
            })?;
        }
    }
    for pair in parameters.windows(2) {
        fan(curve, pair[0], pair[1])?;
        extent(curve, pair[0], pair[1], accuracy)?;
    }
    if curve.summary().is_empty() {
        return Err("the curve has no summary".into());
    }
    Ok(())
}

/// The closed-form vector area against a fan of many thin triangles.
fn fan(curve: &dyn Curve, from: f64, to: f64) -> Result<(), String> {
    const TRIANGLES: u32 = 4000;
    let at = |step: u32| curve.point(from + (to - from) * f64::from(step) / f64::from(TRIANGLES));
    let summed: Vec3 = (0..TRIANGLES).map(|step| at(step).coords.cross(&at(step + 1).coords) * 0.5).sum();
    let stated = curve.vector_area(from, to);
    // The triangles miss the sliver between chord and arc, which shrinks with the square of
    // their number.
    let slack = (1.0 + summed.norm()) / f64::from(TRIANGLES);
    if (stated - summed).norm() > slack {
        return Err(format!(
            "vector area from t={from} to t={to} is stated as {stated:?} but sums to {summed:?}"
        ));
    }
    Ok(())
}

/// The stated box and length of a stretch against a dense polyline along it.
fn extent(curve: &dyn Curve, from: f64, to: f64, accuracy: &Accuracy) -> Result<(), String> {
    const STEPS: u32 = 4000;
    let at = |step: u32| curve.point(from + (to - from) * f64::from(step) / f64::from(STEPS));
    let sampled = krly3d_core::math::bounds::Bounds::of((0..=STEPS).map(at));
    let stated = curve.bounds(from.min(to), from.max(to));
    let length: f64 = (0..STEPS).map(|step| (at(step + 1) - at(step)).norm()).sum();
    // The polyline cuts corners, so it is a little shorter and a little smaller.
    let slack = accuracy.length() + (1.0 + length) / f64::from(STEPS);
    if (stated.low - sampled.low).norm() > slack || (stated.high - sampled.high).norm() > slack {
        return Err(format!(
            "the box from t={from} to t={to} is stated as {stated:?} but the curve fills {sampled:?}"
        ));
    }
    if (curve.length(from, to) - length).abs() > slack {
        return Err(format!(
            "the length from t={from} to t={to} is stated as {} but measures {length}",
            curve.length(from, to)
        ));
    }
    Ok(())
}

fn near(actual: &Point3, expected: &Point3, tolerance: f64) -> Result<(), f64> {
    let gap = (actual - expected).norm();
    if gap <= tolerance {
        Ok(())
    } else {
        Err(gap)
    }
}
