//! A PNG file from raw pixels, written without compression.
//!
//! The pixels go into the file as "stored" blocks of the deflate format, which need no
//! compressor and which every reader of PNG accepts. The price is a file as large as
//! the pixels themselves.

use krly3d_core::Error;

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
/// A stored block states its length in sixteen bits.
const LONGEST_BLOCK: usize = 0xffff;
/// Widths, heights and the lengths of chunks are stated in thirty-one bits.
const LARGEST: usize = 0x7fff_ffff;

/// The bytes of a PNG file that shows `pixels`: rows from the top and, within a row,
/// pixels from the left, each of `channels` bytes — 3 for red, green and blue, 4 for
/// those and opacity.
pub fn encode(width: usize, height: usize, channels: usize, pixels: &[u8]) -> Result<Vec<u8>, Error> {
    encode_checked(width, height, channels, pixels, &|| Ok(()))
}

/// The query uses the same encoder with cooperative checks through copying and checksums.
pub fn encode_checked(
    width: usize,
    height: usize,
    channels: usize,
    pixels: &[u8],
    check: &impl Fn() -> Result<(), Error>,
) -> Result<Vec<u8>, Error> {
    check()?;
    let colour_type: u8 = match channels {
        3 => 2,
        4 => 6,
        other => return Err(Error::input(format!("a pixel has 3 or 4 bytes, not {other}"))),
    };
    let (Ok(wide), Ok(high)) = (u32::try_from(width), u32::try_from(height)) else {
        return Err(Error::input(format!(
            "an image of {width} by {height} pixels is too large for a PNG file"
        )));
    };
    if width == 0 || height == 0 || width > LARGEST || height > LARGEST {
        return Err(Error::input(format!("a PNG file cannot hold an image of {width} by {height} pixels")));
    }
    let row = width.checked_mul(channels).filter(|row| row.checked_mul(height) == Some(pixels.len()));
    let Some(row) = row else {
        return Err(Error::input(format!(
            "an image of {width} by {height} pixels of {channels} bytes does not have {} bytes",
            pixels.len()
        )));
    };
    let mut lines = Vec::with_capacity(pixels.len() + height);
    for line in pixels.chunks(row) {
        check()?;
        // Every line starts with the filter it went through: none.
        lines.push(0);
        for part in line.chunks(LONGEST_BLOCK) {
            check()?;
            lines.extend_from_slice(part);
        }
    }
    let data = stored(&lines, check)?;
    if data.len() > LARGEST {
        return Err(Error::input(format!(
            "an image of {width} by {height} pixels is too large for one chunk"
        )));
    }
    let mut header = Vec::with_capacity(13);
    header.extend(wide.to_be_bytes());
    header.extend(high.to_be_bytes());
    // Eight bits a sample; the only compression and filter methods there are; not interlaced.
    header.extend([8, colour_type, 0, 0, 0]);
    let mut file = SIGNATURE.to_vec();
    chunk(&mut file, b"IHDR", &header, check)?;
    chunk(&mut file, b"IDAT", &data, check)?;
    chunk(&mut file, b"IEND", &[], check)?;
    check()?;
    Ok(file)
}

/// `data` as a zlib stream made of stored blocks.
fn stored(data: &[u8], check: &impl Fn() -> Result<(), Error>) -> Result<Vec<u8>, Error> {
    check()?;
    // Deflate with a 32K window, no preset dictionary; the pair is a multiple of 31, as it must be.
    let mut stream = vec![0x78, 0x01];
    let mut blocks = data.chunks(LONGEST_BLOCK).peekable();
    while let Some(block) = blocks.next() {
        check()?;
        let length = block.len() as u16;
        stream.push(u8::from(blocks.peek().is_none()));
        stream.extend(length.to_le_bytes());
        stream.extend((!length).to_le_bytes());
        stream.extend_from_slice(block);
    }
    stream.extend(adler32(data, check)?.to_be_bytes());
    Ok(stream)
}

fn chunk(
    file: &mut Vec<u8>,
    kind: &[u8; 4],
    data: &[u8],
    check: &impl Fn() -> Result<(), Error>,
) -> Result<(), Error> {
    check()?;
    file.extend((data.len() as u32).to_be_bytes());
    file.extend_from_slice(kind);
    for part in data.chunks(LONGEST_BLOCK) {
        check()?;
        file.extend_from_slice(part);
    }
    file.extend(crc32(&[kind, data], check)?.to_be_bytes());
    Ok(())
}

fn adler32(data: &[u8], check: &impl Fn() -> Result<(), Error>) -> Result<u32, Error> {
    const MODULUS: u32 = 65_521;
    // This many bytes can be added before the sums have to be reduced to stay within 32 bits.
    const RUN: usize = 5552;
    let (mut low, mut high) = (1_u32, 0_u32);
    for run in data.chunks(RUN) {
        check()?;
        for byte in run {
            low += u32::from(*byte);
            high += low;
        }
        low %= MODULUS;
        high %= MODULUS;
    }
    Ok(high << 16 | low)
}

const CRC_TABLE: [u32; 256] = crc_table();

const fn crc_table() -> [u32; 256] {
    let mut table = [0_u32; 256];
    let mut byte = 0;
    while byte < 256 {
        let mut remainder = byte as u32;
        let mut bit = 0;
        while bit < 8 {
            remainder = if remainder & 1 == 1 { 0xedb8_8320 ^ (remainder >> 1) } else { remainder >> 1 };
            bit += 1;
        }
        table[byte] = remainder;
        byte += 1;
    }
    table
}

fn crc32(parts: &[&[u8]], check: &impl Fn() -> Result<(), Error>) -> Result<u32, Error> {
    let mut crc = !0_u32;
    for part in parts {
        for batch in part.chunks(LONGEST_BLOCK) {
            check()?;
            for byte in batch {
                crc = CRC_TABLE[((crc ^ u32::from(*byte)) & 0xff) as usize] ^ (crc >> 8);
            }
        }
    }
    Ok(!crc)
}

#[cfg(test)]
mod tests {
    use super::encode;
    use krly3d_core::ErrorKind;

    /// What zlib and its CRC-32 make of the same pixels at compression level 0, computed
    /// with Python: a 2 by 2 image with opacity and a 3 by 1 image without.
    const WITH_OPACITY: [u8; 86] = [
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00,
        0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x08, 0x06, 0x00, 0x00, 0x00, 0x72, 0xb6, 0x0d, 0x24, 0x00,
        0x00, 0x00, 0x1d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x01, 0x01, 0x12, 0x00, 0xed, 0xff, 0x00, 0xff, 0x00,
        0x00, 0xff, 0x00, 0xff, 0x00, 0x80, 0x00, 0x00, 0x00, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x37, 0x35,
        0x05, 0x27, 0x19, 0xf6, 0x45, 0x0b, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60,
        0x82,
    ];
    const WITHOUT_OPACITY: [u8; 78] = [
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00,
        0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x94, 0x82, 0x83, 0xe3, 0x00,
        0x00, 0x00, 0x15, 0x49, 0x44, 0x41, 0x54, 0x78, 0x01, 0x01, 0x0a, 0x00, 0xf5, 0xff, 0x00, 0x0a, 0x14,
        0x1e, 0x28, 0x32, 0x3c, 0x46, 0x50, 0x5a, 0x06, 0x7c, 0x01, 0xc3, 0x00, 0x59, 0xdd, 0x43, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn an_image_with_opacity_is_the_file_zlib_would_write() {
        let pixels = [255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 17, 34, 51, 68];

        let file = encode(2, 2, 4, &pixels).unwrap();

        assert_eq!(file, WITH_OPACITY);
    }

    #[test]
    fn an_image_without_opacity_is_the_file_zlib_would_write() {
        let pixels = [10, 20, 30, 40, 50, 60, 70, 80, 90];

        let file = encode(3, 1, 3, &pixels).unwrap();

        assert_eq!(file, WITHOUT_OPACITY);
    }

    #[test]
    fn pixels_that_do_not_fill_the_image_are_refused() {
        let error = encode(2, 2, 3, &[0; 11]).unwrap_err();

        assert_eq!(error.kind, ErrorKind::Input);
        assert_eq!(error.message, "an image of 2 by 2 pixels of 3 bytes does not have 11 bytes");
    }

    #[test]
    fn a_pixel_of_another_size_than_three_or_four_bytes_is_refused() {
        let error = encode(1, 1, 2, &[0; 2]).unwrap_err();

        assert_eq!(error.message, "a pixel has 3 or 4 bytes, not 2");
    }

    #[test]
    fn an_image_without_pixels_is_refused() {
        let error = encode(0, 4, 3, &[]).unwrap_err();

        assert_eq!(error.message, "a PNG file cannot hold an image of 0 by 4 pixels");
    }

    #[test]
    fn a_size_that_overflows_is_refused() {
        let error = encode(usize::MAX, 2, 3, &[0; 6]).unwrap_err();

        assert_eq!(error.kind, ErrorKind::Input);
    }

    #[test]
    fn checked_encoding_is_identical_and_stops_in_copying_and_checksums() {
        use krly3d_core::{CallStop, Error};
        use std::cell::Cell;

        let pixels = vec![123_u8; 257 * 129 * 3];
        let checks = Cell::new(0);
        let file = super::encode_checked(257, 129, 3, &pixels, &|| {
            checks.set(checks.get() + 1);
            Ok(())
        })
        .unwrap();
        assert_eq!(file, encode(257, 129, 3, &pixels).unwrap());
        let total = checks.get();
        // Entry, row copies, stored blocks, Adler checksum, IDAT copy/CRC and end.
        for at in [1, 6, 260, 265, total - 5, total - 2, total] {
            for stop in [CallStop::Cancelled, CallStop::DeadlineExceeded] {
                checks.set(0);
                let result = super::encode_checked(257, 129, 3, &pixels, &|| {
                    checks.set(checks.get() + 1);
                    if checks.get() == at {
                        Err(Error::from(stop))
                    } else {
                        Ok(())
                    }
                });
                assert_eq!(result, Err(Error::from(stop)), "checkpoint {at}");
                assert_eq!(checks.get(), at);
            }
        }
    }
}
