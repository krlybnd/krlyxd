# ADR 0010 — Stateless planar B-rep construction contract

Status: implemented locally; adapter and product integration verification is recorded in
[the development log](../DEVELOPMENT-STATUS.md).

## Need

The drawing pilot previously constructed profile/extrusion statements through the legacy
joined peer facade. Moving that caller to a domain-owned contract is a bounded step
of #26 that directly serves #24. General document editing and krlyvis evidence migration
remain separate work.

## Decision

Extend the existing `products/krly3d/brep/{api,engine}` ownership with a bounded,
stateless planar-construction operation and separate `cli`/`mcp` adapters. The API owns
strict input decoding, validation and descriptor/schema; the engine builds a new native
B-rep document with the existing profile/extrude providers. Both adapters call that same
operation. There is no geometry implementation in the adapters or Python orchestrator.

The operation takes outer/hole polygon loops, a positive uniform height, model units
and a caller-supplied source reference. It returns an editable native document, a pinned
node identity, construction provenance and explicit quality/uncertainty assumptions.
The source reference is a caller claim, not verification of the original image. The
pilot supplies the SHA-256 reference of its saved evidence and keeps the request/result.
Process-local session identities differ across otherwise equivalent calls.

Protocol `krly3d.brep.planar/1` is narrowly scoped. It does not replace the existing Rust
snapshot identity contract or promise a portable general geometry encoding. The native
document remains the interoperable model output used by the current mesh/view consumers.

The synchronous MCP tool is `brep_planar`. It has no operation handles, progress stream,
remote deadline or cooperative in-flight cancellation. The product orchestrator enforces
its existing total deadline and process-group cleanup. Do not claim parity with mesh/view
v2 lifecycle tools. Input loop/vertex limits bound the supported request size, not a hard
runtime guarantee for arbitrary geometry.

## Integration and retirement

After the early coarse-preview event, the drawing pilot calls B-rep MCP for the editable
proposal. The exact returned document is loaded into the remaining legacy facade for
likeness, then independently reloaded by mesh/view for volume/closure and rendering.
krlyvis evidence, coarse box creation and likeness still use the active joined facade.
Full MCP-only orchestration is therefore not complete.

No old provider file is superseded by this change. The domain engine still owns the
explicit temporary kernel/std bridge; retain it until all consumers migrate. Any future
whole-file retirement follows the root `.tmp/` policy.

## Verification requirements

Strict request/schema tests; typed API validation at the engine boundary; native model
reload; CLI/MCP canonical-result parity excluding process-local provenance; official SDK
initialization/tool/error/reuse behavior; independent mesh volume/closure for translated,
concave and holed profiles; and the existing single-/multi-part drawing process checks.
Dependency gates prohibit direct geometry imports into the new adapters.
