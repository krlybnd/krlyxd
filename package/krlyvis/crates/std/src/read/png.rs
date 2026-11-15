//! PNG, by the `png` crate: any colour type and bit depth, palettes expanded, 16 bits
//! narrowed to 8; an alpha channel is laid over white.

use super::{LUMA, MOST_PIXELS};
use krly3d_core::{Attempt, Error, Id};
use krlyvis_kernel::ports::read::Read;
use krlyvis_kernel::{Context, Gray, Plugin};
use png::{ColorType, Decoder, Transformations};

pub struct Png;

impl Plugin<Read> for Png {
    fn id(&self) -> Id {
        Id::new("png")
    }

    fn call(&self, _: &Context, bytes: &Vec<u8>) -> Attempt<Gray> {
        Attempt::from(decode(bytes))
    }
}

fn decode(bytes: &[u8]) -> Result<Gray, Error> {
    let broken =
        |error: png::DecodingError| Error::format(format!("not a PNG image this reader takes: {error}"));
    let mut decoder = Decoder::new(bytes);
    decoder.set_transformations(Transformations::EXPAND | Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(broken)?;
    let (width, height) = (reader.info().width as usize, reader.info().height as usize);
    if width == 0 || height == 0 || width.saturating_mul(height) > MOST_PIXELS {
        return Err(Error::format(format!("{width}x{height} pixels is not a size this reader takes")));
    }
    let mut buffer = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut buffer).map_err(broken)?;
    let channels = match frame.color_type {
        ColorType::Grayscale => 1,
        ColorType::GrayscaleAlpha => 2,
        ColorType::Rgb => 3,
        ColorType::Rgba => 4,
        ColorType::Indexed => return Err(Error::format("a palette was left unexpanded")),
    };
    let values = buffer[..frame.buffer_size()].chunks_exact(channels).map(grey).collect();
    Gray::new(width, height, values)
}

/// The brightness of one pixel of 8-bit samples, its alpha (when it has one) laid over white.
fn grey(pixel: &[u8]) -> f64 {
    let sample = |index: usize| f64::from(pixel[index]);
    let (colour, alpha) = match pixel.len() {
        1 => (sample(0), 255.0),
        2 => (sample(0), sample(1)),
        3 => ((0..3).map(|c| sample(c) * LUMA[c]).sum(), 255.0),
        _ => ((0..3).map(|c| sample(c) * LUMA[c]).sum(), sample(3)),
    };
    (colour * alpha + 255.0 * (255.0 - alpha)) / 255.0
}

#[cfg(test)]
mod tests {
    use super::decode;

    /// A PNG of the given colour type and 8-bit samples, made by the `png` crate's encoder.
    fn encoded(width: u32, height: u32, color: png::ColorType, samples: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(samples).unwrap();
        bytes
    }

    #[test]
    fn a_colour_png_reads_as_the_luma_of_each_pixel() {
        let bytes = encoded(2, 1, png::ColorType::Rgb, &[255, 0, 0, 0, 0, 255]);

        let image = decode(&bytes).unwrap();

        assert_eq!(image.size(), (2, 1));
        assert!(
            (image.at(0, 0) - 0.299 * 255.0).abs() < 1e-9 && (image.at(1, 0) - 0.114 * 255.0).abs() < 1e-9
        );
    }

    #[test]
    fn a_transparent_pixel_reads_as_white_paper() {
        let bytes = encoded(2, 1, png::ColorType::GrayscaleAlpha, &[0, 0, 0, 255]);

        let image = decode(&bytes).unwrap();

        assert_eq!((image.at(0, 0), image.at(1, 0)), (255.0, 0.0));
    }

    #[test]
    fn bytes_that_are_not_a_png_are_refused_with_the_reason() {
        let refused = decode(b"P5 1 1 255 x").unwrap_err();

        assert!(refused.message.contains("not a PNG image"), "{}", refused.message);
    }
}
