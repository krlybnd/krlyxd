//! What every vanishing-point method must satisfy: segments drawn exactly towards three
//! known points, one of them at infinity, give back those points with exactly those
//! segments as members, whatever clutter lies between them.

use super::{conditioning, vanish, VanishRequest, Vanishing};
use crate::context::Context;
use crate::image::Pixel;
use crate::ports::detect::Segment;
use krly3d_core::{Attempt, Id};
use nalgebra::{Vector2, Vector3};

const SIZE: (usize, usize) = (800, 600);

/// Where the segments of each group start.
const STARTS: [(f64, f64); 6] =
    [(200.0, 150.0), (350.0, 420.0), (500.0, 250.0), (620.0, 480.0), (150.0, 500.0), (700.0, 120.0)];

/// Segments in no group: each runs at least 20 degrees away from every true point.
const CLUTTER: [((f64, f64), f64); 3] =
    [((400.0, 300.0), 45.0), ((250.0, 350.0), 135.0), ((550.0, 200.0), 30.0)];

pub fn check(cx: &Context, method: &str) -> Result<(), String> {
    let truth =
        [Vector3::new(1500.0, 280.0, 1.0), Vector3::new(-900.0, 320.0, 1.0), Vector3::new(0.05, 1.0, 0.0)];
    let (segments, groups) = drawn(&truth);
    let request = VanishRequest { segments, size: SIZE };
    let found = match vanish(cx, &Id::new(method), &request) {
        Attempt::Done(found) => found,
        other => return Err(format!("`{method}` did not run: {other:?}")),
    };
    if found.windows(2).any(|pair| pair[0].support < pair[1].support) {
        return Err(format!("`{method}` did not sort its points by support: {found:?}"));
    }
    for (point, members) in truth.iter().zip(&groups) {
        if !found.iter().any(|candidate| same(candidate, point, members)) {
            return Err(format!(
                "`{method}` missed the point {point:?} with members {members:?}; found {found:?}"
            ));
        }
    }
    Ok(())
}

/// The segments towards each point, followed by the clutter, and the members of each point.
fn drawn(truth: &[Vector3<f64>; 3]) -> (Vec<Segment>, Vec<Vec<usize>>) {
    let mut segments = Vec::new();
    let mut groups = Vec::new();
    for (group, point) in truth.iter().enumerate() {
        let mut members = Vec::new();
        for (index, &(x, y)) in STARTS.iter().enumerate() {
            let from = Pixel::new(x, y);
            let towards = Vector2::new(point.x - x * point.z, point.y - y * point.z).normalize();
            let length = 60.0 + 10.0 * index as f64 + 40.0 * (2 - group) as f64;
            members.push(segments.len());
            segments.push(Segment { from, to: from + towards * length, width: 1.0, significance: 10.0 });
        }
        groups.push(members);
    }
    for &((x, y), degrees) in &CLUTTER {
        let from = Pixel::new(x, y);
        let along = Vector2::new(f64::cos(f64::to_radians(degrees)), f64::sin(f64::to_radians(degrees)));
        segments.push(Segment { from, to: from + along * 70.0, width: 1.0, significance: 10.0 });
    }
    (segments, groups)
}

/// Whether a found point is the true one with exactly the expected members.
fn same(found: &Vanishing, truth: &Vector3<f64>, members: &[usize]) -> bool {
    let condition = conditioning(SIZE);
    let (a, b) = ((condition * found.point).normalize(), (condition * truth).normalize());
    found.members == members && 1.0 - a.dot(&b).abs() <= 1e-9
}
