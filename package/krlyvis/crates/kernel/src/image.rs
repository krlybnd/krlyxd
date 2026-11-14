//! The picture an analysis starts from.

use krly3d_core::Error;

/// A point of an image in pixels. Pixel (column, row) covers the unit square whose corner
/// nearest the origin is (column, row), so its centre is at (column + 0.5, row + 0.5);
/// x grows to the right and y downward.
pub type Pixel = nalgebra::Point2<f64>;

/// A grey image, row after row, one brightness per pixel on the scale 0 to 255.
#[derive(Clone, Debug, PartialEq)]
pub struct Gray {
    width: usize,
    height: usize,
    values: Vec<f64>,
}

impl Gray {
    pub fn new(width: usize, height: usize, values: Vec<f64>) -> Result<Self, Error> {
        if width == 0 || height == 0 {
            return Err(Error::input("an image needs at least one pixel"));
        }
        if width.checked_mul(height) != Some(values.len()) {
            return Err(Error::input(format!("{} values given for {width}x{height} pixels", values.len())));
        }
        Ok(Self { width, height, values })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    pub fn values(&self) -> &[f64] {
        &self.values
    }

    /// The brightness of the pixel in `column` and `row`; both must lie inside the image.
    pub fn at(&self, column: usize, row: usize) -> f64 {
        self.values[row * self.width + column]
    }
}

/// The middle of an image of the given size.
pub fn centre((width, height): (usize, usize)) -> Pixel {
    Pixel::new(width as f64 / 2.0, height as f64 / 2.0)
}

/// The length of the diagonal of an image of the given size.
pub fn diagonal((width, height): (usize, usize)) -> f64 {
    (width as f64).hypot(height as f64)
}

#[cfg(test)]
mod tests {
    use super::{centre, diagonal, Gray, Pixel};

    #[test]
    fn values_are_read_row_after_row() {
        let image = Gray::new(3, 2, vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0]).unwrap();

        assert_eq!(image.at(2, 0), 2.0);
        assert_eq!(image.at(0, 1), 3.0);
    }

    #[test]
    fn a_wrong_number_of_values_is_refused() {
        assert!(Gray::new(3, 2, vec![0.0; 5]).is_err());
        assert!(Gray::new(0, 2, Vec::new()).is_err());
    }

    #[test]
    fn centre_and_diagonal_follow_the_size() {
        assert_eq!(centre((800, 600)), Pixel::new(400.0, 300.0));
        assert_eq!(diagonal((800, 600)), 1000.0);
    }
}
