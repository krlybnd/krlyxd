//! The number of false alarms: how many rectangles with at least `k` agreeing pixels out
//! of `n` an image of noise would hold, where each pixel agrees with chance `p`.
//!
//! It is the number of tests times the tail of the binomial distribution,
//! `NT * sum_{i >= k} C(n, i) p^i (1 - p)^(n - i)`, worked in logarithms.

use std::f64::consts::{LN_10, PI};

/// The relative error at which the tail of the binomial sum is cut off.
const TOLERANCE: f64 = 0.1;

/// Minus the decimal logarithm of the number of false alarms, `log_nt` being the decimal
/// logarithm of the number of tests.
pub(super) fn log_nfa(n: usize, k: usize, p: f64, log_nt: f64) -> f64 {
    if n == 0 || k == 0 {
        return -log_nt;
    }
    if n == k {
        return -log_nt - n as f64 * p.log10();
    }
    let (nf, kf) = (n as f64, k as f64);
    let odds = p / (1.0 - p);
    let log_first = log_gamma(nf + 1.0) - log_gamma(kf + 1.0) - log_gamma(nf - kf + 1.0)
        + kf * p.ln()
        + (nf - kf) * (1.0 - p).ln();
    let mut term = log_first.exp();
    if term < f64::MIN_POSITIVE {
        // The first term is too small to be represented, so it alone bounds the tail.
        return if kf > nf * p { -log_first / LN_10 - log_nt } else { -log_nt };
    }
    let mut tail = term;
    for i in (k + 1)..=n {
        let ratio = (n - i + 1) as f64 / i as f64;
        let factor = ratio * odds;
        term *= factor;
        tail += term;
        if ratio < 1.0 {
            // The remaining terms shrink at least geometrically: bound what is left.
            let rest = term * ((1.0 - factor.powi((n - i + 1) as i32)) / (1.0 - factor) - 1.0);
            if rest < TOLERANCE * (-tail.log10() - log_nt).abs() * tail {
                break;
            }
        }
    }
    -tail.log10() - log_nt
}

/// The natural logarithm of the gamma function, for positive `x`.
fn log_gamma(x: f64) -> f64 {
    if x > 15.0 {
        windschitl(x)
    } else {
        lanczos(x)
    }
}

/// Lanczos' approximation, good for small arguments.
fn lanczos(x: f64) -> f64 {
    const Q: [f64; 7] = [
        75_122.633_153_0,
        80_916.627_895_2,
        36_308.295_147_7,
        8_687.245_297_05,
        1_168.926_494_79,
        83.867_604_342_4,
        2.506_628_275_11,
    ];
    let mut a = (x + 0.5) * (x + 5.5).ln() - (x + 5.5);
    let mut b = 0.0;
    let mut power = 1.0;
    for (n, q) in Q.iter().enumerate() {
        a -= (x + n as f64).ln();
        b += q * power;
        power *= x;
    }
    a + b.ln()
}

/// Windschitl's approximation, good for large arguments.
fn windschitl(x: f64) -> f64 {
    0.5 * (2.0 * PI).ln() + (x - 0.5) * x.ln() - x
        + 0.5 * x * (x * (1.0 / x).sinh() + 1.0 / (810.0 * x.powi(6))).ln()
}

#[cfg(test)]
mod tests {
    use super::{log_gamma, log_nfa};

    #[test]
    fn log_gamma_matches_factorials_on_both_sides_of_the_switch() {
        let mut factorial: f64 = 1.0;
        for n in 1..30u32 {
            factorial *= f64::from(n);
            let found = log_gamma(f64::from(n) + 1.0);
            assert!((found - factorial.ln()).abs() < 1e-8 * factorial.ln().max(1.0), "{n}: {found}");
        }
    }

    #[test]
    fn the_tail_of_a_small_binomial_is_summed_exactly() {
        // P(X >= 3) for X ~ B(4, 0.5) is 5/16.
        let found = log_nfa(4, 3, 0.5, 0.0);

        assert!((found + (5.0f64 / 16.0).log10()).abs() < 1e-9);
    }

    #[test]
    fn more_agreeing_pixels_are_more_significant() {
        let weaker = log_nfa(100, 30, 0.125, 5.0);
        let stronger = log_nfa(100, 60, 0.125, 5.0);

        assert!(stronger > weaker);
        assert!(stronger > 0.0);
        assert!(log_nfa(100, 12, 0.125, 5.0) < 0.0, "chance agreement is not significant");
    }
}
