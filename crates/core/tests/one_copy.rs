//! What must exist once in the workspace exists once: shared algorithms and the plugin
//! mould live in one module, and a second copy anywhere fails this test.

// Helpers in a test file are test code too; the lint only knows `#[test]` functions.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};

/// Every Rust source file under `crates/` and under `package/`, where the packages built on
/// the same core live.
fn sources() -> Vec<PathBuf> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut found = Vec::new();
    let mut waiting = vec![crates.join("../package"), crates];
    while let Some(folder) = waiting.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() && path.file_name().is_some_and(|name| name != "target") {
                waiting.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                found.push(path);
            }
        }
    }
    found
}

/// The files that hold `needle`, outside this test.
fn holding(needle: &str) -> Vec<PathBuf> {
    let this = Path::new(file!()).file_name().unwrap();
    sources()
        .into_iter()
        .filter(|path| path.file_name() != Some(this))
        .filter(|path| std::fs::read_to_string(path).unwrap().contains(needle))
        .collect()
}

#[test]
fn the_gauss_legendre_table_is_written_once() {
    let files = holding("0.960_289_856_497_536_2");

    assert_eq!(files.len(), 1, "{files:?}");
    assert!(files[0].ends_with("core/src/math/quadrature.rs"), "{files:?}");
}

#[test]
fn the_plugin_mould_is_written_once() {
    for needle in ["pub trait Plugin<", "pub struct Registry<", "pub trait Family"] {
        let files = holding(needle);
        assert_eq!(files.len(), 1, "`{needle}` in {files:?}");
        assert!(files[0].ends_with("core/src/plugin.rs"), "`{needle}` in {files:?}");
    }
}

#[test]
fn the_protocol_schema_is_written_once() {
    let files = holding("pub enum Value {");

    assert_eq!(files.len(), 1, "{files:?}");
    assert!(files[0].ends_with("core/src/schema.rs"), "{files:?}");
}

#[test]
fn the_document_mould_is_written_once() {
    for (needle, home) in [
        ("pub struct Statement {", "core/src/document/op.rs"),
        ("pub struct NodeRef {", "core/src/document/op.rs"),
        ("pub fn key_of(", "core/src/document/cache.rs"),
        ("pub struct Versions<", "core/src/document/versions.rs"),
        ("pub enum Relation {", "core/src/claim.rs"),
    ] {
        let files = holding(needle);
        assert_eq!(files.len(), 1, "`{needle}` in {files:?}");
        assert!(files[0].ends_with(home), "`{needle}` in {files:?}");
    }
}
