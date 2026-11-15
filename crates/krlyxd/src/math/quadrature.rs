//! Gauss–Legendre quadrature: the one table of nodes and weights in the workspace. Every
//! numerical integral (lengths, vector areas, contents of free-form faces) takes its nodes
//! from here.

/// Nodes on [−1, 1] and their weights, eight points: exact for polynomials of degree fifteen.
pub const GAUSS_LEGENDRE_8: [(f64, f64); 8] = [
    (-0.960_289_856_497_536_2, 0.101_228_536_290_376_26),
    (-0.796_666_477_413_626_7, 0.222_381_034_453_374_47),
    (-0.525_532_409_916_329, 0.313_706_645_877_887_3),
    (-0.183_434_642_495_649_8, 0.362_683_783_378_362),
    (0.183_434_642_495_649_8, 0.362_683_783_378_362),
    (0.525_532_409_916_329, 0.313_706_645_877_887_3),
    (0.796_666_477_413_626_7, 0.222_381_034_453_374_47),
    (0.960_289_856_497_536_2, 0.101_228_536_290_376_26),
];

/// The points and weights that integrate over `[from, to]` split into `pieces` equal parts,
/// eight points to each part: `∫f ≈ Σ weight·f(point)`.
pub fn nodes(from: f64, to: f64, pieces: usize) -> impl Iterator<Item = (f64, f64)> {
    let pieces = pieces.max(1);
    let step = (to - from) / pieces as f64;
    (0..pieces).flat_map(move |piece| {
        let middle = from + step * (piece as f64 + 0.5);
        GAUSS_LEGENDRE_8.iter().map(move |(node, weight)| (middle + 0.5 * step * node, weight * 0.5 * step))
    })
}

#[cfg(test)]
mod tests {
    use super::nodes;
    use std::f64::consts::PI;

    #[test]
    fn eight_points_integrate_a_polynomial_of_degree_fifteen_exactly() {
        let integral: f64 = nodes(-1.0, 2.0, 1).map(|(x, w)| w * x.powi(15)).sum();

        assert!((integral - (2.0_f64.powi(16) - 1.0) / 16.0).abs() < 1e-9);
    }

    #[test]
    fn pieces_make_a_smooth_integral_exact_to_round_off() {
        let integral: f64 = nodes(0.0, PI, 4).map(|(x, w)| w * x.sin()).sum();

        assert!((integral - 2.0).abs() < 1e-14);
    }
}
