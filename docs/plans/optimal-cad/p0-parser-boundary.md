# P0 parser boundary: executable counterexample

Date: 2026-11-15. The next planned gate is a typed `DesignSpec`/`FeaturePlan`
contract and a fully consumed, checked mathematical input. The krlysci source
is now located in the `orplex-kalyx` checkout. This record distinguishes the
old binary's failure from the corrected source and newly built CLI.

## Reproducer

Run the existing `.typ` numeric probe and the [negative fixture](../../../cad-knowledge/cases/trailing-token-must-fail.typ) with the old binary:

```sh
.tmp/cad-feasibility/krlysci-target/debug/krlysci run cad-knowledge/theories/planning-quantities.typ
.tmp/cad-feasibility/krlysci-target/debug/krlysci run cad-knowledge/cases/trailing-token-must-fail.typ
```

Before the fix, both commands exit 0. The second equation has an extra `)` after its
otherwise valid tuple. krlysci reports `volumes: [[600.0, 1000.0]]` and no
error, so a successful evaluation did **not** establish that the
entire equation was parsed. This was a concrete fail-closed violation, not a
geometric or unit-checking issue. By contrast, a trailing identifier and `> 0`
were rejected in local probes; the parser bug is not every possible suffix.
The binary SHA-256 is `e5ca78e0088bf45ad5e2c4eedfdc554080d8a799ee3f3b2c3ae0d9a9e1600a4d`;
the negative fixture SHA-256 is `bc6c60754b07e6bb6ada6164f515f5d6140ceb33edf34cbcdb72b7ba2f237a67`.

## Fix and acceptance gate

The [krlysci parser and tests](../../../../../orplex/repos/orplex-kalyx/crates/krlysci/src/eval.rs)
now reject an unconsumed token unless the complete suffix is the supported
`in [start, end]` condition. The new CLI rejects the negative fixture with exit
1 and `unexpected trailing token )`; the valid probe still returns volumes
`[[600.0, 1000.0]]` and costs `[[2.5, 2.5]]`. The scalar check now uses an
absolute residual: `-1` fails, while zero passes. Non-finite literals, derived
values and residuals are rejected. `cargo test --workspace --locked` passes in
the source checkout; `cargo clippy -p krlysci --all-targets --locked -- -D warnings`
and file-local `rustfmt --check` pass. The rebuilt CLI SHA-256 is
`946dba825e3b15d36d94be4073d77cf8151e2a5820347c037372832c682aa6f6`.
This closes these specific QA-MATH defects, not the entire
P0 contract: dimensional types, geometric predicates, `DesignSpec`/`FeaturePlan`
validation and proof-scoped reports remain unimplemented. Numeric outputs
alone cannot certify material geometry or construction.

An isolated TypeScript numeric schema/validator spike was run and then moved to
ignored `.tmp/refactor/rejected-p0-typescript/`: it duplicated a language beside
Typst and had no CAD verifier. It is not part of the active code inventory.
