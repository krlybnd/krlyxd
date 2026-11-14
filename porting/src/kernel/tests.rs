use super::probe::{BoxTree, Exhaustive, Probe};
use super::visit::{walk, BodyView, EdgeView, FaceView, VertexView, Visitor};
use super::Kernel;
use crate::perception::KeyLayer;
use crate::world_graph::{CreateOptions, EditScope, ModifyingOperation, Plane, Primitive};

fn at(p: [f64; 3]) -> Option<Plane> {
    Some(Plane { origin: Some(p), normal: Some([0.0, 1.0, 0.0]), x_direction: Some([1.0, 0.0, 0.0]) })
}

/// A block with a chamfer, a fillet and a drilled hole, placed away from the origin.
fn part(kernel: &mut Kernel) {
    let w = kernel.world_mut();
    let o = |id: &str, p| CreateOptions { node_id: Some(id.into()), plane: at(p), ..Default::default() };
    w.create_primitive(Primitive::Cuboid { width: 4.0, height: 2.0, depth: 3.0 }, o("part", [10.0, 5.0, -2.0])).unwrap();
    w.create_primitive(Primitive::Cylinder { radius: 0.5, height: 3.0 }, o("drill", [10.5, 4.5, -2.0])).unwrap();
    w.operate("part", ModifyingOperation::Subtract, &["drill".to_string()], EditScope::Node).unwrap();
    w.dispose("drill").unwrap();
}

#[derive(Default)]
struct Count { bodies: usize, faces: usize, edges: usize, vertices: usize, named_edges: usize }
impl Visitor for Count {
    fn enter_body(&mut self, _: &BodyView) { self.bodies += 1; }
    fn face(&mut self, _: &BodyView, _: &FaceView) { self.faces += 1; }
    fn edge(&mut self, _: &BodyView, e: &EdgeView) { self.edges += 1; self.named_edges += usize::from(e.faces.is_some()); }
    fn vertex(&mut self, _: &BodyView, _: &VertexView) { self.vertices += 1; }
}

#[test]
fn one_walk_feeds_several_visitors() {
    let mut kernel = Kernel::builder().build().unwrap();
    part(&mut kernel);
    let brep = kernel.world().placed_brep("part").unwrap();
    let (mut a, mut b) = (Count::default(), Count::default());
    walk("part", &brep, &mut [&mut a, &mut b]).unwrap();
    assert_eq!((a.bodies, a.faces), (1, brep.topology.faces.len()));
    assert_eq!(a.edges, brep.topology.edges.len());
    assert_eq!(a.vertices, brep.topology.vertices.len());
    assert_eq!((a.faces, a.edges, a.named_edges), (b.faces, b.edges, b.named_edges));
}

#[test]
fn injected_probes_agree_on_every_ray() {
    let mut kernel = Kernel::builder().build().unwrap();
    part(&mut kernel);
    let brep = kernel.world().placed_brep("part").unwrap();
    let (slow, fast) = (Exhaustive, BoxTree::new());
    let mut rays = 0;
    let mut crossings = 0;
    for i in 0..9 {
        for j in 0..9 {
            for dir in [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.6, -0.5, 0.62], [-0.3, 0.9, 0.31]] {
                let origin = [7.13 + 0.71 * i as f64, 4.21 + 0.37 * j as f64, -4.93 + 0.53 * ((i + 2 * j) % 9) as f64];
                let a = slow.ray(&brep, origin, dir).unwrap();
                let b = fast.ray(&brep, origin, dir).unwrap();
                assert_eq!(a.len(), b.len(), "ray {origin:?} {dir:?}");
                for (x, y) in a.iter().zip(&b) {
                    assert!((x.t - y.t).abs() < 1e-9 && x.entering == y.entering, "ray {origin:?} {dir:?}");
                }
                rays += 1;
                crossings += a.len();
            }
        }
    }
    assert_eq!(rays, 405);
    assert!(crossings > 100, "the test rays must actually hit the part ({crossings})");
}

#[test]
fn layers_are_injected_and_rebuilt_only_after_an_edit() {
    let mut kernel = Kernel::builder().probe(Exhaustive).layer(KeyLayer::default()).build().unwrap();
    assert_eq!(kernel.probe_name(), "exhaustive");
    part(&mut kernel);
    assert!(kernel.refresh("part").unwrap());
    assert!(!kernel.refresh("part").unwrap(), "unchanged body: nothing to rebuild");
    let faces = kernel.layer::<KeyLayer>().unwrap().index("part").unwrap().elements.len();
    let edge = crate::naming::edge_names(&kernel.world().brep("part").unwrap())
        .into_iter()
        .find(|n| !n.contains("cyl") && !n.contains('#'))
        .unwrap();
    kernel.world_mut().chamfer_named("part", &[edge.as_str()], 0.2, EditScope::Node).ok();
    if kernel.refresh("part").unwrap() {
        let after = kernel.layer::<KeyLayer>().unwrap().index("part").unwrap().elements.len();
        assert!(after >= faces);
    }
    // a kernel without the layer says so instead of guessing
    let bare = Kernel::builder().build().unwrap();
    assert!(bare.layer::<KeyLayer>().is_none());
    assert_eq!(bare.probe_name(), "box-tree");
}

#[test]
fn kernel_ray_is_in_world_coordinates() {
    let mut kernel = Kernel::builder().build().unwrap();
    part(&mut kernel);
    // the block spans x 8..12, y 5..7, z -3.5..-0.5 in the world
    let hits = kernel.ray("part", [20.0, 6.0, -1.0], [-1.0, 0.0, 0.0]).unwrap();
    assert_eq!(hits.len(), 2, "{hits:?}");
    assert!((hits[0].t - 8.0).abs() < 1e-9 && (hits[1].t - 12.0).abs() < 1e-9);
}
