//! A NURBS surface: the side of a loft through sections.
//!
//! Points, derivatives and normals are exact evaluations of the rational form, and so are
//! its iso-curves, which serve as the walks. Everything that would need the roots of a
//! system of polynomials is found numerically and marked so: the foot point and the hits of
//! a line (Newton's method from the nearest points of a grid sampled once, on every knot
//! span), and the extremes (Newton on the gradient of the height). `u` may close on itself;
//! then the surface repeats with the length of its domain in `u`.

use crate::curve::Nurbs as NurbsCurve;
use krly3d_core::math::bounds::Bounds;
use krly3d_core::math::mirror::Mirror;
use krly3d_core::math::nurbs;
use krly3d_core::math::{Point3, Sim3, Uv, Vec3};
use krly3d_core::{Error, Exactness};
use krly3d_kernel::ports::surface::{Stretch, Surface};
use nalgebra::Matrix3;
use std::any::Any;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

/// The range of `v` a face covers, when it is a band between two iso-curves.
type Band = Option<(f64, f64)>;

/// Grid samples per knot span in each direction, from which the numerical answers start.
const SAMPLES: usize = 6;
/// Newton steps; from a nearby sample a handful suffice.
const NEWTON: usize = 50;
/// Nearest grid points a projection starts from, and halvings of a step that would not
/// bring the foot nearer.
const SEEDS: usize = 8;
const HALVINGS: usize = 30;
/// A foot this near the point, as a share of the grid spacing, is the point itself.
const ON_SURFACE: f64 = 1e-13;

#[derive(Clone, Debug)]
pub struct Nurbs {
    surface: nurbs::Surface,
    closed_u: bool,
    /// The sampled grid: parameters and points, and the largest distance between neighbours.
    grid: Vec<(Uv, Point3)>,
    spacing: f64,
    /// For each patch between neighbouring knots: a box holding it, and its grid points.
    patches: Vec<(Bounds, Vec<usize>)>,
    /// The bands found for faces on this surface, by a fingerprint of their boundary: a
    /// trim rule asks for every point it places, and the answer only depends on the face.
    bands: Arc<Mutex<Vec<(u64, Band)>>>,
}

impl PartialEq for Nurbs {
    fn eq(&self, other: &Self) -> bool {
        self.surface == other.surface
    }
}

/// The distinct knots of a basis, at which the surface may bend sharply.
fn breaks(basis: &nurbs::Basis) -> Vec<f64> {
    let (start, end) = basis.domain();
    let mut breaks: Vec<f64> =
        basis.knots.iter().copied().filter(|knot| *knot >= start && *knot <= end).collect();
    breaks.dedup();
    breaks
}

fn sampled(breaks: &[f64]) -> Vec<f64> {
    let mut values = Vec::new();
    for span in breaks.windows(2) {
        for sample in 0..SAMPLES {
            values.push(span[0] + (span[1] - span[0]) * sample as f64 / SAMPLES as f64);
        }
    }
    if let Some(last) = breaks.last() {
        values.push(*last);
    }
    values
}

/// The patches between neighbouring knots, each with a box around its sampled points grown
/// by the spacing of the grid (a patch bulges between its samples by less than that).
fn patches(surface: &nurbs::Surface, grid: &[(Uv, Point3)], spacing: f64) -> Vec<(Bounds, Vec<usize>)> {
    let (ub, vb) = (breaks(&surface.u), breaks(&surface.v));
    let mut patches = Vec::new();
    for us in ub.windows(2) {
        for vs in vb.windows(2) {
            let members: Vec<usize> = (0..grid.len())
                .filter(|index| {
                    let uv = grid[*index].0;
                    uv.x >= us[0] && uv.x <= us[1] && uv.y >= vs[0] && uv.y <= vs[1]
                })
                .collect();
            let bounds = Bounds::of(members.iter().map(|index| grid[*index].1)).grown(spacing);
            patches.push((bounds, members));
        }
    }
    patches
}

/// The Newton step towards where the gradient vanishes, taken only along the directions in
/// which the height curves: along a straight run of the surface (the side of a loft between
/// rounded sections) the height does not change at all, the Hessian is singular there, and a
/// plain solve would either stop or, on round-off, leap away from the extreme.
fn newton_step(
    hessian: &nalgebra::Matrix2<f64>,
    gradient: &nalgebra::Vector2<f64>,
) -> Option<nalgebra::Vector2<f64>> {
    let eigen = hessian.symmetric_eigen();
    let largest = eigen.eigenvalues.amax();
    let mut step = nalgebra::Vector2::zeros();
    for (value, vector) in eigen.eigenvalues.iter().zip(eigen.eigenvectors.column_iter()) {
        if value.abs() > CURVING * largest {
            step -= vector * (vector.dot(gradient) / value);
        }
    }
    (largest > 0.0 && step.iter().all(|part| part.is_finite())).then_some(step)
}

/// Below this share of the strongest curvature a direction counts as straight.
const CURVING: f64 = 1e-9;

impl Nurbs {
    /// The foot of `point` reached from `start` by Gauss–Newton on `S(u, v) − p`: each step
    /// solves `JᵀJ δ = −Jᵀ r`, which never points uphill, and is halved until it brings the
    /// foot nearer.
    fn foot(&self, start: Uv, point: &Point3) -> Uv {
        let distance = |uv: Uv| (self.point(uv) - point).norm();
        let mut uv = start;
        let mut now = distance(uv);
        for _ in 0..NEWTON {
            let [s, su, sv, ..] = self.local(uv);
            let r = s - point.coords;
            let normal = nalgebra::Matrix2::new(su.dot(&su), su.dot(&sv), su.dot(&sv), sv.dot(&sv));
            let Some(mut step) = normal.lu().solve(&-nalgebra::Vector2::new(su.dot(&r), sv.dot(&r))) else {
                break;
            };
            let mut moved = false;
            for _ in 0..HALVINGS {
                let next = self.inside(Uv::new(uv.x + step.x, uv.y + step.y));
                let then = distance(next);
                if then < now {
                    (uv, now, moved) = (next, then, true);
                    break;
                }
                step *= 0.5;
            }
            if !moved || now <= self.spacing * ON_SURFACE {
                break;
            }
        }
        uv
    }

    pub fn new(surface: nurbs::Surface) -> Result<Self, Error> {
        let (start, end) = surface.u.domain();
        let (low, high) = surface.v.domain();
        let closed_u = [low, 0.5 * (low + high), high].iter().all(|v| {
            (surface.point(start, *v) - surface.point(end, *v)).norm()
                <= f64::EPSILON * 1e3 * (1.0 + surface.point(start, *v).coords.norm())
        });
        let (us, vs) = (sampled(&breaks(&surface.u)), sampled(&breaks(&surface.v)));
        let mut grid = Vec::with_capacity(us.len() * vs.len());
        let mut spacing: f64 = 0.0;
        for (i, u) in us.iter().enumerate() {
            for (j, v) in vs.iter().enumerate() {
                let point = surface.point(*u, *v);
                if i > 0 {
                    spacing = spacing.max((point - surface.point(us[i - 1], *v)).norm());
                }
                if j > 0 {
                    spacing = spacing.max((point - surface.point(*u, vs[j - 1])).norm());
                }
                grid.push((Uv::new(*u, *v), point));
            }
        }
        if spacing.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            return Err(Error::geometry("the surface has no extent"));
        }
        let patches = patches(&surface, &grid, spacing);
        Ok(Self { surface, closed_u, grid, spacing, patches, bands: Arc::default() })
    }

    pub fn data(&self) -> &nurbs::Surface {
        &self.surface
    }

    /// Effective projection data for the mesh owner's exact in-process cache key.
    /// Transforms retain/move these samples rather than rebuilding them from the net;
    /// the control net alone therefore does not identify numerical projection behavior.
    pub(crate) fn projection_data(&self) -> (&[(Uv, Point3)], f64) {
        (&self.grid, self.spacing)
    }

    /// `uv` taken into the domain: `u` wrapped when it closes, both clamped otherwise.
    fn inside(&self, uv: Uv) -> Uv {
        let (start, end) = self.surface.u.domain();
        let (low, high) = self.surface.v.domain();
        let u = if self.closed_u {
            start + (uv.x - start).rem_euclid(end - start)
        } else {
            uv.x.clamp(start, end)
        };
        Uv::new(u, uv.y.clamp(low, high))
    }

    /// The point and the derivatives `[S, Su, Sv, Suu, Suv, Svv]` at `uv`.
    fn local(&self, uv: Uv) -> [Vec3; 6] {
        let inside = self.inside(uv);
        let d = self.surface.derivatives(inside.x, inside.y, 2);
        [d[0][0], d[1][0], d[0][1], d[2][0], d[1][1], d[0][2]]
    }

    /// The iso-curve along `u` at `v` and along `v` at `u`, as curves.
    fn along_u(&self, v: f64) -> Option<NurbsCurve> {
        NurbsCurve::new(self.surface.along_u(v)).ok()
    }

    fn along_v(&self, u: f64) -> Option<NurbsCurve> {
        NurbsCurve::new(self.surface.along_v(u)).ok()
    }

    /// For a face whose every loop runs along an iso-curve of constant `v`: the range of
    /// `v` it covers, in increasing order. That is the side of a loft between its first and
    /// last section.
    pub fn band(&self, loops: &[Vec<Point3>]) -> Option<(f64, f64)> {
        let mut hasher = DefaultHasher::new();
        for point in loops.iter().flatten() {
            point.coords.iter().for_each(|value| value.to_bits().hash(&mut hasher));
        }
        let fingerprint = hasher.finish();
        if let Ok(known) = self.bands.lock() {
            if let Some((_, band)) = known.iter().find(|(key, _)| *key == fingerprint) {
                return *band;
            }
        }
        let band = self.band_of(loops);
        if let Ok(mut known) = self.bands.lock() {
            known.push((fingerprint, band));
        }
        band
    }

    fn band_of(&self, loops: &[Vec<Point3>]) -> Option<(f64, f64)> {
        let mut levels = Vec::new();
        for points in loops {
            let mut level: Option<f64> = None;
            for point in points {
                let uv = self.project(point);
                let close = (self.point(uv) - point).norm() <= self.spacing * 1e-9;
                let same = level.is_none_or(|known| {
                    (self.point(Uv::new(uv.x, known)) - point).norm() <= self.spacing * 1e-9
                });
                if !close || !same {
                    return None;
                }
                level.get_or_insert(uv.y);
            }
            levels.push(level?);
        }
        match levels.as_slice() {
            [first, second] if first != second => Some((first.min(*second), first.max(*second))),
            _ => None,
        }
    }
}

impl Nurbs {
    /// The surface after an affine motion that scales lengths by `scale`. A grid point moves
    /// with the surface, since the surface at fixed parameters is an affine combination of
    /// its control points.
    fn moved(&self, motion: impl Fn(&Point3) -> Point3, scale: f64) -> Self {
        let grid: Vec<(Uv, Point3)> = self.grid.iter().map(|(uv, point)| (*uv, motion(point))).collect();
        let spacing = self.spacing * scale;
        let patches = self
            .patches
            .iter()
            .map(|(_, members)| {
                (Bounds::of(members.iter().map(|index| grid[*index].1)).grown(spacing), members.clone())
            })
            .collect();
        Self {
            surface: self.surface.mapped(motion),
            closed_u: self.closed_u,
            grid,
            spacing,
            patches,
            bands: Arc::default(),
        }
    }
}

impl Surface for Nurbs {
    fn classes(&self) -> &'static [&'static str] {
        &["nurbs", "surface"]
    }

    fn point(&self, uv: Uv) -> Point3 {
        Point3::from(self.local(uv)[0])
    }

    fn normal(&self, uv: Uv) -> Vec3 {
        let [_, su, sv, ..] = self.local(uv);
        su.cross(&sv).normalize()
    }

    fn project(&self, point: &Point3) -> Uv {
        // Several nearest grid points are tried: on a thin band the nearest one may lie on
        // the other sheet, a hair's breadth away in space but far in the parameters.
        let mut seeds: Vec<&(Uv, Point3)> = self.grid.iter().collect();
        let by_distance = |a: &&(Uv, Point3), b: &&(Uv, Point3)| {
            (a.1 - point).norm_squared().total_cmp(&(b.1 - point).norm_squared())
        };
        let count = SEEDS.min(seeds.len());
        if count == 0 {
            return Uv::origin();
        }
        seeds.select_nth_unstable_by(count - 1, by_distance);
        seeds[..count].sort_by(by_distance);
        let distance = |uv: Uv| (self.point(uv) - point).norm();
        let mut best: Option<(Uv, f64)> = None;
        for (seed, _) in &seeds[..count] {
            let foot = self.foot(*seed, point);
            let off = distance(foot);
            if best.is_none_or(|(_, known)| off < known) {
                best = Some((foot, off));
            }
            // A point found on the surface cannot be bettered.
            if off <= self.spacing * ON_SURFACE {
                break;
            }
        }
        best.map_or(Uv::origin(), |(uv, _)| uv)
    }

    fn line(&self, origin: &Point3, direction: &Vec3, hits: &mut Vec<f64>) -> Exactness {
        // Newton on S(u, v) − (o + t·d) = 0 from the grid points near the line, nearest
        // first; a grid point beside a hit already found would only find it again.
        let unit = direction.normalize();
        let mut seeds: Vec<(f64, Uv, f64)> = Vec::new();
        let near = self
            .patches
            .iter()
            .filter(|(bounds, _)| bounds.line(origin, direction).is_some())
            .flat_map(|(_, members)| members.iter().map(|index| &self.grid[*index]));
        for (uv, point) in near {
            let along = (point - origin).dot(&unit);
            let off = ((point - origin) - unit * along).norm();
            if off <= self.spacing {
                seeds.push((off, *uv, along / direction.norm()));
            }
        }
        seeds.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut found: Vec<(f64, Point3)> = Vec::new();
        for (_, start, along) in seeds {
            let seed_point = self.point(start);
            if found.iter().any(|(_, hit)| (hit - seed_point).norm() <= self.spacing) {
                continue;
            }
            let (mut uv, mut t) = (start, along);
            let mut converged = false;
            let mut last = f64::INFINITY;
            for _ in 0..NEWTON {
                let [s, su, sv, ..] = self.local(uv);
                let residual = s - (origin.coords + direction * t);
                if residual.norm() <= self.spacing * 1e-13 {
                    converged = true;
                    break;
                }
                // Newton halves the residual at least once it is near a root; a residual that
                // stops shrinking means the line misses the surface here.
                if residual.norm() > 0.5 * last && last < self.spacing {
                    break;
                }
                last = residual.norm();
                let jacobian = Matrix3::from_columns(&[su, sv, -direction]);
                let Some(step) = jacobian.lu().solve(&-residual) else { break };
                uv = self.inside(Uv::new(uv.x + step.x, uv.y + step.y));
                t += step.z;
            }
            let at = self.point(uv);
            let on_line = ((at - origin) - unit * (at - origin).dot(&unit)).norm() <= self.spacing * 1e-11;
            if converged || on_line {
                let t = (at - origin).dot(direction) / direction.norm_squared();
                if !found
                    .iter()
                    .any(|(known, point)| (point - at).norm() <= self.spacing * 1e-9 || *known == t)
                {
                    found.push((t, at));
                }
            }
        }
        hits.extend(found.iter().map(|(t, _)| *t));
        Exactness::Numeric { tolerance: self.spacing * 1e-11 }
    }

    fn periods(&self) -> [Option<f64>; 2] {
        let (start, end) = self.surface.u.domain();
        [self.closed_u.then_some(end - start), None]
    }

    fn walk(&self, from: &Point3, to: &Point3) -> Vec<Stretch> {
        // Along v at the start's u to the target's v, then along u at that v.
        let (start, end) = (self.project(from), self.project(to));
        let mut stretches = Vec::new();
        if end.y != start.y {
            if let Some(curve) = self.along_v(start.x) {
                stretches.push(Stretch { curve: Arc::new(curve), from: start.y, to: end.y });
            }
        }
        let mut target = end.x;
        if self.closed_u {
            let (low, high) = self.surface.u.domain();
            let period = high - low;
            let turn = (end.x - start.x + 0.5 * period).rem_euclid(period) - 0.5 * period;
            target = start.x + turn;
        }
        if target != start.x {
            if let Some(curve) = self.along_u(end.y) {
                stretches.push(Stretch { curve: Arc::new(curve), from: start.x, to: target });
            }
        }
        stretches
    }

    fn extremes(&self, direction: &Vec3) -> Vec<Point3> {
        // Interior points where the height along the direction stops changing, refined from
        // the grid points that are higher or lower than their neighbours. Extremes on the
        // edge of the domain are found from the boundary of the face instead.
        let mut found: Vec<Point3> = Vec::new();
        for sign in [1.0, -1.0] {
            let height = |point: &Point3| sign * point.coords.dot(direction);
            for (uv, point) in &self.grid {
                let peak = self
                    .grid
                    .iter()
                    .filter(|(other, _)| {
                        (other - uv).norm() > 0.0 && (self.point(*other) - point).norm() <= 1.5 * self.spacing
                    })
                    .all(|(_, other)| height(other) <= height(point));
                if !peak {
                    continue;
                }
                let mut at = *uv;
                for _ in 0..NEWTON {
                    let [_, su, sv, suu, suv, svv] = self.local(at);
                    let gradient = nalgebra::Vector2::new(su.dot(direction), sv.dot(direction));
                    let hessian = nalgebra::Matrix2::new(
                        suu.dot(direction),
                        suv.dot(direction),
                        suv.dot(direction),
                        svv.dot(direction),
                    );
                    let Some(step) = newton_step(&hessian, &gradient) else { break };
                    let next = self.inside(Uv::new(at.x + step.x, at.y + step.y));
                    if next == at {
                        break;
                    }
                    at = next;
                }
                let [_, su, sv, ..] = self.local(at);
                let flat = su.dot(direction).abs() <= 1e-9 * su.norm() * direction.norm()
                    && sv.dot(direction).abs() <= 1e-9 * sv.norm() * direction.norm();
                let point = self.point(at);
                if flat && !found.iter().any(|known| (known - point).norm() <= self.spacing * 1e-9) {
                    found.push(point);
                }
            }
        }
        found
    }

    fn summary(&self) -> String {
        format!(
            "nurbs degrees={}x{} points={}x{}",
            self.surface.u.degree,
            self.surface.v.degree,
            self.surface.points.len(),
            self.surface.points.first().map_or(0, Vec::len)
        )
    }

    fn transformed(&self, by: &Sim3) -> Arc<dyn Surface> {
        Arc::new(self.moved(|point| by * point, by.scaling()))
    }

    fn mirrored(&self, mirror: &Mirror) -> Option<Arc<dyn Surface>> {
        Some(Arc::new(self.moved(|point| mirror.point(point), 1.0)))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::Nurbs;
    use krly3d_core::math::nurbs::{self, weighted};
    use krly3d_core::math::{Point3, Uv};
    use krly3d_core::Accuracy;
    use krly3d_kernel::ports::surface::{contract, Surface};

    /// A doubly curved patch: a 4 by 4 net of degree 3 with a bump in the middle.
    fn bump() -> Nurbs {
        let points = (0..4)
            .map(|i| {
                (0..4)
                    .map(|j| {
                        let (x, y) = (f64::from(i) * 2.0, f64::from(j) * 2.0);
                        let z = if (1..3).contains(&i) && (1..3).contains(&j) { 3.0 } else { 0.0 };
                        weighted(&Point3::new(x, y, z), if i == 1 { 1.5 } else { 1.0 })
                    })
                    .collect()
            })
            .collect();
        let basis = || nurbs::Basis::new(3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], 4).unwrap();
        Nurbs::new(nurbs::Surface::new(basis(), basis(), points).unwrap()).unwrap()
    }

    #[test]
    fn satisfies_the_surface_contract() {
        let samples = [Uv::new(0.2, 0.3), Uv::new(0.5, 0.5), Uv::new(0.8, 0.35), Uv::new(0.4, 0.75)];

        contract::check(&bump(), &samples, 0.3, &Accuracy::for_scale(10.0).unwrap()).unwrap();
    }

    #[test]
    fn the_top_of_the_bump_is_its_extreme_upward() {
        let bump = bump();

        let top = bump.extremes(&krly3d_core::math::Vec3::z());

        // The extreme is at least as high as any point of a fine grid, and not much higher.
        let highest = top.iter().map(|point| point.z).fold(f64::NEG_INFINITY, f64::max);
        let sampled = (0..=100)
            .flat_map(|i| (0..=100).map(move |j| Uv::new(f64::from(i) / 100.0, f64::from(j) / 100.0)))
            .map(|uv| bump.point(uv).z)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(highest >= sampled && highest - sampled < 1e-3, "{highest} vs {sampled}");
    }

    #[test]
    fn a_point_of_a_thin_steeply_swept_band_projects_onto_itself() {
        use crate::tool::testing::{context, made};
        use krly3d_kernel::schema::Params;
        let cx = context();
        // Two airfoil sections, the second far back: the side of the loft is a thin band
        // whose upper and lower sheets lie closer together than its grid points.
        let lens = |front: f64, back: f64, thickness: f64, y: f64| {
            let (half, middle) = (0.5 * thickness, 0.5 * (front + back));
            let drop = ((0.5 * (back - front)).powi(2) + half * half) / thickness - half;
            let path =
                format!("M {front},0 A {middle},{} {back},0 cw A {middle},{drop} {front},0 cw Z", -drop);
            made(
                &cx,
                "profile",
                Params::new().with("path", path.as_str()).with("plane", "xz").with("at", [0.0, y, 0.0]),
                &[],
            )
        };
        let chine =
            made(&cx, "loft", Params::new(), &[lens(4.6, 11.4, 0.12, -1.2), lens(8.0, 11.4, 0.1, -1.86)]);
        let side = chine.faces.iter().find(|face| face.name == "side").unwrap();

        for edge in &chine.edges {
            for k in 0..=8 {
                let point = edge.point(f64::from(k) / 8.0);
                let foot = side.surface.point(side.surface.project(&point));
                assert!((foot - point).norm() < 1e-9, "{point:?} projects to {foot:?}");
            }
        }
    }
}
