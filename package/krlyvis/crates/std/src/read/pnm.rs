//! The Netpbm formats: grey (`P2`, `P5`) and colour (`P3`, `P6`), plain text or binary,
//! 8 or 16 bits. Colour is turned to grey by the luma weights of ITU-R BT.601.

use super::{LUMA, MOST_PIXELS};
use krly3d_core::{Attempt, Error, Id};
use krlyvis_kernel::ports::read::Read;
use krlyvis_kernel::{Context, Gray, Plugin};

pub struct Pnm;

impl Plugin<Read> for Pnm {
    fn id(&self) -> Id {
        Id::new("pnm")
    }

    fn call(&self, _: &Context, bytes: &Vec<u8>) -> Attempt<Gray> {
        Attempt::from(decode(bytes))
    }
}

fn decode(bytes: &[u8]) -> Result<Gray, Error> {
    let mut cursor = Cursor { bytes, at: 0 };
    let (channels, binary) = match cursor.word()? {
        b"P2" => (1, false),
        b"P5" => (1, true),
        b"P3" => (3, false),
        b"P6" => (3, true),
        other => {
            return Err(Error::format(format!(
                "not a Netpbm image: starts with {:?}",
                String::from_utf8_lossy(other)
            )))
        }
    };
    let (width, height, most) = (cursor.number()?, cursor.number()?, cursor.number()?);
    if width == 0 || height == 0 || width.saturating_mul(height) > MOST_PIXELS {
        return Err(Error::format(format!("{width}x{height} pixels is not a size this reader takes")));
    }
    if most == 0 || most > 65_535 {
        return Err(Error::format(format!("the largest sample value {most} is not in 1..=65535")));
    }
    let count = width * height * channels;
    let samples = if binary { cursor.binary(count, most > 255)? } else { cursor.plain(count)? };
    let scale = 255.0 / most as f64;
    let values = samples
        .chunks_exact(channels)
        .map(|pixel| {
            let grey = if channels == 1 {
                pixel[0] as f64
            } else {
                pixel.iter().zip(LUMA).map(|(&s, w)| s as f64 * w).sum()
            };
            grey * scale
        })
        .collect();
    Gray::new(width, height, values)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    /// The next run of non-blank bytes, after blanks and `#` comments.
    fn word(&mut self) -> Result<&'a [u8], Error> {
        loop {
            match self.bytes.get(self.at) {
                Some(byte) if byte.is_ascii_whitespace() => self.at += 1,
                Some(b'#') => {
                    while self.bytes.get(self.at).is_some_and(|&byte| byte != b'\n') {
                        self.at += 1;
                    }
                }
                Some(_) => break,
                None => return Err(Error::format("the file ends in its header")),
            }
        }
        let start = self.at;
        while self.bytes.get(self.at).is_some_and(|byte| !byte.is_ascii_whitespace()) {
            self.at += 1;
        }
        Ok(&self.bytes[start..self.at])
    }

    fn number(&mut self) -> Result<usize, Error> {
        let word = self.word()?;
        std::str::from_utf8(word)
            .ok()
            .and_then(|text| text.parse().ok())
            .ok_or_else(|| Error::format(format!("{:?} is not a number", String::from_utf8_lossy(word))))
    }

    fn plain(&mut self, count: usize) -> Result<Vec<u32>, Error> {
        (0..count)
            .map(|_| {
                let value =
                    self.number().map_err(|_| Error::format(format!("fewer than {count} samples")))?;
                u32::try_from(value).map_err(|_| Error::format(format!("sample {value} is too large")))
            })
            .collect()
    }

    /// Samples after the one blank that ends the header, one or two bytes each, the
    /// more significant byte first.
    fn binary(&mut self, count: usize, wide: bool) -> Result<Vec<u32>, Error> {
        let start = self.at + 1;
        let size = if wide { 2 } else { 1 };
        let body = self
            .bytes
            .get(start..start + count * size)
            .ok_or_else(|| Error::format(format!("fewer than {count} samples")))?;
        Ok(body
            .chunks_exact(size)
            .map(|sample| sample.iter().fold(0u32, |sum, &byte| sum << 8 | u32::from(byte)))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::Pnm;
    use krlyvis_kernel::ports::read::{contract, Read};
    use krlyvis_kernel::{Gray, VisionBuilder};

    fn check(bytes: &[u8], expected: &Gray) {
        let mut builder = VisionBuilder::new();
        builder.plugin::<Read>(Pnm);
        contract::check(&builder.context(), "pnm", bytes, expected).unwrap();
    }

    #[test]
    fn plain_grey_with_a_comment_is_read_and_scaled_to_255() {
        let expected = Gray::new(2, 2, vec![0.0, 51.0, 102.0, 255.0]).unwrap();

        check(b"P2\n# made by hand\n2 2\n5\n0 1\n2 5\n", &expected);
    }

    #[test]
    fn binary_grey_is_read() {
        let expected = Gray::new(3, 1, vec![0.0, 128.0, 255.0]).unwrap();

        check(b"P5 3 1 255\n\x00\x80\xff", &expected);
    }

    #[test]
    fn sixteen_bit_binary_grey_is_read() {
        let expected = Gray::new(2, 1, vec![0.0, 255.0]).unwrap();

        check(b"P5 2 1 65535\n\x00\x00\xff\xff", &expected);
    }

    #[test]
    fn colour_is_turned_to_grey_by_luma() {
        let white = 255.0 * 0.299 + 255.0 * 0.587 + 255.0 * 0.114;
        let expected = Gray::new(2, 1, vec![255.0 * 0.299, white]).unwrap();

        check(b"P6 2 1 255\n\xff\x00\x00\xff\xff\xff", &expected);
        check(b"P3 2 1 255\n255 0 0 255 255 255\n", &expected);
    }
}
