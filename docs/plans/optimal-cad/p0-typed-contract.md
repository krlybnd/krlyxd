# P0 typed `.typ` contract: first executable fragment

Date: 2026-11-15. This is a bounded input and static-plan-validation slice,
not a completed CAD synthesizer or a geometry verifier. It extends the same
krlysci Rust package that already reads `.typ`; there is no parallel TypeScript
mathematics implementation and no object-name dispatch.

The [Rust contract reader](../../../../../orplex/repos/orplex-kalyx/crates/krlysci/src/cad_contract.rs)
uses `typst-syntax` for the input syntax tree. The public `read_cad_contract`
API returns separate `DesignSpec` and optional `FeaturePlan` values. The
[CLI command](../../../../../orplex/repos/orplex-kalyx/crates/krlysci-cli/src/main.rs)
exposes this reader:

```sh
cargo build --manifest-path /home/krlybnd/orplex/repos/orplex-kalyx/Cargo.toml -p krlysci-cli --locked
/home/krlybnd/orplex/repos/orplex-kalyx/target/debug/krlysci cad-contract cad-knowledge/cases/quantity-contract.typ
```

The [sample](../../../cad-knowledge/cases/quantity-contract.typ) contains one
`#cad_spec` with named parameters and `require(id, predicate)` expressions.
Typst's own `15mm`, `1in`, `90deg` and `1rad` literals are converted to SI
values. `count(3)` is a distinct integer sort. The supported arithmetic is
`+`, `-`, `*`, `/` over scalar/length/area/volume, plus restricted angle
scaling. A requirement may compare values of exactly the same dimension.
No boolean-to-number conversion, silent unit omission on a dimensioned
comparison, unknown symbol, unknown CAD declaration or partially parsed
Typst syntax is accepted. A false requirement remains a reported `holds:false`
check and makes the CLI exit 1; it is distinct from a malformed input error.

The optional `#cad_plan` names the exact `semantic_hash` of the spec and a
node dictionary. For this static pilot the operation grammar is `disk`,
`rectangle`, `annulus`, `extrude`, `union`, `difference`. The validator checks
parameter sort and sign, node input/output kinds, cycles, unknown references,
one Solid output, and whether every node contributes to that output. The
operation is stored as a Rust enum in the parsed plan. It does
not execute these operations. The plan's shape is **not** asserted to satisfy
the spec. Every JSON result says `geometry_verified:false`.

The [focused tests](../../../../../orplex/repos/orplex-kalyx/crates/krlysci/tests/cad_contract.rs)
cover canonical ordering/renaming, unit and count separation, non-finite
values, failed predicates, missing plan inputs, cycles, wrong kinds, unused
nodes and stale spec hashes. `cargo test --workspace --locked` passed (59
tests), as did `cargo clippy -p krlysci -p krlysci-cli --all-targets --locked --
-D warnings`. The sample CLI run exited 0; two modified-input CLI runs exited
1 with the expected false predicate and dimension diagnostic. A CLI run with
an attached plan accepted its typed DAG and rejected a stale spec hash.

This first fragment adds a generic contract reader rather than deleting
existing code. The new Rust module currently has 696 physical lines and its
focused tests 117; the sample adds 9 `.typ` lines. These are a cost to review
against the promised future removal of hand-coded paths, not a code saving.
The external source checkout has no Git HEAD commit, so a normal Git diff
cannot provide a trustworthy before/after line count for its existing files.

The subsequent [review-delta slice](p0-review-delta.md) now implements a
bounded, revision-checked parameter/predicate correction with preservation at
the scalar-contract level.
The further [native measurement slice](p0-native-measurements.md) executes
supplied plans and tests three generic geometric facts; it does not prove the
whole target shape.

Remaining P0 gaps: geometric material-set predicates and independent
verification, named frames/datums, exact decimal/rational equality, tolerance
policy, geometric/structural review deltas, objective-cost terms, versioned
capabilities, bounded execution status, three fully geometric declarative
fixtures and a withheld composition. The scalar checks evaluate the current
parameter assignment only. They do not prove a universal parameter interval,
wall thickness of a solid, through holes, or optimum construction. No solver,
kernel lowering, marketplace or local accepted-intent memory is present.
