//! Growing a band of agreeing pixels from a seed, and tightening it until it fills its
//! rectangle well enough.

use super::field::Field;
use super::rect::Rect;
use std::f64::consts::{PI, TAU};

pub(super) struct Region {
    /// The seed comes first.
    pub(super) pixels: Vec<usize>,
    /// The mean level-line direction of the pixels.
    pub(super) angle: f64,
}

/// The signed difference `a - b` of two angles, in (-pi, pi].
pub(super) fn difference(a: f64, b: f64) -> f64 {
    let turned = (a - b).rem_euclid(TAU);
    if turned > PI {
        turned - TAU
    } else {
        turned
    }
}

/// Whether a pixel's level line (`None`: no direction) agrees with `theta`.
pub(super) fn aligned(angle: Option<f64>, theta: f64, tolerance: f64) -> bool {
    angle.is_some_and(|angle| difference(angle, theta).abs() <= tolerance)
}

/// Collects the unused neighbours, and theirs in turn, whose level lines agree with the
/// running mean direction of the region within `tolerance`.
pub(super) fn grow(field: &Field, used: &mut [bool], seed: usize, tolerance: f64) -> Region {
    let Some(start) = field.angle(seed) else {
        return Region { pixels: Vec::new(), angle: 0.0 };
    };
    used[seed] = true;
    let (mut sum_cos, mut sum_sin) = (start.cos(), start.sin());
    let mut region = Region { pixels: vec![seed], angle: start };
    let mut next = 0;
    while let Some(&pixel) = region.pixels.get(next) {
        next += 1;
        for neighbour in field.neighbours(pixel) {
            let angle = field.angle(neighbour);
            if !used[neighbour] && aligned(angle, region.angle, tolerance) {
                used[neighbour] = true;
                region.pixels.push(neighbour);
                if let Some(angle) = angle {
                    sum_cos += angle.cos();
                    sum_sin += angle.sin();
                }
                region.angle = sum_sin.atan2(sum_cos);
            }
        }
    }
    region
}

/// A region and its rectangle, tightened until the region fills at least `density` of
/// the rectangle: first by growing again with a tolerance learned near the seed, then by
/// shrinking the radius around the seed. `None` when too little is left.
pub(super) fn refine(
    field: &Field,
    used: &mut [bool],
    region: Region,
    rect: Rect,
    density: f64,
) -> Option<(Region, Rect)> {
    if rect.density(region.pixels.len()) >= density {
        return Some((region, rect));
    }
    let seed = *region.pixels.first()?;
    let tolerance = tolerance_near_seed(field, used, &region, rect.width)?;
    let region = grow(field, used, seed, tolerance);
    if region.pixels.len() < 2 {
        return None;
    }
    let rect = Rect::of(&region, field, rect.prec, rect.p);
    if rect.density(region.pixels.len()) >= density {
        return Some((region, rect));
    }
    reduce_radius(field, used, region, rect, density)
}

/// Twice the spread of the directions within `radius` of the seed, about the seed's own
/// direction. Frees every pixel of the region on the way, so that it can grow again.
fn tolerance_near_seed(field: &Field, used: &mut [bool], region: &Region, radius: f64) -> Option<f64> {
    let seed = *region.pixels.first()?;
    let seed_angle = field.angle(seed)?;
    let (seed_x, seed_y) = field.point(seed);
    let (mut sum, mut squares, mut count) = (0.0, 0.0, 0.0);
    for &pixel in &region.pixels {
        used[pixel] = false;
        let (x, y) = field.point(pixel);
        if let (true, Some(angle)) = ((x - seed_x).hypot(y - seed_y) < radius, field.angle(pixel)) {
            let turned = difference(angle, seed_angle);
            sum += turned;
            squares += turned * turned;
            count += 1.0;
        }
    }
    let mean = sum / count;
    Some(2.0 * ((squares - 2.0 * mean * sum) / count + mean * mean).sqrt())
}

/// Drops the pixels farthest from the seed, a quarter of the radius at a time, until the
/// region fills its rectangle well enough.
fn reduce_radius(
    field: &Field,
    used: &mut [bool],
    mut region: Region,
    mut rect: Rect,
    density: f64,
) -> Option<(Region, Rect)> {
    let seed = *region.pixels.first()?;
    let (seed_x, seed_y) = field.point(seed);
    let mut radius =
        (rect.x1 - seed_x).hypot(rect.y1 - seed_y).max((rect.x2 - seed_x).hypot(rect.y2 - seed_y));
    while rect.density(region.pixels.len()) < density {
        radius *= 0.75;
        region.pixels.retain(|&pixel| {
            let (x, y) = field.point(pixel);
            let keep = (x - seed_x).hypot(y - seed_y) <= radius;
            if !keep {
                used[pixel] = false;
            }
            keep
        });
        if region.pixels.len() < 2 {
            return None;
        }
        rect = Rect::of(&region, field, rect.prec, rect.p);
    }
    Some((region, rect))
}

#[cfg(test)]
mod tests {
    use super::{aligned, difference};
    use std::f64::consts::PI;

    #[test]
    fn differences_wrap_around_the_circle() {
        assert!((difference(0.1, -0.1) - 0.2).abs() < 1e-12);
        assert!((difference(PI - 0.1, -PI + 0.1) + 0.2).abs() < 1e-12);
        assert!((difference(PI, 0.0) - PI).abs() < 1e-12);
    }

    #[test]
    fn opposite_level_lines_do_not_agree() {
        assert!(aligned(Some(0.1), 0.0, 0.2));
        assert!(!aligned(Some(PI), 0.0, 0.2));
        assert!(!aligned(None, 0.0, 0.2));
    }
}
