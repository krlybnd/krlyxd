//! Sparse perception index.
//!
//! Each face is an element with a sparse key set K (what it is, where it is),
//! and the faces form a sparse relation matrix R (adjacency, coaxiality …).
//! A query has two parts: q_self (what I look for) and q_near (what should be
//! next to it). The score is
//!
//!     s = K·q_self  +  α·R·K·q_near  +  α²·R²·K·q_near
//!
//! i.e. attention over keys, plus one or two hops of message passing on the
//! B-rep graph. Everything is deterministic and recomputed from geometry.

use super::probe::{axis, kind, num};
use crate::brep::{BrepEnvelope, GeometryError, SurfaceGeometry};
use crate::kernel::visit::{walk, BodyView, EdgeView, FaceView, Visitor};
use crate::kernel::Layer;
use crate::math::{cross, norm, sub};
use std::collections::BTreeMap;

pub struct Index {
    pub elements: Vec<String>,
    keys: BTreeMap<String, Vec<(usize, f64)>>,
    relations: Vec<Vec<(usize, f64)>>,
    pub element_keys: Vec<Vec<String>>,
}

#[derive(Clone, Debug, Default)]
pub struct Query {
    pub this: Vec<(String, f64)>,
    pub near: Vec<(String, f64)>,
}

impl Query {
    /// `cylinder r:3 near:plane near:n:+z`; a weight is given as `key^2`.
    pub fn parse(text: &str) -> Self {
        let mut q = Query::default();
        for token in text.split_whitespace() {
            let (token, weight) = match token.split_once('^') {
                Some((t, w)) => (t, w.parse().unwrap_or(1.0)),
                None => (token, 1.0),
            };
            match token.strip_prefix("near:") {
                Some(rest) => q.near.push((normalise(rest), weight)),
                None => q.this.push((normalise(token), weight)),
            }
        }
        q
    }
}

/// Bare words are kinds (`cylinder` → `kind:cylinder`).
fn normalise(token: &str) -> String {
    if token.contains(':') { token.to_string() } else { format!("kind:{token}") }
}

/// Builds an `Index` while visiting a body (visitor + builder).
pub struct IndexBuilder {
    diag: f64,
    names: Vec<String>,
    ids: Vec<u32>,
    element_keys: Vec<Vec<String>>,
    cylinders: Vec<(usize, crate::brep::Frame3)>,
    links: Vec<(u32, u32)>,
}

impl IndexBuilder {
    pub fn new(body: &BodyView) -> Self {
        let diag = body
            .bounds
            .map(|b| b.axes.iter().map(|a| (a.hi - a.lo) * (a.hi - a.lo)).sum::<f64>().sqrt())
            .unwrap_or(1.0)
            .max(1e-9);
        Self { diag, names: Vec::new(), ids: Vec::new(), element_keys: Vec::new(), cylinders: Vec::new(), links: Vec::new() }
    }

    pub fn face(&mut self, face: &FaceView) {
        let mut keys = vec![format!("name:{}", face.name), format!("kind:{}", kind(face.surface))];
        if let Some((op, _)) = face.name.split_once('(') {
            keys.push(format!("made:{op}"));
        }
        match face.surface {
            SurfaceGeometry::Plane { .. } => keys.push(format!("n:{}", axis(face.normal))),
            SurfaceGeometry::Cylinder { frame, radius } => {
                keys.push(format!("r:{}", num(*radius)));
                keys.push(format!("axis:{}", axis(frame.z).replace('-', "+")));
                self.cylinders.push((self.names.len(), *frame));
            }
            SurfaceGeometry::Sphere { radius, .. } => keys.push(format!("r:{}", num(*radius))),
            SurfaceGeometry::Cone { frame, .. } | SurfaceGeometry::Torus { frame, .. } => {
                keys.push(format!("axis:{}", axis(frame.z).replace('-', "+")));
            }
        }
        // where: grid cells at two scales (coarse = diag/4, fine = diag/16)
        for (level, size) in [(1, self.diag / 4.0), (2, self.diag / 16.0)] {
            let c: Vec<i64> = face.anchor.iter().map(|x| (x / size).floor() as i64).collect();
            keys.push(format!("cell:L{level}:{},{},{}", c[0], c[1], c[2]));
        }
        self.names.push(face.name.to_string());
        self.ids.push(face.id);
        self.element_keys.push(keys);
    }

    pub fn edge(&mut self, edge: &EdgeView) {
        if let Some(pair) = edge.face_ids {
            self.links.push(pair);
        }
    }

    pub fn finish(mut self) -> Index {
        let n = self.names.len();
        let slot: BTreeMap<u32, usize> = self.ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
        let mut relations: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
        let link = |a: usize, b: usize, rel: &mut Vec<Vec<(usize, f64)>>| {
            if a != b && !rel[a].iter().any(|(x, _)| *x == b) {
                rel[a].push((b, 1.0));
                rel[b].push((a, 1.0));
            }
        };
        for (a, b) in &self.links {
            if let (Some(a), Some(b)) = (slot.get(a), slot.get(b)) {
                link(*a, *b, &mut relations);
            }
        }
        for x in 0..self.cylinders.len() {
            for y in x + 1..self.cylinders.len() {
                let ((i, f), (j, g)) = (self.cylinders[x], self.cylinders[y]);
                if norm(cross(f.z, g.z)) <= 1e-9 && norm(cross(sub(g.origin, f.origin), f.z)) <= 1e-6 {
                    let (ni, nj) = (self.names[i].clone(), self.names[j].clone());
                    self.element_keys[i].push(format!("coaxial:{nj}"));
                    self.element_keys[j].push(format!("coaxial:{ni}"));
                    link(i, j, &mut relations);
                }
            }
        }
        let mut keys: BTreeMap<String, Vec<(usize, f64)>> = BTreeMap::new();
        for (i, list) in self.element_keys.iter().enumerate() {
            for k in list {
                keys.entry(k.clone()).or_default().push((i, 1.0));
            }
        }
        Index { elements: self.names, keys, relations, element_keys: self.element_keys }
    }
}

struct Collect(Option<IndexBuilder>);
impl Visitor for Collect {
    fn enter_body(&mut self, body: &BodyView) {
        self.0 = Some(IndexBuilder::new(body));
    }
    fn face(&mut self, _: &BodyView, face: &FaceView) {
        if let Some(b) = &mut self.0 {
            b.face(face);
        }
    }
    fn edge(&mut self, _: &BodyView, edge: &EdgeView) {
        if let Some(b) = &mut self.0 {
            b.edge(edge);
        }
    }
}

/// The perception keys as a kernel layer: one index per body.
#[derive(Default)]
pub struct KeyLayer {
    building: Option<IndexBuilder>,
    done: BTreeMap<String, Index>,
}

impl KeyLayer {
    pub fn index(&self, node: &str) -> Option<&Index> {
        self.done.get(node)
    }
}

impl Visitor for KeyLayer {
    fn enter_body(&mut self, body: &BodyView) {
        self.building = Some(IndexBuilder::new(body));
    }
    fn face(&mut self, _: &BodyView, face: &FaceView) {
        if let Some(b) = &mut self.building {
            b.face(face);
        }
    }
    fn edge(&mut self, _: &BodyView, edge: &EdgeView) {
        if let Some(b) = &mut self.building {
            b.edge(edge);
        }
    }
    fn leave_body(&mut self, body: &BodyView) {
        if let Some(b) = self.building.take() {
            self.done.insert(body.node.to_string(), b.finish());
        }
    }
}

impl Layer for KeyLayer {
    fn name(&self) -> &'static str {
        "keys"
    }
    fn forget(&mut self, node: &str) {
        self.done.remove(node);
    }
    fn visitor(&mut self) -> &mut dyn Visitor {
        self
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Index {
    pub fn build(brep: &BrepEnvelope) -> Result<Self, GeometryError> {
        let mut collect = Collect(None);
        walk("", brep, &mut [&mut collect])?;
        collect.0.map(IndexBuilder::finish).ok_or_else(|| GeometryError::InvalidTopology("empty body".into()))
    }

    /// K·q : one pass over the inverted index.
    fn match_keys(&self, q: &[(String, f64)]) -> Vec<f64> {
        let mut s = vec![0.0; self.elements.len()];
        for (key, weight) in q {
            if let Some(posting) = self.keys.get(key) {
                for (i, v) in posting {
                    s[*i] += weight * v;
                }
            }
        }
        s
    }

    /// R·x : one hop on the relation graph.
    fn hop(&self, x: &[f64]) -> Vec<f64> {
        let mut y = vec![0.0; x.len()];
        for (i, row) in self.relations.iter().enumerate() {
            for (j, w) in row {
                y[i] += w * x[*j];
            }
        }
        y
    }

    pub fn score(&self, q: &Query, alpha: f64, hops: usize) -> Vec<f64> {
        let mut s = self.match_keys(&q.this);
        if !q.near.is_empty() {
            let mut frontier = self.match_keys(&q.near);
            let mut weight = 1.0;
            for _ in 0..hops {
                frontier = self.hop(&frontier);
                weight *= alpha;
                for (si, fi) in s.iter_mut().zip(&frontier) {
                    *si += weight * fi;
                }
            }
        }
        s
    }

    /// Best matches, highest score first; zero scores are dropped.
    pub fn find(&self, q: &Query, k: usize) -> Vec<(String, f64)> {
        let s = self.score(q, 0.5, 2);
        let mut ranked: Vec<(usize, f64)> =
            s.into_iter().enumerate().filter(|(_, v)| *v > 0.0).collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        ranked
            .into_iter()
            .take(k)
            .map(|(i, v)| (self.elements[i].clone(), v))
            .collect()
    }

    pub fn neighbours(&self, element: usize) -> Vec<&str> {
        self.relations[element]
            .iter()
            .map(|(j, _)| self.elements[*j].as_str())
            .collect()
    }
}
