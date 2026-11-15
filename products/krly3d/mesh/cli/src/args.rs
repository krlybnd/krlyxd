use krly3d_mesh::diagnostic;
use std::ffi::OsString;
use std::path::PathBuf;

use crate::Failure;

pub(crate) const USAGE: &str = "Usage: krly3d-mesh --model PATH [--request PATH|-]
       krly3d-mesh --describe
       krly3d-mesh --help

Read one native .k3d model and one canonical mesh operation request (protocol/API 2.0).
The request defaults to stdin; stdin EOF completes the request and does not cancel it.
Stdout contains canonical NDJSON: started, then completed/failed/cancelled.
Diagnostics go to stderr. SIGINT requests cancellation; a closed stdout cancels work.
Exit status: 0 completed, 1 rejected/failed or output error, 130 cancelled.
Input limits: model 16 MiB, request 1 MiB (UTF-8, including whitespace).
--describe prints the shared v2 descriptor and schemas as one JSON object.
";

pub(crate) enum Invocation {
    Help,
    Describe,
    Mesh { model: PathBuf, request: Option<PathBuf> },
}

pub(crate) fn parse(words: impl IntoIterator<Item = OsString>) -> Result<Invocation, Failure> {
    let mut words = words.into_iter().peekable();
    let mut model = None;
    let mut request = None;
    let mut saw_request = false;
    while let Some(word) = words.next() {
        match word.to_str() {
            Some("--help" | "-h" | "--describe") => {
                if model.is_some() || saw_request || words.peek().is_some() {
                    return Err(usage("--help and --describe must be used alone"));
                }
                return Ok(if word == "--describe" { Invocation::Describe } else { Invocation::Help });
            }
            Some("--model") => {
                if model.is_some() {
                    return Err(usage("--model was given more than once"));
                }
                model = Some(PathBuf::from(value(&mut words, "--model")?));
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
    if model == std::path::Path::new("-") {
        return Err(usage("--model requires a file path; stdin is reserved for the request"));
    }
    Ok(Invocation::Mesh { model, request })
}

fn value(words: &mut impl Iterator<Item = OsString>, option: &str) -> Result<OsString, Failure> {
    words
        .next()
        .filter(|word| !word.is_empty() && !word.to_string_lossy().starts_with("--"))
        .ok_or_else(|| usage(format!("{option} needs a path")))
}

fn usage(message: impl Into<String>) -> Failure {
    diagnostic("input", format!("{}\n\n{USAGE}", message.into())).into()
}
