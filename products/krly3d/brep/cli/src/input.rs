// Bounded input pattern adapted from products/krly3d/view/cli/src/input.rs.
use crate::diagnostic;
use krly3d_brep::Diagnostic;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

pub(crate) const REQUEST_LIMIT: u64 = 1024 * 1024;

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
    let mut bytes = Vec::new();
    reader
        .take(REQUEST_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| diagnostic("io", format!("cannot read request ({source}): {error}")))?;
    if bytes.len() as u64 > REQUEST_LIMIT {
        return Err(diagnostic("resource_limit", "request exceeds 1 MiB (1048576 bytes)"));
    }
    String::from_utf8(bytes)
        .map_err(|error| diagnostic("input", format!("request ({source}) must be UTF-8: {error}")))
}

#[cfg(test)]
mod tests {
    use super::{bounded, REQUEST_LIMIT};
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
        let result = bounded(&mut reader, "test");
        assert!(matches!(result, Err(error) if error.code.as_str() == "resource_limit"));
        assert_eq!(reader.0 as u64, REQUEST_LIMIT + 1);
    }
}
