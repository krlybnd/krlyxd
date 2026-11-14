//! The rectangle a region is approximated by, its number of false alarms, and the small
//! changes to it that may make it more significant.

use super::field::Field;
use super::nfa::log_nfa;
use super::region::{aligned, difference, Region};
use std::f64::consts::PI;

/// A rectangle around the segment from (`x1`, `y1`) to (`x2`, `y2`), in pixels of the
/// field, `width` across, together with the angle tolerance it is judged by.
#[derive(Clone, Copy, Debug)]
pub(super) struct Rect {
    pub(super) x1: f64,
    pub(super) y1: f64,
    pub(super) x2: f64,
    pub(super) y2: f64,
    pub(super) width: f64,
    /// The level-line direction; `x1 -> x2` runs along it.
    pub(super) theta: f64,
    /// The angle tolerance in radians.
    pub(super) prec: f64,
    /// The chance that a pixel of noise agrees within `prec`: `prec / pi`.
    pub(super) p: f64,
}

impl Rect {
    /// The smallest rectangle along the principal axis of the region, the pixels
    /// weighted by the size of their gradient.
    pub(super) fn of(region: &Region, field: &Field, prec: f64, p: f64) -> Self {
        let (mut x, mut y, mut sum) = (0.0, 0.0, 0.0);
        for &pixel in &region.pixels {
            let (weight, (px, py)) = (field.norm(pixel), field.point(pixel));
            x += weight * px;
            y += weight * py;
            sum += weight;
        }
        let (x, y) = (x / sum, y / sum);
        let theta = orientation(region, field, (x, y), prec);
        let (dx, dy) = (theta.cos(), theta.sin());
        let (mut along_min, mut along_max, mut across_min, mut across_max) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for &pixel in &region.pixels {
            let (px, py) = field.point(pixel);
            let along = (px - x) * dx + (py - y) * dy;
            let across = -(px - x) * dy + (py - y) * dx;
            along_min = along_min.min(along);
            along_max = along_max.max(along);
            across_min = across_min.min(across);
            across_max = across_max.max(across);
        }
        Self {
            x1: x + along_min * dx,
            y1: y + along_min * dy,
            x2: x + along_max * dx,
            y2: y + along_max * dy,
            width: (across_max - across_min).max(1.0),
            theta,
            prec,
            p,
        }
    }

    fn length(&self) -> f64 {
        (self.x2 - self.x1).hypot(self.y2 - self.y1)
    }

    /// The share of the rectangle that `count` pixels fill.
    pub(super) fn density(&self, count: usize) -> f64 {
        count as f64 / (self.length() * self.width)
    }

    /// The decimal logarithm of how many rectangles as good as this one an image of
    /// noise of the field's size would hold, negated: the larger, the surer.
    pub(super) fn log_nfa(&self, field: &Field, log_nt: f64) -> f64 {
        let (total, agreeing) = self.count(field);
        log_nfa(total, agreeing, self.p, log_nt)
    }

    /// The pixels of the field inside the rectangle, and how many of them agree with it.
    fn count(&self, field: &Field) -> (usize, usize) {
        let (dx, dy) = (self.theta.cos(), self.theta.sin());
        let (half_length, half_width) = (self.length() / 2.0, self.width / 2.0);
        let (mx, my) = ((self.x1 + self.x2) / 2.0, (self.y1 + self.y2) / 2.0);
        let reach_x = half_length * dx.abs() + half_width * dy.abs();
        let reach_y = half_length * dy.abs() + half_width * dx.abs();
        let range = |middle: f64, reach: f64, size: usize| {
            let low = (middle - reach).floor().max(0.0) as usize;
            let high = ((middle + reach).ceil().max(0.0) as usize).min(size.saturating_sub(1));
            low..=high
        };
        let (mut total, mut agreeing) = (0, 0);
        for y in range(my, reach_y, field.height()) {
            for x in range(mx, reach_x, field.width()) {
                let (ox, oy) = (x as f64 - mx, y as f64 - my);
                let inside =
                    (ox * dx + oy * dy).abs() <= half_length && (oy * dx - ox * dy).abs() <= half_width;
                if inside {
                    total += 1;
                    agreeing += usize::from(aligned(field.angle(field.index(x, y)), self.theta, self.prec));
                }
            }
        }
        (total, agreeing)
    }
}

/// The direction of the principal axis of the region (the eigenvector of the smallest
/// eigenvalue of its inertia), turned to agree with the region's level lines.
fn orientation(region: &Region, field: &Field, (x, y): (f64, f64), prec: f64) -> f64 {
    let (mut ixx, mut iyy, mut ixy) = (0.0, 0.0, 0.0);
    for &pixel in &region.pixels {
        let (weight, (px, py)) = (field.norm(pixel), field.point(pixel));
        ixx += (py - y) * (py - y) * weight;
        iyy += (px - x) * (px - x) * weight;
        ixy -= (px - x) * (py - y) * weight;
    }
    let lambda = 0.5 * (ixx + iyy - ((ixx - iyy) * (ixx - iyy) + 4.0 * ixy * ixy).sqrt());
    let theta = if ixx.abs() > iyy.abs() { (lambda - ixx).atan2(ixy) } else { ixy.atan2(lambda - iyy) };
    if difference(theta, region.angle).abs() > prec {
        theta + PI
    } else {
        theta
    }
}

/// How far a narrowing step moves a long side, in pixels.
const DELTA: f64 = 0.5;

/// Tries a finer tolerance, a narrower rectangle, each long side moved in, and a finer
/// tolerance again, keeping whatever is more significant, until the rectangle is
/// significant (more than `log_eps`) or every change has been tried.
pub(super) fn improve(rect: Rect, field: &Field, log_nt: f64, log_eps: f64) -> (Rect, f64) {
    let mut best = (rect, rect.log_nfa(field, log_nt));
    let steps: [fn(&mut Rect) -> bool; 5] = [finer, narrower, narrower_on_left, narrower_on_right, finer];
    for step in steps {
        if best.1 > log_eps {
            break;
        }
        let mut trial = best.0;
        for _ in 0..5 {
            if step(&mut trial) {
                let value = trial.log_nfa(field, log_nt);
                if value > best.1 {
                    best = (trial, value);
                }
            }
        }
    }
    best
}

fn finer(rect: &mut Rect) -> bool {
    rect.p /= 2.0;
    rect.prec = rect.p * PI;
    true
}

fn narrower(rect: &mut Rect) -> bool {
    let room = rect.width - DELTA >= 0.5;
    if room {
        rect.width -= DELTA;
    }
    room
}

fn narrower_on_left(rect: &mut Rect) -> bool {
    shift_in(rect, 1.0)
}

fn narrower_on_right(rect: &mut Rect) -> bool {
    shift_in(rect, -1.0)
}

/// Narrows the rectangle by moving one long side in: the axis moves half as far.
fn shift_in(rect: &mut Rect, side: f64) -> bool {
    if rect.width - DELTA < 0.5 {
        return false;
    }
    let (dx, dy) = (rect.theta.cos(), rect.theta.sin());
    let (shift_x, shift_y) = (-dy * side * DELTA / 2.0, dx * side * DELTA / 2.0);
    rect.x1 += shift_x;
    rect.y1 += shift_y;
    rect.x2 += shift_x;
    rect.y2 += shift_y;
    rect.width -= DELTA;
    true
}
