# Typed mesh pilot

The first mesh contract is implemented as two workspace crates:

- [`krly3d-mesh`](../products/krly3d/mesh/api/src/lib.rs): public types, descriptor and JSON codec; depends on raw primitives and serialization, not geometry engines.
- [`krly3d-mesh-engine`](../products/krly3d/mesh/engine/src/lib.rs): a synchronous bridge owning an existing B-rep `Kernel` and using the existing tessellator.

This reference describes the synchronous typed-result slice of [ADR 0001](decisions/0001-kernel-and-domain-contract.md). A separate [v2 operation API and background runner](MESH-OPERATIONS.md) add handles, terminal events and deadlines, exposed by the [standalone mesh CLI/MCP adapters](MESH-ADAPTERS.md). The original single-pilot acceptance of [#21](https://github.com/krlybnd/krlyxd/issues/21) is complete; remaining domain rollout is tracked in [#26](https://github.com/krlybnd/krlyxd/issues/26).

## Calling the API

Construct a `MeshEngine` from a populated geometry `Kernel`, native document text, or an existing `BrepEngine` via `from_brep`. The [B-rep owner](decisions/0005-brep-snapshot-boundary.md) now loads native documents and resolves/pins geometry; mesh consumes that snapshot without duplicating version resolution. `MeshEngine::call` accepts a `MeshCall` and returns `Result<MeshResult, Diagnostic>`. `prepare` resolves and pins a snapshot; `execute` reads it even after edits or rollback. A prepared request cannot be executed by another engine.

[`codec::decode_call`](../products/krly3d/mesh/api/src/codec.rs) accepts this minimal JSON request:

```json
{
  "request_id": "mesh-1",
  "protocol": { "major": 1, "minor": 0 },
  "api": { "major": 1, "minor": 0 },
  "input": { "node": "part" }
}
```

Defaults are `tolerance = { "value": 0.05, "unit": "model" }` and `triangles = false`. To include indexed geometry, set `input.triangles` to `true`. `part@1` selects a specific version. Tolerance must be finite, positive and greater than the kernel's length accuracy. Metres are not accepted until physical model scaling exists.

The codec rejects duplicate/unknown fields, unsupported versions, invalid types and nonfinite quantities. Integer counters must fit the JSON-safe range; triangle indices use checked `u32` conversion. `encode_call` emits normalized explicit defaults; `encode_result` preserves structured quality and provenance instead of converting everything to legacy floating-point values.

## Result and identity

`MeshResult` includes point/triangle counts, volume, area, open-edge count, quality, provenance and optional geometry. Coordinates are right-handed, Z-up world coordinates in model units. Area and volume are in model units squared/cubed. The open-edge count alone is not a manifold or self-intersection proof.

Quality is sampled exactness; positional uncertainty and confidence are unknown (`None`/JSON `null`). Requested tolerance is not a proof of achieved error. Provenance includes package/engine versions, normalized input, and a B-rep-owned session identity combined with the resolved node version and build key. This is session provenance, not a portable content hash or a PMI anchor.

Included geometry has its own artifact ID and revision 1. Separate calls receive separate artifact IDs; stable IDs across progressive revisions belong to the later streaming implementation. The synchronous call does not retain request IDs as an idempotency/retry database.

## Compatibility and current limits

The legacy `mesh` query and the new bridge use the same [typed calculation](../crates/std/src/query/mesh.rs), `mesh_of` and caches. Existing CLI text, default values, flattened `coordinates`/`corners`, and error responses remain supported. The temporary geometry-to-mesh dependency bridge is intentional; it does not introduce a second tessellator.

The [descriptor](../products/krly3d/mesh/api/src/descriptor.rs) advertises synchronous, read-only execution, no cancellation/deadline support, and no events. Any non-null `timeout_ms`, including zero, is explicitly rejected. Local Rust callers can separately use `execute_controlled(prepared, Arc<CallControl>)` to stop tessellation and result statistics cooperatively. This does not enable wire timeouts or change the descriptor. Operation handles and the standalone CLI/MCP adapters use the separate [v2 API](MESH-OPERATIONS.md). Early partial meshes remain unimplemented.

The synchronous contract currently uses API/protocol 1.0. The [explicit v2 lifecycle decision](decisions/0002-mesh-operation-lifecycle.md) adds event/operation semantics without changing existing calls.

Tests cover [strict wire contracts](../products/krly3d/mesh/api/tests/contract.rs), [snapshot/legacy parity and independent box/sphere checks](../products/krly3d/mesh/engine/tests/bridge.rs), and [legacy query regressions](../crates/std/tests/queries.rs). Current verification and remaining work are in the [development status](DEVELOPMENT-STATUS.md).

## Local controlled execution

`execute_controlled` uses the same prepared snapshot and calculation as `execute`. Its per-call context shares registries and derived caches with the session; a cancelled control is not installed globally and does not affect a later call. `CallControl::with_deadline` accepts an absolute deadline in the injected clock's time base. Cancellation reports `cancelled`, expiry reports `deadline_exceeded`, and neither is a geometric decline.

Checkpoints cover tessellation and result packing/statistics. Cache writes and final Rust result publication are serialized against cancellation via the control's publication guard. A cache entry published before cancellation remains valid; a cancellation that wins the publication guard prevents a later write. No partial result is returned. Plugin calls, allocation, and individual geometry evaluations remain cooperative boundaries, not preemptible work.

This computation layer is also used by the [operation runner](MESH-OPERATIONS.md), which detaches owned snapshot/services so the session can be edited while computation runs. The standalone adapters own their runners and cancel owned work on shutdown. Legacy socket/render disconnect handling is complete (#2); its operation/event migration remains in #26. No wall-clock stop-latency guarantee is claimed.

## Rust preview data

The additive `preview::RenderMesh` type (`krly3d.mesh.preview/1`) carries geometry,
source-face indices, per-corner normals, face names and requested model tolerance.
Its validator checks structural consistency, not achieved geometric error or unit
normals. Existing v1/v2 wire formats are unchanged. The [preview backend](MESH-PREVIEW.md)
uses this data through the engine bridge described in [ADR 0004](decisions/0004-mesh-preview-boundary.md).
