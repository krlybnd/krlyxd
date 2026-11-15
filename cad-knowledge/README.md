# CAD knowledge: executable `.typ` probes

The [typed quantity contract](cases/quantity-contract.typ) runs through the
current krlysci `cad-contract` command:

```sh
/home/krlybnd/orplex/repos/orplex-kalyx/target/debug/krlysci cad-contract cad-knowledge/cases/quantity-contract.typ
```

It checks named length parameters, dimensioned arithmetic and explicit
requirements, returning a semantic spec hash and `geometry_verified:false`.
The [P0 ledger](../docs/plans/optimal-cad/p0-typed-contract.md) records its
accepted grammar, tests and limits. This is a distinct, bounded contract
reader in the same krlysci package; `krlysci run` below remains the original
numeric equation evaluator.

The [first review](cases/quantity-review.typ) and [second review](cases/quantity-review-2.typ)
demonstrate revision-bound parameter corrections and preservation without a
marketplace. Their [review ledger](../docs/plans/optimal-cad/p0-review-delta.md)
gives the exact two-command run and limits. The revisions update the typed
intent; they do not yet rebuild a CAD solid.

The later [native measurement slice](../docs/plans/optimal-cad/p0-native-measurements.md)
adds four `.typ` candidate plans and geometric targets. It also rebuilds a
reviewed ring with the native kernel. These are tests of supplied plans, not
automatic plan synthesis or full geometry proofs.

The [bounded synthesis pilot](../docs/plans/optimal-cad/p1-bounded-synthesis.md)
starts from a [target-only `.typ` case](cases/plus-target-only.typ) and finds a
plan using the same generic operations. Its native result passes eight checks,
while keeping both shape proof and global optimality explicitly unproven.
After a typed review, the [target-only revision](cases/solid-ring-target-v1.typ)
can likewise regenerate the revised solid without writing a new plan.
The [fill-hole review](cases/solid-ring-fill-hole.typ) changes the geometry
target itself; its [run record](../docs/plans/optimal-cad/p1-structural-review-history.md)
shows automatic rebuilding and local accepted-history undo/redo.

The [planning quantities](theories/planning-quantities.typ) run on the existing external krlysci CLI. The equations contain the sole executable volume and score definitions for this probe; no TypeScript/Rust formula implementation was added. The [theory contract](../docs/plans/optimal-cad/theory-contract.md) defines the proposed CAD architecture separately.

## Reproduce

From the repository root, the command actually run on 2026-11-15 was:

```sh
/home/krlybnd/krlybnd/krlyxd/.tmp/cad-feasibility/krlysci-target/debug/krlysci run cad-knowledge/theories/planning-quantities.typ
```

Exit code: `0`; stderr: empty. The exact numeric result was:

```json
{"costs":[[2.5,2.5]],"volumes":[[600.0,1000.0]]}
```

This is the `values` member of stdout, not the whole response. The [run record](cases/planning-quantities.run.json) preserves full stdout, command, working directory and SHA-256 hashes of input and executable. Both generic prism samples use manually normalized mm inputs; their expected volumes are 600 and 1000 mm³. Both fixed candidate metric vectors score 2.5 under equal weights, so this example intentionally does not identify a unique preferred construction.

The command uses the binary built during the [earlier feasibility probes](../docs/plans/optimal-cad/math-probes.json). That executable is a local build artifact, not a committed dependency. To rebuild at a new checkout using the existing external source, use:

```sh
cargo build --manifest-path /home/krlybnd/orplex/repos/orplex-kalyx/Cargo.toml --package krlysci-cli --locked --offline --target-dir /home/krlybnd/krlybnd/krlyxd/.tmp/cad-feasibility/krlysci-target
```

The source checkout moved after the recorded 2026-11-15 run; the command above points to its current location. The recorded output and hashes belong to the older binary, which accepted an unconsumed closing parenthesis. The [P0 parser boundary](../docs/plans/optimal-cad/p0-parser-boundary.md) documents the fixed source, the regression fixture and its new CLI result. Rebuilding changes the executable hash and requires a fresh run record. Paths are specific to this workspace.

## What this establishes

The evaluator accepts the structure/eq notation, tuple outputs, numeric literals, addition and implicit multiplication used here. It executes the numeric formulas from `.typ`; these are not illustrative pseudocode. Arithmetic results were checked against independently calculated constants, and the full output was retained.

The original `run` evaluator does not enforce mm or mm³, parameter positivity, manufacturing tolerances or CAD types. Comments explain units but do not make them type checked. Metadata `holds:true` and the response's empty `checks` map do not establish geometric correctness. The candidate metrics are supplied constants; no feature plan, geometry, candidate generation, constraint solving or optimum proof is produced. The weighted score is only the modeling-cost term; it does not implement review's lexicographic hard gates.

The original equation source uses implicit multiplication intentionally: prior executable probes rejected `/` and `*` operator syntax in that runner. Do not generalize this small accepted fragment into arbitrary mathematical expression support. The trailing-token failure in the historical binary has since been [fixed](../docs/plans/optimal-cad/p0-parser-boundary.md), but successful `run` output still cannot certify arbitrary inequalities or dimensional predicates. The `cad-contract` reader uses a separate, explicit Typst code-expression subset for those checks.

## Agent workflows

The explicitly invoked [formalize-design skill](skills/formalize-design/SKILL.md) prepares typed requirement or review proposals. The [synthesize-and-verify skill](skills/synthesize-and-verify/SKILL.md) checks actual runtime capabilities and reports unsupported synthesis honestly. Neither skill supplies a hidden object-family recipe or a solver absent from the runtime. This directory is project-local knowledge; automatic skill discovery is not asserted for this custom location.
