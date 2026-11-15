//! Controlled closed-loop Douglas–Peucker, with a conservative topology gate.
//! The tolerance is in source pixels, never a change to image resolution or scale.
use super::Checkpoints;
use krly3d_core::Error;
use krlyvis_kernel::{Context, Pixel};

pub(super) fn simplified(
    cx: &Context,
    original: Vec<Vec<Pixel>>,
    tolerance: f64,
) -> Result<Vec<Vec<Pixel>>, Error> {
    if tolerance == 0.0 {
        return Ok(original);
    }
    let mut work = Checkpoints::new(Some(cx))?;
    let mut candidate = Vec::with_capacity(original.len());
    for polygon in &original {
        candidate.push(reduce(polygon, tolerance, &mut work)?);
    }
    // Refuse an unbounded quadratic topology pass; retaining the exact trace is
    // always within tolerance. Downstream admission still enforces its own cap.
    let count = candidate.iter().map(Vec::len).sum::<usize>();
    if count > 2048 || !valid(&candidate, &original, &mut work)? {
        return Ok(original);
    }
    work.check()?;
    Ok(candidate)
}

fn reduce(points: &[Pixel], tolerance: f64, work: &mut Checkpoints<'_>) -> Result<Vec<Pixel>, Error> {
    if points.len() < 4 {
        return Ok(points.to_vec());
    }
    let mut far = 1;
    let mut distance = 0.0;
    for i in 1..points.len() {
        work.step()?;
        let next = (points[i] - points[0]).norm_squared();
        if next > distance {
            (far, distance) = (i, next);
        }
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[far] = true;
    let mut pending = vec![(0, far), (far, points.len())];
    while let Some((first, last)) = pending.pop() {
        work.step()?;
        let (a, b) = (points[first], points[last % points.len()]);
        let mut max = tolerance;
        let mut split = None;
        for (i, point) in points.iter().enumerate().take(last).skip(first + 1) {
            work.step()?;
            let d = segment_distance(*point, a, b);
            if d > max {
                max = d;
                split = Some(i);
            }
        }
        if let Some(i) = split {
            keep[i] = true;
            pending.push((first, i));
            pending.push((i, last));
        }
    }
    let mut result = Vec::new();
    for (point, kept) in points.iter().zip(keep) {
        work.step()?;
        if kept {
            result.push(*point);
        }
    }
    Ok(result)
}

fn segment_distance(p: Pixel, a: Pixel, b: Pixel) -> f64 {
    let d = b - a;
    let length = d.norm_squared();
    if length == 0.0 {
        return (p - a).norm();
    }
    let t = ((p - a).dot(&d) / length).clamp(0.0, 1.0);
    (p - (a + d * t)).norm()
}

fn area(points: &[Pixel], work: &mut Checkpoints<'_>) -> Result<f64, Error> {
    let mut sum = 0.0;
    for i in 0..points.len() {
        work.step()?;
        let (a, b) = (points[i], points[(i + 1) % points.len()]);
        sum += a.x * b.y - a.y * b.x;
    }
    Ok(sum / 2.0)
}

fn turn(a: Pixel, b: Pixel, c: Pixel) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

fn on_segment(a: Pixel, b: Pixel, p: Pixel) -> bool {
    turn(a, b, p) == 0.0
        && p.x >= a.x.min(b.x)
        && p.x <= a.x.max(b.x)
        && p.y >= a.y.min(b.y)
        && p.y <= a.y.max(b.y)
}

fn intersects(a: Pixel, b: Pixel, c: Pixel, d: Pixel) -> bool {
    let (ab_c, ab_d, cd_a, cd_b) = (turn(a, b, c), turn(a, b, d), turn(c, d, a), turn(c, d, b));
    (ab_c.signum() != ab_d.signum()
        && ab_c != 0.0
        && ab_d != 0.0
        && cd_a.signum() != cd_b.signum()
        && cd_a != 0.0
        && cd_b != 0.0)
        || on_segment(a, b, c)
        || on_segment(a, b, d)
        || on_segment(c, d, a)
        || on_segment(c, d, b)
}

fn contains(loop_: &[Pixel], point: Pixel, work: &mut Checkpoints<'_>) -> Result<bool, Error> {
    let mut inside = false;
    for i in 0..loop_.len() {
        work.step()?;
        let (a, b) = (loop_[i], loop_[(i + 1) % loop_.len()]);
        if (a.y > point.y) != (b.y > point.y) && point.x < (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    Ok(inside)
}

fn valid(
    candidate: &[Vec<Pixel>],
    original: &[Vec<Pixel>],
    work: &mut Checkpoints<'_>,
) -> Result<bool, Error> {
    for (k, polygon) in candidate.iter().enumerate() {
        if polygon.len() < 3 {
            return Ok(false);
        }
        let signed = area(polygon, work)?;
        if signed == 0.0 || signed.signum() != area(&original[k], work)?.signum() {
            return Ok(false);
        }
        for i in 0..polygon.len() {
            work.step()?;
            let (a, b, c) = (polygon[i], polygon[(i + 1) % polygon.len()], polygon[(i + 2) % polygon.len()]);
            if a == b || (turn(a, b, c) == 0.0 && (b - a).dot(&(c - b)) < 0.0) {
                return Ok(false);
            }
            for j in i + 2..polygon.len() {
                work.step()?;
                if i == 0 && j + 1 == polygon.len() {
                    continue;
                }
                if intersects(a, b, polygon[j], polygon[(j + 1) % polygon.len()]) {
                    return Ok(false);
                }
            }
            for other in &candidate[..k] {
                for j in 0..other.len() {
                    work.step()?;
                    if intersects(a, b, other[j], other[(j + 1) % other.len()]) {
                        return Ok(false);
                    }
                }
            }
        }
        if k > 0 {
            if !contains(&candidate[0], polygon[0], work)? {
                return Ok(false);
            }
            for other in &candidate[1..k] {
                if contains(other, polygon[0], work)? || contains(polygon, other[0], work)? {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn polygon(points: &[[f64; 2]]) -> Vec<Pixel> {
        points.iter().map(|p| Pixel::new(p[0], p[1])).collect()
    }

    #[test]
    fn removes_raster_stairs_with_bounded_distance_and_preserves_hole() -> Result<(), Error> {
        let cx = krlyvis_kernel::VisionBuilder::new().context();
        let mut outer = vec![Pixel::new(0.0, 0.0), Pixel::new(100.0, 0.0)];
        for i in (1..=100).rev() {
            outer.push(Pixel::new(i as f64, (101 - i) as f64));
            outer.push(Pixel::new((i - 1) as f64, (101 - i) as f64));
        }
        let hole = polygon(&[[10., 10.], [10., 20.], [20., 20.], [20., 10.]]);
        let original = vec![outer, hole];
        assert_eq!(simplified(&cx, original.clone(), 0.)?, original);
        let result = simplified(&cx, original.clone(), 1.)?;
        assert_eq!(result.len(), 2);
        assert!(result[0].len() < 10);
        assert_eq!(result[1], original[1]);
        for (input, output) in original.iter().zip(&result) {
            for p in input {
                let distance = (0..output.len())
                    .map(|i| segment_distance(*p, output[i], output[(i + 1) % output.len()]))
                    .fold(f64::INFINITY, f64::min);
                assert!(distance <= 1.0);
            }
        }
        Ok(())
    }

    #[test]
    fn refuses_collapsed_holes_and_crossed_or_nested_loops() -> Result<(), Error> {
        let cx = krlyvis_kernel::VisionBuilder::new().context();
        let outer = polygon(&[[0., 0.], [20., 0.], [20., 20.], [0., 20.]]);
        let hole = polygon(&[[9., 9.], [9., 10.], [10., 10.], [10., 9.]]);
        let original = vec![outer.clone(), hole];
        assert_eq!(simplified(&cx, original.clone(), 2.)?, original);
        let mut work = Checkpoints::new(None)?;
        let crossing = polygon(&[[5., 5.], [5., 25.], [15., 25.], [15., 5.]]);
        assert!(!valid(&[outer.clone(), crossing], &original, &mut work)?);
        let bow = polygon(&[[0., 0.], [20., 20.], [20., 0.], [0., 20.]]);
        assert!(!valid(&[bow], &[outer], &mut work)?);
        Ok(())
    }

    #[test]
    fn simplification_observes_control_during_long_scan() {
        let points: Vec<_> = (0..4096).map(|i| Pixel::new(i as f64, (i % 2) as f64)).collect();
        let (cx, _) = super::super::control_tests::context(2);
        let result = simplified(&cx, vec![points], 1.);
        assert!(matches!(result, Err(error) if error.kind == krly3d_core::ErrorKind::DeadlineExceeded));
    }
}
