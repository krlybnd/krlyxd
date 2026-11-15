//! Preview parity checks against the exact renderer on deliberately small fixtures.

#![allow(clippy::unwrap_used, clippy::panic)]

use krly3d_core::math::{Point3, Vec3};
use krly3d_core::{Attempt, CallControl, CallStop, Error, ErrorKind};
use krly3d_kernel::schema::Params;
use krly3d_kernel::topo::Body;
use krly3d_kernel::{Context, Plugin};
use krly3d_std::testing::made;
use krly3d_view::ports::render::{render, Frame, Image, Paint, Render, RenderRequest, View};
use krly3d_view_mesh::{render_mesh, Raster, NAME};
use std::collections::BTreeMap;
use std::sync::Arc;

fn context() -> Context {
    let mut builder = krly3d_std::standard();
    krly3d_view::register(&mut builder);
    builder.plugin::<Render>(Raster);
    builder.context()
}

fn block(cx: &Context) -> Arc<Body> {
    made(cx, "box", Params::new().with("w", 12.0).with("d", 8.0).with("h", 6.0), &[])
}

fn request(bodies: &[Arc<Body>], view: View) -> RenderRequest {
    RenderRequest { bodies: bodies.to_vec(), view, size: (224, 168), paint: Vec::new() }
}

fn iso() -> View {
    View { toward: Vec3::new(-1.0, 1.0, -1.0), up: Vec3::z(), frame: Frame::Fit }
}

fn front() -> View {
    View {
        toward: Vec3::y(),
        up: Vec3::z(),
        frame: Frame::Window { center: Point3::new(0.0, 0.0, 3.0), height: 12.0 },
    }
}

fn camera() -> View {
    View {
        toward: Vec3::new(-22.0, 28.0, -17.0),
        up: Vec3::z(),
        frame: Frame::Camera {
            eye: Point3::new(22.0, -28.0, 20.0),
            focal: 420.0,
            centre: Some((119.0, 78.0)),
        },
    }
}

fn iou(first: &Image, second: &Image) -> f64 {
    assert_eq!(first.projection, second.projection);
    let union = first.seen.iter().zip(&second.seen).filter(|(a, b)| a.is_some() || b.is_some()).count();
    let intersection =
        first.seen.iter().zip(&second.seen).filter(|(a, b)| a.is_some() && b.is_some()).count();
    assert!(union > 100, "fixture must cover a meaningful part of the image");
    intersection as f64 / union as f64
}

#[test]
fn box_and_sphere_silhouettes_have_at_least_99_percent_iou_with_exact_in_both_projections() {
    let cx = context();
    let sphere = made(&cx, "sphere", Params::new().with("r", 3.0).with("at", [0.0, 0.0, 3.0]), &[]);
    for (name, body) in [("box", block(&cx)), ("sphere", sphere)] {
        for view in [iso(), front(), camera()] {
            let request = request(&[Arc::clone(&body)], view);
            // Requested tolerance is not an achieved-error claim. These are fixture
            // comparisons at this camera, resolution and explicit tolerance only.
            let preview = render_mesh(&cx, &request, 0.004).into_result().unwrap();
            let exact = render(&cx, "raycast", &request).into_result().unwrap();
            let overlap = iou(&exact, &preview);
            eprintln!("{name}, {:?}: silhouette IoU {overlap:.6}", view.frame);
            assert!(overlap >= 0.99, "{name}, {:?}: IoU {overlap}", view.frame);
            assert!(preview.seen.iter().flatten().all(|seen| seen.body == 0 && seen.face < body.faces.len()));
        }
    }
}

#[test]
fn source_face_paint_and_box_crease_lines_match_raycast() {
    let cx = context();
    let body = block(&cx);
    let top = body.face("top").unwrap().0;
    let mut request = request(&[body], iso());
    request.paint = vec![Paint { body: Some([220, 40, 40]), faces: BTreeMap::from([(top, [40, 220, 60])]) }];
    let exact = render(&cx, "raycast", &request).into_result().unwrap();
    let preview = render_mesh(&cx, &request, 0.05).into_result().unwrap();
    let mut compared = 0;
    let mut painted_top = 0;
    let mut crease = 0;
    for row in 1..request.size.1 - 1 {
        for column in 1..request.size.0 - 1 {
            let at = row * request.size.0 + column;
            let neighbours = [at, at - 1, at + 1, at - request.size.0, at + request.size.0];
            if exact.seen[at].is_none() || neighbours.iter().any(|at| exact.seen[*at] != preview.seen[*at]) {
                continue;
            }
            let colour = &preview.pixels[at * 3..at * 3 + 3];
            assert_eq!(colour, &exact.pixels[at * 3..at * 3 + 3], "pixel {column},{row}");
            compared += 1;
            if colour == [24, 32, 48] {
                crease += 1;
            } else if preview.seen[at].unwrap().face == top {
                assert!(colour[1] > 3 * colour[0]);
                painted_top += 1;
            } else {
                assert!(colour[0] > 3 * colour[1]);
            }
        }
    }
    assert!(compared > 5000 && painted_top > 1000 && crease > 100);
}

#[test]
fn foreground_occludes_the_background_and_later_coincident_bodies_win() {
    let cx = context();
    let back = block(&cx);
    let fore = made(&cx, "sphere", Params::new().with("r", 2.0).with("at", [0.0, -8.0, 3.0]), &[]);
    let asked = request(&[Arc::clone(&back), fore], front());
    let exact = render(&cx, "raycast", &asked).into_result().unwrap();
    let preview = render_mesh(&cx, &asked, 0.004).into_result().unwrap();
    let center = (asked.size.1 / 2) * asked.size.0 + asked.size.0 / 2;
    assert_eq!(preview.seen[center].unwrap().body, 1);
    let mismatched = preview.seen.iter().zip(&exact.seen).filter(|(a, b)| a != b).count();
    assert!(mismatched < 40, "{mismatched} occlusion/source face differences");
    for bodies in
        [vec![Arc::clone(&back), Arc::clone(&back)], vec![Arc::clone(&back), Arc::new((*back).clone())]]
    {
        let image = render_mesh(&cx, &request(&bodies, iso()), 0.05).into_result().unwrap();
        assert!(image.seen.iter().flatten().all(|seen| seen.body == 1));
    }
    // A rear body's later registration must never beat a nearer surface.
    let image = render_mesh(
        &cx,
        &request(
            &[
                made(
                    &cx,
                    "box",
                    Params::new().with("w", 12.0).with("d", 8.0).with("h", 6.0).with("at", [0.0, -1.0, 0.0]),
                    &[],
                ),
                back,
            ],
            front(),
        ),
        0.05,
    )
    .into_result()
    .unwrap();
    assert!(image.seen.iter().flatten().all(|seen| seen.body == 0));
}

#[test]
fn a_camera_inside_a_box_clips_crossing_triangles_without_losing_the_visible_exit() {
    let cx = context();
    let mut asked = request(&[block(&cx)], front());
    asked.view.frame = Frame::Camera { eye: Point3::new(0.0, -3.0, 3.0), focal: 80.0, centre: None };
    let exact = render(&cx, "raycast", &asked).into_result().unwrap();
    let preview = render_mesh(&cx, &asked, 0.05).into_result().unwrap();
    assert!(preview.seen.iter().all(Option::is_some));
    assert_eq!(preview.seen, exact.seen);
    // Every vertex lies behind the same eye when the whole box is behind it.
    asked.view.frame = Frame::Camera { eye: Point3::new(0.0, 10.0, 3.0), focal: 80.0, centre: None };
    let preview = render_mesh(&cx, &asked, 0.05).into_result().unwrap();
    assert!(preview.seen.iter().all(Option::is_none));
}

#[test]
fn plugin_is_deterministic_and_uses_the_mesh_contract_default_tolerance() {
    let cx = context();
    let asked = request(&[block(&cx)], iso());
    assert_eq!(Raster.id().as_str(), NAME);
    let direct = render_mesh(&cx, &asked, krly3d_mesh::DEFAULT_TOLERANCE).into_result().unwrap();
    let registered = render(&cx, NAME, &asked).into_result().unwrap();
    assert_eq!(direct, registered);
    assert_eq!(render(&cx, NAME, &asked).into_result().unwrap(), registered);
}

#[test]
fn invalid_sizes_cameras_and_tolerances_fail_and_a_prior_cancellation_wins() {
    let cx = context();
    let mut asked = request(&[block(&cx)], iso());
    for tolerance in [0.0, -1.0, f64::NAN, f64::INFINITY, cx.accuracy().length()] {
        let error = render_mesh(&cx, &asked, tolerance).into_result().unwrap_err();
        assert_eq!(error.kind, ErrorKind::Input);
    }
    for size in [(0, 1), (1, 0), (usize::MAX, 2), (1, usize::MAX), (2049, 2)] {
        asked.size = size;
        assert_eq!(render_mesh(&cx, &asked, 0.05).into_result().unwrap_err().kind, ErrorKind::Input);
    }
    asked.size = (32, 24);
    for frame in [
        Frame::Camera { eye: Point3::new(f64::NAN, 0.0, 0.0), focal: 40.0, centre: None },
        Frame::Camera { eye: Point3::origin(), focal: 40.0, centre: Some((f64::INFINITY, 0.0)) },
        Frame::Window { center: Point3::new(0.0, f64::NAN, 0.0), height: 5.0 },
    ] {
        asked.view.frame = frame;
        assert_eq!(render_mesh(&cx, &asked, 0.05).into_result().unwrap_err().kind, ErrorKind::Input);
    }
    let control = Arc::new(CallControl::new());
    control.cancel();
    let stopped = cx.with_call_control(control);
    assert_eq!(render_mesh(&stopped, &asked, f64::NAN), Attempt::Failed(Error::from(CallStop::Cancelled)));
}

#[test]
fn an_empty_explicit_window_is_a_valid_white_image() {
    let cx = context();
    let image = render_mesh(&cx, &request(&[], front()), 0.05).into_result().unwrap();
    assert!(image.seen.iter().all(Option::is_none));
    assert!(image.pixels.iter().all(|channel| *channel == 255));
}

#[test]
fn sphere_corner_normals_give_smooth_shading_close_to_exact_surface_normals() {
    let cx = context();
    let body = made(&cx, "sphere", Params::new().with("r", 5.0), &[]);
    let mut asked = request(&[body], iso());
    asked.size = (120, 120);
    let exact = render(&cx, "raycast", &asked).into_result().unwrap();
    let preview = render_mesh(&cx, &asked, 0.01).into_result().unwrap();
    let mut samples = 0;
    let mut close = 0;
    let mut shades = std::collections::BTreeSet::new();
    for row in 1..119 {
        for column in 1..119 {
            let at = row * 120 + column;
            let neighbours = [at, at - 1, at + 1, at - 120, at + 120];
            if neighbours.iter().any(|at| exact.seen[*at].is_none() || preview.seen[*at].is_none()) {
                continue;
            }
            let colour = &preview.pixels[at * 3..at * 3 + 3];
            shades.insert(colour.to_vec());
            samples += 1;
            if colour.iter().zip(&exact.pixels[at * 3..at * 3 + 3]).all(|(a, b)| a.abs_diff(*b) <= 2) {
                close += 1;
            }
        }
    }
    assert!(samples > 2000 && shades.len() > 40);
    assert!(close as f64 / samples as f64 > 0.99, "{close} of {samples} shaded samples agree within 2/255");
}
