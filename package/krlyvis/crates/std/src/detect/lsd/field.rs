//! The level-line field: for every pixel, the direction square to the gradient and the
//! size of the gradient.

use krlyvis_kernel::Gray;

pub(super) struct Field {
    width: usize,
    height: usize,
    /// `None` where the gradient is too weak to trust its direction.
    angles: Vec<Option<f64>>,
    norms: Vec<f64>,
    /// The pixels with a direction, strongest gradient first.
    order: Vec<usize>,
}

impl Field {
    /// The gradient is taken over the 2 x 2 block whose top-left pixel it is stored at;
    /// the last row and column have none. Below `threshold` a direction is not trusted.
    pub(super) fn new(image: &Gray, threshold: f64) -> Self {
        let (width, height) = image.size();
        let mut angles = vec![None; width * height];
        let mut norms = vec![0.0; width * height];
        for y in 0..height.saturating_sub(1) {
            for x in 0..width.saturating_sub(1) {
                let rising = image.at(x + 1, y + 1) - image.at(x, y);
                let falling = image.at(x + 1, y) - image.at(x, y + 1);
                let (gx, gy) = (rising + falling, rising - falling);
                let norm = ((gx * gx + gy * gy) / 4.0).sqrt();
                let index = y * width + x;
                norms[index] = norm;
                if norm > threshold {
                    angles[index] = Some(gx.atan2(-gy));
                }
            }
        }
        let mut order: Vec<usize> = (0..width * height).filter(|&index| angles[index].is_some()).collect();
        order.sort_by(|&a, &b| norms[b].total_cmp(&norms[a]));
        Self { width, height, angles, norms, order }
    }

    pub(super) fn width(&self) -> usize {
        self.width
    }

    pub(super) fn height(&self) -> usize {
        self.height
    }

    pub(super) fn len(&self) -> usize {
        self.angles.len()
    }

    pub(super) fn order(&self) -> &[usize] {
        &self.order
    }

    pub(super) fn angle(&self, index: usize) -> Option<f64> {
        self.angles[index]
    }

    pub(super) fn norm(&self, index: usize) -> f64 {
        self.norms[index]
    }

    pub(super) fn index(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }

    /// The column and row of a pixel, as numbers.
    pub(super) fn point(&self, index: usize) -> (f64, f64) {
        ((index % self.width) as f64, (index / self.width) as f64)
    }

    /// The up to eight pixels around one.
    pub(super) fn neighbours(&self, index: usize) -> impl Iterator<Item = usize> + '_ {
        let (x, y) = (index % self.width, index / self.width);
        let columns = x.saturating_sub(1)..=(x + 1).min(self.width - 1);
        let rows = y.saturating_sub(1)..=(y + 1).min(self.height - 1);
        rows.flat_map(move |row| columns.clone().map(move |column| row * self.width + column))
            .filter(move |&other| other != index)
    }
}

#[cfg(test)]
mod tests {
    use super::Field;
    use krlyvis_kernel::Gray;

    #[test]
    fn a_vertical_step_has_level_lines_running_down_and_no_direction_in_flat_parts() {
        // Dark on the left, bright on the right: the gradient points along +x.
        let values = (0..4 * 3).map(|index| if index % 4 < 2 { 0.0 } else { 100.0 }).collect();
        let field = Field::new(&Gray::new(4, 3, values).unwrap(), 1.0);

        let angle = field.angle(field.index(1, 0)).unwrap();

        assert!((angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert_eq!(field.angle(field.index(0, 0)), None);
        assert_eq!(field.angle(field.index(3, 0)), None, "the last column has no gradient");
        assert_eq!(field.order(), [field.index(1, 0), field.index(1, 1)]);
    }

    #[test]
    fn neighbours_stay_inside_the_image() {
        let field = Field::new(&Gray::new(3, 3, vec![0.0; 9]).unwrap(), 1.0);

        let corner: Vec<usize> = field.neighbours(0).collect();
        let middle = field.neighbours(4).count();

        assert_eq!(corner, [1, 3, 4]);
        assert_eq!(middle, 8);
    }
}
