//! Real roots of univariate equations in closed form, reported with multiplicity.
//!
//! Multiplicity is kept on purpose: a line that grazes a surface yields either no root or
//! a pair of equal roots, never a single one, so the parity of the crossings of a closed
//! body survives round-off. Callers that need distinct points merge equal parameters with
//! their own `Accuracy`.

use std::f64::consts::{FRAC_PI_4, PI, TAU};

/// Up to `N` real roots in ascending order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Roots<const N: usize> {
    values: [f64; N],
    len: usize,
}

impl<const N: usize> Roots<N> {
    pub const fn none() -> Self {
        Self { values: [0.0; N], len: 0 }
    }

    pub fn as_slice(&self) -> &[f64] {
        &self.values[..self.len]
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn push(&mut self, root: f64) {
        if root.is_finite() && self.len < N {
            self.values[self.len] = root;
            self.len += 1;
        }
    }

    fn sorted(mut self) -> Self {
        self.values[..self.len].sort_by(f64::total_cmp);
        self
    }
}

/// Real roots of `a·x² + b·x + c = 0`: none or two for a true quadratic (a double root is
/// reported twice). With `a == 0` the equation is linear and has at most one root.
pub fn quadratic(a: f64, b: f64, c: f64) -> Roots<2> {
    let mut roots = Roots::none();
    if a == 0.0 {
        if b != 0.0 {
            roots.push(-c / b);
        }
        return roots;
    }
    let discriminant = b.mul_add(b, -4.0 * a * c);
    if discriminant < 0.0 {
        return roots;
    }
    // Adding the square root to b with the same sign avoids the cancellation that the
    // textbook formula suffers for the smaller root.
    let q = -0.5 * (b + discriminant.sqrt().copysign(b));
    let first = q / a;
    roots.push(first);
    roots.push(if q != 0.0 { c / q } else { first });
    roots.sorted()
}

/// Real roots of `c[0] + c[1]·x + c[2]·x² + c[3]·x³ = 0`, with multiplicity.
/// A vanishing leading coefficient lowers the degree.
pub fn cubic(c: [f64; 4]) -> Roots<3> {
    let mut roots = Roots::none();
    if c[3] == 0.0 {
        for root in quadratic(c[2], c[1], c[0]).as_slice() {
            roots.push(*root);
        }
        return roots.sorted();
    }
    let scale = root_bound(&c);
    let [a0, a1, a2] = monic::<3>(&c, scale);
    for root in monic_cubic(a2, a1, a0).as_slice() {
        roots.push(polish(&c, root * scale));
    }
    roots.sorted()
}

/// Real roots of `c[0] + c[1]·x + c[2]·x² + c[3]·x³ + c[4]·x⁴ = 0`, with multiplicity.
/// A vanishing leading coefficient lowers the degree.
pub fn quartic(c: [f64; 5]) -> Roots<4> {
    let mut roots = Roots::none();
    if c[4] == 0.0 {
        for root in cubic([c[0], c[1], c[2], c[3]]).as_slice() {
            roots.push(*root);
        }
        return roots.sorted();
    }
    let scale = root_bound(&c);
    let [a0, a1, a2, a3] = monic::<4>(&c, scale);
    for root in monic_quartic(a3, a2, a1, a0).as_slice() {
        roots.push(polish(&c, root * scale));
    }
    roots.sorted()
}

/// Real roots of `y³ + a·y² + b·y + c = 0`: Cardano's formula for one real root, the
/// trigonometric form for three.
fn monic_cubic(a: f64, b: f64, c: f64) -> Roots<3> {
    let shift = a / 3.0;
    let third_p = (b - a * shift) / 3.0;
    let half_q = 0.5 * (2.0 * shift.powi(3) - shift * b + c);
    let discriminant = half_q * half_q + third_p.powi(3);
    let mut roots = Roots::none();
    if discriminant > 0.0 {
        // The cube root of larger magnitude is computed directly; the other follows from
        // their product, which avoids subtracting nearly equal numbers.
        let large = -(half_q + discriminant.sqrt().copysign(half_q)).cbrt();
        let small = if large != 0.0 { -third_p / large } else { 0.0 };
        roots.push(large + small - shift);
    } else {
        let radius = 2.0 * (-third_p).sqrt();
        let cosine = if radius != 0.0 { (-half_q / (-third_p).powf(1.5)).clamp(-1.0, 1.0) } else { 1.0 };
        let angle = cosine.acos() / 3.0;
        for turn in [0.0, 1.0, 2.0] {
            roots.push(radius * (angle - turn * TAU / 3.0).cos() - shift);
        }
    }
    roots.sorted()
}

/// Real roots of `x⁴ + a3·x³ + a2·x² + a1·x + a0 = 0` by Ferrari's method: remove the
/// cubic term, then split the quartic into two quadratics with a root of the resolvent cubic.
fn monic_quartic(a3: f64, a2: f64, a1: f64, a0: f64) -> Roots<4> {
    let shift = 0.25 * a3;
    let p = a2 - 6.0 * shift * shift;
    let q = a1 - 2.0 * a2 * shift + 8.0 * shift.powi(3);
    let r = a0 - a1 * shift + a2 * shift * shift - 3.0 * shift.powi(4);
    // The resolvent m³ + p·m² + (p²/4 − r)·m − q²/8 is negative at m = 0 whenever q ≠ 0,
    // so its largest real root is positive. Round-off can only spoil that when q is
    // negligible, and then the quartic is biquadratic to working precision.
    let split = if q == 0.0 {
        None
    } else {
        monic_cubic(p, 0.25 * p * p - r, -0.125 * q * q).as_slice().last().copied().filter(|m| *m > 0.0)
    };
    let mut roots = Roots::none();
    match split {
        Some(m) => {
            let s = (2.0 * m).sqrt();
            let t = q / (2.0 * s);
            for (linear, constant) in [(-s, 0.5 * p + m + t), (s, 0.5 * p + m - t)] {
                for y in quadratic(1.0, linear, constant).as_slice() {
                    roots.push(y - shift);
                }
            }
        }
        None => {
            for square in quadratic(1.0, p, r).as_slice() {
                if *square >= 0.0 {
                    let y = square.sqrt();
                    roots.push(y - shift);
                    roots.push(-y - shift);
                }
            }
        }
    }
    roots.sorted()
}

/// Angles `θ` in `(−π, π]` with `a0 + a1·cos θ + b1·sin θ + a2·cos 2θ + b2·sin 2θ = 0`,
/// with multiplicity. The coefficients are given as `[a0, a1, b1, a2, b2]`.
///
/// Solved as a quartic in `tan(θ'/2)`, where `θ'` is measured from a chart angle chosen so
/// that no root falls near the pole of the tangent.
pub fn harmonic(coefficients: [f64; 5]) -> Roots<4> {
    let [a0, a1, b1, a2, b2] = coefficients;
    let value = |angle: f64| {
        a0 + a1 * angle.cos() + b1 * angle.sin() + a2 * (2.0 * angle).cos() + b2 * (2.0 * angle).sin()
    };
    let slope = |angle: f64| {
        -a1 * angle.sin() + b1 * angle.cos() - 2.0 * a2 * (2.0 * angle).sin() + 2.0 * b2 * (2.0 * angle).cos()
    };
    // A degree-two trigonometric polynomial has at most four roots per turn, so among eight
    // equally spaced angles the one with the largest value is safely away from all of them.
    let Some(pole) = (0..8)
        .map(|step| f64::from(step) * FRAC_PI_4)
        .max_by(|p, q| value(*p).abs().total_cmp(&value(*q).abs()))
        .filter(|angle| value(*angle) != 0.0)
    else {
        return Roots::none();
    };
    let shift = pole - PI;
    let (sin1, cos1) = shift.sin_cos();
    let (sin2, cos2) = (2.0 * shift).sin_cos();
    let (d, e) = (a1 * cos1 + b1 * sin1, -a1 * sin1 + b1 * cos1);
    let (a2s, b2s) = (a2 * cos2 + b2 * sin2, -a2 * sin2 + b2 * cos2);
    let (a, b, c) = (a0 + a2s, a0 - a2s, 2.0 * b2s);
    let half_angle = quartic([a + d, 2.0 * (c + e), -2.0 * a + 4.0 * b, 2.0 * (e - c), a - d]);
    let mut roots = Roots::none();
    for tangent in half_angle.as_slice() {
        let angle = refine(2.0 * tangent.atan() + shift, value, slope);
        roots.push(wrap(angle));
    }
    roots.sorted()
}

/// Brings an angle into `(−π, π]`.
pub fn wrap(angle: f64) -> f64 {
    let turned = angle.rem_euclid(TAU);
    if turned > PI {
        turned - TAU
    } else {
        turned
    }
}

/// Upper bound on the magnitude of every root (Fujiwara), used to scale the unknown so
/// that the companion matrix is balanced.
fn root_bound(c: &[f64]) -> f64 {
    let degree = c.len() - 1;
    let leading = c[degree];
    let bound =
        (0..degree).map(|k| (c[k] / leading).abs().powf(1.0 / (degree - k) as f64)).fold(0.0_f64, f64::max);
    if bound > 0.0 && bound.is_finite() {
        2.0 * bound
    } else {
        1.0
    }
}

/// Coefficients below the leading one of the monic polynomial in `y = x / scale`.
fn monic<const N: usize>(c: &[f64], scale: f64) -> [f64; N] {
    let leading = c[N] * scale.powi(N as i32);
    std::array::from_fn(|k| c[k] * scale.powi(k as i32) / leading)
}

/// Value and derivative of the polynomial with ascending coefficients `c` at `x`.
fn evaluate(c: &[f64], x: f64) -> (f64, f64) {
    c.iter()
        .rev()
        .fold((0.0, 0.0), |(value, slope), coefficient| (value * x + coefficient, slope * x + value))
}

fn polish(c: &[f64], start: f64) -> f64 {
    refine(start, |x| evaluate(c, x).0, |x| evaluate(c, x).1)
}

/// At a multiple root Newton's method gains about one bit per step, the slowest useful
/// rate, so after this many steps the mantissa is exhausted and more steps cannot help.
const REFINE_STEPS: usize = 64;

/// Newton steps, each kept only if it lowers the residual.
fn refine(start: f64, value: impl Fn(f64) -> f64, slope: impl Fn(f64) -> f64) -> f64 {
    let mut x = start;
    let mut residual = value(x).abs();
    for _ in 0..REFINE_STEPS {
        let derivative = slope(x);
        if residual == 0.0 || derivative == 0.0 {
            break;
        }
        let next = x - value(x) / derivative;
        let next_residual = value(next).abs();
        if next_residual.is_nan() || next_residual >= residual {
            break;
        }
        x = next;
        residual = next_residual;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::{cubic, harmonic, quadratic, quartic, wrap};
    use std::f64::consts::{FRAC_PI_3, FRAC_PI_4, PI};

    fn close(actual: &[f64], expected: &[f64], tolerance: f64) -> bool {
        actual.len() == expected.len() && actual.iter().zip(expected).all(|(a, e)| (a - e).abs() <= tolerance)
    }

    /// A ray along the axis of a torus, `distance` from it, starting `before` ahead of the
    /// torus' middle plane: `((t − before)² + K)² = 4R²ρ²` with `K = ρ² + R² − r²`.
    fn along_torus_axis(ring: f64, tube: f64, distance: f64, before: f64) -> ([f64; 5], Vec<f64>) {
        let k = distance * distance + ring * ring - tube * tube;
        let w = 4.0 * ring * ring * distance * distance;
        // (t² − 2bt + b² + K)² − w, expanded.
        let (b, c) = (-2.0 * before, before * before + k);
        let coefficients = [c * c - w, 2.0 * b * c, b * b + 2.0 * c, 2.0 * b, 1.0];
        let mut expected = Vec::new();
        for sign in [-1.0, 1.0] {
            let square = -k + sign * 2.0 * ring * distance;
            if square >= 0.0 {
                expected.extend([before - square.sqrt(), before + square.sqrt()]);
            }
        }
        expected.sort_by(f64::total_cmp);
        (coefficients, expected)
    }

    #[test]
    fn a_quartic_that_is_biquadratic_after_shifting_is_solved_accurately() {
        // Rays parallel to a torus' axis, as a picture taken along that axis casts them.
        for (distance, before) in [(1.2, 10.0), (1.45, 4.75), (1.1, 37.3), (1.5, 0.001)] {
            let (coefficients, expected) = along_torus_axis(1.3, 0.25, distance, before);

            let roots = quartic(coefficients);

            assert!(
                close(roots.as_slice(), &expected, 1e-9),
                "ρ={distance}: {:?} vs {expected:?}",
                roots.as_slice()
            );
        }
    }

    #[test]
    fn quadratic_returns_both_roots_in_ascending_order() {
        let roots = quadratic(1.0, -4.0, 3.0);

        assert_eq!(roots.as_slice(), [1.0, 3.0]);
    }

    #[test]
    fn quadratic_without_real_roots_returns_none() {
        assert!(quadratic(1.0, 0.0, 1.0).is_empty());
    }

    #[test]
    fn quadratic_reports_a_double_root_twice() {
        let roots = quadratic(1.0, -2.0, 1.0);

        assert_eq!(roots.as_slice(), [1.0, 1.0]);
    }

    #[test]
    fn quadratic_with_no_square_term_is_linear() {
        let roots = quadratic(0.0, 2.0, -8.0);

        assert_eq!(roots.as_slice(), [4.0]);
    }

    #[test]
    fn quadratic_keeps_the_small_root_accurate_when_the_roots_differ_hugely() {
        // x² − 1e8·x + 1 = 0 has roots near 1e-8 and 1e8; the product of the roots is exactly 1.
        let roots = quadratic(1.0, -1e8, 1.0);

        let [small, large] = roots.as_slice() else { panic!("expected two roots, got {roots:?}") };
        assert!((small * large - 1.0).abs() < 1e-15);
        assert!((small - 1e-8).abs() < 1e-23);
    }

    #[test]
    fn quartic_finds_four_simple_roots() {
        // (x − 1)(x − 2)(x − 3)(x − 4)
        let roots = quartic([24.0, -50.0, 35.0, -10.0, 1.0]);

        assert!(close(roots.as_slice(), &[1.0, 2.0, 3.0, 4.0], 1e-12), "{roots:?}");
    }

    #[test]
    fn quartic_without_real_roots_returns_none() {
        assert!(quartic([1.0, 0.0, 0.0, 0.0, 1.0]).is_empty());
    }

    #[test]
    fn quartic_with_a_vanishing_leading_coefficient_is_a_cubic() {
        // (x + 1)(x − 2)(x − 5) = x³ − 6x² + 3x + 10
        let roots = quartic([10.0, 3.0, -6.0, 1.0, 0.0]);

        assert!(close(roots.as_slice(), &[-1.0, 2.0, 5.0], 1e-12), "{roots:?}");
    }

    #[test]
    fn quartic_handles_roots_of_very_different_size() {
        // (x − 1e-3)(x − 1)(x − 1e3)(x + 1e3) = x⁴ − 1.001x³ − 999999.999x² + 1001000x − 1000
        let roots = quartic([-1e3, 1_001_000.0, -999_999.999, -1.001, 1.0]);

        assert!(close(&roots.as_slice()[..1], &[-1e3], 1e-9), "{roots:?}");
        assert!(close(&roots.as_slice()[1..3], &[1e-3, 1.0], 1e-12), "{roots:?}");
        assert!(close(&roots.as_slice()[3..], &[1e3], 1e-9), "{roots:?}");
    }

    #[test]
    fn a_double_root_is_reported_twice_or_not_at_all() {
        // (x − 1)²(x² + 1): round-off decides between a real pair and a complex pair,
        // but the parity of the count must not change.
        let roots = quartic([1.0, -2.0, 2.0, -2.0, 1.0]);

        assert!(roots.is_empty() || roots.len() == 2, "{roots:?}");
        assert!(roots.as_slice().iter().all(|root| (root - 1.0).abs() < 1e-6), "{roots:?}");
    }

    #[test]
    fn cubic_finds_three_simple_roots() {
        // (x + 1)(x − 2)(x − 5) = x³ − 6x² + 3x + 10
        let roots = cubic([10.0, 3.0, -6.0, 1.0]);

        assert!(close(roots.as_slice(), &[-1.0, 2.0, 5.0], 1e-13), "{roots:?}");
    }

    #[test]
    fn cubic_with_one_real_root_returns_only_that_root() {
        // (x − 1)(x² + x + 1) = x³ − 1
        let roots = cubic([-1.0, 0.0, 0.0, 1.0]);

        assert!(close(roots.as_slice(), &[1.0], 1e-15), "{roots:?}");
    }

    #[test]
    fn a_triple_root_keeps_an_odd_count() {
        // (x − 2)³
        let roots = cubic([-8.0, 12.0, -6.0, 1.0]);

        assert!(roots.len() == 1 || roots.len() == 3, "{roots:?}");
        assert!(roots.as_slice().iter().all(|root| (root - 2.0).abs() < 1e-4), "{roots:?}");
    }

    #[test]
    fn harmonic_solves_a_plain_cosine_equation() {
        // cos θ = 1/2
        let roots = harmonic([-0.5, 1.0, 0.0, 0.0, 0.0]);

        assert!(close(roots.as_slice(), &[-FRAC_PI_3, FRAC_PI_3], 1e-14), "{roots:?}");
    }

    #[test]
    fn harmonic_finds_roots_at_zero_and_at_the_half_turn_together() {
        // sin θ = 0: the half turn is where a fixed half-angle substitution would lose a root.
        let roots = harmonic([0.0, 0.0, 1.0, 0.0, 0.0]);

        assert!(close(roots.as_slice(), &[0.0, PI], 1e-14), "{roots:?}");
    }

    #[test]
    fn harmonic_finds_four_roots_of_a_double_angle_equation() {
        // cos 2θ = 0
        let roots = harmonic([0.0, 0.0, 0.0, 1.0, 0.0]);

        assert!(
            close(roots.as_slice(), &[-3.0 * FRAC_PI_4, -FRAC_PI_4, FRAC_PI_4, 3.0 * FRAC_PI_4], 1e-14),
            "{roots:?}"
        );
    }

    #[test]
    fn harmonic_roots_of_a_general_equation_leave_no_residual() {
        let [a0, a1, b1, a2, b2] = [0.3, -1.1, 0.7, 0.9, -0.4];

        let roots = harmonic([a0, a1, b1, a2, b2]);

        assert_eq!(roots.len(), 4, "{roots:?}");
        for angle in roots.as_slice() {
            let residual = a0
                + a1 * angle.cos()
                + b1 * angle.sin()
                + a2 * (2.0 * angle).cos()
                + b2 * (2.0 * angle).sin();
            assert!(residual.abs() < 1e-14, "residual {residual} at {angle}");
        }
    }

    #[test]
    fn harmonic_of_a_constant_has_no_roots() {
        assert!(harmonic([1.0, 0.0, 0.0, 0.0, 0.0]).is_empty());
        assert!(harmonic([0.0; 5]).is_empty());
    }

    #[test]
    fn wrap_brings_angles_into_the_half_open_turn() {
        assert!((wrap(3.0 * PI) - PI).abs() < 1e-15);
        assert!((wrap(-PI) - PI).abs() < 1e-15);
        assert!((wrap(2.5 * PI) - 0.5 * PI).abs() < 1e-15);
    }
}
