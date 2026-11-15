# 0001 — Raw kernel boundary and mesh contract pilot

2026-11-14 · **Raw seed, synchronous typed-result slice, controlled execution, v2 runner and standalone mesh adapters implemented locally.** Design slice of
[#20](https://github.com/krlybnd/krlyxd/issues/20) and
[#21](https://github.com/krlybnd/krlyxd/issues/21), read on this date. Audited commit:
`1aac484a594f1e1f7176fd0309d9d5e04f88ae0b`. Follows the
[product handoff](../PRODUCT-ARCHITECTURE-HANDOFF.md) and
[original wave plan](../../.tmp/docs/MULTI-AGENT-DEVELOPMENT-PLAN.before-wave6.md).

The integrator reports 721 passing baseline tests and integrated lint cleanup; those
checks were not rerun in this worktree. This ADR gates the **next wave**, not the current
#3 cache slice. It does not close either architecture issue.

Wave 2 implemented PR1 and the synchronous portion of PR2. Wave 3 added cooperative
computation and publication guards. Wave 4 adds the [v2 operation lifecycle](0002-mesh-operation-lifecycle.md),
whose [runner and event reference](../MESH-OPERATIONS.md) is the current contract. The
synchronous 1.0 API and descriptor remain unchanged. The v2 pilot advertises only Started
and Completed/Failed/Cancelled; progress and partial artifacts are not implemented.
Wave 5 adds the standalone mesh CLI/MCP adapters; see [ADR 0003](0003-mesh-transport-adapters.md).
Legacy socket/render disconnect handling remains separate #2 work.

The original plan below describes the pre-migration baseline and proposed execution shape;
the explicit version/retention/event decisions in ADR 0002 supersede those proposals.
Current evidence is in [development status](../DEVELOPMENT-STATUS.md).

## What the code establishes

- [core](../../crates/core/src/lib.rs) contains reusable primitives **and** document replay,
  claims, schemas and plugins. [ErrorKind](../../crates/core/src/error.rs) includes
  geometry/topology. [plugin](../../crates/core/src/plugin.rs) is context-generic but
  imports `Schema`; it cannot be moved alone without pulling that dependency along.
- [Context](../../crates/kernel/src/context.rs) caches by `Arc<Body>` identity;
  [Shape](../../crates/kernel/src/ports/shape/mod.rs) takes that B-rep context;
  [Built](../../crates/kernel/src/model/build.rs) contains both shape and body.
  Renaming this kernel would not make it domain-neutral.
- The existing [mesh query](../../crates/std/src/query/mesh.rs) calls
  [mesh_of](../../crates/kernel/src/ports/interpret/mod.rs) and
  [BrepMesh](../../crates/std/src/interpret/mesh/mod.rs). It reports counts, volume, area,
  open sides and optional triangles, marked `Sampled`. Successful meshes are cached
  per body/tolerance bits; no public API-version key exists at this baseline.
- [Session](../../crates/kernel/src/api.rs)/[Endpoint](../../crates/cli/src/endpoint.rs)
  synchronously return one `Reply`. [MCP](../../crates/cli/src/dialect/mcp.rs) drops
  structured data except images and ignores notifications. The
  [message loop](../../crates/cli/src/dialect/mod.rs),
  [listener](../../crates/cli/src/listen.rs) and [peer](../../crates/cli/src/peer.rs)
  block on calls. There are no operation handles, event streams or cancellation yet.
- [krlyvis](../../package/krlyvis/crates/kernel/src/plugin.rs) already re-exports the
  shared plugin implementation, despite stale pending-switch statements in
  [SPEC](../../SPEC.md)/[HANDOFF](../../HANDOFF.md). Its
  [CLI](../../package/krlyvis/crates/cli/Cargo.toml) still imports geometry transport types.

## Boundary decision

Create a small `krlyxd` leaf crate. Its intended surface is IDs, plain numerical math,
quantities, accuracy, exactness, clocks, neutral diagnostics and cooperative call control.
No bodies, meshes, shape interpretation, document/session policy, CLI parsing or MCP
belongs there. Start by extracting genuinely independent leaves, not all of core.

Keep legacy document/schema/plugin/claim code and `Error`/`Attempt` in `krly3d-core` as
support/compatibility code for this pilot. Shared use by vision is not sufficient reason
to put document policy in raw. [Accuracy](../../crates/core/src/accuracy.rs) and
[frame/NURBS math](../../crates/core/src/math/mod.rs) currently return legacy errors;
move them later with explicit conversions, preserving behavior. Do not copy algorithms.

B-rep owns surface/topology semantics, named anchors, edits and the conversion input
contract. Mesh owns the single tessellator, mesh data and mesh quality/operations, consistent
with #4/#13. The current `BrepMesh` implementation in geometry std is temporary legacy
placement, not a change in product ownership; it must not acquire a second implementation.
View/exchange/vision retain their responsibilities. Proposal/PMI semantics stay in
document/domain services; raw IDs and triangle indices are not stable PMI anchors.
CTM, other representations and external extension ABIs are outside this pilot.

Compile dependencies: `domain API -> krlyxd`; `engine -> own API + krlyxd`;
`CLI/MCP -> own API`, with engine construction in composition roots. API must not import
engine. A **temporary mesh-engine bridge** may import existing geometry kernel/std to
resolve a snapshot and tessellate it; neither raw nor the mesh API may do so. Remove that
exception when B-rep exposes its snapshot/conversion input API, without blocking this pilot.
Raw's initial production dependency allowlist is `std`/`nalgebra`, transitively checked
against domain engines, CLI parsers and MCP SDKs.

## One pilot and its minimum types

Choose the **existing `mesh` query**, package `krly3d-mesh`, operation `mesh`. Model loading
uses the existing B-rep session/codec; it is not a second pilot. No new meshing algorithm,
artifact store, schema generator or generic execution framework is required.

Raw additions are small records: `ApiVersion { major: u16, minor: u16 }`, string-backed
`RequestId`, `OperationId`, `ArtifactId`, `Diagnostic { code: Id, message: String }`, and
`Length { value: f64, unit: LengthUnit }` with `LengthUnit::{Model, Metre}`. Reuse existing
`Id`, `Exactness` and clocks. Call control is an atomic cancel flag plus an optional
injected-clock deadline, passed per call; it owns no threads or transport.

The **mesh API**, not raw, owns these concrete records (codec/derive details omitted):

```rust
struct MeshInput { node: String, tolerance: Length, triangles: bool }
struct MeshCall {
    request_id: RequestId, protocol: ApiVersion, api: ApiVersion,
    timeout_ms: Option<u64>, input: MeshInput,
}
struct MeshData { points: Vec<[f64; 3]>, triangles: Vec<[u32; 3]> }
struct Provenance {
    package_version: String, engine_revision: Id,
    source: Id, input: MeshInput,
}
struct Quality {
    exactness: Exactness,
    positional_uncertainty: Option<Length>, confidence: Option<f64>,
}
struct MeshArtifact {
    id: ArtifactId, revision: u32, data: MeshData,
}
struct MeshResult {
    points: u64, triangles: u64, volume: f64, area: f64, open: u64,
    quality: Quality, provenance: Provenance, mesh: Option<MeshArtifact>,
}
struct MeshEvent {
    request_id: RequestId, operation_id: OperationId, seq: u64, body: MeshEventBody,
}
enum MeshEventBody {
    Started(Provenance),
    Progress { stage: Id, completed: u64, total: Option<u64> },
    PartialArtifact { artifact: MeshArtifact, quality: Quality },
    Warning(Diagnostic), Completed(MeshResult), Failed(Diagnostic), Cancelled,
}
```

One descriptor in the mesh API supplies package ID/release SemVer, API/common-envelope
versions, engine revision, operation input/output schemas/defaults, read-only/cancel flags
and supported events. It also fixes coordinates to **right-handed, Z-up world space** and
artifact format to `krly3d.mesh.triangles/1`. Every handle/event refers to that descriptor;
artifacts inherit the operation's source/conventions. Do not duplicate CLI/MCP schemas or
extend the legacy all-`f64` `Value` into a second new contract. At most one central package
metadata archetype may use CUE; no operation-specific CUE files.

Preserve defaults `tolerance=0.05`, `triangles=false`. Accept only finite `Model` lengths
above `Accuracy::length()`; reject `Metre` until model scaling exists. The
[existing contract](../../crates/cli/src/endpoint.rs) has unitless lengths, not millimetres.
Area/volume use model units squared/cubed. Resolve `node`/`node@version` once; source ID
encodes session identity, resolved node version and build key. Keep normalized input in
provenance. Rebinding/rollback cannot retarget work. This is session provenance, not a
persistent digest or a representation-independent anchor.

Preserve `Sampled { samples: triangle_count capped at u32::MAX }`; uncertainty/confidence
are `None` (unknown), not zero. Requested tolerance does not prove achieved error.
`open=0` is an edge-balance statistic, not a full manifold/self-intersection proof.
Convert indices to checked `u32`; wire counters must be JSON-safe integers without silent
rounding. The legacy flattened triangle fields are a compatibility encoding only.

## Shared execution semantics

- API exposes `start(MeshCall) -> Result<OperationId, Diagnostic>`, `next(operation)` and
  `cancel(operation)`; a blocking convenience call collects that same result. Validate
  before acceptance and pin an immutable snapshot. Rejection has no operation/events.
  Request IDs correlate submissions, not retries; reject live-session duplicates.
- Start common protocol and mesh API at `1.0`, separate from package SemVer, solver
  revision and MCP transport revision. Reject unsupported major/minor before work; no
  silent downgrade. Additive optional output fields permit a minor bump; changed units,
  defaults, required fields or event variants require a major bump. Reject unknown input.
- Accepted calls emit Started (`seq=0`), optional intermediate events, then exactly one
  Completed/Failed/Cancelled. Sequence increases per operation; nothing follows terminal
  state. Preserve structured exactness and diagnostics across every adapter. An exhausted
  plugin decline becomes `Failed(unsupported)`; transport parse failures stay transport errors.
- Artifacts retain ID while revision increases; replacement supersedes the previous
  preview and completion identifies the final revision. Previews remain provisional on
  failure/cancel. **The real pilot emits no PartialArtifact**: current tessellation returns
  a whole mesh. With `triangles=true`, completion includes revision 1. Fixture replacement
  tests are useful but do not satisfy #24's real early-preview acceptance.
- Bound queues by bytes and event count; coalesce progress/superseded previews before
  sequence assignment, preserve Started/warnings/terminal, and reserve terminal capacity.
  Cancellation wakes blocked producers. Cap live/retained operation records and inline
  payload size; oversized payloads fail `resource_limit`, never truncate. No durable replay.
- Explicit cancel yields Cancelled; deadline yields `Failed(deadline_exceeded)`.
  Cancel is idempotent for a retained handle; expired/unknown handles return `not_found`.
  Serialize cancel with terminal publication: whichever wins fixes the outcome. Never
  publish/cache a late result after cancel wins. Cancellation is not a geometric decline.
- Check call control during edge sampling, layout, clipping, flipping and refinement,
  including long inner loops/recursion, and before cache/publication. Replace string-only
  tessellator aborts with typed stop/decline outcomes. Run on pinned input/shared engine
  services without holding a session lock or `&mut Session` throughout computation.
- A confirmed disconnect cancels owned work and leaves the document usable by the next
  client. Input/cancel processing must continue during computation. The current client's
  socket **write half-close is normal end-of-input**, not evidence it stopped reading.
  Preserve that behavior. Mutation commit/cancel and peer cancellation remain separate work.

API, CLI and MCP must call this same implementation. CLI event mode emits canonical
newline-delimited events on stdout, diagnostics on stderr; SIGINT requests cancel;
exit codes are 0 completed, 1 failed/rejected, 130 cancelled. Human text is a result projection.

**Proposed MCP binding, subject to the spike below:** `mesh` returns a handle immediately;
`operation_events` drains available events; `operation_cancel` uses the same cancellation.
Carry canonical JSON in text content so no structured result is discarded. This is
incremental pull delivery, not buffering until completion. Failed/rejected/cancelled
operations are error outcomes; acknowledging a cancel request is not itself an error.
Use separately installed domain CLI/MCP packages; preserve legacy monolithic entry points.

Cache identity includes input, tolerance/quality, accuracy/configuration and API/engine
namespace. For today's in-memory cache, immutable context lifetime can supply the latter;
**#3 need not invent a public version or wait for this ADR**. Its deterministic declines
may be cached; cancellation/deadline/transport/resource failures may not. The
[build key](../../crates/core/src/document/cache.rs) uses `DefaultHasher` and is not a
portable content digest. No persistent cache is introduced.

## Next PRs: concrete ownership and gates

Root manifests/lock belong exclusively to the Sol integrator. Proposed new paths below
are not existing packages. Shared-file ownership is reassigned before each PR.

1. **Raw seed — Sol integrator, independently of #3.** New `crates/krlyxd`; move only
   [id](../../crates/core/src/id.rs), [clock](../../crates/core/src/clock.rs),
   [exact](../../crates/core/src/exact.rs), math aliases and
   [quadrature](../../crates/core/src/math/quadrature.rs). Keep core re-exports; add the
   small primitive records/control above. Own
   [core exports](../../crates/core/src/lib.rs), [math exports](../../crates/core/src/math/mod.rs)
   and [one_copy](../../crates/core/tests/one_copy.rs). Gate: identity/one-copy/public API
   tests and dependency allowlist. Defer error-coupled Accuracy/frame/NURBS moves.
2. **Typed mesh result — Sol contract owner, after #3 integrates.** New
   `products/krly3d/mesh/{api,engine}`; descriptor/codec and explicit legacy bridge.
   Factor [mesh query](../../crates/std/src/query/mesh.rs) calculation into one typed path
   used by old and new APIs; own its [query tests](../../crates/std/tests/queries.rs).
   Gate: existing/default/triangle/error result parity. No transport or algorithm redesign.
3. **Controlled execution — same owner, serialized with #3/#4.** Add the operation runner
   in mesh engine; own [Kernel](../../crates/kernel/src/kernel.rs),
   [Context](../../crates/kernel/src/context.rs),
   [interpret](../../crates/kernel/src/ports/interpret/mod.rs) and
   [mesh tessellation](../../crates/std/src/interpret/mesh/mod.rs) including its
   [edges](../../crates/std/src/interpret/mesh/edges.rs),
   [layout](../../crates/std/src/interpret/mesh/layout.rs) and
   [triangulate](../../crates/std/src/interpret/mesh/triangulate.rs) modules.
   Gate: real cooperative stop, immutable snapshot, bounded queue and terminal/cache races.
4. **Adapters/parity — one Sol transport owner with #2/#16 coordination.** New mesh
   `cli`/`mcp` packages and shared contract suite; own necessary changes in
   [endpoint](../../crates/cli/src/endpoint.rs), [dialects](../../crates/cli/src/dialect/mod.rs),
   [listener](../../crates/cli/src/listen.rs), [CLI composition](../../crates/cli/src/lib.rs)
   and [program tests](../../crates/cli/tests/program.rs). Do not copy the RPC codec;
   extract reusable transport only as needed. Gate: the same suite passes through all
   three surfaces; legacy geometry/vision still work. No peer migration in this PR.

Later moves (mesh data, document support, B-rep facade) receive separate file scopes after
pilot acceptance; this ADR does not authorize a whole workspace migration.

## Acceptance and open decisions

The common suite checks defaults, triangles on/off, unknown/bad inputs, missing/versioned
nodes, incompatible versions, deterministic decline, deadline, duplicate request, repeated
cancel and completion/cancel race. Compare normalized result, units, provenance/quality and
event invariants; normalize generated IDs/timing, never absent metadata. Independently
check a `6×4×2` box (volume 48, area 88) and radius-5 sphere at tolerance 0.01 (zero open
sides; volume error ≤ `4π·25·0.01`), following the
[mesh contract](../../crates/kernel/src/ports/interpret/contract.rs).

Use a barrier-controlled **real** meshing run to prove stop at the next bounded work
chunk, no late terminal/cache publication, and unchanged session state. Test slow-consumer
bounds, actual disconnect/next-client responsiveness and normal half-close separately.
Cache tests cover input/tolerance/configuration/version namespace changes and absence of
negative caching for stop/deadline. Mock event tests alone do not satisfy these gates.
Run relevant crate tests per PR; integration runs workspace tests, format check and clippy
with repository lints. Documentation verification here is source/link/scope checking only.

Explicit open decisions/assumptions, to resolve without expanding the pilot:

- **Before PR4:** transport owner validates the proposed MCP pull binding with one actual
  target client and pins supported MCP revisions. No external protocol-conformance claim
  is made here; a required alternative binding must preserve the same events/results.
- **In PR3:** choose operation/payload limits and checkpoint chunk size from measured
  fixture costs; record cancel-to-stop latency. No unmeasured millisecond guarantee.
  Verify existing plugin services are shareable without serializing requests on a session lock.
- **At PR2 review:** confirm session-scoped source identity and unknown uncertainty are
  sufficient for this pilot. Durable provenance/units conversion/PMI anchors and real
  partial meshes remain explicitly deferred; do not add speculative schemas for them.
