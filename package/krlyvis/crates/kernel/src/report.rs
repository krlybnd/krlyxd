//! The scene as text for a reader who acts on numbers, one fact per line.
//!
//! As in krly3d, `~` marks a number found numerically. Every value read from an image
//! is such a number, so measured lengths carry it.

use crate::image::Pixel;
use crate::ports::camera::Camera;
use crate::ports::depth::{Depths, Standing, Unit};
use crate::ports::measure::Measured;
use crate::ports::vanish::Vanishing;
use crate::scene::{Axis, Scene};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// The scene with at most `show` segments listed, longest first.
pub fn report(scene: &Scene, show: usize) -> String {
    let mut lines = vec![format!(
        "image {}x{} · {} segments · {} vanishing points",
        scene.image.width(),
        scene.image.height(),
        scene.segments.len(),
        scene.vanishing.len()
    )];
    lines.extend(scene.vanishing.iter().enumerate().map(|(index, point)| vanishing(index, point)));
    if let Some(camera) = &scene.camera {
        lines.push(camera_line(camera, scene.image.width()));
    }
    if let Some(normal) = scene.ask.plane {
        lines.push(plane_line(scene, normal));
    }
    if let Some(depths) = &scene.depth {
        lines.push(depth_line(depths));
    }
    lines.extend(segment_lines(scene, show));
    lines.extend(scene.notes.iter().map(|note| format!("note: {note}")));
    if !scene.timings.is_empty() {
        let timings: Vec<String> = scene
            .timings
            .iter()
            .map(|(stage, took)| format!("{stage} {:.1} ms", took.as_secs_f64() * 1e3))
            .collect();
        lines.push(format!("time {}", timings.join(", ")));
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

fn vanishing(index: usize, point: &Vanishing) -> String {
    let place = match point.pixel() {
        Some(at) => format!("at {}", pixel(at)),
        None => format!("far, towards {:.1}°", point.point.y.atan2(point.point.x).to_degrees()),
    };
    format!("v{index} {place} · {} segments · {:.0} px", point.members.len(), point.support)
}

fn camera_line(camera: &Camera, width: usize) -> String {
    let mut line =
        format!("camera ~ by {}: f {:.1} px, centre {}", camera.method, camera.focal, pixel(camera.centre));
    let sources: Vec<String> = Axis::ALL
        .iter()
        .map(|&axis| {
            let from =
                camera.axes[axis.index()].map_or("the other two".to_string(), |index| format!("v{index}"));
            format!("{}{} from {from}", axis.letter(), if axis == Axis::Z { " (up)" } else { "" })
        })
        .collect();
    let _ = write!(line, "; {}", sources.join(", "));
    if let Some(degrees) = camera.off_square {
        let _ = write!(line, "; y off square by {degrees:.1}°");
    }
    let horizon = camera.horizon();
    if horizon.y.abs() > f64::EPSILON * horizon.norm() {
        let row = |column: f64| -(horizon.x * column + horizon.z) / horizon.y;
        let _ = write!(line, "; horizon y {:.1} at x 0, {:.1} at x {width}", row(0.0), row(width as f64));
    }
    line
}

fn plane_line(scene: &Scene, normal: Axis) -> String {
    let [first, second] = normal.others();
    let unit = match scene.ask.known {
        Some(known) => format!("unit set by s{} = {}", known.segment, known.length),
        None => "unit: the distance from the camera to the plane".to_string(),
    };
    format!(
        "plane {}{} (square to {}) · {unit} · ends given as ({}, {})",
        first.letter(),
        second.letter(),
        normal.letter(),
        first.letter(),
        second.letter()
    )
}

fn depth_line(depths: &Depths) -> String {
    let unit = match depths.unit {
        Unit::Eye(height) => format!("unit set by the camera {height} above the ground"),
        Unit::Known(known) => format!("unit set by s{} standing {} high", known.segment, known.length),
        Unit::CameraHeight => "unit: the camera's height above the ground".to_string(),
    };
    format!(
        "depth · {} vertical segments taken to stand on the ground with their lower end · {unit} · \
         given as h height, d distance of the foot",
        depths.standing.len()
    )
}

fn segment_lines(scene: &Scene, show: usize) -> Vec<String> {
    let towards = scene.vanishing_of();
    let measured: BTreeMap<usize, &Measured> =
        scene.measured.iter().map(|found| (found.segment, found)).collect();
    let standing: BTreeMap<usize, &Standing> = scene
        .depth
        .iter()
        .flat_map(|depths| depths.standing.iter().map(|found| (found.segment, found)))
        .collect();
    let mut lines: Vec<String> = scene
        .segments
        .iter()
        .enumerate()
        .take(show)
        .map(|(id, segment)| {
            let mut line =
                format!("s{id} {}-{} {:.1} px", pixel(segment.from), pixel(segment.to), segment.length());
            if let Some(Some(point)) = towards.get(id) {
                let _ = write!(line, " v{point}");
            }
            if let Some(found) = measured.get(&id) {
                let _ = write!(
                    line,
                    " ~{:.3} ({:.3}, {:.3})-({:.3}, {:.3})",
                    found.length, found.from.x, found.from.y, found.to.x, found.to.y
                );
            }
            if let Some(found) = standing.get(&id) {
                let _ = write!(line, " h ~{:.3} d ~{:.3}", found.height, found.distance);
            }
            line
        })
        .collect();
    let hidden = scene.segments.len().saturating_sub(show);
    if hidden > 0 {
        lines.push(format!("… {hidden} shorter segments not shown"));
    }
    lines
}

fn pixel(at: Pixel) -> String {
    format!("({:.1}, {:.1})", at.x, at.y)
}

#[cfg(test)]
mod tests {
    use super::report;
    use crate::image::{Gray, Pixel};
    use crate::ports::detect::Segment;
    use crate::ports::measure::Measured;
    use crate::ports::vanish::Vanishing;
    use crate::scene::{Ask, Axis, Known, Scene};
    use nalgebra::{Vector2, Vector3};
    use std::sync::Arc;

    fn scene() -> Scene {
        let image = Arc::new(Gray::new(4, 3, vec![0.0; 12]).unwrap());
        let ask =
            Ask { plane: Some(Axis::Y), known: Some(Known { segment: 0, length: 4.0 }), ..Ask::default() };
        let mut scene = Scene::new(image, ask);
        let segment = |x: f64| Segment {
            from: Pixel::new(x, 0.0),
            to: Pixel::new(x, 2.0),
            width: 1.0,
            significance: 5.0,
        };
        scene.segments = vec![segment(1.0), segment(2.0)];
        scene.vanishing =
            vec![Vanishing { point: Vector3::new(0.0, 1.0, 0.0), members: vec![0, 1], support: 4.0 }];
        scene.measured = vec![Measured {
            segment: 0,
            length: 4.0,
            from: Vector2::new(0.0, 1.0),
            to: Vector2::new(4.0, 1.0),
        }];
        scene.notes.push("camera: fewer than two vanishing points".to_string());
        scene
    }

    #[test]
    fn the_report_lists_points_segments_measures_and_notes() {
        let text = report(&scene(), 1);

        assert_eq!(
            text,
            "image 4x3 · 2 segments · 1 vanishing points\n\
             v0 far, towards 90.0° · 2 segments · 4 px\n\
             plane zx (square to y) · unit set by s0 = 4 · ends given as (z, x)\n\
             s0 (1.0, 0.0)-(1.0, 2.0) 2.0 px v0 ~4.000 (0.000, 1.000)-(4.000, 1.000)\n\
             … 1 shorter segments not shown\n\
             note: camera: fewer than two vanishing points\n"
        );
    }
}
