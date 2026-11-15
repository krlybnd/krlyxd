use krly3d_view_api::{diagnostic, Diagnostic};
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

pub(crate) const MODEL_LIMIT: u64 = 16 * 1024 * 1024;
pub(crate) const REQUEST_LIMIT: u64 = 1024 * 1024;

pub(crate) fn read(path: Option<&Path>, kind: &str, limit: u64) -> Result<String, Diagnostic> {
    let source = path.map_or_else(|| "stdin".to_owned(), |path| path.display().to_string());
    let label = format!("{kind} ({source})");
    match path {
        Some(path) => {
            let file = File::open(path)
                .map_err(|error| diagnostic("io", format!("cannot open {label}: {error}")))?;
            bounded(file, &label, limit)
        }
        None => bounded(io::stdin().lock(), &label, limit),
    }
}

fn bounded(reader: impl Read, label: &str, limit: u64) -> Result<String, Diagnostic> {
    // Enforce the bound on bytes actually read, including pipes and growing files.
    // A single extra byte distinguishes the inclusive limit from oversized input.
    let mut bytes = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| diagnostic("io", format!("cannot read {label}: {error}")))?;
    if bytes.len() as u64 > limit {
        return Err(diagnostic("resource_limit", format!("{label} exceeds {limit} bytes")));
    }
    String::from_utf8(bytes).map_err(|error| diagnostic("input", format!("{label} must be UTF-8: {error}")))
}

#[cfg(test)]
mod tests {
    use super::bounded;
    use std::io::{self, Read};

    #[test]
    fn an_unending_reader_is_stopped_after_one_byte_past_the_limit() {
        struct Repeating {
            read: usize,
        }
        impl Read for Repeating {
            fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
                self.read += bytes.len();
                bytes.fill(b' ');
                Ok(bytes.len())
            }
        }
        let mut reader = Repeating { read: 0 };
        let result = bounded(&mut reader, "request", 10);
        assert!(matches!(result, Err(error) if error.code.as_str() == "resource_limit"));
        assert_eq!(reader.read, 11);
    }
}
