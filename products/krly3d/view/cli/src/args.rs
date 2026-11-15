use krly3d_view_api::{diagnostic, Diagnostic};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub(crate) const USAGE: &str = "Usage: krly3d-view --model PATH [--request PATH|-]
       krly3d-view --describe
       krly3d-view --help

Render one node from a native .k3d model using synchronous view protocol/API 1.0.
The request defaults to stdin; EOF completes the single JSON request.
Success writes exactly one result JSON object and a newline to stdout.
Errors write a diagnostic JSON object to stderr and exit 1; success exits 0.
Input limits: model 16 MiB, request 1 MiB (UTF-8, including whitespace).
--describe prints the shared descriptor with call_schema and output_schema.
--help (-h) prints this usage text. --help and --describe must be used alone.
--operations accepts protocol/API 2.0 and emits Started then one terminal NDJSON event.
Optional timeout_ms applies from submission; zero expires immediately.
--describe-operations prints its descriptor. Output failure cancels work.
SIGINT uses default OS handling
and may terminate the process without a diagnostic or result.
";

pub(crate) enum Invocation {
    Help,
    Describe,
    DescribeOperations,
    Render { model: PathBuf, request: Option<PathBuf>, operations: bool },
}

pub(crate) fn parse(words: impl IntoIterator<Item = OsString>) -> Result<Invocation, Diagnostic> {
    let mut words = words.into_iter().peekable();
    let mut operations = false;
    let mut model = None;
    let mut request = None;
    let mut saw_request = false;
    while let Some(word) = words.next() {
        match word.to_str() {
            Some("--help" | "-h" | "--describe" | "--describe-operations") => {
                if model.is_some() || saw_request || operations || words.peek().is_some() {
                    return Err(usage("--help and --describe must be used alone"));
                }
                return Ok(if word == "--describe" {
                    Invocation::Describe
                } else if word == "--describe-operations" {
                    Invocation::DescribeOperations
                } else {
                    Invocation::Help
                });
            }
            Some("--operations") => {
                if operations {
                    return Err(usage("--operations was given more than once"));
                }
                operations = true;
            }
            Some("--model") => {
                if model.is_some() {
                    return Err(usage("--model was given more than once"));
                }
                let path = PathBuf::from(value(&mut words, "--model")?);
                if path == Path::new("-") {
                    return Err(usage("--model requires a file path; stdin is reserved for the request"));
                }
                model = Some(path);
            }
            Some("--request") => {
                if saw_request {
                    return Err(usage("--request was given more than once"));
                }
                saw_request = true;
                let path = value(&mut words, "--request")?;
                request = (path != "-").then(|| PathBuf::from(path));
            }
            _ => return Err(usage(format!("unknown argument {word:?}"))),
        }
    }
    let model = model.ok_or_else(|| usage("--model PATH is required"))?;
    Ok(Invocation::Render { model, request, operations })
}

fn value(words: &mut impl Iterator<Item = OsString>, option: &str) -> Result<OsString, Diagnostic> {
    words
        .next()
        .filter(|word| !word.is_empty() && word != "-h" && !word.to_string_lossy().starts_with("--"))
        .ok_or_else(|| usage(format!("{option} needs a path")))
}

fn usage(message: impl Into<String>) -> Diagnostic {
    diagnostic("input", format!("{}\n\n{USAGE}", message.into()))
}
