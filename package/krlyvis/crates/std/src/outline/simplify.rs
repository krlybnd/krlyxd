//! Douglas–Peucker on a closed polygon: the fewest corners that stay within a distance of
//! the polygon traced.

use krlyvis_kernel::Pixel;

/// The polygon with the corners dropped that lie within `tolerance` of the line between
/// the corners kept around them.
pub fn simplified(polygon: &[Pixel], tolerance: f64) -> Vec<Pixel> {
    if polygon.len() < 4 || tolerance <= 0.0 {
        return polygon.to_vec();
    }
    // A closed polygon is split at its first point and the point farthest from it.
    let far = (1..polygon.len())
        .max_by(|a, b| (polygon[*a] - polygon[0]).norm().total_cmp(&(polygon[*b] - polygon[0]).norm()))
        .unwrap_or(polygon.len() / 2);
    let mut keep = vec![false; polygon.len()];
    keep[0] = true;
    keep[far] = true;
    mark(polygon, 0, far, tolerance, &mut keep);
    let wrapped: Vec<Pixel> = polygon[far..].iter().chain(std::iter::once(&polygon[0])).copied().collect();
    let mut tail = vec![false; wrapped.len()];
    mark(&wrapped, 0, wrapped.len() - 1, tolerance, &mut tail);
    for (offset, kept) in tail.iter().enumerate().take(wrapped.len() - 1).skip(1) {
        keep[far + offset] |= *kept;
    }
    polygon.iter().zip(keep).filter(|(_, kept)| *kept).map(|(point, _)| *point).collect()
}

fn mark(points: &[Pixel], first: usize, last: usize, tolerance: f64, keep: &mut [bool]) {
    if last <= first + 1 {
        return;
    }
    let (a, b) = (points[first], points[last]);
    let chord = b - a;
    let length = chord.norm();
    let distance = |point: &Pixel| {
        let along = point - a;
        if length == 0.0 {
            along.norm()
        } else {
            (chord.x * along.y - chord.y * along.x).abs() / length
        }
    };
    let (index, farthest) = (first + 1..last)
        .map(|index| (index, distance(&points[index])))
        .max_by(|p, q| p.1.total_cmp(&q.1))
        .unwrap_or((first, 0.0));
    if farthest > tolerance {
        keep[index] = true;
        mark(points, first, index, tolerance, keep);
        mark(points, index, last, tolerance, keep);
    }
}

#[cfg(test)]
mod tests {
    use super::simplified;
    use krlyvis_kernel::Pixel;

    #[test]
    fn the_points_along_the_sides_of_a_square_are_dropped() {
        let mut square = Vec::new();
        for k in 0..10 {
            square.push(Pixel::new(k as f64, 0.0));
        }
        for k in 0..10 {
            square.push(Pixel::new(10.0, k as f64));
        }
        for k in 0..10 {
            square.push(Pixel::new(10.0 - k as f64, 10.0));
        }
        for k in 0..10 {
            square.push(Pixel::new(0.0, 10.0 - k as f64));
        }

        let corners = simplified(&square, 0.5);

        assert_eq!(corners.len(), 4, "{corners:?}");
    }
}
