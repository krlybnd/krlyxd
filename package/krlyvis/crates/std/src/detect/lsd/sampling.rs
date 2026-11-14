//! Scaling an image down through a Gaussian filter, one axis at a time.

use krly3d_core::Error;
use krlyvis_kernel::Gray;

/// How many deviations the filter reaches: the weights it leaves out are below 10^-3.
const REACH: f64 = 3.0;

/// The image scaled by `scale` (at most 1). The filter's deviation is `sigma_scale /
/// scale` pixels of the original image.
pub(super) fn gaussian(image: &Gray, scale: f64, sigma_scale: f64) -> Result<Gray, Error> {
    let sigma = sigma_scale / scale;
    let (width, height) = image.size();
    let (out_width, out_height) = (scaled(width, scale), scaled(height, scale));
    let columns = taps(width, out_width, scale, sigma);
    let rows = taps(height, out_height, scale, sigma);
    let mut across = vec![0.0; out_width * height];
    for y in 0..height {
        for (x, weights) in columns.iter().enumerate() {
            across[y * out_width + x] =
                weights.iter().map(|&(column, weight)| image.at(column, y) * weight).sum();
        }
    }
    let mut values = vec![0.0; out_width * out_height];
    for (y, weights) in rows.iter().enumerate() {
        for x in 0..out_width {
            values[y * out_width + x] =
                weights.iter().map(|&(row, weight)| across[row * out_width + x] * weight).sum();
        }
    }
    Gray::new(out_width, out_height, values)
}

fn scaled(size: usize, scale: f64) -> usize {
    (size as f64 * scale).ceil() as usize
}

/// For each output pixel, the input pixels it is made of and their weights. The centre
/// of output pixel `o` lies at `(o + 0.5) / scale` in the input; beyond the border the
/// image is mirrored.
fn taps(size: usize, out_size: usize, scale: f64, sigma: f64) -> Vec<Vec<(usize, f64)>> {
    let half = (sigma * (2.0 * REACH * 10f64.ln()).sqrt()).ceil() as i64;
    (0..out_size)
        .map(|out| {
            let at = (out as f64 + 0.5) / scale - 0.5;
            let nearest = (at + 0.5).floor() as i64;
            let mut weights: Vec<(usize, f64)> = (-half..=half)
                .map(|offset| {
                    let distance = (nearest + offset) as f64 - at;
                    (mirror(nearest + offset, size), (-0.5 * (distance / sigma).powi(2)).exp())
                })
                .collect();
            let total: f64 = weights.iter().map(|&(_, weight)| weight).sum();
            for (_, weight) in &mut weights {
                *weight /= total;
            }
            weights
        })
        .collect()
}

/// The pixel an index outside `0..size` stands for when the image is mirrored at its
/// borders.
fn mirror(index: i64, size: usize) -> usize {
    let size = size as i64;
    let folded = index.rem_euclid(2 * size);
    (if folded >= size { 2 * size - 1 - folded } else { folded }) as usize
}

#[cfg(test)]
mod tests {
    use super::{gaussian, mirror};
    use krlyvis_kernel::Gray;

    #[test]
    fn indices_outside_the_image_are_mirrored() {
        assert_eq!(mirror(-1, 5), 0);
        assert_eq!(mirror(-2, 5), 1);
        assert_eq!(mirror(5, 5), 4);
        assert_eq!(mirror(12, 5), 2);
    }

    #[test]
    fn a_flat_image_stays_flat_and_shrinks_by_the_scale() {
        let image = Gray::new(10, 5, vec![80.0; 50]).unwrap();

        let scaled = gaussian(&image, 0.8, 0.6).unwrap();

        assert_eq!(scaled.size(), (8, 4));
        assert!(scaled.values().iter().all(|&value| (value - 80.0).abs() < 1e-9));
    }

    #[test]
    fn a_step_stays_where_it_was() {
        // Dark left of x = 50, bright right of it; the mean of a row keeps the step's place.
        let values = (0..100 * 4).map(|index| if index % 100 < 50 { 0.0 } else { 100.0 }).collect();
        let image = Gray::new(100, 4, values).unwrap();

        let scaled = gaussian(&image, 0.8, 0.6).unwrap();

        let bright: f64 = (0..scaled.width()).map(|x| scaled.at(x, 1)).sum::<f64>() / 100.0;
        let step = scaled.width() as f64 - bright;
        assert!((step - 40.0).abs() < 0.05, "step at {step}");
    }
}
