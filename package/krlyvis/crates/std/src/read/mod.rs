//! Readers: the bytes of an image file into a grey image. Colour is turned to grey by the
//! luma weights of ITU-R BT.601, and a transparent pixel counts as white paper.

pub mod jpeg;
pub mod png;
pub mod pnm;

use krlyvis_kernel::ports::read::Read;
use krlyvis_kernel::VisionBuilder;

/// More pixels than this are refused rather than allocated.
pub(crate) const MOST_PIXELS: usize = 1 << 28;

/// Weights of red, green and blue in the brightness a reader keeps.
pub(crate) const LUMA: [f64; 3] = [0.299, 0.587, 0.114];

pub fn register(builder: &mut VisionBuilder) {
    builder.plugin::<Read>(pnm::Pnm).plugin::<Read>(png::Png).plugin::<Read>(jpeg::Jpeg);
}

/// The format a file name says, as the readers are named.
pub fn format_of(path: &str) -> Option<&'static str> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    match extension.as_str() {
        "pgm" | "ppm" | "pnm" => Some("pnm"),
        "png" => Some("png"),
        "jpg" | "jpeg" => Some("jpeg"),
        _ => None,
    }
}
