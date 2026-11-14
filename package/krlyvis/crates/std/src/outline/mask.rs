//! Which pixels are the subject: those on the far side of a threshold from the background,
//! one connected blob of them, its holes filled.

use krly3d_core::Error;
use krlyvis_kernel::{Gray, Pixel};
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
    let (width, height) = image.size();
    let [left, top, right, bottom] =
        [region[0].min(width), region[1].min(height), region[2].min(width), region[3].min(height)];
    if right <= left + 2 || bottom <= top + 2 {
        return Err(Error::input(format!("the region {region:?} holds no picture to outline")));
    }
    let threshold = threshold.unwrap_or_else(|| otsu(image, [left, top, right, bottom]));
    let background = border_median(image, [left, top, right, bottom]);
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
    let mut blob = vec![false; width * height];
    let start = match seed {
        Some(seed) => {
            let (x, y) = (seed.x.floor() as usize, seed.y.floor() as usize);
            if !(left..right).contains(&x) || !(top..bottom).contains(&y) || !stands(x, y) {
                return Err(Error::input(format!("the seed {seed} is not on the subject")));
            }
            Some((x, y))
        }
        None => largest(width, [left, top, right, bottom], &stands),
    };
    let Some(start) = start else { return Ok(None) };
    let mut queue = VecDeque::from([start]);
    blob[index(start.0, start.1)] = true;
    while let Some((x, y)) = queue.pop_front() {
        for (nx, ny) in neighbours(x, y, [left, top, right, bottom]) {
            if !blob[index(nx, ny)] && stands(nx, ny) {
                blob[index(nx, ny)] = true;
                queue.push_back((nx, ny));
            }
        }
    }
    // A hole is what the outside of the region cannot reach without crossing the blob.
    let mut outside = vec![false; width * height];
    let mut queue: VecDeque<(usize, usize)> = VecDeque::new();
    for (x, y) in border(left, top, right, bottom) {
        if !blob[index(x, y)] && !outside[index(x, y)] {
            outside[index(x, y)] = true;
            queue.push_back((x, y));
        }
    }
    while let Some((x, y)) = queue.pop_front() {
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
            if !outside[index(x, y)] {
                inside[index(x, y)] = true;
                pixels += 1;
            }
        }
    }
    Ok(Some(Mask { width, height, inside, pixels, threshold, dark }))
}

/// The threshold that best splits the brightness of the region in two (Otsu's method).
fn otsu(image: &Gray, [left, top, right, bottom]: [usize; 4]) -> f64 {
    let mut histogram = [0_u64; 256];
    for y in top..bottom {
        for x in left..right {
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
    if best < 0.0 {
        histogram.iter().rposition(|count| *count > 0).map_or(0.0, |level| level as f64 + 0.5)
    } else {
        split
    }
}

/// The middle brightness along the region's border: what the background looks like.
fn border_median(image: &Gray, [left, top, right, bottom]: [usize; 4]) -> f64 {
    let mut values: Vec<f64> = border(left, top, right, bottom).map(|(x, y)| image.at(x, y)).collect();
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

/// The largest four-connected blob of pixels that stand apart, by one of its pixels.
fn largest(
    width: usize,
    [left, top, right, bottom]: [usize; 4],
    stands: &dyn Fn(usize, usize) -> bool,
) -> Option<(usize, usize)> {
    let mut seen = vec![false; width * (bottom.max(1))];
    let mut best: Option<((usize, usize), usize)> = None;
    for y in top..bottom {
        for x in left..right {
            if seen[y * width + x] || !stands(x, y) {
                continue;
            }
            let mut size = 0;
            let mut queue = VecDeque::from([(x, y)]);
            seen[y * width + x] = true;
            while let Some((cx, cy)) = queue.pop_front() {
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
    best.map(|(start, _)| start)
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
