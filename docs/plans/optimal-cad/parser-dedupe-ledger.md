# D1 strict MCP JSON parser migration ledger

Date: 2026-11-15. Scope: PLAN §10 D1 only. Measurements count physical lines, including blanks/comments/inline tests, from captured working-tree originals rather than the already dirty HEAD diff.

## Ownership and replacement

All four MCP adapters now alias the transport-only [krly-mcp-json](../../../crates/mcp-json/src/lib.rs) crate as their private `json` module. It contains the original implementation with only `pub(crate)` visibility widened to `pub`. The raw mathematical kernel has no dependency on this crate. Framing and lifecycle implementations remain in their existing adapters.

Dependencies: the shared crate uses existing `serde` and `serde_json` (`raw_value`) packages. Each adapter gains one local path dependency. Mesh, B-rep and vision drop their now-unused direct `serde` dependency; view retains `serde` for `EncodedResult` deserialization. No new registry package/version is introduced. Root workspace membership was coordinated with the root agent.

## Archived originals

Each original below is archived at `.tmp/refactor/d1-parser/<original-path>`. Its replacement owner is the shared crate above; reason: verified identical behavior, with only provenance comments differing. No active adapter declares `mod json` or imports these source paths.

- `products/krly3d/mesh/mcp/src/json.rs`: 118 lines; SHA-256 `f90ef29dfefc3605e665439963e01c127dbb4ad94d88daf10c66cbc40149affa`.
- `products/krly3d/view/mcp/src/json.rs`: 118 lines; SHA-256 `f90ef29dfefc3605e665439963e01c127dbb4ad94d88daf10c66cbc40149affa`.
- `products/krly3d/brep/mcp/src/json.rs`: 119 lines; SHA-256 `147e017be935ab1f3663c8ded75bef1493ef17e2cf6464be07dc09c0755732ab`.
- `products/krlyvis/mcp/src/json.rs`: 119 lines; SHA-256 `f13e7b90c5cc790e9f88e815f3b16ba8290c14bddc821488894db183485bf4f2`.

Restore: copy each archived original to its original path, and restore the four adapter manifests and library roots from `.tmp/refactor/d1-parser/baseline/<original-path>`. Remove the newly added shared crate/member and its four lockfile edges plus package entry to fully undo D1. Do not overwrite unrelated dirty changes. The ignored archive is local recovery material, not a published backup. Baseline/after path, hash and physical-line snapshots are in `.tmp/refactor/d1-parser/{baseline,after}.json`.

## Physical line accounting

- Original parser files retired: 474 lines (118 + 118 + 119 + 119).
- Retained shared implementation: 118 handwritten production lines. Gross duplicate removal: 356 lines. All four import replacements are line-neutral.
- New parser regression tests: 43 lines. The workspace dependency-boundary test needed a 12-line net adjustment to admit this exact transport crate for MCP adapters while asserting that the crate cannot import local domain code. Existing adapter/process tests are retained unchanged.
- Rust source net reduction including those tests: 301 physical lines. Added generated source: 0.
- Manifest overhead: 13 lines for the shared manifest; adapter manifests add four dependency lines and remove three serde lines; root workspace adds one member line. Total manifest delta: +15 lines.
- Attributable lockfile delta: +9 lines (one 8-line local package block, four edges added, three serde edges removed). This is not the entire preexisting dirty lockfile diff.
- Net active reduction including Rust, tests, manifests and attributable lock metadata: 277 physical lines. Documentation is reported separately below.

## Verification

- `cargo test -p krly-mcp-json -p krly3d-mesh-mcp -p krly3d-view-mcp -p krly3d-brep-mcp -p krlyvis-drawing-mcp`: 72 tests passed, including binary/process suites.
- The same five packages pass `cargo check --all-targets` and `cargo clippy --all-targets --no-deps`.
- Fresh external target directory: `cargo check --locked --all-targets` for all five packages passes while the entire D1 archive/baseline directory is moved outside the workspace; it is restored afterward. This demonstrates no dependency on D1 archived files. Other concurrent work’s `.tmp` contents were not moved.
- Consumer scan finds one `UniqueVisitor`, in the shared crate, no local parser module declarations, and all four local path dependencies.
- The workspace `cargo check --workspace --all-targets --locked --offline` passes; the dependency-boundary test passes after its exact transport exception. After the separate reconstruction regression and canonical fixture fixes, `cargo test --workspace --locked --offline -q --no-fail-fast` passes across the full workspace.
- Preserved coverage includes nested/escaped duplicate aliases, parse-vs-invalid-request codes/messages, NaN/Infinity and overflow exponent rejection, byte-exact large-integer/decimal raw tokens, domain diagnostics, malformed fields, UTF-8 framing, message limits, cancellation and process lifecycle. Direct shared regression tests add explicit parser error/message and raw-token assertions.

Documentation delta: this ledger adds 40 physical lines; no theory or plan file is changed by D1.
