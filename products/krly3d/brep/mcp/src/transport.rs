// Generic bounded framing adapted from products/krly3d/view/mcp/src/transport.rs.
use crate::Server;
use serde_json::Value;
use std::io::{self, BufRead, Write};

/// Maximum bytes before the newline of one incoming JSON-RPC message.
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

/// Read, execute, and flush one request at a time. Building and output writes
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
        if count > MAX_MESSAGE_BYTES - frame.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "MCP message exceeds 1 MiB"));
        }
        frame.extend_from_slice(&bytes[..count]);
        input.consume(count + usize::from(end.is_some()));
        if end.is_some() {
            return Ok(Some(frame));
        }
    }
}
