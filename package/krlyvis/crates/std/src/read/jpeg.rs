//! JPEG, by the `zune-jpeg` crate, decoded straight to brightness.

use super::MOST_PIXELS;
use krly3d_core::{Attempt, Error, Id};
use krlyvis_kernel::ports::read::Read;
use krlyvis_kernel::{Context, Gray, Plugin};
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

pub struct Jpeg;

impl Plugin<Read> for Jpeg {
    fn id(&self) -> Id {
        Id::new("jpeg")
    }

    fn call(&self, _: &Context, bytes: &Vec<u8>) -> Attempt<Gray> {
        Attempt::from(decode(bytes))
    }
}

/// The longest side a JPEG may have, so that a forged header cannot ask for an
/// allocation beyond what the readers take.
const LONGEST_SIDE: usize = 1 << 15;

fn decode(bytes: &[u8]) -> Result<Gray, Error> {
    let broken = |error: zune_jpeg::errors::DecodeErrors| {
        Error::format(format!("not a JPEG image this reader takes: {error:?}"))
    };
    let options = DecoderOptions::default()
        .jpeg_set_out_colorspace(ColorSpace::Luma)
        .set_max_width(LONGEST_SIDE)
        .set_max_height(LONGEST_SIDE);
    let mut decoder = JpegDecoder::new_with_options(bytes, options);
    decoder.decode_headers().map_err(broken)?;
    let (width, height) = decoder.dimensions().ok_or_else(|| Error::format("a JPEG without a size"))?;
    if width == 0 || height == 0 || width.saturating_mul(height) > MOST_PIXELS {
        return Err(Error::format(format!("{width}x{height} pixels is not a size this reader takes")));
    }
    let samples = decoder.decode().map_err(broken)?;
    if samples.len() != width * height {
        return Err(Error::format(format!("a {width}x{height} JPEG decoded to {} samples", samples.len())));
    }
    Gray::new(width, height, samples.into_iter().map(f64::from).collect())
}

#[cfg(test)]
mod tests {
    use super::decode;

    /// A grey 16x8 JPEG, darker on the left half than on the right, written by hand once
    /// with Python's Pillow (`Image.new("L", (16, 8)); paste 40 | 220; save quality=95`).
    const HALVES: &[u8] = include_bytes!("halves.jpg");

    #[test]
    fn a_jpeg_reads_as_its_brightness() {
        let image = decode(HALVES).unwrap();

        assert_eq!(image.size(), (16, 8));
        assert!((image.at(2, 4) - 40.0).abs() < 6.0 && (image.at(13, 4) - 220.0).abs() < 6.0);
    }

    #[test]
    fn bytes_that_are_not_a_jpeg_are_refused_with_the_reason() {
        let refused = decode(b"\x89PNG not really").unwrap_err();

        assert!(refused.message.contains("not a JPEG image"), "{}", refused.message);
    }
}
