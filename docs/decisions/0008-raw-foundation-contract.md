# ADR 0008 — Settle the raw foundation contract

Date: 2026-11-14. Status: implemented boundary decision for #20.

The raw seed already has IDs, quantities, math aliases, quadrature, clocks, exactness,
diagnostics and cooperative control. The remaining #20 questions are ownership and
shared metadata, not permission to move every mathematically named module into raw.

## Ownership and errors

Keep `Accuracy`, frame construction and NURBS in the active `krly3d-core` geometry-support
layer. Their present error signatures are domain input/geometry diagnostics and their
consumers are geometry algorithms. `Accuracy` scales model-relative tolerances; frame
routines classify degenerate directions; NURBS validates/interpolates geometric curves
and surfaces. They are explicitly marked domain support, not promised raw APIs.
Their current locations are compatibility paths, and have one-copy ownership gates.
A future B-rep package move belongs to #26 after consumers migrate, not to raw #20.

Do not pull `ErrorKind::Topology`, plugin `Attempt` dispatch or document/replay policy
into raw to make the move easier. Raw `Diagnostic` is a neutral code/message pair.
The established `From<CallStop>` conversion remains the stop→domain-error boundary;
quality validation is mapped by each domain to its established diagnostics. There is
no blanket conversion that would silently collapse every raw code to a geometry error.
The stable math aliases keep the same Rust types, so no coordinate-value wrapper copies
are needed. No existing public geometry signature changes in this decision.

## Common evidence and domain records

Add raw `Coordinates` (handedness/up axis), `Confidence` (finite [0,1]), and
`PositionalUncertainty` (explicit unknown versus measured nonnegative `Length`).
Neither coordinates nor quantities transform data, infer physical scale or calibration.
`Exactness`, IDs and length quantities remain shared. Mesh/view reuse the validators
while preserving their established wire encodings and domain record types.

Keep aggregate quality/provenance records domain-owned: mesh needs mesh input and
solver provenance; view additionally records camera/backend and requested tolerance;
B-rep owns immutable source/session identity and resolved versions. Generic raw IDs
and producer metadata are the common provenance primitives; no universal geometry
source record, durable digest or confidence model is implied. A request tolerance
must never be promoted to achieved uncertainty. Unknown stays unknown.

## One minimal metadata policy

Use Cargo as the package name/SemVer source of truth. Raw `PackageMetadata` validates
identifier/token structure; raw `ContractMetadata` carries that producer identity,
protocol/API versions, engine revision, coordinates and length unit. Domain code owns
version negotiation, schemas and capability flags. Package metadata validation does
not enforce a universal API-major value or invent compatibility from matching names.

The single [central metadata test](../../crates/core/tests/package_metadata.rs) reads
Cargo metadata, validates every workspace package, validates the mesh v1/v2 and view v1
public descriptors and checks package versions against Cargo. The existing transitive
raw dependency gate still permits only the reviewed nalgebra graph. API/domain gates
remain in force. Metadata tests add dev dependencies only; raw gains no dependency.

No CUE is needed for this Rust/Cargo-only contract. Do not create duplicate package
manifests or per-operation CUE files. If a non-Rust package needs the same policy later,
add one shared archetype/validator at that integration boundary; do not claim such a
cross-language registry exists now.

## Evidence and limits

Public raw tests distinguish unknown from zero, preserve units and reject invalid
confidence/uncertainty/identity tokens. Central descriptor and one-copy/dependency
tests enforce adoption and ownership. Mesh/view wire regression and SDK parity keep
existing external behavior. This settles #20's foundation boundary; it does not close
#26 domain rollout, #22 CTM registration or geometry/API migration work.
