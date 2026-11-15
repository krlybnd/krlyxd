// Generic bounded framing adapted from products/krly3d/brep/mcp/src/transport.rs.
use crate::Server;
use serde_json::Value;
use std::io::{self, BufRead, Write};

/// Maximum bytes before the newline of one incoming JSON-RPC message.
pub const MAX_MESSAGE_BYTES: usize = 24 * 1024 * 1024;

/// Read, execute, and flush one request at a time. Drawing reads and output writes
/// block all further input, including ping and cancellation notifications. EOF
/// is observed after the current response. No in-flight request cancellation.
pub fn serve(mut server: Server, mut input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    while let Some(frame) = frame(&mut input)? {
        let response = match std::str::from_utf8(&frame) {
            Ok(text) => server.handle(text),
            Err(_) => Some(crate::json::Failure::new(-32700, "Invalid UTF-8 JSON").response(Value::Null)),
        };
        if let Some(response) = response {
            serde_json::to_writer(&mut output, &response)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
    Ok(())
}

pub fn serve_stdio(server: Server) -> io::Result<()> {
    serve(server, io::stdin().lock(), io::BufWriter::new(io::stdout().lock()))
}

fn frame(input: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    frame_with_limit(input, MAX_MESSAGE_BYTES)
}

fn frame_with_limit(input: &mut impl BufRead, limit: usize) -> io::Result<Option<Vec<u8>>> {
    let mut frame = Vec::new();
    loop {
        let bytes = input.fill_buf()?;
        if bytes.is_empty() {
            return if frame.is_empty() {
                Ok(None)
            } else {
                Err(io::Error::new(io::ErrorKind::InvalidData, "unterminated MCP message"))
            };
        }
        let end = bytes.iter().position(|byte| *byte == b'\n');
        let count = end.unwrap_or(bytes.len());
        if count > limit - frame.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "MCP message exceeds 24 MiB"));
        }
        frame.extend_from_slice(&bytes[..count]);
        input.consume(count + usize::from(end.is_some()));
        if end.is_some() {
            return Ok(Some(frame));
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{frame_with_limit, MAX_MESSAGE_BYTES};
    use std::io::{self, BufReader, Cursor};

    // Exercise the production framing loop with a small limit, avoiding repeated
    // 24 MiB buffers for every combination of framing and reader fragmentation.
    #[test]
    fn exact_bound_and_crlf_account_for_every_byte_before_the_newline() {
        assert_eq!(MAX_MESSAGE_BYTES, 24 * 1024 * 1024);
        assert_eq!(MAX_MESSAGE_BYTES, krlyvis_api::MAX_CALL_BYTES);
        for capacity in [1, 7, 31, 65] {
            for suffix in [b"\n".as_slice(), b"\r\n"] {
                let mut bytes = vec![b' '; 64 - (suffix.len() - 1)];
                bytes.extend_from_slice(suffix);
                bytes.extend_from_slice(b"{}\n");
                let mut input = BufReader::with_capacity(capacity, Cursor::new(bytes));
                assert_eq!(frame_with_limit(&mut input, 64).unwrap().unwrap().len(), 64);
                assert_eq!(frame_with_limit(&mut input, 64).unwrap().unwrap(), b"{}");
                assert!(frame_with_limit(&mut input, 64).unwrap().is_none());
            }
        }
    }

    #[test]
    fn oversized_unterminated_and_empty_frames_are_distinct() {
        for capacity in [1, 7, 31, 65] {
            for suffix in [b"".as_slice(), b"\n", b"\r\n"] {
                let mut bytes = vec![b' '; 65];
                bytes.extend_from_slice(suffix);
                let mut input = BufReader::with_capacity(capacity, Cursor::new(bytes));
                let error = frame_with_limit(&mut input, 64).unwrap_err();
                assert_eq!(error.kind(), io::ErrorKind::InvalidData);
                assert!(error.to_string().contains("exceeds 24 MiB"));
            }
            let mut input = BufReader::with_capacity(capacity, Cursor::new(vec![b' '; 64]));
            let error = frame_with_limit(&mut input, 64).unwrap_err();
            assert!(error.to_string().contains("unterminated"));
        }
        let mut input = Cursor::new(b"\n");
        assert!(frame_with_limit(&mut input, 64).unwrap().unwrap().is_empty());
        assert!(frame_with_limit(&mut input, 64).unwrap().is_none());
    }

    #[test]
    fn oversized_frames_stop_before_consuming_the_overflowing_chunk() {
        let mut input = Cursor::new(vec![b' '; 128]);
        assert!(frame_with_limit(&mut input, 64).is_err());
        assert_eq!(input.position(), 0);
    }
}
