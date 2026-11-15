use crate::diagnostic;
use krlyvis_api::Diagnostic;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

pub(crate) const USAGE: &str = "Usage: krlyvis-drawing [--request PATH|-]
       krlyvis-drawing --describe
       krlyvis-drawing --help

Read one embedded drawing image synchronously, without writing files.
Accepts the complete krlyvis.drawing/1 JSON request, limited to 24 MiB
(25,165,824 bytes, including whitespace), encoded as UTF-8.
With no arguments or --request -, read stdin through EOF.
Success writes one canonical result JSON object and newline to stdout, exit 0.
Errors write a diagnostic JSON object to stderr, exit 1.
Image data is base64, with a 16 MiB decoded limit enforced by the API.
--describe prints the shared API descriptor and schemas as one JSON object.
--help (-h) and --describe must be used alone.
Synchronous execution has no in-flight request cancellation or timeouts.
SIGINT uses default OS handling and may terminate without a JSON result.
";

pub(crate) enum Invocation {
    Help,
    Describe,
    Read { request: Option<PathBuf> },
}

pub(crate) fn parse(words: impl IntoIterator<Item = OsString>) -> Result<Invocation, Diagnostic> {
    let mut words = words.into_iter();
    let invocation = match words.next() {
        None => Invocation::Read { request: None },
        Some(word) => match word.to_str() {
            Some("--help" | "-h") => Invocation::Help,
            Some("--describe") => Invocation::Describe,
            Some("--request") => {
                let path = words
                    .next()
                    .filter(|path| {
                        !path.is_empty()
                            && path != OsStr::new("-h")
                            && !path.to_string_lossy().starts_with("--")
                    })
                    .ok_or_else(|| usage("--request needs a path or -"))?;
                Invocation::Read { request: (path != "-").then(|| PathBuf::from(path)) }
            }
            _ => return Err(usage(format!("unknown argument {word:?}"))),
        },
    };
    if words.next().is_some() {
        return Err(usage("unexpected extra argument; options must be used alone"));
    }
    Ok(invocation)
}

fn usage(message: impl Into<String>) -> Diagnostic {
    diagnostic("input", format!("{}\n\n{USAGE}", message.into()))
}
