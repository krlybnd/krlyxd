#![allow(clippy::unwrap_used)]
use krly3d_field::{Bounds, Node, Recipe};
use krly3d_field_engine::{mesh, mesh_controlled, Progress};
use krly3d_mesh::MeshData;
use krlyxd::CallControl;
use std::collections::BTreeMap;

fn sphere(center: [f64; 3], radius: f64) -> Node {
    Node::Ellipsoid { center, radii: [radius; 3], rotation: [0.0; 3] }
}
fn recipe(nodes: Vec<Node>, cell_size: f64) -> Recipe {
    Recipe {
        version: 1,
        units: "mm".into(),
        bounds: Bounds { min: [-3.0; 3], max: [3.0; 3] },
        cell_size,
        root: nodes.len() - 1,
        nodes,
    }
}
fn volume(mesh: &MeshData) -> f64 {
    mesh.triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| mesh.points[i as usize]);
            (a[0] * (b[1] * c[2] - b[2] * c[1])
                + a[1] * (b[2] * c[0] - b[0] * c[2])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0
        })
        .sum()
}
fn closed(mesh: &MeshData) {
    let mut edges = BTreeMap::<(u32, u32), (usize, i32)>::new();
    for &[a, b, c] in &mesh.triangles {
        assert!(a != b && b != c && c != a);
        for (u, v) in [(a, b), (b, c), (c, a)] {
            let entry = edges.entry((u.min(v), u.max(v))).or_default();
            entry.0 += 1;
            entry.1 += if u < v { 1 } else { -1 };
        }
    }
    assert!(!edges.is_empty());
    assert!(edges.values().all(|&(count, direction)| count == 2 && direction == 0));
    assert!(mesh.points.iter().flatten().all(|v| v.is_finite()));
}
#[test]
fn sphere_volume_converges_with_outward_watertight_faces() {
    let coarse = mesh(&recipe(vec![sphere([0.0; 3], 1.0)], 0.25)).unwrap();
    let fine = mesh(&recipe(vec![sphere([0.0; 3], 1.0)], 0.125)).unwrap();
    closed(&coarse);
    closed(&fine);
    let exact = 4.0 * std::f64::consts::PI / 3.0;
    let error = (volume(&fine) - exact).abs();
    assert!(error < (volume(&coarse) - exact).abs() * 0.4);
    assert!(error / exact < 0.015);
}
#[test]
fn exact_grid_vertex_intersections_are_welded() {
    let m = mesh(&recipe(vec![sphere([0.0; 3], 1.0)], 0.2)).unwrap();
    closed(&m);
    let mut positions = BTreeMap::new();
    for p in &m.points {
        assert!(positions.insert(p.map(f64::to_bits), ()).is_none());
    }
}
#[test]
fn union_can_produce_two_disconnected_closed_components() {
    let m = mesh(&recipe(
        vec![sphere([-1.3, 0.0, 0.0], 0.75), sphere([1.3, 0.0, 0.0], 0.75), Node::Union { a: 0, b: 1 }],
        0.16,
    ))
    .unwrap();
    closed(&m);
    let mut parent: Vec<usize> = (0..m.points.len()).collect();
    fn root(parent: &[usize], mut i: usize) -> usize {
        while parent[i] != i {
            i = parent[i];
        }
        i
    }
    for t in &m.triangles {
        for &v in &t[1..] {
            let a = root(&parent, t[0] as usize);
            let b = root(&parent, v as usize);
            parent[b] = a;
        }
    }
    let components: std::collections::BTreeSet<_> =
        m.triangles.iter().map(|t| root(&parent, t[0] as usize)).collect();
    assert_eq!(components.len(), 2);
}
#[test]
fn subtraction_orients_inner_shell_and_intersection_matches_lens_volume() {
    let shell = mesh(&recipe(
        vec![sphere([0.0; 3], 1.5), sphere([0.0; 3], 0.8), Node::Subtract { a: 0, b: 1 }],
        0.15,
    ))
    .unwrap();
    closed(&shell);
    let exact = 4.0 * std::f64::consts::PI / 3.0 * (1.5_f64.powi(3) - 0.8_f64.powi(3));
    assert!((volume(&shell) / exact - 1.0).abs() < 0.025);
    let lens = mesh(&recipe(
        vec![sphere([-0.5, 0.0, 0.0], 1.0), sphere([0.5, 0.0, 0.0], 1.0), Node::Intersection { a: 0, b: 1 }],
        0.12,
    ))
    .unwrap();
    closed(&lens);
    assert!((volume(&lens) / (5.0 * std::f64::consts::PI / 12.0) - 1.0).abs() < 0.04);
}
#[test]
fn rotated_primitives_capsule_and_smooth_blend() {
    let ellipsoid = Node::Ellipsoid { center: [0.0; 3], radii: [1.6, 0.6, 0.8], rotation: [0.0, 90.0, 0.0] };
    let m = mesh(&recipe(vec![ellipsoid], 0.12)).unwrap();
    closed(&m);
    let extents: [f64; 3] = std::array::from_fn(|i| m.points.iter().map(|p| p[i].abs()).fold(0.0, f64::max));
    assert!((extents[0] - 0.8).abs() < 0.02 && (extents[2] - 1.6).abs() < 0.02);
    let nodes = vec![
        Node::RoundedBox {
            center: [0.0; 3],
            half_size: [0.8, 0.5, 0.4],
            radius: 0.2,
            rotation: [13.0, 35.0, 21.0],
        },
        Node::Capsule { a: [0.0; 3], b: [1.5, 0.0, 0.8], radius: 0.45 },
    ];
    let mut hard = nodes.clone();
    hard.push(Node::Union { a: 0, b: 1 });
    let mut smooth = nodes;
    smooth.push(Node::SmoothUnion { a: 0, b: 1, k: 0.6 });
    let hard = mesh(&recipe(hard, 0.13)).unwrap();
    let smooth = mesh(&recipe(smooth, 0.13)).unwrap();
    closed(&hard);
    closed(&smooth);
    assert!(volume(&smooth) > volume(&hard));
}
#[test]
fn cropped_empty_excessive_and_cyclic_requests_fail() {
    let mut r = recipe(vec![sphere([0.0; 3], 4.0)], 0.2);
    assert_eq!(mesh(&r).unwrap_err().code, "boundary");
    r.nodes[0] = sphere([15.0; 3], 1.0);
    assert_eq!(mesh(&r).unwrap_err().code, "empty");
    r.cell_size = 1e-6;
    assert_eq!(mesh(&r).unwrap_err().code, "limit");
    r.cell_size = 0.2;
    r.nodes[0] = Node::Union { a: 0, b: 0 };
    assert_eq!(mesh(&r).unwrap_err().code, "input");
    r.nodes[0] = sphere([0.0; 3], f64::NAN);
    assert_eq!(mesh(&r).unwrap_err().code, "input");
    r.nodes = vec![sphere([0.0; 3], 1.0); 129];
    assert_eq!(mesh(&r).unwrap_err().code, "input");
}
#[test]
fn cancellation_and_failed_progress_stop_without_returning_partial_mesh() {
    let r = recipe(vec![sphere([0.0; 3], 1.0)], 0.2);
    let control = CallControl::new();
    control.cancel();
    assert_eq!(mesh_controlled(&r, &control, &mut |_| Ok(())).unwrap_err().code, "stopped");
    let control = CallControl::new();
    assert_eq!(
        mesh_controlled(&r, &control, &mut |event| {
            if matches!(event, Progress::Mesh { .. }) {
                control.cancel();
            }
            Ok(())
        })
        .unwrap_err()
        .code,
        "stopped"
    );
    assert_eq!(
        mesh_controlled(&r, &CallControl::new(), &mut |_| Err(krly3d_field::Error::new("sink", "closed")))
            .unwrap_err()
            .code,
        "sink"
    );
}
#[test]
fn strict_bounded_json_and_grid_validation() {
    let r = recipe(vec![sphere([0.0; 3], 1.0)], 0.2);
    let mut value = serde_json::to_value(&r).unwrap();
    assert!(krly3d_field::decode(&serde_json::to_vec(&value).unwrap()).is_ok());
    value["nodes"][0]["unknown"] = true.into();
    assert!(krly3d_field::decode(&serde_json::to_vec(&value).unwrap()).is_err());
    assert_eq!(
        krly3d_field::decode(&vec![b' '; krly3d_field::MAX_RECIPE_BYTES + 1]).unwrap_err().code,
        "limit"
    );
    let mut r = r;
    r.bounds.max[0] = r.bounds.min[0];
    assert!(r.validate().is_err());
    r.bounds.max[0] = f64::INFINITY;
    assert!(r.validate().is_err());
}

#[test]
fn touching_rounded_boxes_and_small_smooth_union_have_no_internal_faces() {
    for radius in [0.0, 0.125] {
        let boxes: Vec<_> = [-1.0, 1.0]
            .map(|sign| Node::RoundedBox {
                center: [sign * (0.5 + radius), 0.0, 0.0],
                half_size: [0.5; 3],
                radius,
                rotation: [0.0; 3],
            })
            .into();
        for operation in [Node::Union { a: 0, b: 1 }, Node::SmoothUnion { a: 0, b: 1, k: 0.0001 }] {
            let mut nodes = boxes.clone();
            nodes.push(operation);
            let m = mesh(&recipe(nodes, 0.125)).unwrap();
            closed(&m);
            assert!(volume(&m) > 1.9);
            assert!(!m.triangles.iter().any(|t| t.iter().all(|&i| m.points[i as usize][0] == 0.0)));
        }
    }
}
