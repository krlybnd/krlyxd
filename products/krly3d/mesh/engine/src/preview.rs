//! Rendering metadata over the sole shared tessellator and its body/context cache.

use krly3d_core::{Attempt, Error};
use krly3d_kernel::mesh::Mesh;
use krly3d_kernel::ports::interpret::mesh_of;
use krly3d_kernel::ports::shape::Leaf;
use krly3d_kernel::topo::Body;
use krly3d_kernel::Context;
use krly3d_mesh::preview::RenderMesh;
use krly3d_mesh::{Length, LengthUnit, MeshData};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

/// Uses recorded triangle provenance and source-surface outward normals, without
/// welding corner normals across creases. Unsupported provenance explicitly declines.
/// The requested tolerance does not establish an achieved error bound.
pub fn mesh_for_body(cx: &Context, body: &Arc<Body>, tolerance: f64) -> Attempt<Arc<RenderMesh>> {
    match mesh_for_body_checked(cx, body, tolerance) {
        Ok(attempt) => attempt,
        Err(error) => Attempt::Failed(error),
    }
}

fn mesh_for_body_checked(
    cx: &Context,
    body: &Arc<Body>,
    tolerance: f64,
) -> Result<Attempt<Arc<RenderMesh>>, Error> {
    cx.check_control()?;
    if !tolerance.is_finite() || tolerance <= cx.accuracy().length() {
        return Err(Error::input(format!(
            "a mesh tolerance must be above the length tolerance, got {tolerance}"
        )));
    }
    let memo = cx.derived(body, || Attempt::Done(RenderMeshes::default())).into_result()?;
    let key = tolerance.to_bits();
    let known = memo.made.lock().unwrap_or_else(PoisonError::into_inner).get(&key).cloned();
    if let Some(known) = known {
        return cx.publish_control(|| Attempt::Done(known));
    }
    let found = match mesh_of(cx, &Leaf::of(Arc::clone(body)), tolerance) {
        Attempt::Done(mesh) => mesh,
        Attempt::Declined(reason) => {
            cx.check_control()?;
            return Ok(Attempt::Declined(reason));
        }
        Attempt::Failed(error) => return Err(error),
    };
    cx.check_control()?;
    let Some(source_faces) = &found.source_faces else {
        return cx.publish_control(|| {
            Attempt::declined("render mesh requires recorded source faces from its tessellator")
        });
    };
    let made = Arc::new(convert(cx, body, &found, source_faces, tolerance)?);
    cx.publish_control(|| {
        let mut kept = memo.made.lock().unwrap_or_else(PoisonError::into_inner);
        Attempt::Done(Arc::clone(kept.entry(key).or_insert_with(|| Arc::clone(&made))))
    })
}

/// This separate derived type shares the context/body lifetime rules of `mesh_of`.
/// Only successful, fully converted results enter the tolerance map.
#[derive(Default)]
struct RenderMeshes {
    made: Mutex<BTreeMap<u64, Arc<RenderMesh>>>,
}

fn convert(
    cx: &Context,
    body: &Body,
    mesh: &Mesh,
    faces: &[usize],
    tolerance: f64,
) -> Result<RenderMesh, Error> {
    cx.check_control()?;
    if faces.len() != mesh.triangles.len() {
        return Err(Error::geometry("render mesh source face count must match its triangles"));
    }
    let mut result = RenderMesh {
        geometry: MeshData {
            points: Vec::with_capacity(mesh.points.len()),
            triangles: Vec::with_capacity(mesh.triangles.len()),
        },
        source_faces: Vec::with_capacity(faces.len()),
        corner_normals: Vec::with_capacity(faces.len()),
        face_names: Vec::with_capacity(body.faces.len()),
        tolerance: Length { value: tolerance, unit: LengthUnit::Model },
    };
    for (index, point) in mesh.points.iter().enumerate() {
        checkpoint(cx, index)?;
        if point.coords.iter().any(|value| !value.is_finite()) {
            return Err(Error::geometry("render mesh positions must be finite"));
        }
        result.geometry.points.push([point.x, point.y, point.z]);
    }
    for (index, face) in body.faces.iter().enumerate() {
        checkpoint(cx, index)?;
        result.face_names.push(face.name.clone());
    }
    for (index, (triangle, source)) in mesh.triangles.iter().zip(faces).enumerate() {
        checkpoint(cx, index)?;
        let face = body
            .faces
            .get(*source)
            .ok_or_else(|| Error::geometry("a render mesh triangle references a missing source face"))?;
        let mut corners = [0; 3];
        let mut normals = [[0.0; 3]; 3];
        for corner in 0..3 {
            let point = mesh
                .points
                .get(triangle[corner])
                .ok_or_else(|| Error::geometry("a render mesh triangle references a missing point"))?;
            corners[corner] = u32::try_from(triangle[corner])
                .map_err(|_| Error::unsupported("a render mesh point index exceeds the u32 range"))?;
            let normal = face.outward_normal(point);
            if normal.iter().any(|value| !value.is_finite()) || normal.norm_squared() == 0.0 {
                return Err(Error::geometry("render mesh source normals must be finite and nonzero"));
            }
            normals[corner] = [normal.x, normal.y, normal.z];
        }
        result.geometry.triangles.push(corners);
        result.source_faces.push(
            u32::try_from(*source)
                .map_err(|_| Error::unsupported("a render mesh face index exceeds the u32 range"))?,
        );
        result.corner_normals.push(normals);
    }
    cx.check_control()?;
    Ok(result)
}

fn checkpoint(cx: &Context, index: usize) -> Result<(), Error> {
    if index.is_multiple_of(64) {
        cx.check_control()?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::{mesh_for_body, RenderMeshes};
    use krly3d_core::math::{Point3, Vec3};
    use krly3d_core::{Attempt, CallControl, CallStop, Clock, Error, Id};
    use krly3d_kernel::mesh::Mesh;
    use krly3d_kernel::ports::interpret::{mesh_of, MeshRequest, ToMesh};
    use krly3d_kernel::ports::shape::Leaf;
    use krly3d_kernel::schema::Params;
    use krly3d_kernel::topo::Body;
    use krly3d_kernel::{Context, KernelBuilder, Plugin};
    use krly3d_std::interpret::mesh::BrepMesh;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc, Barrier, Mutex};
    use std::time::Duration;

    fn body(tool: &str, params: Params) -> Arc<Body> {
        let cx = krly3d_std::standard().context();
        Arc::new(krly3d_kernel::ports::tool::run(&cx, &Id::new(tool), &params, &[]).into_result().unwrap())
    }

    fn block() -> Arc<Body> {
        body("box", Params::new().with("w", 6.0).with("d", 4.0).with("h", 2.0))
    }

    #[test]
    fn recorded_mapping_and_outward_corner_normals_preserve_the_shared_geometry() {
        let cx = krly3d_std::standard().context();
        for body in [
            block(),
            body("cylinder", Params::new().with("r", 3.0).with("h", 5.0)),
            body("sphere", Params::new().with("r", 3.0)),
        ] {
            let mesh = mesh_of(&cx, &Leaf::of(Arc::clone(&body)), 0.1).into_result().unwrap();
            let render = mesh_for_body(&cx, &body, 0.1).into_result().unwrap();
            render.validate().unwrap();
            assert_eq!(
                render.geometry.points,
                mesh.points.iter().map(|p| [p.x, p.y, p.z]).collect::<Vec<_>>()
            );
            assert_eq!(
                render.geometry.triangles,
                mesh.triangles.iter().map(|t| t.map(|i| i as u32)).collect::<Vec<_>>()
            );
            assert_eq!(
                render.source_faces.iter().map(|i| *i as usize).collect::<Vec<_>>(),
                *mesh.source_faces.as_ref().unwrap()
            );
            assert_eq!(render.face_names, body.faces.iter().map(|f| f.name.clone()).collect::<Vec<_>>());
            for source in 0..body.faces.len() {
                assert!(render.source_faces.contains(&(source as u32)), "missing face {source}");
            }
            for (index, triangle) in mesh.triangles.iter().enumerate() {
                let face = &body.faces[render.source_faces[index] as usize];
                let [a, b, c] = mesh.corners(index);
                let winding = (b - a).cross(&(c - a));
                assert!(
                    winding.dot(&face.outward_normal(&Point3::from((a.coords + b.coords + c.coords) / 3.0)))
                        >= 0.0
                );
                for (corner, point) in triangle.iter().enumerate() {
                    let point = mesh.points[*point];
                    let normal = Vec3::from(render.corner_normals[index][corner]);
                    assert!((normal.norm() - 1.0).abs() < 1e-12);
                    assert!((normal - face.outward_normal(&point)).norm() < 1e-12);
                    assert!(face.surface.distance(&point).abs() <= 0.1);
                    if face.surface.classes().contains(&"sphere") {
                        assert!(normal.dot(&point.coords.normalize()) > 1.0 - 1e-12);
                    }
                }
            }
        }
    }

    #[test]
    fn crease_normals_are_distinct_and_curved_normals_vary_by_corner() {
        let cx = krly3d_std::standard().context();
        let mesh = mesh_for_body(&cx, &block(), 0.1).into_result().unwrap();
        for point in 0..mesh.geometry.points.len() {
            let mut normals = Vec::new();
            for (index, triangle) in mesh.geometry.triangles.iter().enumerate() {
                for (corner, id) in triangle.iter().enumerate() {
                    if *id as usize == point && !normals.contains(&mesh.corner_normals[index][corner]) {
                        normals.push(mesh.corner_normals[index][corner]);
                    }
                }
            }
            assert_eq!(normals.len(), 3, "a box corner must retain all three face normals");
        }
        let ball = body("sphere", Params::new().with("r", 3.0));
        let mesh = mesh_for_body(&cx, &ball, 0.1).into_result().unwrap();
        assert!(mesh.corner_normals.iter().any(|n| n[0] != n[1] && n[1] != n[2]));
        let mut reversed = ball.as_ref().clone();
        for face in &mut reversed.faces {
            face.outward = !face.outward;
        }
        let mesh = mesh_for_body(&cx, &Arc::new(reversed), 0.1).into_result().unwrap();
        for (index, triangle) in mesh.geometry.triangles.iter().enumerate() {
            for (corner, point) in triangle.iter().enumerate() {
                let radial = Vec3::from(mesh.geometry.points[*point as usize]).normalize();
                assert!(Vec3::from(mesh.corner_normals[index][corner]).dot(&radial) < -1.0 + 1e-12);
            }
        }
    }

    #[test]
    fn preview_keeps_v1_result_and_wire_data_unchanged() {
        use krly3d_kernel::model::{NodeRef, Statement};
        use krly3d_kernel::ports::interpret::body_of;
        use krly3d_mesh::{MeshCall, MeshInput, RequestId};

        let mut kernel = krly3d_std::standard().build();
        kernel
            .apply(
                Statement::new(
                    "part",
                    "box",
                    Params::new().with("w", 6.0).with("d", 4.0).with("h", 2.0),
                    &[],
                )
                .unwrap(),
            )
            .unwrap();
        let engine = crate::MeshEngine::new(kernel).unwrap();
        let mut input = MeshInput::new("part");
        input.triangles = true;
        let call = MeshCall::new(RequestId::new("preview-parity"), input);
        let mut before = engine.call(call.clone()).unwrap();
        let built = engine.kernel().model().built(&NodeRef::parse("part").unwrap()).unwrap();
        let cx = engine.kernel().context();
        let body = body_of(cx, &built.shape).into_result().unwrap();
        let render = mesh_for_body(cx, &body, call.input.tolerance.value).into_result().unwrap();
        let after = engine.call(call).unwrap();
        assert_eq!(render.geometry, before.mesh.as_ref().unwrap().data);
        // v1 deliberately issues a fresh artifact identity for each execution.
        before.mesh.as_mut().unwrap().id = after.mesh.as_ref().unwrap().id.clone();
        assert_eq!(before, after);
        let wire = krly3d_mesh::codec::encode_result(&after).unwrap();
        assert_eq!(wire, krly3d_mesh::codec::encode_result(&before).unwrap());
        for field in ["source_faces", "corner_normals", "face_names"] {
            assert!(!wire.contains(field));
        }
    }

    struct Counting(Arc<AtomicUsize>);

    impl Plugin<ToMesh> for Counting {
        fn id(&self) -> Id {
            Id::new("brep")
        }

        fn call(&self, cx: &Context, request: &MeshRequest) -> Attempt<Arc<Mesh>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            BrepMesh.call(cx, request)
        }
    }

    fn counted(calls: &Arc<AtomicUsize>) -> Context {
        let mut builder = KernelBuilder::new();
        builder.plugin::<ToMesh>(Counting(Arc::clone(calls)));
        builder.context()
    }

    #[test]
    fn warm_renders_share_conversion_and_tessellation_by_body_context_and_tolerance() {
        let calls = Arc::new(AtomicUsize::new(0));
        let cx = counted(&calls);
        let body = block();
        let shared = mesh_of(&cx, &Leaf::of(Arc::clone(&body)), 0.1).into_result().unwrap();
        let first = mesh_for_body(&cx, &body, 0.1).into_result().unwrap();
        let controlled = cx.with_call_control(Arc::new(CallControl::new()));
        let again = mesh_for_body(&controlled, &body, 0.1).into_result().unwrap();
        assert!(Arc::ptr_eq(&first, &again));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(Arc::ptr_eq(
            &shared,
            &mesh_of(&cx, &Leaf::of(Arc::clone(&body)), 0.1).into_result().unwrap()
        ));
        assert!(!Arc::ptr_eq(&first, &mesh_for_body(&cx, &body, 0.2).into_result().unwrap()));
        assert!(!Arc::ptr_eq(
            &first,
            &mesh_for_body(&cx, &Arc::new(body.as_ref().clone()), 0.1).into_result().unwrap()
        ));
        assert!(!Arc::ptr_eq(&first, &mesh_for_body(&counted(&calls), &body, 0.1).into_result().unwrap()));
        assert_eq!(calls.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn cancelled_calls_reject_cold_and_warm_results_and_fresh_controls_can_retry() {
        let calls = Arc::new(AtomicUsize::new(0));
        let cx = counted(&calls);
        let body = block();
        let control = Arc::new(CallControl::new());
        control.cancel();
        let stopped = cx.with_call_control(control);
        for warm in [false, true] {
            if warm {
                mesh_for_body(&cx, &body, 0.1).into_result().unwrap();
            }
            assert_eq!(
                mesh_for_body(&stopped, &body, 0.1),
                Attempt::Failed(Error::from(CallStop::Cancelled))
            );
            assert_eq!(calls.load(Ordering::SeqCst), usize::from(warm));
        }
        let fresh = cx.with_call_control(Arc::new(CallControl::new()));
        assert!(Arc::ptr_eq(
            &mesh_for_body(&cx, &body, 0.1).into_result().unwrap(),
            &mesh_for_body(&fresh, &body, 0.1).into_result().unwrap()
        ));
        for tolerance in [0.0, -1.0, f64::NAN, f64::INFINITY, cx.accuracy().length()] {
            assert!(matches!(mesh_for_body(&cx, &body, tolerance), Attempt::Failed(_)));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    struct Supplied(Arc<Mutex<Attempt<Arc<Mesh>>>>);

    impl Plugin<ToMesh> for Supplied {
        fn id(&self) -> Id {
            Id::new("brep")
        }

        fn call(&self, _: &Context, _: &MeshRequest) -> Attempt<Arc<Mesh>> {
            self.0.lock().unwrap().clone()
        }
    }

    #[test]
    fn absent_metadata_declines_and_malformed_data_fails_without_conversion_cache_entries() {
        let body = block();
        let original = mesh_of(&counted(&Arc::new(AtomicUsize::new(0))), &Leaf::of(Arc::clone(&body)), 0.1)
            .into_result()
            .unwrap();
        let mut cases = Vec::new();
        let mut mesh = original.as_ref().clone();
        mesh.source_faces = None;
        cases.push((mesh, true));
        let mut mesh = original.as_ref().clone();
        mesh.source_faces.as_mut().unwrap().pop();
        cases.push((mesh, false));
        let mut mesh = original.as_ref().clone();
        mesh.source_faces.as_mut().unwrap()[0] = body.faces.len();
        cases.push((mesh, false));
        let mut mesh = original.as_ref().clone();
        mesh.triangles[0][0] = mesh.points.len();
        cases.push((mesh, false));
        let mut mesh = original.as_ref().clone();
        mesh.points[0].x = f64::NAN;
        cases.push((mesh, false));
        for (mesh, declined) in cases {
            let mut builder = KernelBuilder::new();
            builder.plugin::<ToMesh>(Supplied(Arc::new(Mutex::new(Attempt::Done(Arc::new(mesh))))));
            let cx = builder.context();
            let answer = mesh_for_body(&cx, &body, 0.1);
            if declined {
                assert!(
                    matches!(answer, Attempt::Declined(reason) if reason.contains("recorded source faces"))
                );
            } else {
                assert!(matches!(answer, Attempt::Failed(_)));
            }
            let memo = cx.derived(&body, || Attempt::Done(RenderMeshes::default())).into_result().unwrap();
            assert!(memo.made.lock().unwrap().is_empty());
        }
        let supplied = Arc::new(Mutex::new(Attempt::Failed(Error::geometry("transient"))));
        let mut builder = KernelBuilder::new();
        builder.plugin::<ToMesh>(Supplied(Arc::clone(&supplied)));
        let cx = builder.context();
        assert_eq!(mesh_for_body(&cx, &body, 0.1), Attempt::Failed(Error::geometry("transient")));
        *supplied.lock().unwrap() = Attempt::declined("transient decline");
        assert!(matches!(mesh_for_body(&cx, &body, 0.1), Attempt::Declined(_)));
        *supplied.lock().unwrap() = Attempt::Done(original);
        assert!(matches!(mesh_for_body(&cx, &body, 0.1), Attempt::Done(_)));
    }

    struct GateClock {
        checks: AtomicUsize,
        pause_at: usize,
        gate: Arc<Barrier>,
        entered: mpsc::SyncSender<()>,
        elapsed: AtomicUsize,
    }

    impl Clock for GateClock {
        fn now(&self) -> Duration {
            if self.checks.fetch_add(1, Ordering::SeqCst) + 1 == self.pause_at {
                self.entered.send(()).unwrap();
                self.gate.wait();
            }
            Duration::from_secs(self.elapsed.load(Ordering::SeqCst) as u64)
        }
    }

    #[test]
    fn final_metadata_decline_boundary_gives_cancellation_and_deadline_priority() {
        for (pause_at, stop) in [(5, CallStop::Cancelled), (6, CallStop::DeadlineExceeded)] {
            let body = block();
            let mut builder = KernelBuilder::new();
            builder
                .plugin::<ToMesh>(Supplied(Arc::new(Mutex::new(Attempt::Done(Arc::new(Mesh::default()))))));
            let cx = builder.context();
            mesh_of(&cx, &Leaf::of(Arc::clone(&body)), 0.1).into_result().unwrap();
            cx.derived(&body, || Attempt::Done(RenderMeshes::default())).into_result().unwrap();
            let gate = Arc::new(Barrier::new(2));
            let (entered, reached) = mpsc::sync_channel(1);
            let clock = Arc::new(GateClock {
                checks: AtomicUsize::new(0),
                pause_at,
                gate: Arc::clone(&gate),
                entered,
                elapsed: AtomicUsize::new(0),
            });
            let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(1)));
            let controlled = cx.with_call_control(Arc::clone(&control));
            let target = Arc::clone(&body);
            let running = std::thread::spawn(move || mesh_for_body(&controlled, &target, 0.1));
            reached.recv_timeout(Duration::from_secs(10)).unwrap();
            match stop {
                // Pause the preceding check after its flag read, so only the
                // final decline publication can observe this cancellation.
                CallStop::Cancelled => control.cancel(),
                CallStop::DeadlineExceeded => clock.elapsed.store(1, Ordering::SeqCst),
            }
            gate.wait();
            assert_eq!(running.join().unwrap(), Attempt::Failed(Error::from(stop)));
            let memo = cx.derived(&body, || Attempt::Done(RenderMeshes::default())).into_result().unwrap();
            assert!(memo.made.lock().unwrap().is_empty());
            assert!(matches!(mesh_for_body(&cx, &body, 0.1), Attempt::Declined(_)));
        }
    }

    #[test]
    fn mid_conversion_stops_and_publication_deadlines_do_not_cache_a_render_result() {
        for (pause_at, stop) in
            [(10, CallStop::Cancelled), (10, CallStop::DeadlineExceeded), (15, CallStop::DeadlineExceeded)]
        {
            let calls = Arc::new(AtomicUsize::new(0));
            let cx = counted(&calls);
            let body = block();
            // Prewarm only the shared tessellation, then repeat valid triangles to
            // exercise conversion checkpoints without a second tessellator.
            let mesh = mesh_of(&cx, &Leaf::of(Arc::clone(&body)), 0.1).into_result().unwrap();
            let mut mesh = mesh.as_ref().clone();
            mesh.triangles = vec![mesh.triangles[0]; 257];
            mesh.source_faces = Some(vec![mesh.source_faces.as_ref().unwrap()[0]; 257]);
            let mut builder = KernelBuilder::new();
            builder.plugin::<ToMesh>(Supplied(Arc::new(Mutex::new(Attempt::Done(Arc::new(mesh))))));
            let cx = builder.context();
            mesh_of(&cx, &Leaf::of(Arc::clone(&body)), 0.1).into_result().unwrap();
            cx.derived(&body, || Attempt::Done(RenderMeshes::default())).into_result().unwrap();
            let gate = Arc::new(Barrier::new(2));
            let (entered, reached) = mpsc::sync_channel(1);
            let clock = Arc::new(GateClock {
                checks: AtomicUsize::new(0),
                pause_at,
                gate: Arc::clone(&gate),
                entered,
                elapsed: AtomicUsize::new(0),
            });
            let control = Arc::new(CallControl::with_deadline(clock.clone(), Duration::from_secs(1)));
            let controlled = cx.with_call_control(Arc::clone(&control));
            let target = Arc::clone(&body);
            let running = std::thread::spawn(move || mesh_for_body(&controlled, &target, 0.1));
            reached.recv_timeout(Duration::from_secs(10)).unwrap();
            match stop {
                CallStop::Cancelled => control.cancel(),
                CallStop::DeadlineExceeded => clock.elapsed.store(1, Ordering::SeqCst),
            }
            gate.wait();
            assert_eq!(running.join().unwrap(), Attempt::Failed(Error::from(stop)));
            let memo = cx.derived(&body, || Attempt::Done(RenderMeshes::default())).into_result().unwrap();
            assert!(memo.made.lock().unwrap().is_empty());
            let fresh = cx.with_call_control(Arc::new(CallControl::new()));
            let render = mesh_for_body(&fresh, &body, 0.1).into_result().unwrap();
            assert!(Arc::ptr_eq(&render, &mesh_for_body(&cx, &body, 0.1).into_result().unwrap()));
        }
    }
}
