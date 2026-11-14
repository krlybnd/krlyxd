//! What every depth method must satisfy: posts of known height standing on the ground,
//! drawn by a known camera at a known height, come back with their heights and the
//! distances of their feet; a slanting segment is not taken for a post; one known
//! height sets the same unit as the known camera height.

use super::{depth, DepthRequest, Depths, Unit};
use crate::context::Context;
use crate::ports::detect::Segment;
use crate::ports::sample;
use crate::scene::Known;
use krly3d_core::{Attempt, Id};
use nalgebra::{Vector2, Vector3};

const CLOSE: f64 = 1e-9;
/// The sample camera stands this high above the ground, at (-5, -14).
const EYE: [f64; 3] = [-5.0, -14.0, 7.0];
/// Posts as (x, y, height).
const POSTS: [(f64, f64, f64); 4] = [(0.0, 0.0, 6.0), (12.0, 0.0, 3.5), (4.0, 9.0, 10.0), (-3.0, 2.0, 1.0)];

pub fn check(cx: &Context, method: &str) -> Result<(), String> {
    let camera = sample::camera(35.0, 15.0);
    let eye = Vector3::from(EYE);
    let seen =
        |point: Vector3<f64>| sample::project(&camera, &eye, &point).ok_or("a post is behind the camera");
    let mut segments = Vec::new();
    for (x, y, height) in POSTS {
        // Drawn top first, so that a method cannot take the first end for the foot.
        let (top, foot) = (seen(Vector3::new(x, y, height))?, seen(Vector3::new(x, y, 0.0))?);
        segments.push(Segment { from: top, to: foot, width: 1.0, significance: 10.0 });
    }
    let (from, to) = (seen(Vector3::new(2.0, 2.0, 0.0))?, seen(Vector3::new(6.0, 3.0, 4.0))?);
    segments.push(Segment { from, to, width: 1.0, significance: 10.0 });

    let by_eye =
        DepthRequest { camera: camera.clone(), segments: segments.clone(), eye: Some(EYE[2]), known: None };
    let found = run(cx, method, &by_eye)?;
    agrees(&found, method)?;
    if found.unit != Unit::Eye(EYE[2]) {
        return Err(format!("`{method}` set the unit by {:?}", found.unit));
    }
    let known = Known { segment: 2, length: POSTS[2].2 };
    let by_post = DepthRequest { camera, segments, eye: None, known: Some(known) };
    let found = run(cx, method, &by_post)?;
    if found.unit != Unit::Known(known) {
        return Err(format!("`{method}` set the unit by {:?}", found.unit));
    }
    agrees(&found, method)
}

fn run(cx: &Context, method: &str, request: &DepthRequest) -> Result<Depths, String> {
    match depth(cx, &Id::new(method), request) {
        Attempt::Done(found) => Ok(found),
        other => Err(format!("`{method}` did not run: {other:?}")),
    }
}

fn agrees(found: &Depths, method: &str) -> Result<(), String> {
    let ids: Vec<usize> = found.standing.iter().map(|standing| standing.segment).collect();
    if ids != [0, 1, 2, 3] {
        return Err(format!("`{method}` took segments {ids:?} for the posts 0 to 3"));
    }
    for (standing, (x, y, height)) in found.standing.iter().zip(POSTS) {
        let foot = Vector2::new(x - EYE[0], y - EYE[1]);
        let off = (standing.height - height).abs() > CLOSE * height
            || (standing.distance - foot.norm()).abs() > CLOSE * foot.norm()
            || (standing.foot - foot).norm() > CLOSE * foot.norm();
        if off {
            return Err(format!(
                "`{method}` found {standing:?} for a post {height} high with its foot at {foot}"
            ));
        }
    }
    Ok(())
}
