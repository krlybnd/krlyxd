#![allow(clippy::unwrap_used, clippy::panic)]

use super::*;
use krly3d_core::math::Point3;
use krly3d_core::{CallControl, CallStop, Clock, ErrorKind};
use krly3d_mesh::{Length, LengthUnit, MeshData};
use krly3d_view::ports::render::Pinhole;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

fn context() -> Context {
    krly3d_std::standard().context()
}

fn projection(perspective: bool) -> Projection {
    Projection {
        center: Point3::origin(),
        right: Vec3::x(),
        up: -Vec3::y(),
        toward: Vec3::z(),
        pixel: if perspective { 1.0 / 16.0 } else { 1.0 },
        size: (32, 24),
        pinhole: perspective.then_some(Pinhole { eye: Point3::origin(), focal: 16.0, centre: (16.0, 12.0) }),
    }
}

fn buffer(cx: &Context, perspective: bool) -> Buffer {
    Buffer::new(cx, projection(perspective), 32 * 24).unwrap()
}

fn screen(x: f64, y: f64, depth: f64) -> Screen {
    Screen { x: (x * SUBPIXEL as f64) as i64, y: (y * SUBPIXEL as f64) as i64, depth, normal: -Vec3::z() }
}

fn mesh(points: Vec<[f64; 3]>, triangles: Vec<[u32; 3]>) -> RenderMesh {
    RenderMesh {
        source_faces: vec![0; triangles.len()],
        corner_normals: vec![[[0.0, 0.0, -1.0]; 3]; triangles.len()],
        geometry: MeshData { points, triangles },
        face_names: vec!["surface".into()],
        tolerance: Length { value: 0.05, unit: LengthUnit::Model },
    }
}

#[test]
fn shared_edges_through_pixel_centres_have_exactly_one_owner_for_either_diagonal_and_winding() {
    let cx = context();
    let points =
        [screen(2.5, 2.5, 1.0), screen(29.5, 2.5, 1.0), screen(29.5, 21.5, 1.0), screen(2.5, 21.5, 1.0)];
    for indices in [[[0, 1, 2], [0, 2, 3]], [[0, 1, 3], [1, 2, 3]]] {
        for reversed in [false, true] {
            let mut coverage = [0; 32 * 24];
            for triangle in indices {
                let mut triangle = triangle.map(|index| points[index]);
                if reversed {
                    triangle.swap(1, 2);
                }
                let mut raster = buffer(&cx, false);
                raster.triangle(&cx, triangle, Seen { body: 0, face: 0 }).unwrap();
                for (count, seen) in coverage.iter_mut().zip(&raster.sights) {
                    *count += usize::from(seen.is_some());
                }
            }
            for (at, count) in coverage.into_iter().enumerate() {
                let inside = (2..29).contains(&(at % 32)) && (2..21).contains(&(at / 32));
                assert_eq!(count, usize::from(inside), "pixel {},{}", at % 32, at / 32);
            }
        }
    }
}

#[test]
fn clipping_huge_triangles_across_near_and_screen_planes_fills_the_viewport_without_cracks() {
    let cx = context();
    for triangles in [vec![[0, 1, 2], [0, 2, 3]], vec![[0, 1, 3], [1, 2, 3]]] {
        let points = [[-1e6, -1e6], [1e6, -1e6], [1e6, 1e6], [-1e6, 1e6]].map(|[x, y]| [x, y, 1.0 + 0.5 * x]);
        let mut raster = buffer(&cx, true);
        raster.body(&cx, 0, &mesh(points.to_vec(), triangles)).unwrap();
        for (at, sight) in raster.sights.iter().enumerate() {
            let sight = sight.unwrap_or_else(|| panic!("hole at {},{}", at % 32, at / 32));
            let across = (at % 32) as f64 + 0.5 - 16.0;
            let expected = 1.0 / (1.0 - 0.5 * across / 16.0);
            assert!((sight.depth - expected).abs() < 0.001, "{} vs {expected}", sight.depth);
        }
    }
}

#[test]
fn perspective_depth_and_normals_use_reciprocal_depth_weights_and_settle_occlusion() {
    let cx = context();
    let triangle = [
        Screen { normal: Vec3::x(), ..screen(0.0, 0.0, 1.0) },
        Screen { normal: Vec3::y(), ..screen(32.0, 0.0, 2.0) },
        Screen { normal: -Vec3::z(), ..screen(0.0, 24.0, 8.0) },
    ];
    let mut raster = buffer(&cx, true);
    raster.triangle(&cx, triangle, Seen { body: 0, face: 0 }).unwrap();
    raster.triangle(&cx, triangle.map(|v| Screen { depth: 2.0, ..v }), Seen { body: 0, face: 1 }).unwrap();
    let sight = raster.sights[8 * 32 + 8].unwrap();
    let weights = [1.0 - 8.5 / 32.0 - 8.5 / 24.0, 8.5 / 32.0, 8.5 / 24.0];
    let reciprocal = Vec3::new(weights[0], weights[1] / 2.0, -weights[2] / 8.0);
    let expected_depth = 1.0 / (reciprocal.x + reciprocal.y - reciprocal.z);
    assert_eq!(sight.seen.face, 0);
    assert!((sight.depth - expected_depth).abs() < 1e-12);
    assert!((sight.facing - reciprocal.normalize()).norm() < 1e-12);
    let (affine_depth, _) = interpolate(triangle, weights, false).unwrap();
    assert!(affine_depth > 2.0 && expected_depth < 2.0);
}

#[test]
fn borders_distinguish_creases_and_depth_steps_but_leave_smooth_source_faces_unlined() {
    let cx = context();
    let mut raster = buffer(&cx, false);
    let here = Sight { seen: Seen { body: 0, face: 0 }, depth: 2.0, facing: -Vec3::z(), size: 0.1 };
    raster.sights.fill(Some(here));
    let at = 10 * 32 + 10;
    raster.sights[at + 1] = Some(Sight { seen: Seen { body: 0, face: 1 }, ..here });
    assert!(!raster.border(at));
    raster.sights[at + 1].as_mut().unwrap().facing = Vec3::x();
    assert!(raster.border(at));
    raster.sights[at + 1].as_mut().unwrap().facing = here.facing;
    raster.sights[at + 1].as_mut().unwrap().depth = 3.0;
    assert!(raster.border(at));
}

#[test]
fn malformed_public_mesh_data_is_rejected_before_any_indexing() {
    let cx = context();
    let body = krly3d_std::testing::made(
        &cx,
        "box",
        krly3d_kernel::schema::Params::new().with("w", 2.0).with("d", 2.0).with("h", 2.0),
        &[],
    );
    let mut original = mesh(vec![[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [0.0, 1.0, 1.0]], vec![[0, 1, 2]]);
    original.face_names = body.faces.iter().map(|face| face.name.clone()).collect();
    validate_mesh(&cx, &original, &body).unwrap();
    let mut variants = vec![original.clone(); 6];
    variants[0].geometry.triangles[0][0] = u32::MAX;
    variants[1].source_faces[0] = u32::MAX;
    variants[2].corner_normals.clear();
    variants[3].geometry.points[0][0] = f64::NAN;
    variants[4].corner_normals[0][0] = [0.0; 3];
    variants[5].face_names.reverse();
    for malformed in variants {
        assert_eq!(validate_mesh(&cx, &malformed, &body).unwrap_err().kind, ErrorKind::Geometry);
    }
}

struct GateClock {
    checks: AtomicUsize,
    gate_at: usize,
    entered: mpsc::SyncSender<()>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl Clock for GateClock {
    fn now(&self) -> Duration {
        if self.checks.fetch_add(1, Ordering::SeqCst) + 1 == self.gate_at {
            self.entered.send(()).unwrap();
            self.release.lock().unwrap().recv_timeout(Duration::from_secs(10)).unwrap();
        }
        Duration::ZERO
    }
}

#[test]
fn cancellation_in_a_pixel_loop_stops_partway_through_a_triangle() {
    let base = context();
    let mut raster = buffer(&base, false);
    let (entered, reached) = mpsc::sync_channel(1);
    let (resume, release) = mpsc::sync_channel(1);
    let clock = Arc::new(GateClock {
        checks: AtomicUsize::new(0),
        gate_at: 4,
        entered,
        release: Mutex::new(release),
    });
    let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(1)));
    let cx = base.with_call_control(Arc::clone(&control));
    let worker = std::thread::spawn(move || {
        let result = raster.triangle(
            &cx,
            [screen(0.0, 0.0, 1.0), screen(32.0, 0.0, 1.0), screen(0.0, 24.0, 1.0)],
            Seen { body: 0, face: 0 },
        );
        (result, raster.sights.iter().flatten().count())
    });
    let reached = reached.recv_timeout(Duration::from_secs(10));
    control.cancel();
    let _ = resume.send(());
    assert!(reached.is_ok());
    let (result, pixels) = worker.join().unwrap();
    assert_eq!(result, Err(Error::from(CallStop::Cancelled)));
    assert!(pixels > 0 && pixels < 32 * 24 / 2, "{pixels} pixels drawn before stop");
    assert!(clock.checks.load(Ordering::SeqCst) <= 4);
}

struct ExpiringClock {
    checks: AtomicUsize,
    expire_at: AtomicUsize,
}

impl ExpiringClock {
    fn new() -> Arc<Self> {
        Arc::new(Self { checks: AtomicUsize::new(0), expire_at: AtomicUsize::new(usize::MAX) })
    }

    fn reset(&self, expire_at: usize) {
        self.checks.store(0, Ordering::SeqCst);
        self.expire_at.store(expire_at, Ordering::SeqCst);
    }

    fn controlled(self: &Arc<Self>, base: &Context) -> Context {
        base.with_call_control(Arc::new(CallControl::with_deadline(self.clone(), Duration::from_secs(1))))
    }
}

impl Clock for ExpiringClock {
    fn now(&self) -> Duration {
        if self.checks.fetch_add(1, Ordering::SeqCst) + 1 >= self.expire_at.load(Ordering::SeqCst) {
            Duration::from_secs(1)
        } else {
            Duration::ZERO
        }
    }
}

#[test]
fn deadlines_in_paint_and_at_final_publication_never_return_a_partial_image() {
    let base = context();
    let clock = ExpiringClock::new();
    let cx = clock.controlled(&base);
    let image = buffer(&base, false).paint(&cx, &[]).unwrap();
    let total = clock.checks.load(Ordering::SeqCst);
    for expire_at in [total / 2, total] {
        clock.reset(expire_at);
        assert_eq!(buffer(&base, false).paint(&cx, &[]), Err(Error::from(CallStop::DeadlineExceeded)));
    }
    clock.reset(usize::MAX);
    assert_eq!(buffer(&base, false).paint(&cx, &[]).unwrap(), image);

    // Measure the public API too: its final guard comes after image publication.
    let request = RenderRequest {
        bodies: Vec::new(),
        view: krly3d_view::ports::render::View {
            toward: Vec3::z(),
            up: -Vec3::y(),
            frame: krly3d_view::ports::render::Frame::Window { center: Point3::origin(), height: 24.0 },
        },
        size: (32, 24),
        paint: Vec::new(),
    };
    clock.reset(usize::MAX);
    let expected = render_mesh(&cx, &request, 0.05).into_result().unwrap();
    let total = clock.checks.load(Ordering::SeqCst);
    clock.reset(total);
    assert_eq!(render_mesh(&cx, &request, 0.05), Attempt::Failed(Error::from(CallStop::DeadlineExceeded)));
    clock.reset(usize::MAX);
    assert_eq!(render_mesh(&cx, &request, 0.05).into_result().unwrap(), expected);
}

#[test]
fn a_deadline_inside_rasterization_stops_a_triangle_and_retry_is_complete() {
    let base = context();
    let clock = ExpiringClock::new();
    let cx = clock.controlled(&base);
    let mut raster = buffer(&base, false);
    let triangle = [screen(0.0, 0.0, 1.0), screen(32.0, 0.0, 1.0), screen(0.0, 24.0, 1.0)];
    clock.reset(4);
    assert_eq!(
        raster.triangle(&cx, triangle, Seen { body: 0, face: 0 }),
        Err(Error::from(CallStop::DeadlineExceeded))
    );
    let partial = raster.sights.iter().flatten().count();
    assert!(partial > 0 && partial < 32 * 24 / 2);
    clock.reset(usize::MAX);
    let mut retry = buffer(&base, false);
    retry.triangle(&cx, triangle, Seen { body: 0, face: 0 }).unwrap();
    assert_eq!(retry.sights.iter().flatten().count(), 32 * 24 / 2);
}

#[test]
fn near_intersections_remain_on_the_positive_plane_despite_extreme_endpoint_depths() {
    let near = 1e-9;
    let vertices = vec![
        Vertex { point: Vec3::new(-1.0, -1.0, -1e100), normal: -Vec3::z() },
        Vertex { point: Vec3::new(1.0, -1.0, 1.0), normal: -Vec3::z() },
        Vertex { point: Vec3::new(0.0, 1.0, 1.0), normal: -Vec3::z() },
    ];
    let clipped = clip(vertices, Plane::new(Vec3::z(), -near));
    assert_eq!(clipped.len(), 4);
    assert!(clipped.iter().all(|vertex| vertex.point.z >= near));
    assert_eq!(clipped.iter().filter(|vertex| vertex.point.z == near).count(), 2);
}
