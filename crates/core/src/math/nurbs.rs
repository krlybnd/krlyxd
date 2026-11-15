//! Non-uniform rational B-splines: the curves and surfaces that pass through sections
//!
//! Domain geometry support: deliberately retains krly3d error/accuracy semantics.
//! This is not part of the neutral krlyxd API (see ADR 0008).
//! where no closed form does.
//!
//! The algorithms are those of Piegl and Tiller, "The NURBS Book" (second edition):
//! A2.1 (knot span), A2.3 (basis functions and their derivatives), A3.2/A4.2 (rational
//! curve derivatives), A3.6/A4.4 (rational surface derivatives) and A9.1 (global
//! interpolation). Points are kept homogeneous, `(w·x, w·y, w·z, w)`, so that a rational
//! curve is a polynomial one in four dimensions and conics are exact.

use super::{Point3, Vec3};
use crate::Error;
use nalgebra::{DMatrix, DVector, Vector4};

/// A point with its weight, in homogeneous form `(w·x, w·y, w·z, w)`.
pub type Weighted = Vector4<f64>;

/// `point` with weight `weight`, homogeneous.
pub fn weighted(point: &Point3, weight: f64) -> Weighted {
    Vector4::new(point.x * weight, point.y * weight, point.z * weight, weight)
}

/// A B-spline basis: a degree and a clamped, non-decreasing knot vector.
#[derive(Clone, Debug, PartialEq)]
pub struct Basis {
    pub degree: usize,
    pub knots: Vec<f64>,
}

impl Basis {
    /// Checks that the knots are non-decreasing and clamped for `count` control points.
    pub fn new(degree: usize, knots: Vec<f64>, count: usize) -> Result<Self, Error> {
        if degree == 0 || count <= degree {
            return Err(Error::input(format!(
                "a B-spline of degree {degree} needs more than {degree} control points"
            )));
        }
        if knots.len() != count + degree + 1 {
            return Err(Error::input(format!(
                "{count} control points of degree {degree} need {} knots, got {}",
                count + degree + 1,
                knots.len()
            )));
        }
        if knots.windows(2).any(|pair| pair[1] < pair[0] || !pair[1].is_finite()) {
            return Err(Error::input("the knots must be finite and must not decrease"));
        }
        let clamped = |range: &[f64]| range.iter().all(|knot| *knot == range[0]);
        if !clamped(&knots[..=degree])
            || !clamped(&knots[knots.len() - degree - 1..])
            || knots[degree] >= knots[count]
        {
            return Err(Error::input(
                "the knot vector must be clamped: the first and last knots repeated degree + 1 times",
            ));
        }
        Ok(Self { degree, knots })
    }

    /// The parameter range: from the first to the last knot.
    pub fn domain(&self) -> (f64, f64) {
        (self.knots[self.degree], self.knots[self.knots.len() - self.degree - 1])
    }

    fn count(&self) -> usize {
        self.knots.len() - self.degree - 1
    }

    /// The index `i` of the knot span `[uᵢ, uᵢ₊₁)` holding `u`, with the end of the domain
    /// put into the last non-empty span (A2.1).
    pub fn span(&self, u: f64) -> usize {
        let (p, n) = (self.degree, self.count() - 1);
        if u >= self.knots[n + 1] {
            return n;
        }
        if u <= self.knots[p] {
            return p;
        }
        let (mut low, mut high) = (p, n + 1);
        let mut middle = (low + high) / 2;
        while u < self.knots[middle] || u >= self.knots[middle + 1] {
            if u < self.knots[middle] {
                high = middle;
            } else {
                low = middle;
            }
            middle = (low + high) / 2;
        }
        middle
    }

    /// The `degree + 1` basis functions that do not vanish on the span, and their
    /// derivatives up to `order`: `result[k][j]` is the k-th derivative of `N_{span−p+j}` (A2.3).
    pub fn derivatives(&self, span: usize, u: f64, order: usize) -> Vec<Vec<f64>> {
        let p = self.degree;
        let knots = &self.knots;
        let mut ndu = vec![vec![0.0; p + 1]; p + 1];
        let (mut left, mut right) = (vec![0.0; p + 1], vec![0.0; p + 1]);
        ndu[0][0] = 1.0;
        for j in 1..=p {
            left[j] = u - knots[span + 1 - j];
            right[j] = knots[span + j] - u;
            let mut saved = 0.0;
            for r in 0..j {
                ndu[j][r] = right[r + 1] + left[j - r];
                let temp = ndu[r][j - 1] / ndu[j][r];
                ndu[r][j] = saved + right[r + 1] * temp;
                saved = left[j - r] * temp;
            }
            ndu[j][j] = saved;
        }
        let order = order.min(p);
        let mut ders = vec![vec![0.0; p + 1]; order + 1];
        for j in 0..=p {
            ders[0][j] = ndu[j][p];
        }
        let mut a = vec![vec![0.0; p + 1]; 2];
        for r in 0..=p {
            let (mut s1, mut s2) = (0, 1);
            a[0][0] = 1.0;
            for k in 1..=order {
                let mut d = 0.0;
                let rk = r as isize - k as isize;
                let pk = p - k;
                if r >= k {
                    let rk = rk as usize;
                    a[s2][0] = a[s1][0] / ndu[pk + 1][rk];
                    d = a[s2][0] * ndu[rk][pk];
                }
                let j1 = if rk >= -1 { 1 } else { (-rk) as usize };
                let j2 = if r as isize - 1 <= pk as isize { k - 1 } else { p - r };
                for j in j1..=j2 {
                    let index = (rk + j as isize) as usize;
                    a[s2][j] = (a[s1][j] - a[s1][j - 1]) / ndu[pk + 1][index];
                    d += a[s2][j] * ndu[index][pk];
                }
                if r <= pk {
                    a[s2][k] = -a[s1][k - 1] / ndu[pk + 1][r];
                    d += a[s2][k] * ndu[r][pk];
                }
                ders[k][r] = d;
                std::mem::swap(&mut s1, &mut s2);
            }
        }
        let mut factor = p as f64;
        for (k, row) in ders.iter_mut().enumerate().skip(1) {
            for value in row.iter_mut() {
                *value *= factor;
            }
            factor *= (p - k) as f64;
        }
        ders
    }
}

fn binomial(n: usize, k: usize) -> f64 {
    (0..k).fold(1.0, |product, i| product * (n - i) as f64 / (i + 1) as f64)
}

/// The derivatives of a rational point from those of its homogeneous form (A4.2/A4.4,
/// one parameter at a time): `C⁽ᵏ⁾ = (A⁽ᵏ⁾ − Σᵢ C(k,i)·w⁽ⁱ⁾·C⁽ᵏ⁻ⁱ⁾) / w`.
fn rational(homogeneous: &[Weighted]) -> Vec<Vec3> {
    let mut result: Vec<Vec3> = Vec::with_capacity(homogeneous.len());
    for k in 0..homogeneous.len() {
        let mut value = homogeneous[k].xyz();
        for i in 1..=k {
            value -= result[k - i] * (binomial(k, i) * homogeneous[i].w);
        }
        result.push(value / homogeneous[0].w);
    }
    result
}

/// A NURBS curve.
#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    pub basis: Basis,
    pub points: Vec<Weighted>,
}

impl Curve {
    pub fn new(degree: usize, knots: Vec<f64>, points: Vec<Weighted>) -> Result<Self, Error> {
        if points.iter().any(|point| {
            point.w.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
                || !point.iter().all(|value| value.is_finite())
        }) {
            return Err(Error::input("every control point needs a positive weight and finite coordinates"));
        }
        Ok(Self { basis: Basis::new(degree, knots, points.len())?, points })
    }

    pub fn domain(&self) -> (f64, f64) {
        self.basis.domain()
    }

    /// The point and its derivatives up to `order` at `u`.
    pub fn derivatives(&self, u: f64, order: usize) -> Vec<Vec3> {
        let span = self.basis.span(u);
        let basis = self.basis.derivatives(span, u, order);
        let p = self.basis.degree;
        let homogeneous: Vec<Weighted> = (0..=order)
            .map(|k| {
                basis.get(k).map_or_else(Vector4::zeros, |values| {
                    (0..=p).map(|j| self.points[span - p + j] * values[j]).sum()
                })
            })
            .collect();
        rational(&homogeneous)
    }

    pub fn point(&self, u: f64) -> Point3 {
        Point3::from(self.derivatives(u, 0)[0])
    }

    /// The curve after every control point is moved by `motion`, which must be affine.
    pub fn mapped(&self, motion: impl Fn(&Point3) -> Point3) -> Self {
        let points = self
            .points
            .iter()
            .map(|point| weighted(&motion(&Point3::from(point.xyz() / point.w)), point.w))
            .collect();
        Self { basis: self.basis.clone(), points }
    }
}

/// A NURBS surface; `points[i][j]` is the control point `i` along `u` and `j` along `v`.
#[derive(Clone, Debug, PartialEq)]
pub struct Surface {
    pub u: Basis,
    pub v: Basis,
    pub points: Vec<Vec<Weighted>>,
}

impl Surface {
    pub fn new(u: Basis, v: Basis, points: Vec<Vec<Weighted>>) -> Result<Self, Error> {
        if points.len() != u.count() || points.iter().any(|row| row.len() != v.count()) {
            return Err(Error::input("the control net does not match the two knot vectors"));
        }
        if points.iter().flatten().any(|point| {
            point.w.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
                || !point.iter().all(|value| value.is_finite())
        }) {
            return Err(Error::input("every control point needs a positive weight and finite coordinates"));
        }
        Ok(Self { u, v, points })
    }

    /// `result[k][l]` is the derivative k times along u and l times along v, `k + l ≤ order`.
    pub fn derivatives(&self, u: f64, v: f64, order: usize) -> Vec<Vec<Vec3>> {
        let (su, sv) = (self.u.span(u), self.v.span(v));
        let (nu, nv) = (self.u.derivatives(su, u, order), self.v.derivatives(sv, v, order));
        let (p, q) = (self.u.degree, self.v.degree);
        let mut homogeneous = vec![vec![Vector4::zeros(); order + 1]; order + 1];
        for k in 0..=order.min(p) {
            for l in 0..=(order - k).min(q) {
                let mut sum = Vector4::zeros();
                for (i, &u_basis) in nu[k].iter().enumerate().take(p + 1) {
                    for (j, &v_basis) in nv[l].iter().enumerate().take(q + 1) {
                        sum += self.points[su - p + i][sv - q + j] * (u_basis * v_basis);
                    }
                }
                homogeneous[k][l] = sum;
            }
        }
        // A4.4: SKL = (Aᵏˡ − Σⱼ C(l,j)wᵒʲ Sᵏˡ⁻ʲ − Σᵢ C(k,i)wⁱᵒ Sᵏ⁻ⁱˡ − Σᵢⱼ C(k,i)C(l,j)wⁱʲ Sᵏ⁻ⁱˡ⁻ʲ) / w
        let w = |k: usize, l: usize| homogeneous[k][l].w;
        let mut result = vec![vec![Vec3::zeros(); order + 1]; order + 1];
        for k in 0..=order {
            for l in 0..=(order - k) {
                let mut value = homogeneous[k][l].xyz();
                for j in 1..=l {
                    value -= result[k][l - j] * (binomial(l, j) * w(0, j));
                }
                for i in 1..=k {
                    value -= result[k - i][l] * (binomial(k, i) * w(i, 0));
                    let mut inner = Vec3::zeros();
                    for j in 1..=l {
                        inner += result[k - i][l - j] * (binomial(l, j) * w(i, j));
                    }
                    value -= inner * binomial(k, i);
                }
                result[k][l] = value / w(0, 0);
            }
        }
        result
    }

    pub fn point(&self, u: f64, v: f64) -> Point3 {
        Point3::from(self.derivatives(u, v, 0)[0][0])
    }

    /// The curve `v ↦ S(u, v)` at a fixed `u`, exactly: its control points are those of
    /// the rows of the net combined with the basis along `u`, in homogeneous form.
    pub fn along_v(&self, u: f64) -> Curve {
        let span = self.u.span(u);
        let basis = &self.u.derivatives(span, u, 0)[0];
        let p = self.u.degree;
        let points = (0..self.v.count())
            .map(|j| (0..=p).map(|i| self.points[span - p + i][j] * basis[i]).sum())
            .collect();
        Curve { basis: self.v.clone(), points }
    }

    /// The curve `u ↦ S(u, v)` at a fixed `v`, exactly.
    pub fn along_u(&self, v: f64) -> Curve {
        let span = self.v.span(v);
        let basis = &self.v.derivatives(span, v, 0)[0];
        let q = self.v.degree;
        let points =
            self.points.iter().map(|row| (0..=q).map(|j| row[span - q + j] * basis[j]).sum()).collect();
        Curve { basis: self.u.clone(), points }
    }

    /// The surface after every control point is moved by the affine `motion`.
    pub fn mapped(&self, motion: impl Fn(&Point3) -> Point3) -> Self {
        let points = self
            .points
            .iter()
            .map(|row| {
                row.iter()
                    .map(|point| weighted(&motion(&Point3::from(point.xyz() / point.w)), point.w))
                    .collect()
            })
            .collect();
        Self { u: self.u.clone(), v: self.v.clone(), points }
    }
}

/// The circular arc about `centre` from `start` through the signed angle `sweep` (at most a
/// whole turn) about the unit `axis`, as an exact rational quadratic curve: one segment per
/// quarter turn or less, with middle weights `cos(Δ/2)` (A7.1).
pub fn arc(centre: &Point3, axis: &Vec3, start: &Point3, sweep: f64) -> Result<Curve, Error> {
    let radius = (start - centre).norm();
    if radius.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
        || sweep == 0.0
        || sweep.abs() > std::f64::consts::TAU
    {
        return Err(Error::input("an arc needs a radius and a sweep of at most a whole turn"));
    }
    let segments = (sweep.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
    let step = sweep / segments as f64;
    let x = (start - centre) / radius;
    let y = axis.cross(&x);
    let at = |angle: f64| centre + (x * angle.cos() + y * angle.sin()) * radius;
    let middle_weight = (0.5 * step).cos();
    let mut points = vec![weighted(&at(0.0), 1.0)];
    let mut knots = vec![0.0; 3];
    for segment in 0..segments {
        let (from, to) = (step * segment as f64, step * (segment + 1) as f64);
        // The corner where the tangents at the two ends meet, at r / cos(Δ/2) on the bisector.
        let corner = centre
            + (x * (0.5 * (from + to)).cos() + y * (0.5 * (from + to)).sin()) * (radius / middle_weight);
        points.push(weighted(&corner, middle_weight));
        points.push(weighted(&at(to), 1.0));
        let knot = (segment + 1) as f64 / segments as f64;
        knots.extend([knot, knot]);
    }
    knots.pop();
    knots.push(1.0);
    knots.push(1.0);
    Curve::new(2, knots, points)
}

/// The straight segment from `from` to `to` as a curve of degree 1.
pub fn segment(from: &Point3, to: &Point3) -> Result<Curve, Error> {
    Curve::new(1, vec![0.0, 0.0, 1.0, 1.0], vec![weighted(from, 1.0), weighted(to, 1.0)])
}

/// Parameters for interpolating through `count` items placed `distances` apart by the
/// chord-length method, scaled to [0, 1]; equal spacing when the distances vanish.
pub fn chord_parameters(distances: &[f64]) -> Vec<f64> {
    let total: f64 = distances.iter().sum();
    let mut parameters = vec![0.0];
    let mut run = 0.0;
    for distance in distances {
        run += if total > 0.0 { distance / total } else { 1.0 / distances.len() as f64 };
        parameters.push(run);
    }
    if let Some(last) = parameters.last_mut() {
        *last = 1.0;
    }
    parameters
}

/// The clamped knot vector of degree `p` for interpolation at `parameters`, by averaging
/// (equation 9.8).
pub fn averaged_knots(parameters: &[f64], p: usize) -> Vec<f64> {
    let n = parameters.len() - 1;
    let mut knots = vec![0.0; p + 1];
    for j in 1..=n - p {
        knots.push(parameters[j..j + p].iter().sum::<f64>() / p as f64);
    }
    knots.extend(std::iter::repeat_n(1.0, p + 1));
    knots
}

/// The control points of the B-spline of degree `p` on `knots` that passes through each
/// `values[k]` at `parameters[k]` (A9.1), for homogeneous values of any kind.
pub fn interpolate(
    values: &[Weighted],
    parameters: &[f64],
    p: usize,
    knots: &[f64],
) -> Result<Vec<Weighted>, Error> {
    let count = values.len();
    let basis = Basis::new(p, knots.to_vec(), count)?;
    let mut matrix = DMatrix::zeros(count, count);
    for (row, parameter) in parameters.iter().enumerate() {
        let span = basis.span(*parameter);
        for (offset, value) in basis.derivatives(span, *parameter, 0)[0].iter().enumerate() {
            matrix[(row, span - p + offset)] = *value;
        }
    }
    let lu = matrix.lu();
    let mut solved = vec![Vector4::zeros(); count];
    for coordinate in 0..4 {
        let right = DVector::from_iterator(count, values.iter().map(|value| value[coordinate]));
        let column =
            lu.solve(&right).ok_or_else(|| Error::geometry("the interpolation system is singular"))?;
        for (index, value) in column.iter().enumerate() {
            solved[index][coordinate] = *value;
        }
    }
    Ok(solved)
}

/// Raises the degree of a curve by one (A5.9 in its simplest form, segment by segment, by
/// knot refinement of the Bézier pieces). Used to give sections a common degree.
pub fn elevated(curve: &Curve) -> Result<Curve, Error> {
    let p = curve.basis.degree;
    // Split into Bézier pieces by inserting every interior knot up to multiplicity p.
    let mut working = curve.clone();
    let interior: Vec<f64> = {
        let mut distinct: Vec<f64> = curve.basis.knots[p + 1..curve.basis.knots.len() - p - 1].to_vec();
        distinct.dedup();
        distinct
    };
    for knot in &interior {
        let multiplicity = working.basis.knots.iter().filter(|k| *k == knot).count();
        for _ in multiplicity..p {
            working = inserted(&working, *knot)?;
        }
    }
    // Each Bézier piece of degree p becomes one of degree p + 1 (equation 5.36).
    let pieces = (working.points.len() - 1) / p;
    let mut points = Vec::with_capacity(pieces * (p + 1) + 1);
    for piece in 0..pieces {
        let control = &working.points[piece * p..=piece * p + p];
        for i in 0..=p + 1 {
            if piece > 0 && i == 0 {
                continue;
            }
            let lower = if i > 0 { control[i - 1] * (i as f64 / (p + 1) as f64) } else { Vector4::zeros() };
            let upper =
                if i <= p { control[i] * (1.0 - i as f64 / (p + 1) as f64) } else { Vector4::zeros() };
            points.push(lower + upper);
        }
    }
    let (start, end) = curve.domain();
    let mut knots = vec![start; p + 2];
    for knot in &interior {
        knots.extend(std::iter::repeat_n(*knot, p + 1));
    }
    knots.extend(std::iter::repeat_n(end, p + 2));
    Curve::new(p + 1, knots, points)
}

/// The same curve with one more knot at `knot` (A5.1, Boehm's insertion).
pub fn inserted(curve: &Curve, knot: f64) -> Result<Curve, Error> {
    let p = curve.basis.degree;
    let knots = &curve.basis.knots;
    let k = curve.basis.span(knot);
    let mut points = Vec::with_capacity(curve.points.len() + 1);
    for i in 0..=curve.points.len() {
        let point = if i + p <= k {
            curve.points[i]
        } else if i > k {
            curve.points[i - 1]
        } else {
            let alpha = (knot - knots[i]) / (knots[i + p] - knots[i]);
            curve.points[i] * alpha + curve.points[i - 1] * (1.0 - alpha)
        };
        points.push(point);
    }
    let mut refined = knots.clone();
    refined.insert(k + 1, knot);
    Curve::new(p, refined, points)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::{FRAC_PI_2, PI, TAU};

    #[test]
    fn the_basis_functions_sum_to_one_and_their_derivatives_to_zero() {
        let basis = Basis::new(3, vec![0.0, 0.0, 0.0, 0.0, 0.3, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0], 7).unwrap();
        for u in [0.0, 0.1, 0.3, 0.42, 0.5, 0.77, 1.0] {
            let ders = basis.derivatives(basis.span(u), u, 2);
            assert!((ders[0].iter().sum::<f64>() - 1.0).abs() < 1e-14, "at {u}");
            assert!(
                ders[1].iter().sum::<f64>().abs() < 1e-12 && ders[2].iter().sum::<f64>().abs() < 1e-10,
                "at {u}"
            );
        }
    }

    #[test]
    fn a_rational_arc_lies_exactly_on_its_circle_and_has_the_right_tangent() {
        let centre = Point3::new(1.0, -2.0, 0.5);
        let axis = Vec3::new(0.0, 0.0, 1.0);
        for sweep in [FRAC_PI_2, 2.0, PI, -4.0, TAU] {
            let arc = arc(&centre, &axis, &Point3::new(4.0, -2.0, 0.5), sweep).unwrap();
            for step in 0..=20 {
                let u = f64::from(step) / 20.0;
                let [point, tangent] = [0, 1].map(|k| arc.derivatives(u, 1)[k]);
                assert!(((Point3::from(point) - centre).norm() - 3.0).abs() < 1e-14, "sweep {sweep} at {u}");
                assert!(tangent.dot(&(Point3::from(point) - centre)).abs() < 1e-11, "sweep {sweep} at {u}");
            }
            let end = arc.point(1.0) - centre;
            assert!((end - Vec3::new(sweep.cos(), sweep.sin(), 0.0) * 3.0).norm() < 1e-14);
        }
    }

    #[test]
    fn derivatives_agree_with_central_differences() {
        let arc = arc(&Point3::origin(), &Vec3::z(), &Point3::new(2.0, 0.0, 0.0), 2.5).unwrap();
        let h = 1e-5;
        for u in [0.1, 0.37, 0.8] {
            let ders = arc.derivatives(u, 2);
            let first = (arc.point(u + h) - arc.point(u - h)) / (2.0 * h);
            let second = (arc.point(u + h) - arc.point(u) * 2.0 + arc.point(u - h).coords) / (h * h);
            assert!((ders[1] - first).norm() < 1e-6, "first at {u}");
            assert!((ders[2] - second).norm() < 1e-3, "second at {u}");
        }
    }

    #[test]
    fn interpolation_passes_through_its_points_and_elevation_keeps_the_curve() {
        let values: Vec<Weighted> =
            [[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [3.0, 1.0, 1.0], [4.0, 4.0, 0.0], [6.0, 3.0, 2.0]]
                .iter()
                .map(|p| weighted(&Point3::new(p[0], p[1], p[2]), 1.0))
                .collect();
        let distances: Vec<f64> =
            values.windows(2).map(|pair| (pair[1].xyz() - pair[0].xyz()).norm()).collect();
        let parameters = chord_parameters(&distances);
        let knots = averaged_knots(&parameters, 3);

        let curve =
            Curve::new(3, knots.clone(), interpolate(&values, &parameters, 3, &knots).unwrap()).unwrap();

        for (value, parameter) in values.iter().zip(&parameters) {
            assert!((curve.point(*parameter).coords - value.xyz()).norm() < 1e-12);
        }
        let raised = elevated(&curve).unwrap();
        assert_eq!(raised.basis.degree, 4);
        for step in 0..=10 {
            let u = f64::from(step) / 10.0;
            assert!((raised.point(u) - curve.point(u)).norm() < 1e-12, "at {u}");
        }
    }
}
