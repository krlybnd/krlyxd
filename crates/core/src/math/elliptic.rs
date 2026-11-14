//! Carlson's symmetric elliptic integrals, by duplication.
//!
//! Each step of the duplication theorem moves the three arguments together by a factor of
//! four while keeping the integral; once they agree to the stated tolerance, a fifth-order
//! series finishes it. The truncation error is then below the round-off of a double
//! (Carlson 1995; the constants as in Numerical Recipes, third edition).

/// Agreement of the arguments at which the series of `rf` takes over; its error is of the
/// order of this to the sixth power.
const RF_TOLERANCE: f64 = 0.0025;
/// The same for `rd`.
const RD_TOLERANCE: f64 = 0.0015;
/// Duplication steps never needed for finite arguments of the same order; a bound on work.
const STEPS: usize = 200;

/// `R_F(x, y, z) = ½∫₀^∞ dt / √((t+x)(t+y)(t+z))`, for arguments `≥ 0` of which at most one is 0.
pub fn rf(x: f64, y: f64, z: f64) -> f64 {
    let (mut x, mut y, mut z) = (x, y, z);
    let (mut dx, mut dy, mut dz, mut mean) = (0.0, 0.0, 0.0, 0.0);
    for _ in 0..STEPS {
        let (sx, sy, sz) = (x.sqrt(), y.sqrt(), z.sqrt());
        let lambda = sx * (sy + sz) + sy * sz;
        x = 0.25 * (x + lambda);
        y = 0.25 * (y + lambda);
        z = 0.25 * (z + lambda);
        mean = (x + y + z) / 3.0;
        (dx, dy, dz) = ((mean - x) / mean, (mean - y) / mean, (mean - z) / mean);
        if dx.abs().max(dy.abs()).max(dz.abs()) <= RF_TOLERANCE {
            break;
        }
    }
    let e2 = dx * dy - dz * dz;
    let e3 = dx * dy * dz;
    (1.0 + (e2 / 24.0 - 0.1 - 3.0 / 44.0 * e3) * e2 + e3 / 14.0) / mean.sqrt()
}

/// `R_D(x, y, z) = 3/2·∫₀^∞ dt / ((t+z)·√((t+x)(t+y)(t+z)))`, for `x, y ≥ 0` not both 0 and `z > 0`.
pub fn rd(x: f64, y: f64, z: f64) -> f64 {
    let (mut x, mut y, mut z) = (x, y, z);
    let (mut sum, mut factor) = (0.0, 1.0);
    let (mut dx, mut dy, mut dz, mut mean) = (0.0, 0.0, 0.0, 0.0);
    for _ in 0..STEPS {
        let (sx, sy, sz) = (x.sqrt(), y.sqrt(), z.sqrt());
        let lambda = sx * (sy + sz) + sy * sz;
        sum += factor / (sz * (z + lambda));
        factor *= 0.25;
        x = 0.25 * (x + lambda);
        y = 0.25 * (y + lambda);
        z = 0.25 * (z + lambda);
        mean = 0.2 * (x + y + 3.0 * z);
        (dx, dy, dz) = ((mean - x) / mean, (mean - y) / mean, (mean - z) / mean);
        if dx.abs().max(dy.abs()).max(dz.abs()) <= RD_TOLERANCE {
            break;
        }
    }
    let (c1, c2, c3, c4) = (3.0 / 14.0, 1.0 / 6.0, 9.0 / 22.0, 3.0 / 26.0);
    let (c5, c6) = (0.25 * c3, 1.5 * c4);
    let ea = dx * dy;
    let eb = dz * dz;
    let ec = ea - eb;
    let ed = ea - 6.0 * eb;
    let ee = ed + ec + ec;
    3.0 * sum
        + factor
            * (1.0 + ed * (-c1 + c5 * ed - c6 * dz * ee) + dz * (c2 * ee + dz * (-c3 * ec + dz * c4 * ea)))
            / (mean * mean.sqrt())
}

/// `R_G(x, y, z)`, the symmetric integral of the second kind, for arguments `≥ 0`:
/// `2·R_G = z·R_F − (x − z)(y − z)·R_D / 3 + √(x·y / z)` with `z` the middle argument,
/// which keeps the second term from cancelling the first (DLMF 19.21.10).
pub fn rg(x: f64, y: f64, z: f64) -> f64 {
    let mut sorted = [x, y, z];
    sorted.sort_by(f64::total_cmp);
    let [low, middle, high] = sorted;
    if middle == 0.0 {
        return 0.5 * high.sqrt();
    }
    0.5 * (middle * rf(low, high, middle) - (low - middle) * (high - middle) * rd(low, high, middle) / 3.0
        + (low * high / middle).sqrt())
}

#[cfg(test)]
mod tests {
    use super::{rd, rf, rg};
    use std::f64::consts::PI;

    #[test]
    fn the_integrals_have_their_known_values() {
        // DLMF 19.20: R_F(x,x,x) = 1/√x, R_D(x,x,x) = x^(−3/2), R_G(x,x,x) = √x,
        // R_F(1,2,0), R_D(0,2,1) as in Carlson (1995), and R_G(0,y,y) = π·√y / 4.
        assert!((rf(4.0, 4.0, 4.0) - 0.5).abs() < 1e-15);
        assert!((rd(4.0, 4.0, 4.0) - 0.125).abs() < 1e-15);
        assert!((rg(9.0, 9.0, 9.0) - 3.0).abs() < 1e-15);
        assert!((rf(1.0, 2.0, 0.0) - 1.311_028_777_146_059_9).abs() < 1e-14);
        assert!((rd(0.0, 2.0, 1.0) - 1.797_210_352_103_388_3).abs() < 1e-14);
        assert!((rg(0.0, 16.0, 16.0) - PI).abs() < 1e-14);
    }
}
