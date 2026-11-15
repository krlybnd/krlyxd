# D4 Rust duplication audit and narrow extraction

Date: 2026-11-15. Baselines are captured working-tree files, not HEAD: this workspace already contains unrelated changes and untracked source. D1's parser is excluded. No Cargo, root plan/accounting or kernel mathematics files were edited.

## Changed ownership

The mesh and view runners had identical public `RunnerLimits` fields/defaults and identical validity predicates. The inert configuration now has one owner: [runner_limits.rs](../../../crates/krlyxd/src/runner_limits.rs), exposed by [krlyxd's module root](../../../crates/krlyxd/src/lib.rs). The existing [mesh runner](../../../products/krly3d/mesh/engine/src/runner.rs) and [view runner](../../../products/krly3d/view/renderer/src/runner.rs) publicly re-export it at their existing `runner::RunnerLimits` paths and call `is_valid()`.

All seven fields, their visibility/types, `Clone`/`Copy`/`Debug`, default values and constructor error code/message are preserved. The two paths now intentionally name the same Rust type; runtime behavior is unchanged. The shared module starts no threads, imports no engine/transport, owns no event encoding and introduces no new crate/dependency. Byte scopes still belong to the individual runners. The exact validity condition remains six strictly positive fields and `max_queued_events >= 2`.

Consumers were found in mesh/view CLI and MCP production adapters, their tests, mesh engine runner/parity tests and view renderer operation tests. These remain consumers through their original import paths. The larger runner implementations remain separate: their prepared tasks, typed results/events, canonical encoded-byte accounting, diagnostic text and operation ID namespaces are domain-owned. A full extraction would require a parameterized execution/event adapter and broader lifecycle regression work; it is not a safe file deletion.

## Audit findings retained

A lexical screening of 465 Rust files under crates/package/products found no byte-identical whole-file groups and no non-entrypoint source stem without any module declaration/path reference among 290 screened source files. This is candidate screening, not proof of reachability: identical stems can occur in unrelated modules. Local scan evidence is in `.tmp/refactor/d4-rust/audit.json`.

- View base64 and PNG wrappers have live use: picture production uses cancellable encoding; renderer production also uses checked PNG encoding; integration tests and public API users use the unchecked wrappers. Removing a wrapper as apparently uncalled would change the public surface.
- Mesh/view/B-rep bounded-input helpers are not behaviorally identical. Mesh has an interruptible reader thread and connection checks; view and B-rep are synchronous, use different limit parameters and preserve different diagnostic messages. Existing process tests exercise those differences. No shared reader abstraction was added.
- Repeated tiny diagnostic constructors return the same neutral pair, but their visibility and dependency placement differ. Adding a helper framework solely to remove three-line constructors is unjustified here.
- krlyvis port modules are declared and publicly exposed. This audit does not infer that public plugins are dead merely because there is no direct local caller.

## Archive and restore

Before mutation, complete originals were copied to `.tmp/refactor/d4-rust/<original-path>` for the two runners and krlyxd module root. Their source hashes and line counts are recorded in `.tmp/refactor/d4-rust/baseline.json`. These are ignored local recovery files, not a durable backup.

To restore D4 only: copy the archived runner files to their original paths; remove only `pub mod runner_limits;` from the current krlyxd module root; remove the newly added shared module. If no later changes touched the module root, its archived full original may instead be restored. Check for concurrent edits first; never overwrite another change with a whole-file restore. No manifest or lockfile restoration is needed.

## Physical-line accounting

- Two original runners: 395 + 451 = 846 lines.
- Two resulting runners: 361 + 417 = 778 lines; gross removal from original locations: 68 lines.
- Shared configuration/validator: 40 new production lines; module registration: 1 line.
- Net active Rust reduction: **27 physical lines**. Added test lines: 0; generated source: 0; manifests/lock metadata: 0.
- This ledger is documentation overhead, reported separately from production reduction. The local audit/archive JSON is recovery evidence outside active source.

## Verification

Before extraction, the 18 mesh runner and 4 view operations tests passed. The same command passed after extraction:

```sh
cargo test --locked --offline -p krly3d-mesh-engine --test runner -p krly3d-view-renderer --test operations
```

These 22 existing tests cover invalid/zero limits, two-event admission, encoded-byte reservation/overflow, retained requests, cancellation/deadline precedence, detached workers, panic cleanup and snapshot results. Existing tests already exercise the moved predicate; no tests mirroring the implementation were added.

```sh
cargo check --locked --offline --all-targets -p krly3d-mesh-cli -p krly3d-mesh-mcp -p krly3d-view-cli -p krly3d-view-mcp
```

All four production consumer packages and their targets compile through the preserved re-export paths. Exploratory pre-change verification also passed 4 base64 unit tests and 22 view/B-rep CLI unit/process tests. These are retained-boundary evidence, not extra coverage attributed to the extraction. Local Markdown links, whitespace and targeted diff checks pass.

Root integration verification after the extraction: `cargo test --workspace --locked --offline -q --no-fail-fast` and `cargo check --workspace --all-targets --locked --offline -q` both completed with exit code 0.
