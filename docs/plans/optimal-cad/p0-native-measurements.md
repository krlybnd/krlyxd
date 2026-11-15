# P0: Typst plan → native solid → measured checks

Date: 2026-11-15. This is an executable vertical slice, not automatic plan
synthesis or proof of complete shape equality.

The [Typst contract reader](../../../../../orplex/repos/orplex-kalyx/crates/krlysci/src/cad_contract.rs)
accepts a hash-bound `#cad_target` alongside `#cad_spec` and `#cad_plan`.
The typed checks are `require_volume(id, lower, upper)`,
`require_span(id, x|y|z, lower, upper)`, and
`require_material(id, x, y, z, true|false)`. `pi` is a dimensionless constant.
The output uses SI units. The [review delta](p0-review-delta.md) invalidates
both old plan and target. `cad-candidate` binds fresh ones to the new spec hash.

The [native checker](../../../crates/cli/src/cad_check.rs) consumes that JSON
without an absolute Cargo dependency between repositories. It maps the
typed operations to existing kernel plugins: disk/circle, rectangle,
annulus/profile, extrude, union and difference/subtract. The `measure`,
`bounds` and `ray` queries check the resulting solid separately from its
construction plan. The bridge validates schema version, parameter sorts,
plan DAG and binding hashes. It expects JSON from the krlysci parser; it does
not recompute the semantic hash of arbitrary JSON input.

Individual results are `pass`, `fail` or `unknown`. All checks passing yields
overall `tested`; a violated check yields `fail`; a boundary sample or
kernel/query refusal yields `unknown`. `geometry_verified` remains `false`:
finite measurements and samples cannot establish full material-set equality.
Exit code 0 means `tested`; `fail` and `unknown` exit 1.

The same path built and checked four distinct Typst fixtures, with five
passing checks each:

- [Ring](../../../cad-knowledge/cases/solid-ring.typ): volume `4.750088092227767e-6 m³`.
- [Block](../../../cad-knowledge/cases/solid-block.typ): volume `1.2000000000000002e-6 m³`.
- [Cylinder](../../../cad-knowledge/cases/solid-cylinder.typ): volume `1.5079644737231006e-6 m³`.
- [Cutout composition](../../../cad-knowledge/cases/solid-cutout.typ): rectangle
  extrusion minus cylinder extrusion; volume `1.0586283305884594e-6 m³`.

The [ring review](../../../cad-knowledge/cases/solid-ring-review.typ) changed
height from 8 to 10 mm. Its [new candidate](../../../cad-knowledge/cases/solid-ring-candidate-v1.typ)
bound a plan and target to revision 1 and rebuilt the native solid. All five
checks passed; volume became `5.937610115284709e-6 m³`. A deliberately wrong
volume interval returned `fail` with exit 1. The checker test requires an
on-boundary material probe to return `unknown`.

Reproduce from the krlyxd root after building both CLIs:

```sh
cargo build -p krly3d-cli --bin krly3d-cad-check --locked
cargo build --manifest-path /home/krlybnd/orplex/repos/orplex-kalyx/Cargo.toml -p krlysci-cli --locked
/home/krlybnd/orplex/repos/orplex-kalyx/target/debug/krlysci cad-contract cad-knowledge/cases/solid-cutout.typ > /tmp/solid-cutout-contract.json
target/debug/krly3d-cad-check /tmp/solid-cutout-contract.json
/home/krlybnd/orplex/repos/orplex-kalyx/target/debug/krlysci cad-review cad-knowledge/cases/solid-ring.typ cad-knowledge/cases/solid-ring-review.typ > /tmp/solid-ring-review.json
/home/krlybnd/orplex/repos/orplex-kalyx/target/debug/krlysci cad-candidate /tmp/solid-ring-review.json cad-knowledge/cases/solid-ring-candidate-v1.typ > /tmp/solid-ring-candidate-v1.json
target/debug/krly3d-cad-check /tmp/solid-ring-candidate-v1.json
```

QA: publication-as-code `cargo test --workspace --locked` passed 68 tests,
and its krlysci/CLI Clippy passed with `-D warnings`. krlyxd
`cargo test -p krly3d-cli --locked` passed 127 tests, and targeted Clippy
passed with `-D warnings`. The four fixtures, reviewed rebuild and negative
CLI probe also ran successfully.

No old production CAD code was removed here. The new native bridge is 482
physical lines; the Typst contract reader grew from 732 to 947 physical lines
at this checkpoint. This is an implementation cost. The publication-as-code
checkout has no HEAD, so its precise added/deleted diff is unavailable. The
krlyxd root Cargo files already had unrelated pending edits, so their full
Git diff is not attributable to this slice.

Remaining gates: stable frames/datums, structural review edits, full
material-set verification, candidate generation and cost search, Studio
selection/diff/undo, and persisted accepted history. These fixtures prescribe
their candidate plans; they do not demonstrate requirements-to-plan synthesis.
