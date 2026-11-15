use crate::{output, Failure, TICK};
use krly3d_mesh::diagnostic;
use std::fs::File;
use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};

pub(crate) const MODEL_LIMIT: u64 = 16 * 1024 * 1024;
pub(crate) const REQUEST_LIMIT: u64 = 1024 * 1024;

/// Read at most limit + 1 bytes, including from a pipe or a growing file. Reading
/// happens off the control thread so SIGINT can end an unfinished stdin request.
pub(crate) fn read(path: Option<PathBuf>, limit: u64, interrupted: &AtomicBool) -> Result<String, Failure> {
    let label = path.as_ref().map_or_else(|| "stdin".to_owned(), |path| path.display().to_string());
    let (sent, received) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("mesh-cli-input".into())
        .spawn(move || {
            let result = match path {
                Some(path) => File::open(path).and_then(|file| bounded(file, limit)),
                None => bounded(io::stdin().lock(), limit),
            };
            let _ = sent.send(result);
        })
        .map_err(|error| diagnostic("resource_limit", format!("could not start input reader: {error}")))?;
    loop {
        if interrupted.load(Ordering::Relaxed) {
            return Err(Failure::cancelled("interrupted while reading input"));
        }
        output::check_connection()?;
        match received.recv_timeout(TICK) {
            Ok(result) => {
                return result.map_err(|error| diagnostic("input", format!("{label}: {error}")).into())
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(diagnostic("internal", "input reader stopped without a result").into());
            }
        }
    }
}

fn bounded(reader: impl Read, limit: u64) -> io::Result<String> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("input exceeds {limit} bytes")));
    }
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}
