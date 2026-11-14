//! Every model file in `models/` is a test: it must read, build into valid bodies, and
//! write back as the text it was read from.

// Helpers in a test file are test code too; the lint only knows `#[test]` functions.
#![allow(clippy::unwrap_used, clippy::panic)]

use krly3d_core::Accuracy;
use krly3d_kernel::ports::read::read;
use krly3d_kernel::ports::write::write;
use std::path::Path;

#[test]
fn every_model_file_builds_and_writes_back_unchanged() {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models");
    let mut files: Vec<_> = std::fs::read_dir(&folder).unwrap().map(|entry| entry.unwrap().path()).collect();
    files.retain(|file| file.extension().is_some_and(|extension| extension == "k3d"));
    files.sort();
    assert!(!files.is_empty(), "no model files in {}", folder.display());
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        let mut builder = krly3d_std::standard();
        builder.accuracy(Accuracy::for_scale(100.0).unwrap());
        let mut kernel = builder.build();

        let document = read(kernel.context(), "k3d", &text)
            .into_result()
            .unwrap_or_else(|error| panic!("{}: {error}", file.display()));
        let delta = kernel.open(document).unwrap_or_else(|error| panic!("{}: {error}", file.display()));

        assert!(delta.committed && !delta.nodes.is_empty(), "{}: nothing was built", file.display());
        let written = write(kernel.context(), "k3d", kernel.document()).into_result().unwrap();
        assert_eq!(written, text, "{} is not in the form the writer gives it", file.display());
    }
}
