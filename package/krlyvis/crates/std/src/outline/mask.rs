//! Which pixels are the subject: those on the far side of a threshold from the background,
//! one connected blob of them, its holes filled.

use super::Checkpoints;
use krly3d_core::Error;
use krlyvis_kernel::{Context, Gray, Pixel};
use std::collections::VecDeque;

/// The subject of a region of a picture, as a mask over the whole picture.
pub struct Mask {
    pub width: usize,
    pub height: usize,
    pub inside: Vec<bool>,
    pub pixels: usize,
    pub threshold: f64,
    pub dark: bool,
}

impl Mask {
    pub fn at(&self, x: i64, y: i64) -> bool {
        x >= 0
            && y >= 0
            && (x as usize) < self.width
            && (y as usize) < self.height
            && self.inside[y as usize * self.width + x as usize]
    }
}

/// The subject of `region` ([left, top, right, bottom), clamped to the picture). Nothing is
/// found when no pixel stands apart from the background.
pub fn subject(
    image: &Gray,
    region: [usize; 4],
    seed: Option<Pixel>,
    threshold: Option<f64>,
) -> Result<Option<Mask>, Error> {
    selected(None, image, region, seed, threshold, true)
}

/// Preserve void pixels of the selected component for hole-aware tracing.
pub fn subject_with_holes(
    image: &Gray,
    region: [usize; 4],
    seed: Option<Pixel>,
    threshold: Option<f64>,
) -> Result<Option<Mask>, Error> {
    selected(None, image, region, seed, threshold, false)
}

/// The same hole-preserving selection with cooperative provider checkpoints.
pub fn subject_with_holes_controlled(
    cx: &Context,
    image: &Gray,
    region: [usize; 4],
    seed: Option<Pixel>,
    threshold: Option<f64>,
) -> Result<Option<Mask>, Error> {
    selected(Some(cx), image, region, seed, threshold, false)
}

fn selected(
    cx: Option<&Context>,
    image: &Gray,
    region: [usize; 4],
    seed: Option<Pixel>,
    threshold: Option<f64>,
    fill: bool,
) -> Result<Option<Mask>, Error> {
    let mut work = Checkpoints::new(cx)?;
    let (width, height) = image.size();
    let [left, top, right, bottom] =
        [region[0].min(width), region[1].min(height), region[2].min(width), region[3].min(height)];
    if right <= left + 2 || bottom <= top + 2 {
        return Err(Error::input(format!("the region {region:?} holds no picture to outline")));
    }
    let threshold = match threshold {
        Some(value) => value,
        None => otsu(image, [left, top, right, bottom], &mut work)?,
    };
    let background = border_median(image, [left, top, right, bottom], &mut work)?;
    let dark = background > threshold;
    let stands = |x: usize, y: usize| {
        let value = image.at(x, y);
        if dark {
            value < threshold
        } else {
            value > threshold
        }
    };
    let index = |x: usize, y: usize| y * width + x;
    work.check()?;
    let mut blob = vec![false; width * height];
    let start = match seed {
        Some(seed) => {
            let (x, y) = (seed.x.floor() as usize, seed.y.floor() as usize);
            if !(left..right).contains(&x) || !(top..bottom).contains(&y) || !stands(x, y) {
                return Err(Error::input(format!("the seed {seed} is not on the subject")));
            }
            Some((x, y))
        }
        None => largest(width, [left, top, right, bottom], &stands, &mut work)?,
    };
    let Some(start) = start else { return Ok(None) };
    let mut queue = VecDeque::from([start]);
    blob[index(start.0, start.1)] = true;
    let mut pixels = 0;
    while let Some((x, y)) = queue.pop_front() {
        work.step()?;
        pixels += 1;
        for (nx, ny) in neighbours(x, y, [left, top, right, bottom]) {
            if !blob[index(nx, ny)] && stands(nx, ny) {
                blob[index(nx, ny)] = true;
                queue.push_back((nx, ny));
            }
        }
    }
    work.check()?;
    let mask = Mask { width, height, inside: blob, pixels, threshold, dark };
    Ok(Some(if fill { fill_holes(mask, [left, top, right, bottom], &mut work)? } else { mask }))
}

fn fill_holes(
    mask: Mask,
    [left, top, right, bottom]: [usize; 4],
    work: &mut Checkpoints<'_>,
) -> Result<Mask, Error> {
    let Mask { width, height, inside: blob, threshold, dark, .. } = mask;
    let index = |x: usize, y: usize| y * width + x;
    // A hole is what the outside of the region cannot reach without crossing the blob.
    let mut outside = vec![false; width * height];
    let mut queue: VecDeque<(usize, usize)> = VecDeque::new();
    for (x, y) in border(left, top, right, bottom) {
        work.step()?;
        if !blob[index(x, y)] && !outside[index(x, y)] {
            outside[index(x, y)] = true;
            queue.push_back((x, y));
        }
    }
    while let Some((x, y)) = queue.pop_front() {
        work.step()?;
        for (nx, ny) in neighbours(x, y, [left, top, right, bottom]) {
            if !blob[index(nx, ny)] && !outside[index(nx, ny)] {
                outside[index(nx, ny)] = true;
                queue.push_back((nx, ny));
            }
        }
    }
    let mut inside = vec![false; width * height];
    let mut pixels = 0;
    for y in top..bottom {
        for x in left..right {
            work.step()?;
            if !outside[index(x, y)] {
                inside[index(x, y)] = true;
                pixels += 1;
            }
        }
    }
    work.check()?;
    Ok(Mask { width, height, inside, pixels, threshold, dark })
}

/// The threshold that best splits the brightness of the region in two (Otsu's method).
fn otsu(
    image: &Gray,
    [left, top, right, bottom]: [usize; 4],
    work: &mut Checkpoints<'_>,
) -> Result<f64, Error> {
    work.check()?;
    let mut histogram = [0_u64; 256];
    for y in top..bottom {
        for x in left..right {
            work.step()?;
            histogram[image.at(x, y).clamp(0.0, 255.0).round() as usize] += 1;
        }
    }
    let total: u64 = histogram.iter().sum();
    let sum: f64 = histogram.iter().enumerate().map(|(level, count)| level as f64 * *count as f64).sum();
    let (mut below, mut below_sum, mut best, mut split) = (0_u64, 0.0, -1.0, 0.0);
    for (level, count) in histogram.iter().enumerate() {
        below += count;
        below_sum += level as f64 * *count as f64;
        let above = total - below;
        if below == 0 || above == 0 {
            continue;
        }
        let (mean_below, mean_above) = (below_sum / below as f64, (sum - below_sum) / above as f64);
        let between = below as f64 * above as f64 * (mean_below - mean_above).powi(2);
        if between > best {
            best = between;
            split = level as f64 + 0.5;
        }
    }
    // A picture of one brightness has no split; nothing then stands apart from it.
    work.check()?;
    Ok(if best < 0.0 {
        histogram.iter().rposition(|count| *count > 0).map_or(0.0, |level| level as f64 + 0.5)
    } else {
        split
    })
}

/// The middle brightness along the region's border: what the background looks like.
fn border_median(
    image: &Gray,
    [left, top, right, bottom]: [usize; 4],
    work: &mut Checkpoints<'_>,
) -> Result<f64, Error> {
    work.check()?;
    let mut values = Vec::new();
    for (x, y) in border(left, top, right, bottom) {
        work.step()?;
        values.push(image.at(x, y));
    }
    work.check()?;
    values.sort_by(f64::total_cmp);
    work.check()?;
    Ok(values[values.len() / 2])
}

/// The largest four-connected blob of pixels that stand apart, by one of its pixels.
fn largest(
    width: usize,
    [left, top, right, bottom]: [usize; 4],
    stands: &dyn Fn(usize, usize) -> bool,
    work: &mut Checkpoints<'_>,
) -> Result<Option<(usize, usize)>, Error> {
    work.check()?;
    let mut seen = vec![false; width * (bottom.max(1))];
    let mut best: Option<((usize, usize), usize)> = None;
    for y in top..bottom {
        for x in left..right {
            work.step()?;
            if seen[y * width + x] || !stands(x, y) {
                continue;
            }
            let mut size = 0;
            let mut queue = VecDeque::from([(x, y)]);
            seen[y * width + x] = true;
            while let Some((cx, cy)) = queue.pop_front() {
                work.step()?;
                size += 1;
                for (nx, ny) in neighbours(cx, cy, [left, top, right, bottom]) {
                    if !seen[ny * width + nx] && stands(nx, ny) {
                        seen[ny * width + nx] = true;
                        queue.push_back((nx, ny));
                    }
                }
            }
            if best.is_none_or(|(_, known)| size > known) {
                best = Some(((x, y), size));
            }
        }
    }
    work.check()?;
    Ok(best.map(|(start, _)| start))
}

fn neighbours(
    x: usize,
    y: usize,
    [left, top, right, bottom]: [usize; 4],
) -> impl Iterator<Item = (usize, usize)> {
    let steps = [(1_i64, 0_i64), (-1, 0), (0, 1), (0, -1)];
    steps.into_iter().filter_map(move |(dx, dy)| {
        let (nx, ny) = (x as i64 + dx, y as i64 + dy);
        let inside = nx >= left as i64 && ny >= top as i64 && nx < right as i64 && ny < bottom as i64;
        inside.then_some((nx as usize, ny as usize))
    })
}

fn border(left: usize, top: usize, right: usize, bottom: usize) -> impl Iterator<Item = (usize, usize)> {
    let rows = (left..right).flat_map(move |x| [(x, top), (x, bottom - 1)]);
    let columns = (top..bottom).flat_map(move |y| [(left, y), (right - 1, y)]);
    rows.chain(columns)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{largest, otsu, subject_with_holes, subject_with_holes_controlled};
    use crate::outline::control_tests;
    use crate::outline::Checkpoints;
    use krly3d_core::ErrorKind;
    use krlyvis_kernel::{Gray, Pixel};
    use std::cell::Cell;
    use std::sync::atomic::Ordering;

    #[test]
    fn histogram_checks_inside_a_single_long_row() {
        let image = Gray::new(8192, 1, vec![40.0; 8192]).unwrap();
        let (cx, clock) = control_tests::context(3);
        let mut work = Checkpoints::new(Some(&cx)).unwrap();
        // Admission, histogram entry and its first chunk pass. Expiry is at
        // pixel 1025, before the only row has completed.
        let error = otsu(&image, [0, 0, 8192, 1], &mut work).unwrap_err();
        assert_eq!(error.kind, ErrorKind::DeadlineExceeded);
        assert_eq!(clock.checks.load(Ordering::Relaxed), 4);
    }

    #[test]
    fn largest_checks_inside_both_background_scan_and_component_flood() {
        for foreground in [false, true] {
            let (cx, clock) = control_tests::context(3);
            let mut work = Checkpoints::new(Some(&cx)).unwrap();
            let visited = Cell::new(0);
            let stands = |_: usize, _: usize| {
                visited.set(visited.get() + 1);
                foreground
            };
            // An empty image stays in the scan; a solid image enters its flood
            // at the first pixel. Both must stop before inspecting the full area.
            let error = largest(128, [0, 0, 128, 128], &stands, &mut work).unwrap_err();
            assert_eq!(error.kind, ErrorKind::DeadlineExceeded);
            assert_eq!(clock.checks.load(Ordering::Relaxed), 4);
            assert!((1000..2000).contains(&visited.get()));
        }
    }

    #[test]
    fn seeded_selection_checks_inside_its_own_flood_without_largest_search() {
        let image = Gray::new(128, 128, {
            let mut values = vec![230.0; 128 * 128];
            for y in 1..127 {
                for x in 1..127 {
                    values[y * 128 + x] = 40.0;
                }
            }
            values
        })
        .unwrap();
        let (cx, clock) = control_tests::context(6);
        // The six initial checks cover entry, border collection/sort and mask
        // allocation. The next is within the seed's flood, after 512 pixels.
        let error = subject_with_holes_controlled(
            &cx,
            &image,
            [0, 0, 128, 128],
            Some(Pixel::new(2.5, 2.5)),
            Some(128.0),
        )
        .err()
        .unwrap();
        assert_eq!(error.kind, ErrorKind::DeadlineExceeded);
        assert_eq!(clock.checks.load(Ordering::Relaxed), 7);
        let recovered = subject_with_holes(&image, [0, 0, 128, 128], Some(Pixel::new(2.5, 2.5)), Some(128.0))
            .unwrap()
            .unwrap();
        assert_eq!(recovered.pixels, 126 * 126);
    }
}
