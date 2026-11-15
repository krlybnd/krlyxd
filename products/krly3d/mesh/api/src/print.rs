//! Conservative print preflight for indexed meshes, independent of a geometry engine.
//!
//! Bounded numerical intersection checks supplement topology checks; they are not
//! an exact geometric proof or a wall thickness test. Export eligibility is not print readiness.

use crate::{diagnostic, Diagnostic, MeshData};
use std::collections::{BTreeMap, BTreeSet};

pub const REPORT_FORMAT: &str = "krly3d.mesh.print-report/1";
pub const MAX_POINTS: usize = 1_000_000;
pub const MAX_TRIANGLES: usize = 2_000_000;

#[derive(Clone, Debug, PartialEq)]
pub struct IntersectionCheck {
    /// `passed`, `failed`, or `unknown`. `passed` is a completed numerical test.
    pub status: &'static str,
    pub tested_pairs: usize,
    pub node_visits: usize,
    pub first_pair: Option<[usize; 2]>,
    pub tolerance: f64,
    pub reason: Option<&'static str>,
}

impl IntersectionCheck {
    fn unknown(reason: &'static str) -> Self {
        Self {
            status: "unknown",
            tested_pairs: 0,
            node_visits: 0,
            first_pair: None,
            tolerance: 0.0,
            reason: Some(reason),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "status": self.status, "method": "bounded_bvh_triangle_contacts/1",
            "exact": false, "tested_pairs": self.tested_pairs, "node_visits": self.node_visits,
            "first_pair": self.first_pair, "tolerance_input_units": self.tolerance,
            "candidate_limit": intersections::MAX_PAIRS, "node_visit_limit": intersections::MAX_VISITS,
            "reason": self.reason,
            "limitations": "floating-point contact test; contacts within tolerance of a shared vertex/edge are treated as adjacency; not an exact proof or a wall-thickness/support test",
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrintReport {
    pub points: usize,
    pub triangles: usize,
    pub unused_points: usize,
    pub coincident_points: usize,
    pub degenerate_triangles: usize,
    pub boundary_edges: usize,
    pub nonmanifold_edges: usize,
    pub inconsistent_edges: usize,
    pub nonmanifold_vertices: usize,
    /// Triangle components connected through shared edges.
    pub connected_components: usize,
    /// Signed volume in input units cubed. Meaningful as enclosed volume only
    /// after closure/orientation checks; intersections are not ruled out.
    pub signed_volume: f64,
    pub topology_valid: bool,
    /// Passes the implemented checks, not an overall printability guarantee.
    pub export_eligible: bool,
    pub self_intersections: IntersectionCheck,
}

impl PrintReport {
    /// Stable, explicitly versioned JSON. Unknown checks are never reported as passing.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "format": REPORT_FORMAT,
            "points": self.points, "triangles": self.triangles,
            "unused_points": self.unused_points, "coincident_points": self.coincident_points,
            "degenerate_triangles": self.degenerate_triangles,
            "boundary_edges": self.boundary_edges, "nonmanifold_edges": self.nonmanifold_edges,
            "inconsistent_edges": self.inconsistent_edges,
            "nonmanifold_vertices": self.nonmanifold_vertices,
            "connected_components": self.connected_components,
            "signed_volume": self.signed_volume, "volume_units": "input_units_cubed",
            "topology_valid": self.topology_valid, "export_eligible": self.export_eligible,
            "self_intersections": self.self_intersections.to_json(),
            "wall_thickness": {"status": "unknown", "reason": "not implemented"},
            "print_ready": false,
            "diagnostics": self.diagnostics(),
        })
    }

    pub fn require_exportable(&self) -> Result<(), Diagnostic> {
        if self.export_eligible {
            Ok(())
        } else {
            Err(diagnostic("print_preflight", self.diagnostics().join("; ")))
        }
    }

    pub fn diagnostics(&self) -> Vec<String> {
        let mut found = Vec::new();
        for (count, label) in [
            (self.coincident_points, "coincident points with distinct indices"),
            (self.degenerate_triangles, "degenerate triangles"),
            (self.boundary_edges, "boundary edges"),
            (self.nonmanifold_edges, "nonmanifold edges"),
            (self.inconsistent_edges, "inconsistently oriented edges"),
            (self.nonmanifold_vertices, "nonmanifold vertices"),
        ] {
            if count != 0 {
                found.push(format!("{count} {label}"));
            }
        }
        if self.connected_components != 1 {
            found.push(format!("expected one connected component, found {}", self.connected_components));
        }
        if self.signed_volume <= 0.0 {
            found.push("signed volume must be positive".into());
        }
        if self.self_intersections.status != "passed" {
            found.push(format!(
                "self intersection check {}: {}",
                self.self_intersections.status,
                self.self_intersections.reason.unwrap_or("intersecting triangle contact detected")
            ));
        }
        found.push("wall thickness, slicer supports and physical print readiness are not established".into());
        found
    }
}

#[derive(Default)]
struct Edge {
    faces: Vec<usize>,
    balance: i64,
}

/// Structural input failures are errors; geometric failures are reported as counts.
pub fn report(mesh: &MeshData) -> Result<PrintReport, Diagnostic> {
    validate_input(mesh)?;
    let mut edges: BTreeMap<(u32, u32), Edge> = BTreeMap::new();
    let mut incident = vec![Vec::new(); mesh.points.len()];
    let mut degenerate_triangles = 0;
    let origin = mesh.points.first().copied().unwrap_or([0.0; 3]);
    let mut signed_volume = 0.0;
    let mut compensation = 0.0;
    for (face, triangle) in mesh.triangles.iter().enumerate() {
        let [a, b, c] = triangle.map(|i| mesh.points[i as usize]);
        let normal = cross(sub(b, a), sub(c, a));
        let term = dot(sub(a, origin), cross(sub(b, origin), sub(c, origin))) / 6.0;
        if !normal.iter().all(|v| v.is_finite()) || !term.is_finite() {
            return Err(diagnostic("numeric_range", "mesh arithmetic exceeded finite f64 range"));
        }
        if normal == [0.0; 3]
            || triangle[0] == triangle[1]
            || triangle[1] == triangle[2]
            || triangle[2] == triangle[0]
        {
            degenerate_triangles += 1;
        }
        let adjusted = term - compensation;
        let total = signed_volume + adjusted;
        compensation = (total - signed_volume) - adjusted;
        signed_volume = total;
        for corner in 0..3 {
            let from = triangle[corner];
            let to = triangle[(corner + 1) % 3];
            incident[from as usize].push(face);
            let edge = edges.entry((from.min(to), from.max(to))).or_default();
            edge.faces.push(face);
            edge.balance += if from < to { 1 } else { -1 };
        }
    }
    if !signed_volume.is_finite() {
        return Err(diagnostic("numeric_range", "nonfinite mesh volume"));
    }
    let mut adjacency = vec![Vec::new(); mesh.triangles.len()];
    let mut boundary_edges = 0;
    let mut nonmanifold_edges = 0;
    let mut inconsistent_edges = 0;
    let mut invalid_vertices = BTreeSet::new();
    for (&(a, b), edge) in &edges {
        if edge.faces.len() == 1 {
            boundary_edges += 1;
        }
        if edge.faces.len() > 2 || a == b {
            nonmanifold_edges += 1;
        }
        if edge.faces.len() == 2 && edge.balance != 0 {
            inconsistent_edges += 1;
        }
        if edge.faces.len() != 2 || a == b {
            invalid_vertices.insert(a as usize);
            invalid_vertices.insert(b as usize);
        }
        if let Some(&first) = edge.faces.first() {
            for &other in edge.faces.iter().skip(1) {
                adjacency[first].push(other);
                adjacency[other].push(first);
            }
        }
    }
    let connected_components = components(&adjacency);
    // An interior manifold vertex has one connected fan. Edge degree two above
    // establishes the cycle degrees; this test additionally rejects pinched fans.
    for (vertex, faces) in incident.iter().enumerate() {
        if !faces.is_empty()
            && !invalid_vertices.contains(&vertex)
            && !connected_fan(vertex, faces, &adjacency, mesh)
        {
            invalid_vertices.insert(vertex);
        }
    }
    let mut positions = BTreeSet::new();
    let coincident_points = mesh
        .points
        .iter()
        .filter(|p| !positions.insert(p.map(|v| if v == 0.0 { 0 } else { v.to_bits() })))
        .count();
    let topology_valid = !mesh.triangles.is_empty()
        && degenerate_triangles == 0
        && boundary_edges == 0
        && nonmanifold_edges == 0
        && inconsistent_edges == 0
        && invalid_vertices.is_empty();
    let self_intersections = if topology_valid && coincident_points == 0 {
        intersections::check(mesh, intersections::MAX_PAIRS, intersections::MAX_VISITS)
    } else {
        IntersectionCheck::unknown("topology preflight failed")
    };
    Ok(PrintReport {
        points: mesh.points.len(),
        triangles: mesh.triangles.len(),
        unused_points: incident.iter().filter(|faces| faces.is_empty()).count(),
        coincident_points,
        degenerate_triangles,
        boundary_edges,
        nonmanifold_edges,
        inconsistent_edges,
        nonmanifold_vertices: invalid_vertices.len(),
        connected_components,
        signed_volume,
        topology_valid,
        export_eligible: topology_valid
            && connected_components == 1
            && signed_volume > 0.0
            && coincident_points == 0
            && self_intersections.status == "passed",
        self_intersections,
    })
}

fn validate_input(mesh: &MeshData) -> Result<(), Diagnostic> {
    if mesh.points.len() > MAX_POINTS || mesh.triangles.len() > MAX_TRIANGLES {
        return Err(diagnostic("resource_limit", "print preflight mesh size limit exceeded"));
    }
    if mesh.points.iter().flatten().any(|v| !v.is_finite()) {
        return Err(diagnostic("input", "mesh points must be finite"));
    }
    if mesh.triangles.iter().flatten().any(|&i| i as usize >= mesh.points.len()) {
        return Err(diagnostic("input", "triangle references a missing point"));
    }
    Ok(())
}

fn connected_fan(vertex: usize, faces: &[usize], adjacency: &[Vec<usize>], mesh: &MeshData) -> bool {
    let allowed: BTreeSet<usize> = faces.iter().copied().collect();
    let mut seen = BTreeSet::new();
    let mut pending = vec![faces[0]];
    while let Some(face) = pending.pop() {
        if !seen.insert(face) {
            continue;
        }
        for &other in &adjacency[face] {
            // Adjacency must be through an edge containing this vertex, not the
            // opposite edge of two triangles that happen to include the vertex.
            if allowed.contains(&other)
                && mesh.triangles[face]
                    .iter()
                    .any(|&v| v as usize != vertex && mesh.triangles[other].contains(&v))
                && !seen.contains(&other)
            {
                pending.push(other);
            }
        }
    }
    seen.len() == allowed.len()
}

fn components(adjacency: &[Vec<usize>]) -> usize {
    let mut seen = vec![false; adjacency.len()];
    let mut count = 0;
    for start in 0..adjacency.len() {
        if seen[start] {
            continue;
        }
        count += 1;
        let mut pending = vec![start];
        while let Some(face) = pending.pop() {
            if seen[face] {
                continue;
            }
            seen[face] = true;
            pending.extend(adjacency[face].iter().copied().filter(|&other| !seen[other]));
        }
    }
    count
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// All computations are normalized to the mesh extent. A balanced bounding-box
/// hierarchy avoids quadratic work for ordinary surfaces; both pair tests and
/// traversal are capped for adversarial overlapping geometry. Shared-index
/// vertices/edges are permitted contacts, not blanket reasons to skip a pair.
mod intersections {
    use super::{cross, dot, sub, IntersectionCheck, MeshData};

    pub(super) const MAX_PAIRS: usize = 8_000_000;
    pub(super) const MAX_VISITS: usize = 24_000_000;
    const EPS: f64 = 128.0 * f64::EPSILON;
    type Point = [f64; 3];

    #[derive(Clone, Copy)]
    struct Bounds {
        low: Point,
        high: Point,
    }

    impl Bounds {
        fn point(p: Point) -> Self {
            Self { low: p, high: p }
        }
        fn union(self, other: Self) -> Self {
            Self {
                low: std::array::from_fn(|i| self.low[i].min(other.low[i])),
                high: std::array::from_fn(|i| self.high[i].max(other.high[i])),
            }
        }
        fn overlaps(self, other: Self) -> bool {
            (0..3).all(|i| self.low[i] <= other.high[i] && other.low[i] <= self.high[i])
        }
        fn longest_axis(self) -> usize {
            let mut axis = 0;
            for i in 1..3 {
                if self.high[i] - self.low[i] > self.high[axis] - self.low[axis] {
                    axis = i;
                }
            }
            axis
        }
    }

    enum Children {
        Leaf(Vec<usize>),
        Branch(Box<Tree>, Box<Tree>),
    }
    struct Tree {
        bounds: Bounds,
        count: usize,
        children: Children,
    }

    fn tree(indices: &mut [usize], bounds: &[Bounds]) -> Tree {
        let area = indices.iter().skip(1).fold(bounds[indices[0]], |b, &i| b.union(bounds[i]));
        let count = indices.len();
        let children = if count <= 8 {
            Children::Leaf(indices.to_vec())
        } else {
            let axis = area.longest_axis();
            let middle = count / 2;
            indices.select_nth_unstable_by(middle, |&a, &b| {
                (bounds[a].low[axis] + bounds[a].high[axis])
                    .total_cmp(&(bounds[b].low[axis] + bounds[b].high[axis]))
                    .then(a.cmp(&b))
            });
            let (left, right) = indices.split_at_mut(middle);
            Children::Branch(Box::new(tree(left, bounds)), Box::new(tree(right, bounds)))
        };
        Tree { bounds: area, count, children }
    }

    pub(super) fn check(mesh: &MeshData, pair_limit: usize, visit_limit: usize) -> IntersectionCheck {
        if mesh.triangles.is_empty() || mesh.points.is_empty() {
            return IntersectionCheck::unknown("empty mesh");
        }
        let bounds =
            mesh.points.iter().skip(1).fold(Bounds::point(mesh.points[0]), |b, &p| b.union(Bounds::point(p)));
        let axis = bounds.longest_axis();
        let extent = bounds.high[axis] - bounds.low[axis];
        if !extent.is_finite() || extent <= 0.0 {
            return IntersectionCheck::unknown("numeric range");
        }
        let points: Vec<_> =
            mesh.points.iter().map(|p| std::array::from_fn(|i| (p[i] - bounds.low[i]) / extent)).collect();
        let boxes: Vec<_> = mesh
            .triangles
            .iter()
            .map(|t| {
                t.iter().skip(1).fold(Bounds::point(points[t[0] as usize]), |b, &i| {
                    b.union(Bounds::point(points[i as usize]))
                })
            })
            .collect();
        let mut indices: Vec<_> = (0..mesh.triangles.len()).collect();
        let root = tree(&mut indices, &boxes);
        let mut result = IntersectionCheck {
            status: "passed",
            tested_pairs: 0,
            node_visits: 0,
            first_pair: None,
            tolerance: extent * EPS,
            reason: None,
        };
        let mut pending = vec![(&root, &root)];
        while let Some((a, b)) = pending.pop() {
            result.node_visits += 1;
            if result.node_visits > visit_limit {
                result.status = "unknown";
                result.reason = Some("BVH node visit budget exceeded");
                break;
            }
            if !a.bounds.overlaps(b.bounds) {
                continue;
            }
            match (&a.children, &b.children) {
                (Children::Leaf(left), Children::Leaf(right)) => {
                    if !leaf_pairs(
                        mesh,
                        &points,
                        &boxes,
                        left,
                        right,
                        std::ptr::eq(a, b),
                        pair_limit,
                        &mut result,
                    ) {
                        break;
                    }
                }
                (Children::Branch(left, right), _) if std::ptr::eq(a, b) => {
                    pending.extend([
                        (left.as_ref(), left.as_ref()),
                        (left.as_ref(), right.as_ref()),
                        (right.as_ref(), right.as_ref()),
                    ]);
                }
                (Children::Branch(left, right), _) if a.count >= b.count => {
                    pending.extend([(left.as_ref(), b), (right.as_ref(), b)]);
                }
                (_, Children::Branch(left, right)) => {
                    pending.extend([(a, left.as_ref()), (a, right.as_ref())]);
                }
                (Children::Branch(left, right), _) => {
                    pending.extend([(left.as_ref(), b), (right.as_ref(), b)]);
                }
            }
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn leaf_pairs(
        mesh: &MeshData,
        points: &[Point],
        boxes: &[Bounds],
        left: &[usize],
        right: &[usize],
        same: bool,
        limit: usize,
        result: &mut IntersectionCheck,
    ) -> bool {
        for &a in left {
            for &b in right {
                if (same && a >= b) || !boxes[a].overlaps(boxes[b]) {
                    continue;
                }
                if result.tested_pairs == limit {
                    result.status = "unknown";
                    result.reason = Some("triangle candidate budget exceeded");
                    return false;
                }
                result.tested_pairs += 1;
                match contact(mesh.triangles[a], mesh.triangles[b], points) {
                    Some(false) => {}
                    Some(true) => {
                        result.status = "failed";
                        result.first_pair = Some([a, b]);
                        return false;
                    }
                    None => {
                        result.status = "unknown";
                        result.reason = Some("triangle normal outside numeric range");
                        return false;
                    }
                }
            }
        }
        true
    }

    fn unit_normal(t: [Point; 3]) -> Option<Point> {
        let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
        let length = n[0].hypot(n[1]).hypot(n[2]);
        (length.is_finite() && length > 0.0).then(|| n.map(|v| v / length))
    }

    fn contact(a: [u32; 3], b: [u32; 3], points: &[Point]) -> Option<bool> {
        let ta = a.map(|i| points[i as usize]);
        let tb = b.map(|i| points[i as usize]);
        let na = unit_normal(ta)?;
        let nb = unit_normal(tb)?;
        let shared: Vec<_> = a.iter().filter(|i| b.contains(i)).map(|&i| points[i as usize]).collect();
        if shared.len() == 3 {
            return Some(true);
        }
        // Distinct planes sharing an edge can intersect only on that edge.
        let crossed = cross(na, nb);
        if shared.len() == 2 && crossed[0].hypot(crossed[1]).hypot(crossed[2]) > EPS * 8.0 {
            return Some(false);
        }
        let da = std::array::from_fn(|i| if b.contains(&a[i]) { 0.0 } else { dot(nb, sub(ta[i], tb[0])) });
        let db = std::array::from_fn(|i| if a.contains(&b[i]) { 0.0 } else { dot(na, sub(tb[i], ta[0])) });
        if separated(da) || separated(db) {
            return Some(false);
        }
        Some(edges_contact(ta, da, tb, nb, &shared) || edges_contact(tb, db, ta, na, &shared))
    }

    fn separated(distances: [f64; 3]) -> bool {
        distances.iter().all(|&d| d > EPS) || distances.iter().all(|&d| d < -EPS)
    }

    fn edges_contact(
        source: [Point; 3],
        distance: [f64; 3],
        target: [Point; 3],
        normal: Point,
        shared: &[Point],
    ) -> bool {
        for i in 0..3 {
            let j = (i + 1) % 3;
            let (a, b) = (source[i], source[j]);
            let (da, db) = (distance[i], distance[j]);
            if da.abs() <= EPS && db.abs() <= EPS {
                if let Some((lo, hi)) = clip_segment(a, b, target, normal) {
                    if !expected(lerp(a, b, lo), shared) || !expected(lerp(a, b, hi), shared) {
                        return true;
                    }
                }
            } else {
                for (p, d) in [(a, da), (b, db)] {
                    if d.abs() <= EPS && inside(p, target, normal) && !expected(p, shared) {
                        return true;
                    }
                }
                if (da > 0.0 && db < 0.0) || (da < 0.0 && db > 0.0) {
                    let p = lerp(a, b, da / (da - db));
                    if inside(p, target, normal) && !expected(p, shared) {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn expected(p: Point, shared: &[Point]) -> bool {
        let distance = |a: Point, b: Point| {
            let v = sub(a, b);
            v[0].hypot(v[1]).hypot(v[2])
        };
        match shared {
            [a] => distance(p, *a) <= EPS * 8.0,
            [a, b] => {
                let edge = sub(*b, *a);
                let fraction = (dot(sub(p, *a), edge) / dot(edge, edge)).clamp(0.0, 1.0);
                distance(p, lerp(*a, *b, fraction)) <= EPS * 8.0
            }
            _ => false,
        }
    }

    fn lerp(a: Point, b: Point, t: f64) -> Point {
        std::array::from_fn(|i| a[i] + t * (b[i] - a[i]))
    }

    fn projection(normal: Point) -> (usize, usize) {
        let mut axis = 0;
        for i in 1..3 {
            if normal[i].abs() > normal[axis].abs() {
                axis = i;
            }
        }
        ((axis + 1) % 3, (axis + 2) % 3)
    }

    fn side(a: Point, b: Point, p: Point, axes: (usize, usize)) -> f64 {
        let (u, v) = axes;
        (b[u] - a[u]) * (p[v] - a[v]) - (b[v] - a[v]) * (p[u] - a[u])
    }

    fn inside(p: Point, t: [Point; 3], normal: Point) -> bool {
        let axes = projection(normal);
        let sign = side(t[0], t[1], t[2], axes).signum();
        (0..3).all(|i| {
            let j = (i + 1) % 3;
            let length = (t[j][axes.0] - t[i][axes.0]).hypot(t[j][axes.1] - t[i][axes.1]);
            side(t[i], t[j], p, axes) * sign >= -EPS * length
        })
    }

    fn clip_segment(a: Point, b: Point, t: [Point; 3], normal: Point) -> Option<(f64, f64)> {
        let axes = projection(normal);
        let sign = side(t[0], t[1], t[2], axes).signum();
        let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
        for i in 0..3 {
            let j = (i + 1) % 3;
            let fa = side(t[i], t[j], a, axes) * sign;
            let fb = side(t[i], t[j], b, axes) * sign;
            if fa < 0.0 && fb < 0.0 {
                return None;
            }
            if fa < 0.0 {
                lo = lo.max(fa / (fa - fb));
            }
            if fb < 0.0 {
                hi = hi.min(fa / (fa - fb));
            }
            if lo > hi {
                return None;
            }
        }
        Some((lo, hi))
    }

    #[cfg(test)]
    mod tests {
        use super::{check, contact, MAX_PAIRS, MAX_VISITS};
        use crate::MeshData;

        #[test]
        fn distinguishes_crossings_coplanar_overlap_and_disjoint_planes() {
            let mut p =
                vec![[0., 0., 0.], [2., 0., 0.], [0., 2., 0.], [0.5, 0.5, -1.], [0.5, 0.5, 1.], [1., 1., 1.]];
            assert_eq!(contact([0, 1, 2], [3, 4, 5], &p), Some(true));
            p[3] = [0.2, 0.2, 0.];
            p[4] = [0.4, 0.2, 0.];
            p[5] = [0.2, 0.4, 0.];
            assert_eq!(contact([0, 1, 2], [3, 4, 5], &p), Some(true));
            for point in &mut p[3..] {
                point[2] = 0.1;
            }
            assert_eq!(contact([0, 1, 2], [3, 4, 5], &p), Some(false));
        }

        #[test]
        fn shared_indices_do_not_hide_folded_or_vertex_crossings() {
            let p = vec![
                [0., 0., 0.],
                [1., 0., 0.],
                [0., 1., 0.],
                [0., -1., 0.],
                [0.2, 0.2, 0.],
                [0.2, 0.2, -1.],
                [0.2, 0.2, 1.],
            ];
            assert_eq!(contact([0, 1, 2], [1, 0, 3], &p), Some(false));
            assert_eq!(contact([0, 1, 2], [1, 0, 4], &p), Some(true));
            assert_eq!(contact([0, 1, 2], [0, 5, 6], &p), Some(true));
            assert_eq!(contact([0, 1, 2], [0, 3, 5], &p), Some(false));
        }

        #[test]
        fn budget_is_unknown_and_bvh_finds_nonlocal_pair() {
            let mesh = MeshData {
                points: vec![
                    [0., 0., 0.],
                    [2., 0., 0.],
                    [0., 2., 0.],
                    [0.5, 0.5, -1.],
                    [0.5, 0.5, 1.],
                    [1., 1., 1.],
                ],
                triangles: vec![[0, 1, 2], [3, 4, 5]],
            };
            assert_eq!(check(&mesh, 0, MAX_VISITS).status, "unknown");
            assert_eq!(check(&mesh, MAX_PAIRS, 0).status, "unknown");
            let found = check(&mesh, MAX_PAIRS, MAX_VISITS);
            assert_eq!(found.status, "failed");
            assert!(found.first_pair.is_some());
        }

        #[test]
        fn character_tilted_coplanar_disjoint_pairs_remain_disjoint_after_f32_roundtrip() {
            // Actual character faces 54291/54296 (f64) and 51090/51093 (STL).
            // PyMeshLab selected these tilted pairs, although dominant-axis 2D
            // separation has a positive gap. Preserve the real coordinates.
            let fixtures = [
                vec![
                    [17.69254063674358, 7.945129973075213, 16.71821439720873],
                    [17.69254063674358, 8.04225352112676, 16.71821439720873],
                    [17.78947368421052, 8.04225352112676, 16.78929863201782],
                    [17.69254063674358, 7.394366197183103, 16.71821439720873],
                    [17.78947368421052, 8.016113170724088, 16.78929863201782],
                    [17.78947368421052, 7.394366197183103, 16.78929863201782],
                ],
                vec![
                    [15.203007698059082, 7.417841911315918, 14.892557144165039],
                    [15.203007698059082, 8.042253494262695, 14.892557144165039],
                    [15.290060043334961, 8.042253494262695, 14.956395149230957],
                    [15.203007698059082, 7.394366264343262, 14.892557144165039],
                    [15.290060043334961, 7.481589317321777, 14.956395149230957],
                    [15.290060043334961, 7.394366264343262, 14.956395149230957],
                ],
            ];
            for points in fixtures {
                let rounded: Vec<_> = points.iter().map(|p| p.map(|v| f64::from(v as f32))).collect();
                for points in [points, rounded] {
                    assert_eq!(contact([0, 1, 2], [3, 4, 5], &points), Some(false));
                    let mesh = MeshData { points, triangles: vec![[0, 1, 2], [3, 4, 5]] };
                    assert_eq!(check(&mesh, MAX_PAIRS, MAX_VISITS).status, "passed");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::report;
    use crate::MeshData;

    fn cube() -> MeshData {
        MeshData {
            points: vec![
                [0., 0., 0.],
                [1., 0., 0.],
                [1., 1., 0.],
                [0., 1., 0.],
                [0., 0., 1.],
                [1., 0., 1.],
                [1., 1., 1.],
                [0., 1., 1.],
            ],
            triangles: vec![
                [0, 2, 1],
                [0, 3, 2],
                [4, 5, 6],
                [4, 6, 7],
                [0, 1, 5],
                [0, 5, 4],
                [1, 2, 6],
                [1, 6, 5],
                [2, 3, 7],
                [2, 7, 6],
                [3, 0, 4],
                [3, 4, 7],
            ],
        }
    }

    #[test]
    fn cube_passes_but_print_readiness_remains_unknown() {
        let found = report(&cube()).unwrap();
        assert!(found.export_eligible);
        assert_eq!(found.signed_volume, 1.0);
        assert_eq!(found.connected_components, 1);
        assert_eq!(found.to_json()["print_ready"], false);
        assert_eq!(found.to_json()["self_intersections"]["status"], "passed");
    }

    #[test]
    fn rejects_flipped_hole_nonmanifold_and_disconnected() {
        let mut mesh = cube();
        mesh.triangles[0].swap(0, 1);
        assert_eq!(report(&mesh).unwrap().inconsistent_edges, 3);
        assert!(!report(&mesh).unwrap().export_eligible);
        let mut mesh = cube();
        mesh.triangles.pop();
        assert_eq!(report(&mesh).unwrap().boundary_edges, 3);
        let mut mesh = cube();
        mesh.triangles.push(mesh.triangles[0]);
        assert_eq!(report(&mesh).unwrap().nonmanifold_edges, 3);
        let mut mesh = cube();
        mesh.points.extend(cube().points.iter().map(|p| [p[0] + 3., p[1], p[2]]));
        mesh.triangles.extend(cube().triangles.iter().map(|t| t.map(|i| i + 8)));
        let found = report(&mesh).unwrap();
        assert_eq!(found.connected_components, 2);
        assert!(found.topology_valid);
        assert!(!found.export_eligible);
    }

    #[test]
    fn closed_positive_single_component_can_still_self_intersect() {
        let mut mesh = cube();
        mesh.points[6] = [0.5, 0.5, -0.5];
        let found = report(&mesh).unwrap();
        assert!(found.topology_valid);
        assert_eq!(found.connected_components, 1);
        assert!(found.signed_volume > 0.0);
        assert_eq!(found.self_intersections.status, "failed");
        assert!(!found.export_eligible);
        assert!(found.require_exportable().is_err());
    }

    #[test]
    fn overlapping_closed_components_have_an_intersection_witness() {
        let mut mesh = cube();
        mesh.points.extend(cube().points.iter().map(|p| [p[0] + 0.3, p[1] + 0.4, p[2] + 0.5]));
        mesh.triangles.extend(cube().triangles.iter().map(|t| t.map(|i| i + 8)));
        let found = report(&mesh).unwrap();
        assert!(found.topology_valid);
        assert_eq!(found.self_intersections.status, "failed");
        assert!(found.self_intersections.first_pair.is_some());
    }

    #[test]
    fn detects_pinched_vertex_and_invalid_input() {
        let mut mesh = cube();
        mesh.points.extend(cube().points.iter().skip(1).map(|p| [-p[0], -p[1], -p[2]]));
        mesh.triangles.extend(
            cube().triangles.iter().map(|t| [t[0], t[2], t[1]].map(|i| if i == 0 { 0 } else { i + 7 })),
        );
        let found = report(&mesh).unwrap();
        assert_eq!(found.nonmanifold_edges, 0);
        assert_eq!(found.nonmanifold_vertices, 1);
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut mesh = cube();
            mesh.points[0][0] = value;
            assert!(report(&mesh).is_err());
        }
        let mut mesh = cube();
        mesh.triangles[0][0] = u32::MAX;
        assert!(report(&mesh).is_err());
        let mut mesh = cube();
        mesh.triangles[0] = [0, 0, 1];
        assert!(report(&mesh).unwrap().degenerate_triangles > 0);
        let mut mesh = cube();
        for t in &mut mesh.triangles {
            t.swap(0, 1);
        }
        assert!(!report(&mesh).unwrap().export_eligible);
    }
}
