use crate::diagnostic;
use krly3d_brep::Diagnostic;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

pub(crate) const USAGE: &str = "Usage: krly3d-brep [--request PATH|-]
       krly3d-brep --describe
       krly3d-brep --help

Build one planar B-rep extrusion in a new document synchronously.
Accepts the complete krly3d.brep.planar/1 JSON request, limited to 1 MiB
(1,048,576 bytes, including whitespace), encoded as UTF-8.
With no arguments or --request -, read stdin through EOF.
Success writes one canonical result JSON object and newline to stdout, exit 0.
Errors write a diagnostic JSON object to stderr, exit 1.
--describe prints the shared API descriptor and schemas as one JSON object.
--help (-h) and --describe must be used alone.
Synchronous execution has no in-flight request cancellation or timeouts.
SIGINT uses default OS handling and may terminate without a JSON result.
";

pub(crate) enum Invocation {
    Help,
    Describe,
    Build { request: Option<PathBuf> },
}

pub(crate) fn parse(words: impl IntoIterator<Item = OsString>) -> Result<Invocation, Diagnostic> {
    let mut words = words.into_iter();
    let invocation = match words.next() {
        None => Invocation::Build { request: None },
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
                Invocation::Build { request: (path != "-").then(|| PathBuf::from(path)) }
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
