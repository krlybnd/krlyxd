//! The whole analysis on a picture drawn by a known camera: two walls of a building with
//! windows. What it finds is compared with what the camera that drew the picture says.

// Helpers in a test file are test code too; the lint only knows `#[test]` functions.
#![allow(clippy::unwrap_used, clippy::panic)]

use krly3d_core::FakeClock;
use krlyvis_kernel::ports::camera::Camera;
use krlyvis_kernel::ports::detect::Segment;
use krlyvis_kernel::ports::sample::{self, Canvas};
use krlyvis_kernel::report::report;
use krlyvis_kernel::{Ask, Axis, Gray, Known, Pixel, Scene};
use nalgebra::Vector3;
use std::sync::Arc;

const EYE: [f64; 3] = [-6.0, -12.0, 1.6];
/// The front wall lies in y = 0 from x = 0 to `WIDTH`, the side wall in x = 0 from
/// y = 0 to `DEPTH`; both are `HEIGHT` high.
const WIDTH: f64 = 12.0;
const DEPTH: f64 = 8.0;
const HEIGHT: f64 = 6.0;

fn truth() -> Camera {
    sample::camera(53.0, -4.0)
}

fn eye() -> Vector3<f64> {
    Vector3::from(EYE)
}

/// A rectangle of the front wall (`y = 0`) or the side wall (`x = 0`), by its spans
/// along the wall and up.
fn on_wall(front: bool, along: (f64, f64), up: (f64, f64)) -> Vec<Pixel> {
    let point = |a: f64, z: f64| if front { Vector3::new(a, 0.0, z) } else { Vector3::new(0.0, a, z) };
    [point(along.0, up.0), point(along.1, up.0), point(along.1, up.1), point(along.0, up.1)]
        .iter()
        .map(|corner| sample::project(&truth(), &eye(), corner).unwrap())
        .collect()
}

fn picture() -> Gray {
    let mut canvas = Canvas::new(sample::SIZE, 210.0);
    canvas.fill(&on_wall(true, (0.0, WIDTH), (0.0, HEIGHT)), 140.0);
    canvas.fill(&on_wall(false, (0.0, DEPTH), (0.0, HEIGHT)), 90.0);
    for floor in [(1.0, 2.4), (3.6, 5.0)] {
        for column in 0..4 {
            let left = 1.0 + 3.0 * f64::from(column);
            canvas.fill(&on_wall(true, (left, left + 1.6), floor), 40.0);
        }
        for column in 0..3 {
            let left = 1.0 + 2.6 * f64::from(column);
            canvas.fill(&on_wall(false, (left, left + 1.6), floor), 40.0);
        }
    }
    canvas.image().unwrap()
}

fn analyse(image: &Arc<Gray>, ask: Ask) -> Scene {
    let mut builder = krlyvis_std::standard();
    builder.clock(Arc::new(FakeClock::default()));
    builder.build().analyse(Arc::clone(image), ask).unwrap()
}

/// Where a pixel of the front wall lies on it, by the camera that drew the picture.
fn on_front_wall(pixel: Pixel) -> Vector3<f64> {
    let truth = truth();
    let ray = truth.rotation.transpose() * truth.ray(pixel);
    eye() + ray * (-EYE[1] / ray.y)
}

fn true_length(segment: &Segment) -> f64 {
    (on_front_wall(segment.to) - on_front_wall(segment.from)).norm()
}

fn on_front(segment: &Segment) -> bool {
    [segment.from, segment.to].iter().all(|&end| {
        let point = on_front_wall(end);
        (-0.05..=WIDTH + 0.05).contains(&point.x) && (-0.05..=HEIGHT + 0.05).contains(&point.z)
    })
}

/// The axis square to the front wall as the analysis names it: the horizontal axis it
/// found along the front wall is the other one.
fn front_normal(camera: &Camera) -> Axis {
    let along_front = truth().axis(Axis::X);
    if camera.axis(Axis::X).dot(&along_front).abs() > 0.99 {
        Axis::Y
    } else {
        Axis::X
    }
}

#[test]
fn a_drawn_building_gives_back_the_camera_and_the_lengths_on_its_front_wall() {
    let image = Arc::new(picture());
    let first = analyse(&image, Ask::default());
    let camera = first.camera.clone().unwrap_or_else(|| panic!("no camera:\n{}", report(&first, 40)));
    assert!((camera.focal - 700.0).abs() < 0.02 * 700.0, "focal {}", camera.focal);
    assert!(camera.off_square.is_some_and(|degrees| degrees < 1.0), "off square {:?}", camera.off_square);
    let normal = front_normal(&camera);
    let (known, reference) = first
        .segments
        .iter()
        .enumerate()
        .find(|(_, segment)| on_front(segment))
        .map(|(id, segment)| (id, true_length(segment)))
        .unwrap();

    let ask = Ask {
        plane: Some(normal),
        known: Some(Known { segment: known, length: reference }),
        ..Ask::default()
    };
    let scene = analyse(&image, ask);
    println!("{}", report(&scene, 40));

    let on_front: Vec<_> =
        scene.measured.iter().filter(|found| on_front(&scene.segments[found.segment])).collect();
    assert!(on_front.len() >= 20, "only {} segments of the front wall measured", on_front.len());
    for found in on_front {
        let segment = &scene.segments[found.segment];
        let expected = true_length(segment);
        assert!(
            (found.length - expected).abs() <= 0.015 * expected.max(1.0),
            "s{} measures {} instead of {expected}",
            found.segment,
            found.length
        );
    }
}

#[test]
fn the_corners_of_the_drawn_building_stand_six_high_at_their_true_distances() {
    let image = Arc::new(picture());
    let scene = analyse(&image, Ask { eye: Some(EYE[2]), ..Ask::default() });
    println!("{}", report(&scene, 12));
    let depths = scene.depth.clone().unwrap();
    // The vertical edges of the building, by where they stand: their distances from the
    // point below the camera.
    let corners = [(0.0, 0.0), (WIDTH, 0.0), (0.0, DEPTH)];

    for (x, y) in corners {
        let distance = (x - EYE[0]).hypot(y - EYE[1]);
        let found = depths.standing.iter().find(|standing| {
            (standing.distance - distance).abs() <= 0.03 * distance
                && (standing.height - HEIGHT).abs() <= 0.03 * HEIGHT
        });
        assert!(found.is_some(), "no edge {HEIGHT} high at {distance} among {:?}", depths.standing);
    }
}

#[test]
fn every_standing_segment_agrees_with_what_the_drawing_camera_gives() {
    use krlyvis_kernel::ports::depth::{depth, Depth, DepthRequest};
    let image = Arc::new(picture());
    let scene = analyse(&image, Ask { eye: Some(EYE[2]), ..Ask::default() });
    let mut builder = krlyvis_kernel::VisionBuilder::new();
    builder.plugin::<Depth>(krlyvis_std::depth::ground::Ground::default());
    let request =
        DepthRequest { camera: truth(), segments: scene.segments.clone(), eye: Some(EYE[2]), known: None };
    let truth = depth(&builder.context(), &krly3d_core::Id::new("ground"), &request).into_result().unwrap();

    let found = scene.depth.unwrap().standing;
    // Every vertical edge whose lower end lies below the horizon is taken to stand: the
    // three edges of the building, and the ground-floor window edges, which do not touch
    // the ground and come out too far and too high, as with the drawing camera itself.
    assert_eq!(found.len(), truth.standing.len(), "standing segments");
    for standing in &found {
        let Some(expected) = truth.standing.iter().find(|other| other.segment == standing.segment) else {
            continue;
        };
        assert!(
            (standing.height - expected.height).abs() <= 0.02 * expected.height.abs().max(1.0)
                && (standing.distance - expected.distance).abs() <= 0.02 * expected.distance,
            "s{}: {standing:?} instead of {expected:?}",
            standing.segment
        );
    }
}
