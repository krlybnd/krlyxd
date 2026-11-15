# P1 pilot: target-only `.typ` → bounded candidate search

Date: 2026-11-15. This is a bounded search over the existing executable CAD
grammar, not a general optimal-construction solver.

The [target-only fixture](../../../cad-knowledge/cases/plus-target-only.typ)
contains `#cad_spec` and `#cad_target`, without `#cad_plan`. The
[Typst reader](../../../../../orplex/repos/orplex-kalyx/crates/krlysci/src/cad_contract.rs)
type-checks the scalar and geometric quantities. The
[native CLI](../../../crates/cli/src/cad_check.rs) accepts its JSON with
`--synthesize`. The [enumerator](../../../crates/cli/src/cad_check/cad_synth.rs)
tries disk, rectangle and annulus regions with parameter-bound extrusion,
then union or difference of two solids. It uses necessary analytic volume
and span bounds to discard impossible candidates. Every reported success is
rebuilt and checked by the existing native `measure`, `bounds` and `ray`
queries. There is no branch keyed to a product or object family.

The search is deterministic and capped at 1..6 positive length parameters,
two solids, and a configurable 1..50,000 native candidate attempts (default
2,000). Simple two-node plans are tried before Boolean plans; within a tier,
a numeric heuristic orders candidates. `cost_nodes` records the chosen plan's
node count, but global optimality is not established. The result always says
`optimality_proven:false` and `geometry_verified:false`. A failed or exhausted
search returns `unknown`, including when the candidate budget is exhausted;
it does not claim that no solution exists.

The target-only cross composition produced two perpendicular rectangle
extrusions joined by `union`, with 5 nodes. Native measurement returned
`6.999999999999999e-7 m³`; all 8 target checks passed in one native attempt.
The returned `plan` was also inserted into the original contract JSON and
replayed through the ordinary native checker; all 8 checks passed again.
Removing the handwritten plans from the prior ring, block, cylinder and
cutout fixtures also let the same search rebuild each one in one native
attempt, with all 5 checks passing in each case. The unit test additionally
forces a contradictory material sample with a one-attempt budget and checks
the explicit `unknown` result.

Reproduce the persistent target-only fixture:

```sh
/home/krlybnd/orplex/repos/orplex-kalyx/target/debug/krlysci cad-contract cad-knowledge/cases/plus-target-only.typ > /tmp/plus-target-only.json
target/debug/krly3d-cad-check --synthesize /tmp/plus-target-only.json 2000
```

QA: `cargo test -p krly3d-cli --locked` passed 128 tests; targeted Clippy
with `-D warnings` and workspace formatting passed. The fixture passed
through both CLIs, and four earlier cases passed with their plan declarations
removed into temporary test inputs. A temporary, parser-valid target with
contradictory material samples and a budget of one returned `unknown` and
exit code 1.

The same path also rebuilt a reviewed revision without a handwritten new
plan. The [ring review](../../../cad-knowledge/cases/solid-ring-review.typ)
raised `h` from 8 to 10 mm. The
[revision-1 target](../../../cad-knowledge/cases/solid-ring-target-v1.typ)
contains only `#cad_target`. `cad-candidate` bound it to the reviewed spec;
`--synthesize` found a 2-node annulus/extrude plan in one attempt. All five
checks passed; measured height was `0.01 m` and volume
`5.937610115284709e-6 m³`. Reproduce after building both CLIs:

```sh
/home/krlybnd/orplex/repos/orplex-kalyx/target/debug/krlysci cad-review cad-knowledge/cases/solid-ring.typ cad-knowledge/cases/solid-ring-review.typ > /tmp/solid-ring-review-synth.json
/home/krlybnd/orplex/repos/orplex-kalyx/target/debug/krlysci cad-candidate /tmp/solid-ring-review-synth.json cad-knowledge/cases/solid-ring-target-v1.typ > /tmp/solid-ring-target-v1.json
target/debug/krly3d-cad-check --synthesize /tmp/solid-ring-target-v1.json 2000
```

No production code was removed in this slice. The new enumerator is 329
physical Rust lines; the native CLI file grew from 482 to 535 physical lines
including its new test. This adds 382 physical Rust lines. It is a working
capability, not a code-saving refactor. This count excludes the 17-line
fixtures and documentation. The external Typst contract reader grew from
947 to 951 physical lines to allow target-only candidates after review.

The later [structural review and local history slice](p1-structural-review-history.md)
adds a typed geometry-check delta, a topological rebuild, and accepted CLI
revisions with undo/redo. Broader search, parameter solving, frames/datums,
counterexample refinement, Studio review and full material-set verification
remain open.
