//! Content reuse and invalidation exercised through the active mesher.

use super::{identity, memo, read_mesh, read_mesh_config, triangulate, Abort, BrepMesh, Config};
use crate::tool::testing::{context, made};
use krly3d_core::math::{bounds::Bounds, nurbs, Iso3, Point3, Sim3, Uv, Vec3};
use krly3d_core::{Accuracy, Attempt, Exactness, FakeClock};
use krly3d_kernel::mesh::Mesh;
use krly3d_kernel::ports::curve::Curve;
use krly3d_kernel::ports::interpret::{mesh_of, MeshDeclineStats, MeshRequest};
use krly3d_kernel::ports::shape::{Leaf, Shape, Signed};
use krly3d_kernel::ports::surface::{Stretch, Surface};
use krly3d_kernel::schema::Params;
use krly3d_kernel::topo::{Body, Edge, Face};
use krly3d_kernel::{Context, Registries};
use std::any::Any;
use std::cell::Cell;
use std::sync::Arc;

fn small_budget() -> Config {
    let mut config = Config::default();
    config.quality.triangles = 1;
    config
}

fn real_mesh(
    cx: &Context,
    body: &Arc<Body>,
    tolerance: f64,
    config: Config,
    calls: &Cell<usize>,
) -> Attempt<Arc<Mesh>> {
    read_mesh_config(
        cx,
        &MeshRequest { shape: Leaf::of(Arc::clone(body)), tolerance },
        config,
        |work, flat, surface, points, tolerance| {
            calls.set(calls.get() + 1);
            triangulate::triangles(work, flat, surface, points, tolerance)
        },
    )
}

#[test]
fn many_successful_tolerances_release_old_native_meshes_after_readers_drop() {
    let cx = context();
    let profile = made(
        &cx,
        "profile",
        Params::new()
            .with("path", "M 10,20 L 20,20 L 20,28 L 10,28 Z M 12,22 L 14,22 L 14,24 L 12,24 Z")
            .with("plane", "xy"),
        &[],
    );
    let body = made(&cx, "extrude", Params::new().with("h", 3.0), &[profile]);
    let shape = Leaf::of(Arc::clone(&body));
    let mut meshes = Vec::new();
    for index in 0..100 {
        let tolerance = 0.05 + f64::from(index) * 0.001;
        let mesh = mesh_of(&cx, &shape, tolerance).into_result().unwrap();
        assert!((mesh.volume() - 228.0).abs() < 1e-8);
        assert!(mesh.open_sides().is_empty());
        assert!(Arc::ptr_eq(&mesh, &mesh_of(&cx, &shape, tolerance).into_result().unwrap()));
        meshes.push(Arc::downgrade(&mesh));
        drop(mesh);
        assert_eq!(meshes.iter().filter(|mesh| mesh.upgrade().is_some()).count(), meshes.len().min(8));
    }
    for (index, mesh) in meshes.iter().enumerate() {
        assert_eq!(mesh.upgrade().is_some(), index >= 92, "FIFO retention at tolerance index {index}");
    }
    drop(shape);
    drop(body);
    drop(cx);
    assert!(meshes.iter().all(|mesh| mesh.upgrade().is_none()));
}

#[test]
fn independently_built_spheres_share_a_real_budget_decline() {
    let cx = context();
    let first = made(&cx, "sphere", Params::new().with("r", 5.0), &[]);
    let other = made(&cx, "sphere", Params::new().with("r", 5.0), &[]);
    assert!(!Arc::ptr_eq(&first, &other));
    assert!(!Arc::ptr_eq(&first.faces[0].surface, &other.faces[0].surface));
    let (config, calls) = (small_budget(), Cell::new(0));
    let decline = real_mesh(&cx, &first, 0.22, config, &calls);
    assert_eq!(
        decline,
        Attempt::declined("face `skin`: the face needs more than 1 triangles for a tolerance of 0.22")
    );
    drop(first);
    assert_eq!(real_mesh(&cx, &other, 0.22, config, &calls), decline);
    assert_eq!(calls.get(), 1);
    assert_eq!(
        memo::memo(&cx, &other, config).unwrap().declines.0.stats(),
        MeshDeclineStats { hits: 1, misses: 1, entries: 1 }
    );
    let mut renamed = other.as_ref().clone();
    renamed.faces[0].name = "renamed".into();
    let renamed = real_mesh(&cx, &Arc::new(renamed), 0.22, config, &calls);
    assert_eq!(
        renamed,
        Attempt::declined("face `renamed`: the face needs more than 1 triangles for a tolerance of 0.22")
    );
    let changed_geometry = made(&cx, "sphere", Params::new().with("r", 6.0), &[]);
    assert!(matches!(real_mesh(&cx, &changed_geometry, 0.22, config, &calls), Attempt::Declined(_)));
    assert_eq!(calls.get(), 3);
    // The default-quality request runs successfully after the low-budget refusal.
    assert!(matches!(real_mesh(&cx, &other, 0.22, Config::default(), &calls), Attempt::Done(_)));
    assert_eq!(
        BrepMesh::decline_stats(&cx, &other),
        Attempt::Done(MeshDeclineStats { hits: 0, misses: 1, entries: 0 })
    );
}

#[test]
fn nonfinite_stored_data_opts_out_without_constructing_an_identity() {
    let cx = context();
    let body = Body { vertices: vec![Point3::new(f64::NAN, 0.0, 0.0)], ..Body::default() };
    assert_eq!(identity::key(&cx, &body, Config::default()).unwrap(), None);
}

#[test]
fn transformed_nurbs_projection_state_is_not_conflated_with_a_rebuilt_net() {
    let basis = nurbs::Basis::new(1, vec![0.0, 0.0, 1.0, 1.0], 2).unwrap();
    let weighted = |x, y, z| nurbs::weighted(&Point3::new(x, y, z), 1.0);
    let data = nurbs::Surface::new(
        basis.clone(),
        basis,
        vec![
            vec![weighted(0.1, 0.2, 0.3), weighted(0.4, 1.7, 0.1)],
            vec![weighted(2.3, 0.1, 0.4), weighted(2.8, 2.1, 1.9)],
        ],
    )
    .unwrap();
    let original = crate::surface::Nurbs::new(data).unwrap();
    let moved = original.transformed(&Sim3::new(Vec3::new(0.3, -0.7, 1.1), Vec3::new(0.0, 0.0, 0.37), 1.3));
    let moved_nurbs = moved.as_any().downcast_ref::<crate::surface::Nurbs>().unwrap();
    let rebuilt = crate::surface::Nurbs::new(moved_nurbs.data().clone()).unwrap();
    assert_eq!(moved_nurbs.data(), rebuilt.data());
    assert_ne!(moved_nurbs.projection_data(), rebuilt.projection_data());
    let body = |surface| Body {
        faces: vec![Face { name: "patch".into(), surface, outward: true, loops: vec![] }],
        ..Body::default()
    };
    let cx = context();
    let moved_key = identity::key(&cx, &body(moved), Config::default()).unwrap().unwrap();
    let rebuilt_key = identity::key(&cx, &body(Arc::new(rebuilt)), Config::default()).unwrap().unwrap();
    assert_ne!(moved_key, rebuilt_key);
}

#[test]
fn separately_built_nurbs_lofts_reuse_the_real_side_budget_refusal() {
    let cx = context();
    let loft = || {
        let section = |radius: f64, z: f64| {
            made(
                &cx,
                "profile",
                Params::new().with("path", format!("O 0,0 {radius}").as_str()).with("at", [0.0, 0.0, z]),
                &[],
            )
        };
        made(&cx, "loft", Params::new(), &[section(4.0, 0.0), section(6.0, 4.0), section(2.0, 9.0)])
    };
    let (first, other) = (loft(), loft());
    let side =
        first.faces.iter().position(|face| face.surface.as_any().is::<crate::surface::Nurbs>()).unwrap();
    assert!(!Arc::ptr_eq(&first.faces[side].surface, &other.faces[side].surface));
    assert_eq!(
        identity::key(&cx, &first, small_budget()).unwrap(),
        identity::key(&cx, &other, small_budget()).unwrap()
    );
    let calls = Cell::new(0);
    let decline = real_mesh(&cx, &first, 0.22, small_budget(), &calls);
    assert!(
        matches!(&decline, Attempt::Declined(reason) if reason.contains("face `side`") && reason.contains("more than 1 triangles") && reason.contains("0.22")),
        "{decline:?}"
    );
    let made = calls.get();
    assert!(made > 0);
    assert_eq!(real_mesh(&cx, &other, 0.22, small_budget(), &calls), decline);
    assert_eq!(calls.get(), made);
}

#[test]
fn real_mesher_invalidates_each_quality_api_solver_and_tolerance_change() {
    let cx = context();
    let body = made(&cx, "sphere", Params::new().with("r", 5.0), &[]);
    let (base, calls) = (small_budget(), Cell::new(0));
    assert!(matches!(real_mesh(&cx, &body, 0.22, base, &calls), Attempt::Declined(_)));
    let mut variants = vec![];
    let mut changed = base;
    changed.quality.triangles += 1;
    variants.push(changed);
    let mut changed = base;
    changed.quality.refinement_rounds -= 1;
    variants.push(changed);
    let mut changed = base;
    changed.quality.flip_rounds -= 1;
    variants.push(changed);
    variants.extend([
        Config { api: krly3d_mesh::ApiVersion { minor: base.api.minor + 1, ..base.api }, ..base },
        Config { solver: "krly3d.std.brep-mesh/next", ..base },
        Config { engine: "bridge/next", ..base },
        Config { artifact: "triangles/next", ..base },
    ]);
    for (index, config) in variants.into_iter().enumerate() {
        let decline = real_mesh(&cx, &body, 0.22, config, &calls);
        assert!(
            matches!(&decline, Attempt::Declined(reason) if reason.contains(&format!("more than {} triangles", config.quality.triangles)))
        );
        assert_eq!(calls.get(), index + 2);
        assert_eq!(real_mesh(&cx, &body, 0.22, config, &calls), decline);
        assert_eq!(calls.get(), index + 2);
    }
    let before = calls.get();
    assert!(matches!(
        real_mesh(&cx, &body, f64::from_bits(0.22_f64.to_bits() + 1), base, &calls),
        Attempt::Declined(_)
    ));
    assert_eq!(calls.get(), before + 1);
    // The old namespace remains valid after other settings are requested.
    assert!(matches!(real_mesh(&cx, &body, 0.22, base, &calls), Attempt::Declined(_)));
    assert_eq!(calls.get(), before + 1);
}

#[test]
fn exact_content_key_tracks_geometry_topology_diagnostics_and_effective_accuracy() {
    let cx = context();
    let body = made(&cx, "box", Params::new().with("w", 2.0).with("d", 3.0).with("h", 4.0), &[]);
    let original = identity::key(&cx, &body, Config::default()).unwrap().unwrap();
    let check = |change: fn(&mut Body)| {
        let mut changed = body.as_ref().clone();
        change(&mut changed);
        assert_ne!(identity::key(&cx, &changed, Config::default()).unwrap().unwrap(), original);
    };
    for change in [
        (|b: &mut Body| b.vertices[0].x = f64::from_bits(b.vertices[0].x.to_bits() + 1)) as fn(&mut Body),
        |b| b.solid = !b.solid,
        |b| b.faces[0].name.push('!'),
        |b| b.faces[0].outward = !b.faces[0].outward,
        |b| b.faces[0].loops[0][0].forward = !b.faces[0].loops[0][0].forward,
        |b| b.faces[0].loops[0][0].edge += 1,
        |b| b.faces[0].loops[0].reverse(),
        |b| b.edges[0].range.1 += 1.0,
        |b| b.edges[0].ends = None,
        |b| b.edges[0].label = Some("label".into()),
        |b| {
            b.edges[0].curve =
                Arc::new(crate::curve::Line::new(Point3::new(0.0, 0.0, 1.0), Vec3::x()).unwrap())
        },
        |b| b.faces[0].surface = Arc::new(crate::surface::Plane::new(Iso3::translation(1.0, 0.0, 0.0))),
    ] {
        check(change);
    }
    let base = Accuracy::default();
    let keyed = |accuracy| {
        identity::key(
            &Context::new(accuracy, Arc::new(FakeClock::default()), Registries::default()),
            &body,
            Config::default(),
        )
        .unwrap()
        .unwrap()
    };
    for accuracy in [
        Accuracy::for_scale(2.0).unwrap(),
        base.with_resolution(1e-8).unwrap(),
        base.with_angular(1e-8).unwrap(),
    ] {
        assert_ne!(keyed(base), keyed(accuracy));
    }
}

#[derive(Debug)]
struct OpaqueSphere(crate::surface::Sphere);

impl Surface for OpaqueSphere {
    fn classes(&self) -> &'static [&'static str] {
        self.0.classes()
    }
    fn point(&self, uv: Uv) -> Point3 {
        self.0.point(uv)
    }
    fn normal(&self, uv: Uv) -> Vec3 {
        self.0.normal(uv)
    }
    fn project(&self, at: &Point3) -> Uv {
        self.0.project(at)
    }
    fn line(&self, at: &Point3, direction: &Vec3, hits: &mut Vec<f64>) -> Exactness {
        self.0.line(at, direction, hits)
    }
    fn periods(&self) -> [Option<f64>; 2] {
        self.0.periods()
    }
    fn poles(&self) -> Vec<f64> {
        self.0.poles()
    }
    fn walk(&self, from: &Point3, to: &Point3) -> Vec<Stretch> {
        self.0.walk(from, to)
    }
    fn extremes(&self, direction: &Vec3) -> Vec<Point3> {
        self.0.extremes(direction)
    }
    fn summary(&self) -> String {
        self.0.summary()
    }
    fn transformed(&self, by: &Sim3) -> Arc<dyn Surface> {
        self.0.transformed(by)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug)]
struct OpaqueLine(crate::curve::Line);

impl Curve for OpaqueLine {
    fn classes(&self) -> &'static [&'static str] {
        self.0.classes()
    }
    fn point(&self, t: f64) -> Point3 {
        self.0.point(t)
    }
    fn tangent(&self, t: f64) -> Vec3 {
        self.0.tangent(t)
    }
    fn project(&self, at: &Point3) -> f64 {
        self.0.project(at)
    }
    fn period(&self) -> Option<f64> {
        self.0.period()
    }
    fn vector_area(&self, from: f64, to: f64) -> Vec3 {
        self.0.vector_area(from, to)
    }
    fn bounds(&self, from: f64, to: f64) -> Bounds {
        self.0.bounds(from, to)
    }
    fn length(&self, from: f64, to: f64) -> f64 {
        self.0.length(from, to)
    }
    fn summary(&self) -> String {
        self.0.summary()
    }
    fn transformed(&self, by: &Sim3) -> Arc<dyn Curve> {
        self.0.transformed(by)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[test]
fn opaque_geometry_and_oversized_content_retry_even_at_the_same_address() {
    let cx = context();
    let body = made(&cx, "sphere", Params::new().with("r", 5.0), &[]);
    let sphere = *body.faces[0].surface.as_any().downcast_ref::<crate::surface::Sphere>().unwrap();
    let mut opaque_surface = body.as_ref().clone();
    opaque_surface.faces[0].surface = Arc::new(OpaqueSphere(sphere));
    let mut opaque_curve = body.as_ref().clone();
    opaque_curve.edges.push(Edge {
        curve: Arc::new(OpaqueLine(crate::curve::Line::new(Point3::origin(), Vec3::x()).unwrap())),
        range: (0.0, 1.0),
        ends: None,
        label: None,
    });
    let mut oversized = body.as_ref().clone();
    oversized.faces[0].name = "x".repeat(identity::MAX_BYTES);
    for body in [opaque_surface, opaque_curve, oversized] {
        let body = Arc::new(body);
        assert_eq!(identity::key(&cx, &body, small_budget()).unwrap(), None);
        let calls = Cell::new(0);
        let first = real_mesh(&cx, &body, 0.22, small_budget(), &calls);
        assert!(matches!(first, Attempt::Declined(_)));
        assert_eq!(real_mesh(&cx, &body, 0.22, small_budget(), &calls), first);
        assert_eq!(calls.get(), 2);
        assert_eq!(
            BrepMesh::decline_stats(&cx, &body),
            Attempt::Done(MeshDeclineStats { hits: 0, misses: 2, entries: 0 })
        );
    }
}

#[test]
fn a_budget_after_consulting_trim_plugins_is_not_remembered() {
    let cx = context();
    let mut cone = made(&cx, "cone", Params::new().with("r", 4.0).with("h", 9.0), &[]).as_ref().clone();
    cone.faces.retain(|face| face.surface.as_any().is::<crate::surface::Cone>());
    assert_eq!(cone.faces.len(), 1);
    let body = Arc::new(cone);
    let request = MeshRequest { shape: Leaf::of(Arc::clone(&body)), tolerance: 0.22 };
    let calls = Cell::new(0);
    let run = || {
        read_mesh(&cx, &request, |work, _, _, _, _| {
            assert!(!work.cacheable);
            calls.set(calls.get() + 1);
            Err(Abort::Budget("budget after plugin consultation".into()))
        })
    };
    let first = run();
    assert!(matches!(first, Attempt::Declined(_)));
    assert_eq!(run(), first);
    assert_eq!(calls.get(), 2);
    assert_eq!(
        BrepMesh::decline_stats(&cx, &body),
        Attempt::Done(MeshDeclineStats { hits: 0, misses: 2, entries: 0 })
    );
}

#[derive(Debug)]
struct OpaqueShape;

impl Shape for OpaqueShape {
    fn classes(&self) -> &'static [&'static str] {
        &["brep", "shape"]
    }
    fn distance(&self, _: &Context, _: &Point3) -> Attempt<Signed> {
        Attempt::declined("opaque")
    }
    fn bounds(&self, _: &Context) -> Attempt<Bounds> {
        Attempt::declined("opaque")
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
}

#[test]
fn a_shape_class_name_does_not_opt_unknown_geometry_into_brep_caching() {
    let cx = context();
    let request = MeshRequest { shape: Arc::new(OpaqueShape), tolerance: 0.22 };
    for _ in 0..2 {
        assert_eq!(
            read_mesh(&cx, &request, |_, _, _, _, _| panic!("unknown shape reached triangulation")),
            Attempt::declined("the brep mesh rule reads B-rep leaves")
        );
    }
}

#[test]
fn bounded_content_eviction_and_context_isolation_recompute_instead_of_reusing_stale_stats() {
    let cx = context();
    let body = made(&cx, "sphere", Params::new().with("r", 5.0), &[]);
    let (config, calls) = (small_budget(), Cell::new(0));
    let decline = real_mesh(&cx, &body, 0.22, config, &calls);
    assert!(matches!(decline, Attempt::Declined(_)));
    assert_eq!(real_mesh(&cx, &body, 0.22, config, &calls), decline);
    assert_eq!(calls.get(), 1);
    // Fill the documented 64-body retention budget with different diagnostic identities.
    for index in 0..64 {
        let mut changed = body.as_ref().clone();
        changed.faces[0].name = format!("other-{index}");
        assert_eq!(memo::memo(&cx, &Arc::new(changed), config).unwrap().declines.0.stats().entries, 0);
    }
    assert_eq!(real_mesh(&cx, &body, 0.22, config, &calls), decline);
    assert_eq!(calls.get(), 2);
    assert_eq!(
        memo::memo(&cx, &body, config).unwrap().declines.0.stats(),
        MeshDeclineStats { hits: 0, misses: 1, entries: 1 }
    );
    let independent = context();
    assert_eq!(real_mesh(&independent, &body, 0.22, config, &calls), decline);
    assert_eq!(calls.get(), 3);
}
