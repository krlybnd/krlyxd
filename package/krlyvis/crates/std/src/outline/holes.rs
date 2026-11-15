//! Pixel-edge loops for one selected component, retaining its holes.
//! Optional bounded simplification keeps the original loops if topology would change.
//! Corner-touching boundaries are rejected rather than inventing topology.
use super::mask::{subject_with_holes_controlled, Mask};
use super::Checkpoints;
use krly3d_core::{Attempt, Error, Id};
use krlyvis_kernel::ports::outline::{Outline, OutlineRequest, Shape};
use krlyvis_kernel::{Context, Pixel, Plugin};
use std::collections::BTreeMap;

type Corner = (i64, i64);
pub struct TraceHoles;
impl Plugin<Outline> for TraceHoles {
    fn id(&self) -> Id {
        Id::new("trace-holes")
    }
    fn call(&self, cx: &Context, request: &OutlineRequest) -> Attempt<Shape> {
        match outlined(cx, request) {
            Ok(Some(shape)) => Attempt::Done(shape),
            Ok(None) => Attempt::declined("nothing in the picture stands apart from its background"),
            Err(error) => Attempt::Failed(error),
        }
    }
}
fn outlined(cx: &Context, request: &OutlineRequest) -> Result<Option<Shape>, Error> {
    cx.check_control()?;
    if !request.simplify.is_finite() || request.simplify < 0.0 {
        return Err(Error::input("simplify must be finite and nonnegative"));
    }
    let (w, h) = request.image.size();
    let Some(mask) = subject_with_holes_controlled(
        cx,
        &request.image,
        request.region.unwrap_or([0, 0, w, h]),
        request.seed,
        request.threshold,
    )?
    else {
        return Ok(None);
    };
    let polygons = loops(cx, &mask)?;
    let polygons = super::simplify_holes::simplified(cx, polygons, request.simplify)?;
    cx.check_control()?;
    Ok(Some(Shape { polygons, pixels: mask.pixels, threshold: mask.threshold, dark: mask.dark }))
}
fn loops(cx: &Context, mask: &Mask) -> Result<Vec<Vec<Pixel>>, Error> {
    let mut work = Checkpoints::new(Some(cx))?;
    let mut edges = pixel_edges(mask, &mut work)?;
    work.check()?;
    let mut polygons = Vec::new();
    while let Some((&start, _)) = edges.first_key_value() {
        work.check()?;
        let mut at = start;
        let mut points = Vec::new();
        loop {
            work.step()?;
            points.push(at);
            at = edges.remove(&at).ok_or_else(|| Error::geometry("open pixel boundary"))?;
            if at == start {
                break;
            }
        }
        let n = points.len();
        let mut corners = Vec::new();
        for i in 0..n {
            work.step()?;
            let (a, b, c) = (points[(i + n - 1) % n], points[i], points[(i + 1) % n]);
            if (b.0 - a.0, b.1 - a.1) != (c.0 - b.0, c.1 - b.1) {
                corners.push(Pixel::new(b.0 as f64, b.1 as f64));
            }
        }
        // Calculate areas once with checkpoints; sorting only compares scalars.
        polygons.push((signed_area(&corners, &mut work)?, corners));
    }
    work.check()?;
    polygons.sort_by(|a, b| b.0.total_cmp(&a.0));
    work.check()?;
    if polygons.is_empty() || polygons[0].0 <= 0.0 {
        return Err(Error::geometry("expected one outer boundary and interior holes"));
    }
    let mut result = Vec::with_capacity(polygons.len());
    for (index, (area, polygon)) in polygons.into_iter().enumerate() {
        work.step()?;
        if index > 0 && area >= 0.0 {
            return Err(Error::geometry("expected one outer boundary and interior holes"));
        }
        result.push(polygon);
    }
    work.check()?;
    Ok(result)
}

fn pixel_edges(mask: &Mask, work: &mut Checkpoints<'_>) -> Result<BTreeMap<Corner, Corner>, Error> {
    let mut edges = BTreeMap::<Corner, Corner>::new();
    for y in 0..mask.height as i64 {
        for x in 0..mask.width as i64 {
            work.step()?;
            if !mask.at(x, y) {
                continue;
            }
            // Subject on the right in y-down coordinates: outer positive, holes negative.
            for (outside, from, to) in [
                ((x, y - 1), (x, y), (x + 1, y)),
                ((x + 1, y), (x + 1, y), (x + 1, y + 1)),
                ((x, y + 1), (x + 1, y + 1), (x, y + 1)),
                ((x - 1, y), (x, y + 1), (x, y)),
            ] {
                if !mask.at(outside.0, outside.1) && edges.insert(from, to).is_some() {
                    return Err(Error::geometry(
                        "outline boundaries touch at a pixel corner; topology is ambiguous",
                    ));
                }
            }
        }
    }
    Ok(edges)
}
fn signed_area(points: &[Pixel], work: &mut Checkpoints<'_>) -> Result<f64, Error> {
    let mut sum = 0.0;
    for (a, b) in points.iter().zip(points.iter().cycle().skip(1)) {
        work.step()?;
        sum += a.x * b.y - b.x * a.y;
    }
    Ok(sum / 2.0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{loops, signed_area};
    use crate::outline::mask::Mask;
    use crate::outline::Checkpoints;
    use krlyvis_kernel::ports::outline::OutlineRequest;
    use krlyvis_kernel::Pixel;
    #[test]
    fn satisfies_shared_contract_and_legacy_trace_still_fills_holes() {
        use krlyvis_kernel::{
            ports::outline::{contract, outline},
            ports::sample::Canvas,
            VisionBuilder,
        };
        use std::sync::Arc;
        let mut builder = VisionBuilder::new();
        super::super::register(&mut builder);
        let cx = builder.context();
        contract::check(&cx, "trace-holes").unwrap();
        let mut canvas = Canvas::new((24, 24), 230.0);
        let rect = |lo: f64, hi: f64| {
            vec![Pixel::new(lo, lo), Pixel::new(hi, lo), Pixel::new(hi, hi), Pixel::new(lo, hi)]
        };
        canvas.fill(&rect(2.0, 22.0), 40.0);
        canvas.fill(&rect(8.0, 16.0), 230.0);
        let request = OutlineRequest {
            image: Arc::new(canvas.image().unwrap()),
            region: None,
            seed: None,
            threshold: None,
            simplify: 1.0,
        };
        let holes = outline(&cx, "trace-holes", &request).into_result().unwrap();
        let legacy = outline(&cx, "trace", &request).into_result().unwrap();
        assert_eq!((holes.polygons.len(), holes.pixels), (2, 336));
        assert_eq!((legacy.polygons.len(), legacy.pixels), (1, 400));
    }
    #[test]
    fn preserves_multiple_holes_and_exact_material_area() {
        let w = 12;
        let h = 10;
        let inside: Vec<_> = (0..h)
            .flat_map(|y| {
                (0..w).map(move |x| {
                    (1..11).contains(&x)
                        && (1..9).contains(&y)
                        && !((3..5).contains(&x) && (3..5).contains(&y))
                        && !((7..9).contains(&x) && (3..7).contains(&y))
                })
            })
            .collect();
        let pixels = inside.iter().filter(|v| **v).count();
        let mask = Mask { width: w, height: h, inside, pixels, threshold: 0.5, dark: true };
        let cx = krlyvis_kernel::VisionBuilder::new().context();
        let polygons = loops(&cx, &mask).unwrap();
        assert_eq!(polygons.len(), 3);
        let mut work = Checkpoints::new(None).unwrap();
        assert_eq!(polygons.iter().map(|p| signed_area(p, &mut work).unwrap()).sum::<f64>(), pixels as f64);
        assert!(polygons.iter().all(|p| p.len() == 4));
    }
    #[test]
    fn rejects_diagonal_boundary_contacts() {
        let mask = Mask {
            width: 2,
            height: 2,
            inside: vec![true, false, false, true],
            pixels: 2,
            threshold: 0.5,
            dark: true,
        };
        let cx = krlyvis_kernel::VisionBuilder::new().context();
        assert!(loops(&cx, &mask).is_err());
    }

    #[test]
    fn full_resolution_round_hole_loses_stairs_without_losing_topology() {
        let width = 512;
        let inside: Vec<_> = (0..width)
            .flat_map(|y| {
                (0..width).map(move |x| {
                    let dx = x as f64 - 255.5;
                    let dy = y as f64 - 255.5;
                    let radius = dx.hypot(dy);
                    (80.0..200.0).contains(&radius)
                })
            })
            .collect();
        let mask = Mask {
            width,
            height: width,
            pixels: inside.iter().filter(|v| **v).count(),
            inside,
            threshold: 128.0,
            dark: true,
        };
        let cx = krlyvis_kernel::VisionBuilder::new().context();
        let exact = loops(&cx, &mask).unwrap();
        assert!(exact.iter().map(Vec::len).sum::<usize>() > 256);
        let smooth = super::super::simplify_holes::simplified(&cx, exact.clone(), 1.0).unwrap();
        assert_eq!(smooth.len(), 2);
        assert!(smooth.iter().map(Vec::len).sum::<usize>() < 256);
        let mut work = Checkpoints::new(None).unwrap();
        assert!(signed_area(&smooth[0], &mut work).unwrap() > 0.0);
        assert!(signed_area(&smooth[1], &mut work).unwrap() < 0.0);
        let area = smooth.iter().map(|p| signed_area(p, &mut work).unwrap()).sum::<f64>();
        assert!((area / mask.pixels as f64 - 1.0).abs() < 0.01);
    }

    #[test]
    fn checks_inside_edge_scan_and_a_single_long_boundary_walk() {
        use crate::outline::control_tests;
        use krly3d_core::ErrorKind;
        use std::sync::atomic::Ordering;

        let width = 2048;
        let mut inside = vec![false; width * 3];
        inside[width + 1..2 * width - 1].fill(true);
        let mask = Mask { width, height: 3, inside, pixels: width - 2, threshold: 128.0, dark: true };
        // Budget 2 expires at scan pixel 1025. Budget 10 passes the entire
        // 6144-pixel scan and enters the 4094-edge loop, stopping at edge 1025.
        // Boundary reduction cannot begin because this loop is still open.
        for allowed in [2, 10] {
            let (cx, clock) = control_tests::context(allowed);
            let error = loops(&cx, &mask).unwrap_err();
            assert_eq!(error.kind, ErrorKind::DeadlineExceeded);
            assert_eq!(clock.checks.load(Ordering::Relaxed), allowed + 1);
        }
        let base = krlyvis_kernel::VisionBuilder::new().context();
        let result = loops(&base, &mask).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 4);
        let mut work = Checkpoints::new(None).unwrap();
        assert_eq!(signed_area(&result[0], &mut work).unwrap(), mask.pixels as f64);
    }
}
