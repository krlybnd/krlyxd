//! Dialects: the languages a client speaks to the session in.
//!
//! Every dialect is JSON-RPC 2.0 with one message per line; they differ in what the
//! methods are called and how a reply is laid out. A dialect knows the session only
//! through [`Endpoint::methods`] and [`Endpoint::call`], so none of them names a tool.

pub mod mcp;
pub mod plain;

use crate::endpoint::Endpoint;
use crate::rpc::{self, Failure};
use serde_json::Value as Json;
use std::io::{self, BufRead, Read, Write};

pub trait Dialect {
    /// The word that chooses the dialect on the command line, as `--word`.
    fn name(&self) -> &'static str;

    /// What the dialect is, in one line of the help text.
    fn about(&self) -> &'static str;

    /// Answers one request against the endpoint.
    fn answer(
        &mut self,
        session: &mut dyn Endpoint,
        method: &str,
        params: Option<&Json>,
    ) -> Result<Json, Failure>;
}

/// Every dialect, the default one first. A new dialect is a new file and one line here.
pub fn all() -> Vec<Box<dyn Dialect>> {
    vec![Box::new(plain::Plain), Box::new(mcp::Mcp)]
}

pub fn named(name: &str) -> Option<Box<dyn Dialect>> {
    all().into_iter().find(|dialect| dialect.name() == name)
}

/// A message may be this many bytes long. A longer one is refused without being kept,
/// so that a peer cannot make the server hold an arbitrary amount of memory.
pub const LONGEST_MESSAGE: usize = 16 * 1024 * 1024;

/// Reads messages from `input`, one per line, until it ends, and writes the answer to
/// each as one line of `output`.
pub fn serve(
    dialect: &mut dyn Dialect,
    session: &mut dyn Endpoint,
    mut input: impl BufRead,
    mut output: impl Write,
) -> io::Result<()> {
    let mut line = Vec::new();
    loop {
        line.clear();
        let answer = match read_line(&mut input, &mut line)? {
            Line::End => return Ok(()),
            Line::TooLong => {
                Some(rpc::refused(&format!("a message may be at most {LONGEST_MESSAGE} bytes long")))
            }
            Line::Whole if line.trim_ascii().is_empty() => None,
            Line::Whole => {
                rpc::answer(line.trim_ascii(), &mut |method, params| dialect.answer(session, method, params))
            }
        };
        if let Some(answer) = answer {
            serde_json::to_writer(&mut output, &answer)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
}

enum Line {
    Whole,
    TooLong,
    End,
}

/// Reads up to the next line break into `line`. Of a line that is too long nothing is
/// kept and the rest is skipped, so reading goes on with the line after it.
fn read_line(input: &mut impl BufRead, line: &mut Vec<u8>) -> io::Result<Line> {
    let most = LONGEST_MESSAGE as u64 + 1;
    if input.by_ref().take(most).read_until(b'\n', line)? == 0 {
        return Ok(Line::End);
    }
    if line.len() <= LONGEST_MESSAGE || line.ends_with(b"\n") {
        return Ok(Line::Whole);
    }
    line.clear();
    loop {
        let ahead = input.fill_buf()?;
        match ahead.iter().position(|byte| *byte == b'\n') {
            Some(at) => {
                input.consume(at + 1);
                return Ok(Line::TooLong);
            }
            None if ahead.is_empty() => return Ok(Line::TooLong),
            None => {
                let skipped = ahead.len();
                input.consume(skipped);
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::{serve, Dialect};
    use crate::endpoint::Endpoint;
    use krly3d_core::Accuracy;
    use krly3d_kernel::Session;
    use serde_json::Value as Json;

    pub(crate) fn session() -> Session {
        let mut builder = krly3d_std::standard();
        krly3d_view::register(&mut builder);
        krly3d_exchange::register(&mut builder);
        builder.accuracy(Accuracy::for_scale(100.0).unwrap());
        builder.session()
    }

    /// What the dialect writes back for the lines of `input`, one parsed answer per line.
    pub(crate) fn exchange(dialect: &mut dyn Dialect, session: &mut dyn Endpoint, input: &str) -> Vec<Json> {
        let mut output = Vec::new();
        serve(dialect, session, input.as_bytes(), &mut output).unwrap();
        let text = String::from_utf8(output).unwrap();
        text.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{exchange, session};
    use super::{all, named, serve, LONGEST_MESSAGE};
    use serde_json::json;

    #[test]
    fn every_dialect_is_found_by_its_own_name() {
        for dialect in all() {
            let found = named(dialect.name());

            assert!(found.is_some_and(|found| found.name() == dialect.name()));
        }
        assert!(named("morse").is_none());
    }

    #[test]
    fn every_answer_is_one_line() {
        let mut session = session();
        let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"methods\"}\n";
        let mut output = Vec::new();

        serve(named("plain").unwrap().as_mut(), &mut session, input.as_bytes(), &mut output).unwrap();

        assert_eq!(output.iter().filter(|byte| **byte == b'\n').count(), 1);
        assert_eq!(output.last(), Some(&b'\n'));
    }

    #[test]
    fn blank_lines_are_passed_over() {
        let mut session = session();
        let input = "\n   \n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"mark\"}\n\r\n";

        let answers = exchange(named("plain").unwrap().as_mut(), &mut session, input);

        assert_eq!(answers.len(), 1);
        assert_eq!(answers[0]["id"], json!(1));
    }

    #[test]
    fn the_last_line_is_answered_without_a_line_break_after_it() {
        let mut session = session();

        let answers = exchange(
            named("plain").unwrap().as_mut(),
            &mut session,
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"mark\"}",
        );

        assert_eq!(answers[0]["result"]["text"], json!("mark 1"));
    }

    #[test]
    fn a_line_that_is_too_long_is_refused_and_the_next_one_is_answered() {
        let mut session = session();
        let mut input = "x".repeat(LONGEST_MESSAGE + 100);
        input.push_str("\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"mark\"}\n");

        let answers = exchange(named("plain").unwrap().as_mut(), &mut session, &input);

        assert_eq!(answers.len(), 2);
        assert_eq!(answers[0]["error"]["code"], json!(-32600));
        assert_eq!(answers[1]["id"], json!(2));
    }

    #[test]
    fn a_message_of_exactly_the_longest_length_is_read() {
        let mut session = session();
        let frame = "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"mark\",\"params\":{}}";
        let padding = " ".repeat(LONGEST_MESSAGE - frame.len());
        let input = format!("{frame}{padding}\n");

        let answers = exchange(named("plain").unwrap().as_mut(), &mut session, &input);

        assert_eq!(answers[0]["result"]["text"], json!("mark 1"));
    }
}
