//! Bytes as base64 text (RFC 4648, the standard alphabet, padded), which is how an image
//! travels inside a text answer.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        // Three bytes are four digits of six bits; a last group that is short is padded.
        let mut word = 0_u32;
        for place in 0..3 {
            word = word << 8 | group.get(place).map_or(0, |byte| u32::from(*byte));
        }
        for digit in 0..4 {
            if digit <= group.len() {
                text.push(char::from(ALPHABET[(word >> (18 - 6 * digit) & 0x3f) as usize]));
            } else {
                text.push('=');
            }
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::encode;

    #[test]
    fn the_test_vectors_of_the_rfc_are_encoded_as_it_states() {
        let encoded = [b"".as_slice(), b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar"].map(encode);

        assert_eq!(encoded, ["", "Zg==", "Zm8=", "Zm9v", "Zm9vYg==", "Zm9vYmE=", "Zm9vYmFy"]);
    }

    #[test]
    fn every_digit_of_the_alphabet_is_reached() {
        let all: Vec<u8> = (0..=255).collect();

        let encoded = encode(&all);

        assert!(encoded.starts_with("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8gISIjJCUmJygpKissLS4v"));
        assert!(encoded.ends_with("8PHy8/T19vf4+fr7/P3+/w=="));
        assert_eq!(encoded.len(), 344);
    }

    #[test]
    fn the_signature_of_a_png_file_starts_as_every_png_in_base64_does() {
        let encoded = encode(&[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, 0, 0, 13]);

        assert_eq!(encoded, "iVBORw0KGgoAAAAN");
    }
}
