//! `krly3d` – the kernel as a JSON-RPC 2.0 server over stdin/stdout.
use krly3d::protocol::Session;
use std::io::{BufRead, Write};

fn main() {
    let mut session = Session::new();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = session.handle_line(&line) {
            if writeln!(out, "{reply}").and_then(|_| out.flush()).is_err() {
                break;
            }
        }
    }
}
