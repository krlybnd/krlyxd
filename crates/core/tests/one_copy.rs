//! What must exist once in the workspace exists once: shared algorithms and the plugin
//! mould live in one module, and a second copy anywhere fails this test.

// Helpers in a test file are test code too; the lint only knows `#[test]` functions.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// Every Rust source file under `crates/`, `package/`, and the optional `products/`
/// tree, where domain packages built on the same core live.
fn sources() -> Vec<PathBuf> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut found = Vec::new();
    let mut waiting = vec![crates.join("../package"), crates];
    let products = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../products");
    if products.exists() {
        waiting.push(products);
    }
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
    assert!(files[0].ends_with("krlyxd/src/math/quadrature.rs"), "{files:?}");
}

#[test]
fn raw_leaves_and_math_aliases_are_defined_once() {
    for (needle, home) in [
        ("pub struct Id(", "krlyxd/src/id.rs"),
        ("pub struct Confidence(", "krlyxd/src/quality.rs"),
        ("pub struct PositionalUncertainty(", "krlyxd/src/quality.rs"),
        ("pub struct ContractMetadata<", "krlyxd/src/metadata.rs"),
        ("pub struct Coordinates {", "krlyxd/src/metadata.rs"),
        ("pub trait Clock:", "krlyxd/src/clock.rs"),
        ("pub struct SystemClock {", "krlyxd/src/clock.rs"),
        ("pub struct FakeClock {", "krlyxd/src/clock.rs"),
        ("pub enum Exactness {", "krlyxd/src/exact.rs"),
        ("pub type Vec3 =", "krlyxd/src/math/mod.rs"),
        ("pub type Point3 =", "krlyxd/src/math/mod.rs"),
        ("pub type Uv =", "krlyxd/src/math/mod.rs"),
        ("pub type Mat4 =", "krlyxd/src/math/mod.rs"),
        ("pub type Iso3 =", "krlyxd/src/math/mod.rs"),
        ("pub type Sim3 =", "krlyxd/src/math/mod.rs"),
        ("pub fn nodes(from: f64, to: f64, pieces: usize)", "krlyxd/src/math/quadrature.rs"),
    ] {
        let files = holding(needle);
        assert_eq!(files.len(), 1, "`{needle}` in {files:?}");
        assert!(files[0].ends_with(home), "`{needle}` in {files:?}");
    }
}

#[test]
fn error_coupled_geometry_support_has_one_domain_owned_implementation() {
    for (needle, home) in [
        ("pub struct Accuracy {", "core/src/accuracy.rs"),
        ("pub fn frame(origin: &Point3", "core/src/math/frame.rs"),
        ("pub struct Basis {", "core/src/math/nurbs.rs"),
    ] {
        let files = holding(needle);
        assert_eq!(files.len(), 1, "`{needle}` in {files:?}");
        assert!(files[0].ends_with(home), "`{needle}` in {files:?}");
    }
}

#[test]
fn legacy_root_and_module_paths_keep_identical_raw_types() {
    macro_rules! identical {
        ($raw:ty, $($legacy:ty),+ $(,)?) => {
            $(assert_eq!(TypeId::of::<$raw>(), TypeId::of::<$legacy>());)+
        };
    }
    identical!(krlyxd::Id, krly3d_core::Id, krly3d_core::id::Id);
    identical!(krlyxd::Exactness, krly3d_core::Exactness, krly3d_core::exact::Exactness);
    identical!(krlyxd::FakeClock, krly3d_core::FakeClock, krly3d_core::clock::FakeClock);
    identical!(krlyxd::SystemClock, krly3d_core::SystemClock, krly3d_core::clock::SystemClock);
    identical!(dyn krlyxd::Clock, dyn krly3d_core::Clock, dyn krly3d_core::clock::Clock);
    identical!(krlyxd::math::Vec3, krly3d_core::math::Vec3);
    identical!(krlyxd::math::Point3, krly3d_core::math::Point3);
    identical!(krlyxd::math::Uv, krly3d_core::math::Uv);
    identical!(krlyxd::math::Mat4, krly3d_core::math::Mat4);
    identical!(krlyxd::math::Iso3, krly3d_core::math::Iso3);
    identical!(krlyxd::math::Sim3, krly3d_core::math::Sim3);

    // These assignments require the same implementation, without adapters or conversions.
    let raw: krlyxd::Id = krly3d_core::id::Id::pair(&"plane".into(), &"sphere".into());
    let legacy: krly3d_core::Id = raw;
    assert_eq!(legacy.as_str(), "plane|sphere");
    let clock: Box<dyn krlyxd::Clock> = Box::new(krly3d_core::FakeClock::default());
    let legacy_clock: Box<dyn krly3d_core::clock::Clock> = clock;
    assert_eq!(legacy_clock.now(), std::time::Duration::ZERO);
}

#[test]
fn legacy_quadrature_is_the_same_iterator_implementation() {
    fn same_type<T>(_: &T, _: &T) {}
    let raw = krlyxd::math::quadrature::nodes(-1.0, 2.0, 2);
    let legacy = krly3d_core::math::quadrature::nodes(-1.0, 2.0, 2);
    same_type(&raw, &legacy);

    assert_eq!(raw.collect::<Vec<_>>(), legacy.collect::<Vec<_>>());
    assert_eq!(krlyxd::math::quadrature::GAUSS_LEGENDRE_8, krly3d_core::math::quadrature::GAUSS_LEGENDRE_8);
}

/// Include build dependencies as production edges; dev dependencies cannot leak
/// test-only metadata parsing into the raw production dependency graph.
fn production_dependencies(node: &Value) -> impl Iterator<Item = &str> {
    node["deps"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|dependency| {
            dependency["dep_kinds"]
                .as_array()
                .unwrap()
                .iter()
                .any(|kind| kind["kind"].is_null() || kind["kind"] == "build")
        })
        .map(|dependency| dependency["pkg"].as_str().unwrap())
}

#[test]
fn production_edges_include_normal_and_build_but_skip_dev_only() {
    let node = serde_json::json!({
        "deps": [
            { "pkg": "normal", "dep_kinds": [{ "kind": null }] },
            { "pkg": "build", "dep_kinds": [{ "kind": "build" }] },
            { "pkg": "dev_only", "dep_kinds": [{ "kind": "dev" }] },
            { "pkg": "mixed", "dep_kinds": [{ "kind": "dev" }, { "kind": null }] },
            { "pkg": "build_and_dev", "dep_kinds": [{ "kind": "build" }, { "kind": "dev" }] }
        ]
    });

    assert_eq!(
        production_dependencies(&node).collect::<Vec<_>>(),
        ["normal", "build", "mixed", "build_and_dev"]
    );
}

fn production_closure<'a>(nodes: &BTreeMap<&'a str, &'a Value>, root: &'a str) -> BTreeSet<&'a str> {
    let mut found = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        if found.insert(id) {
            pending.extend(production_dependencies(nodes[id]));
        }
    }
    found
}

#[test]
fn metadata_gates_raw_and_mesh_production_dependencies_including_transitives() {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--locked", "--offline", "--all-features"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let metadata: Value = serde_json::from_slice(&output.stdout).unwrap();
    let packages: BTreeMap<_, _> = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|package| (package["id"].as_str().unwrap(), package))
        .collect();
    let nodes: BTreeMap<_, _> = metadata["resolve"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| (node["id"].as_str().unwrap(), node))
        .collect();
    let raw = packages.iter().find(|(_, package)| package["name"] == "krlyxd").unwrap().0;
    let direct: Vec<_> = production_dependencies(nodes[raw]).collect();
    assert_eq!(direct.len(), 1, "raw may depend directly only on nalgebra: {direct:?}");
    assert_eq!(packages[direct[0]]["name"], "nalgebra");

    // A shared planar representation may use raw vocabulary, but cannot acquire
    // image/format codecs or depend back on one of its geometry consumers.
    let planar = packages.iter().find(|(_, package)| package["name"] == "krly2d-api").unwrap().0;
    let planar_direct: Vec<_> = production_dependencies(nodes[planar]).collect();
    assert_eq!(planar_direct.len(), 1, "krly2d API depends only on raw vocabulary");
    assert_eq!(packages[planar_direct[0]]["name"], "krlyxd");

    let allowed_numeric_dependencies = BTreeSet::from([
        "approx",
        "autocfg",
        "bytemuck",
        "glam",
        "matrixmultiply",
        "nalgebra",
        "nalgebra-macros",
        "num-bigint",
        "num-complex",
        "num-integer",
        "num-rational",
        "num-traits",
        "proc-macro2",
        "quote",
        "rawpointer",
        "safe_arch",
        "simba",
        "syn",
        "typenum",
        "unicode-ident",
        "wide",
    ]);
    let numeric_closure = production_closure(&nodes, direct[0]);
    let raw_closure = production_closure(&nodes, raw);
    for id in raw_closure.into_iter().filter(|id| id != raw) {
        let package = packages[id];
        let name = package["name"].as_str().unwrap();
        assert!(numeric_closure.contains(id), "non-nalgebra dependency in raw: {name}");
        assert!(allowed_numeric_dependencies.contains(name), "unreviewed raw dependency: {name}");
        assert!(
            package["source"].as_str().is_some_and(|source| source.starts_with("registry+")),
            "raw must not depend on local domain, CLI or MCP packages: {name}"
        );
    }
    // The B-rep bridge may reach mesh data through legacy std, but does not own
    // a mesh conversion implementation or directly import another domain engine.
    let brep = packages.iter().find(|(_, package)| package["name"] == "krly3d-brep-engine").unwrap().0;
    for id in production_dependencies(nodes[brep]) {
        let package = packages[id];
        if package["source"].is_null() {
            let name = package["name"].as_str().unwrap();
            assert!(
                ["krly3d-brep", "krly2d-api", "krlyxd", "krly3d-core", "krly3d-kernel", "krly3d-std"]
                    .contains(&name),
                "B-rep engine directly imports another domain or adapter: {name}"
            );
        }
    }
    // MCP adapters may share transport parsing, which must stay independent of
    // local geometry/domain crates. CLI adapters do not need that transport.
    let transport = packages.iter().find(|(_, package)| package["name"] == "krly-mcp-json").unwrap().0;
    for id in production_dependencies(nodes[transport]) {
        let package = packages[id];
        assert!(
            package["source"].as_str().is_some_and(|source| source.starts_with("registry+")),
            "MCP JSON transport imports a local domain crate: {}",
            package["name"]
        );
    }
    // Domain adapters compose their public contract and engine, never geometry internals.
    for (adapter, allowed) in [
        ("krly3d-view-cli", ["krly3d-view-api", "krly3d-view-renderer"]),
        ("krly3d-view-mcp", ["krly3d-view-api", "krly3d-view-renderer"]),
        ("krly3d-brep-cli", ["krly3d-brep", "krly3d-brep-engine"]),
        ("krly3d-brep-mcp", ["krly3d-brep", "krly3d-brep-engine"]),
        ("krlyvis-drawing-cli", ["krlyvis-api", "krlyvis-engine"]),
        ("krlyvis-drawing-mcp", ["krlyvis-api", "krlyvis-engine"]),
    ] {
        let root = packages.iter().find(|(_, package)| package["name"] == adapter).unwrap().0;
        for dependency in nodes[root]["deps"].as_array().unwrap() {
            if !dependency["dep_kinds"].as_array().unwrap().iter().any(|kind| kind["kind"].is_null()) {
                continue;
            }
            let package = packages[dependency["pkg"].as_str().unwrap()];
            if package["source"].is_null() {
                let name = package["name"].as_str().unwrap();
                let shared_transport = adapter.ends_with("-mcp") && name == "krly-mcp-json";
                assert!(
                    allowed.contains(&name) || shared_transport,
                    "{adapter} directly imports geometry internals: {name}"
                );
            }
        }
    }
    // Prevent adapters from creeping into the contract or the computation layer.
    gate_runtime_dependencies(&packages, &nodes);
    // The temporary engine -> legacy geometry bridge remains explicitly allowed.
    for (name, allowed_local) in [
        ("krly2d-api", &["krly2d-api", "krlyxd"][..]),
        ("krlyvis-api", &["krlyvis-api", "krlyxd"][..]),
        (
            "krlyvis-engine",
            &[
                "krlyvis-engine",
                "krlyvis-api",
                "krly2d-api",
                "krlyvis-kernel",
                "krlyvis-std",
                "krly3d-core",
                "krlyxd",
            ][..],
        ),
        ("krly3d-mesh", &["krly3d-mesh", "krlyxd"][..]),
        ("krly3d-brep", &["krly3d-brep", "krlyxd"][..]),
        ("krly3d-view-api", &["krly3d-view-api", "krly3d-brep", "krlyxd"][..]),
        (
            "krly3d-view-renderer",
            &[
                "krly3d-view-renderer",
                "krly3d-view-api",
                "krly3d-view",
                "krly3d-view-mesh",
                "krly3d-brep",
                "krly3d-brep-engine",
                "krly2d-api",
                "krly3d-mesh",
                "krly3d-mesh-engine",
                "krlyxd",
                "krly3d-core",
                "krly3d-kernel",
                "krly3d-std",
            ][..],
        ),
        (
            "krly3d-brep-engine",
            &[
                "krly3d-brep-engine",
                "krly2d-api",
                "krly3d-brep",
                "krly3d-mesh",
                "krlyxd",
                "krly3d-core",
                "krly3d-kernel",
                "krly3d-std",
            ][..],
        ),
        (
            "krly3d-mesh-engine",
            &[
                "krly3d-mesh-engine",
                "krly3d-brep-engine",
                "krly2d-api",
                "krly3d-brep",
                "krly3d-mesh",
                "krlyxd",
                "krly3d-core",
                "krly3d-kernel",
                "krly3d-std",
            ][..],
        ),
        ("krly3d-view", &["krly3d-view", "krly3d-mesh", "krlyxd", "krly3d-core", "krly3d-kernel"][..]),
        (
            "krly3d-view-mesh",
            &[
                "krly3d-view-mesh",
                "krly3d-view",
                "krly3d-mesh-engine",
                "krly3d-brep-engine",
                "krly2d-api",
                "krly3d-brep",
                "krly3d-mesh",
                "krlyxd",
                "krly3d-core",
                "krly3d-kernel",
                "krly3d-std",
            ][..],
        ),
    ] {
        let root = packages.iter().find(|(_, package)| package["name"] == name).unwrap().0;
        for id in production_closure(&nodes, root) {
            let package = packages[id];
            if package["source"].is_null() {
                let dependency = package["name"].as_str().unwrap();
                assert!(allowed_local.contains(&dependency), "{name} imports forbidden layer {dependency}");
            }
        }
    }
}

fn gate_runtime_dependencies(packages: &BTreeMap<&str, &Value>, nodes: &BTreeMap<&str, &Value>) {
    for (name, allowed) in [
        (
            "krly3d-reconstruction",
            &[
                "krlyxd",
                "krlyvis-api",
                "krlyvis-engine",
                "krly3d-brep",
                "krly3d-brep-engine",
                "krly2d-api",
                "krly3d-mesh",
                "krly3d-mesh-engine",
                "krly3d-view-api",
                "krly3d-view-renderer",
            ][..],
        ),
        (
            "krly3d-reconstruction-cli",
            &["krly3d-reconstruction", "krlyvis-api", "krly3d-mesh", "krly3d-view-api", "krlyxd"][..],
        ),
    ] {
        let root = packages.iter().find(|(_, package)| package["name"] == name).unwrap().0;
        for id in production_dependencies(nodes[root]) {
            let package = packages[id];
            if package["source"].is_null() {
                let dependency = package["name"].as_str().unwrap();
                assert!(
                    allowed.contains(&dependency),
                    "{name} imports forbidden direct dependency {dependency}"
                );
            }
        }
        if name == "krly3d-reconstruction" {
            for id in production_closure(nodes, root) {
                let dependency = packages[id]["name"].as_str().unwrap();
                assert!(
                    !dependency.ends_with("-cli") && !dependency.ends_with("-mcp"),
                    "host imports transport {dependency}"
                );
            }
        }
    }
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
