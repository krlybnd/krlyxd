# P0 review-delta: executable typed correction

Date: 2026-11-15. The second executable slice is a **specification revision**,
not a native CAD edit. The [Rust review reader](../../../../../orplex/repos/orplex-kalyx/crates/krlysci/src/cad_review.rs)
accepts one `#cad_review` from Typst syntax. It requires a model ID, exact base
revision and semantic hash. `updates` uses `change(old, new)` for a typed
parameter; `replace` uses `change_requirement(old, new)` for a named predicate;
`add` introduces a named predicate; `retract` uses
`retract_requirement(old)` and requires an exact old predicate; `preserve`
names parameters or requirements that may not be changed. Other
requirements remain in place by default. A change is rejected if the old value
or predicate is wrong, a dimension changes, an unchanged hard requirement fails,
or an equality involving a modified parameter lacks explicit replacement.

The output contains complete before/after specifications and a revision record.
It carries `geometry_verified:false`. Any existing `FeaturePlan` is recorded as
invalidated; it is not silently rebound to the new spec hash. JSON output can
serve as the next revision's base after internal consistency checks. This is
local replay input, not authenticated provenance, automatic approval, persisted
history, kernel undo or geometric verification.

The [first review](../../../cad-knowledge/cases/quantity-review.typ) raises
`h` from 8 to 10 mm while explicitly preserving the two radii and three named
relations. The [second review](../../../cad-knowledge/cases/quantity-review-2.typ)
widens `R` from 15 to 16 mm against revision 1. Run them in order:

```sh
cd /home/krlybnd/orplex/repos/orplex-kalyx
cargo run -q -p krlysci-cli --locked -- cad-review /home/krlybnd/krlybnd/krlyxd/cad-knowledge/cases/quantity-contract.typ /home/krlybnd/krlybnd/krlyxd/cad-knowledge/cases/quantity-review.typ > /tmp/krlyxd-cad-review-state.json
cargo run -q -p krlysci-cli --locked -- cad-review /tmp/krlyxd-cad-review-state.json /home/krlybnd/krlybnd/krlyxd/cad-knowledge/cases/quantity-review-2.typ
```

The [focused tests](../../../../../orplex/repos/orplex-kalyx/crates/krlysci/tests/cad_review.rs)
exercise success, a second revision, wrong model/revision/hash/old value,
preservation, violated hard constraints, equality replacement, explicit
addition/retraction, tampered JSON and plan invalidation.
`cargo test --workspace --locked` passed all 66 tests;
`cargo clippy -p krlysci -p krlysci-cli --all-targets --locked -- -D warnings`
and formatting passed. Both CLI examples exited 0. The first JSON reports
revision 1 and `geometry_verified:false`; the second reports revision 2.

This slice **does not remove existing production code**. The new review module
is 450 physical lines and its focused tests 164 physical lines at this
checkpoint, plus two small `.typ` fixtures. The external checkout has no Git
HEAD, so a precise repo-wide added/deleted line diff is unavailable. Count this
as an implementation cost, not a code-saving claim. The previously recorded
191-line net reduction belongs to the earlier krlyxd baseline only.
The contract and review readers share one CAD-declaration collector rather
than maintaining two AST walkers (7 fewer physical lines across the two modules
at the pre-add/retract checkpoint).

Remaining P0/P1/P2 gates: geometric predicates and independent material-set
verification, a typed plan-to-kernel adapter, search/cost/unknown status,
selection-bound Studio review, geometric diff, acceptance/undo and persisted
local history. Natural-language review interpretation is not implemented by
this deterministic parser; an agent must propose the explicit delta.

Follow-up: the [native measurement slice](p0-native-measurements.md) now
rebinds a fresh plan and target to a reviewed revision and rebuilds its solid.
The [bounded synthesis pilot](p1-bounded-synthesis.md) also accepts a
target-only reviewed candidate and constructs a fresh plan automatically.
Full material-set verification and Studio review remain open.
