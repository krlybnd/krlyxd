// Bounded input pattern adapted from products/krly3d/brep/cli/src/input.rs.
use crate::diagnostic;
use krlyvis_api::Diagnostic;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

pub(crate) const REQUEST_LIMIT: u64 = 24 * 1024 * 1024;

pub(crate) fn read(path: Option<&Path>) -> Result<String, Diagnostic> {
    let source = path.map_or_else(|| "stdin".to_owned(), |path| path.display().to_string());
    match path {
        Some(path) => {
            let file = File::open(path)
                .map_err(|error| diagnostic("io", format!("cannot open request ({source}): {error}")))?;
            bounded(file, &source)
        }
        None => bounded(io::stdin().lock(), &source),
    }
}

fn bounded(reader: impl Read, source: &str) -> Result<String, Diagnostic> {
    bounded_with_limit(reader, source, REQUEST_LIMIT)
}

fn bounded_with_limit(reader: impl Read, source: &str, limit: u64) -> Result<String, Diagnostic> {
    let mut bytes = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| diagnostic("io", format!("cannot read request ({source}): {error}")))?;
    if bytes.len() as u64 > limit {
        return Err(diagnostic("resource_limit", "request exceeds 24 MiB (25165824 bytes)"));
    }
    String::from_utf8(bytes)
        .map_err(|error| diagnostic("input", format!("request ({source}) must be UTF-8: {error}")))
}

#[cfg(test)]
mod tests {
    use super::{bounded_with_limit, REQUEST_LIMIT};
    use std::io::{self, Read};

    #[test]
    fn an_unending_reader_stops_one_byte_beyond_the_limit() {
        struct Repeating(usize);
        impl Read for Repeating {
            fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
                self.0 += bytes.len();
                bytes.fill(b' ');
                Ok(bytes.len())
            }
        }
        let mut reader = Repeating(0);
        assert_eq!(REQUEST_LIMIT, 24 * 1024 * 1024);
        assert_eq!(REQUEST_LIMIT as usize, krlyvis_api::MAX_CALL_BYTES);
        let result = bounded_with_limit(&mut reader, "test", 64);
        assert!(matches!(result, Err(error) if error.code.as_str() == "resource_limit"));
        assert_eq!(reader.0, 65);
    }

    #[test]
    fn exact_bound_empty_utf8_and_io_errors_are_distinct() {
        assert!(matches!(bounded_with_limit(&b"abcd"[..], "test", 4), Ok(text) if text == "abcd"));
        assert!(matches!(bounded_with_limit(&b""[..], "test", 4), Ok(text) if text.is_empty()));
        assert!(
            matches!(bounded_with_limit(&b"\xff"[..], "test", 4), Err(error) if error.code.as_str() == "input")
        );
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("broken input"))
            }
        }
        assert!(matches!(bounded_with_limit(Broken, "test", 4), Err(error) if error.code.as_str() == "io"));
    }
}
