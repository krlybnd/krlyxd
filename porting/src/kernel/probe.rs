//! Probe: the abstract "eye" of the kernel. Implementations are injected into
//! the `Kernel`, so the strategy (exhaustive, box tree, later GPU) is swappable
//! and every implementation must give identical answers.

use crate::brep::{BrepEnvelope, GeometryError, PatchBounds};
use crate::math::{Interval, Point3};
use crate::perception::probe::{hit_face, unit, Hit};
use crate::query::ray_roots::ray_box_domain;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

pub trait Probe: Send {
    fn name(&self) -> &'static str;
    /// All crossings of `origin + t·dir (t ≥ 0)` with the body, sorted by t.
    fn ray(&self, brep: &BrepEnvelope, origin: Point3, dir: Point3) -> Result<Vec<Hit>, GeometryError>;
}

fn finish(brep: &BrepEnvelope, mut hits: Vec<Hit>) -> Vec<Hit> {
    hits.sort_by(|a, b| a.t.total_cmp(&b.t));
    let mut out: Vec<Hit> = Vec::with_capacity(hits.len());
    for h in hits {
        // a crossing on an edge is reported by both faces: keep it once
        if out.last().is_none_or(|l| (l.t - h.t).abs() > brep.accuracy.geometric) {
            out.push(h);
        }
    }
    out
}

fn domain_of(brep: &BrepEnvelope, origin: Point3, dir: Point3) -> Result<Option<Interval>, GeometryError> {
    Ok(ray_box_domain(brep.bounds_unchecked()?, origin, dir)?
        .map(|d| Interval { lo: d.lo.max(0.0), hi: d.hi }))
}

/// Tests every face. O(faces) per ray; the reference implementation.
pub struct Exhaustive;

impl Probe for Exhaustive {
    fn name(&self) -> &'static str {
        "exhaustive"
    }
    fn ray(&self, brep: &BrepEnvelope, origin: Point3, dir: Point3) -> Result<Vec<Hit>, GeometryError> {
        let dir = unit(dir).ok_or_else(|| GeometryError::InvalidTopology("zero ray direction".into()))?;
        let Some(domain) = domain_of(brep, origin, dir)? else { return Ok(Vec::new()) };
        let mut hits = Vec::new();
        for face in &brep.topology.faces {
            hit_face(brep, face, origin, dir, domain, &mut hits)?;
        }
        Ok(finish(brep, hits))
    }
}

/// Bounding-box tree over the faces: a ray only meets the faces whose boxes it
/// pierces. O(log faces) per ray on well-spread bodies.
#[derive(Default)]
pub struct BoxTree {
    cache: Mutex<BTreeMap<(String, u64, usize), Arc<Tree>>>,
}

struct Tree {
    nodes: Vec<Node>,
}

struct Node {
    bounds: PatchBounds,
    /// leaf: faces; inner: indices of the two children
    faces: Vec<u32>,
    children: Option<(usize, usize)>,
}

fn hull(a: PatchBounds, b: PatchBounds) -> PatchBounds {
    PatchBounds { axes: std::array::from_fn(|i| a.axes[i].hull(b.axes[i])) }
}

impl Tree {
    fn build(brep: &BrepEnvelope) -> Result<Self, GeometryError> {
        let mut items = Vec::with_capacity(brep.topology.faces.len());
        for face in &brep.topology.faces {
            items.push((face.id, super::visit::face_bounds(brep, face.id)?));
        }
        let mut tree = Tree { nodes: Vec::new() };
        if !items.is_empty() {
            tree.split(items);
        }
        Ok(tree)
    }

    fn split(&mut self, mut items: Vec<(u32, PatchBounds)>) -> usize {
        let bounds = items.iter().skip(1).fold(items[0].1, |acc, (_, b)| hull(acc, *b));
        let index = self.nodes.len();
        self.nodes.push(Node { bounds, faces: Vec::new(), children: None });
        if items.len() <= 4 {
            self.nodes[index].faces = items.iter().map(|(f, _)| *f).collect();
            return index;
        }
        // median split along the longest axis of the node
        let axis = (0..3)
            .max_by(|a, b| bounds.axes[*a].width().total_cmp(&bounds.axes[*b].width()))
            .unwrap_or(0);
        items.sort_by(|a, b| a.1.axes[axis].midpoint().total_cmp(&b.1.axes[axis].midpoint()));
        let right = items.split_off(items.len() / 2);
        let l = self.split(items);
        let r = self.split(right);
        self.nodes[index].children = Some((l, r));
        index
    }
}

impl BoxTree {
    pub fn new() -> Self {
        Self::default()
    }

    fn tree(&self, brep: &BrepEnvelope) -> Result<Arc<Tree>, GeometryError> {
        // id + revision + face count identify one state of one body
        let key = (brep.id.clone(), brep.revision, brep.topology.faces.len());
        let mut cache = self.cache.lock().map_err(|_| GeometryError::InvalidTopology("probe cache poisoned".into()))?;
        if let Some(tree) = cache.get(&key) {
            return Ok(Arc::clone(tree));
        }
        let tree = Arc::new(Tree::build(brep)?);
        if cache.len() > 256 {
            cache.clear();
        }
        cache.insert(key, Arc::clone(&tree));
        Ok(tree)
    }
}

impl Probe for BoxTree {
    fn name(&self) -> &'static str {
        "box-tree"
    }
    fn ray(&self, brep: &BrepEnvelope, origin: Point3, dir: Point3) -> Result<Vec<Hit>, GeometryError> {
        let dir = unit(dir).ok_or_else(|| GeometryError::InvalidTopology("zero ray direction".into()))?;
        let tree = self.tree(brep)?;
        let Some(root) = tree.nodes.first() else { return Ok(Vec::new()) };
        // the root box is the body's box: no need to re-enclose the body per ray
        let Some(domain) = ray_box_domain(Some(root.bounds), origin, dir)? else { return Ok(Vec::new()) };
        let domain = Interval { lo: domain.lo.max(0.0), hi: domain.hi };
        let mut hits = Vec::new();
        let mut stack = vec![0usize];
        while let Some(i) = stack.pop() {
            let node = &tree.nodes[i];
            if ray_box_domain(Some(node.bounds), origin, dir)?.is_none() {
                continue;
            }
            match node.children {
                Some((l, r)) => {
                    stack.push(l);
                    stack.push(r);
                }
                None => {
                    for f in &node.faces {
                        hit_face(brep, &brep.topology.faces[*f as usize], origin, dir, domain, &mut hits)?;
                    }
                }
            }
        }
        Ok(finish(brep, hits))
    }
}
