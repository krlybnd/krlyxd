# B-rep planar construction API and adapters

The bounded operation `krly3d.brep.planar/1` constructs a fresh editable document from
an outer polygon, optional holes and a positive uniform extrusion height. Its API,
engine and standalone adapters live under `products/krly3d/brep/`. The engine uses
the existing profile/extrude/native-document providers. See [ADR 0010](decisions/0010-brep-planar-contract.md).

## Use

```sh
cargo build -p krly3d-brep-cli -p krly3d-brep-mcp
printf '%s\n' '{"protocol":"krly3d.brep.planar/1","request_id":"plate","input":{"polygons":[[[0,0],[10,0],[10,8],[0,8]],[[2,2],[2,6],[6,6],[6,2]]],"height":3,"length_unit":"model","source":"example:plate"}}' | target/debug/krly3d-brep
target/debug/krly3d-brep --describe
target/debug/krly3d-brep-mcp
```

The CLI reads one request from stdin or `--request PATH` and writes one JSON result
to stdout. Errors use stderr and a nonzero exit. The MCP server reads NDJSON on stdio;
after initialization, call `brep_planar` with the same complete request envelope.
Its input schema comes from the API descriptor and its text content is the canonical
API result. The server neither reads arbitrary model paths nor writes output files.

The first polygon is the outer boundary, subsequent polygons are holes; coordinates
stay in the XY plane with the bottom at z=0. Height and coordinates use model units.
At most 256 total supplied vertices are supported. Coordinate magnitudes and height
are capped at 1e9 model units to bound the numerical domain. Unknown fields, duplicate JSON
keys, invalid dimensions/units/protocol and nonfinite values are rejected. Structural
API checks do not prove polygon topology; native providers must accept the geometry.
Inspect `--describe` for the domain byte limit and schema. Both adapters also bound
incoming input/messages to 1 MiB.

## Result and identity

The result includes `model` (editable native k3d text), `node` (`part@1`), the request
ID/source, snapshot provenance and quality. The profile is named `outline_profile`.
Every call uses an independent document and session; a failed request cannot partially
mutate a later call. Session/source identity is process-local provenance, not a durable
content hash. Equivalent inputs can have equivalent model text and different identities.

The input `source` is an unauthenticated caller-supplied reference. Geometry is constructed
from supplied coordinates; unknown measurement confidence/uncertainty stays unknown.
An analytic exactness marker does not establish that the input contour matches a physical
object. Uniform thickness, through holes and unobserved surfaces remain assumptions.

## Lifecycle and product integration

This is a synchronous stateless operation, not the mesh/view v2 operation lifecycle.
Computation blocks the MCP request loop; there is no operation handle, remote deadline,
partial result, cancel tool or MCP cancellation-notification binding. Default SIGINT
terminates the adapter process. Input limits are not a hard computation-time bound.

The [drawing pilot](../products/krly3d/reconstruction/cli/README.md) uses the real MCP tool
for its editable proposal, retains the exact request/result and the saved evidence digest,
and keeps its total deadline/process-group cleanup. Afterward it reloads the returned
document for mesh volume/closure, view rendering and legacy likeness. The sheet runner
inherits this route per part. krlyvis evidence now uses its drawing MCP operation;
coarse-box/likeness still use the legacy facade. Internal MCP-only routing was
superseded by [ADR 0011](decisions/0011-shared-runtime.md).

The [local host](SHARED-RUNTIME.md) calls `build_region_retained_controlled` and passes the
returned owner directly to mesh. `BrepEngine::to_k3d` performs explicit export only
for files/wire adapters. `build_planar` delegates to those same operations, preserving
the existing public response. General editing and the remaining full-flow migration
stay open in #24/#26.

The native input boundary now also exposes `build_region_retained` for a shared
`Arc<krly2d_api::Region>`. The local host passes the vision-owned region through
that entry point; the legacy `build_planar_retained` wire-input bridge creates a
region and delegates to the same builder. The returned owner retains the identical
region Arc. Structural 2D validation does not bypass B-rep topology checks, the
256-vertex bound, or the coordinate/height bounds above. The current bridge accepts
only straight boundaries in model units; analytic curves and metre units return
an explicit unsupported error, without sampling or inferred unit conversion.
See [krly2d](KRLY2D.md) and [ADR 0012](decisions/0012-krly2d-planar-domain.md).

The native controlled entry points `build_region_retained_controlled` and
`build_planar_retained_controlled` take the caller's `Arc<CallControl>`. They check
path/topology/edge-pair/containment/finite-solid loops and native provider boundaries,
then guard result acceptance. Old entry points delegate with an unlimited token;
the external synchronous CLI/MCP contract above remains unchanged.

The retained owner and snapshot do not retain the construction token. Document/model
state transfers through a checkpoint into an uncontrolled kernel sharing the original
services; later cancellation cannot invalidate the ready artifact. This transfer
starts a fresh document-build cache, so later edits may rebuild nodes. Provider
operations without their own checkpoints, allocation and destructors remain synchronous.
Construction and callback-capable destruction run outside the publication mutex.

## Checks

`tests/brep_adapter_smoke.py` uses the official Python MCP SDK and JSON Schema validation,
compares CLI/MCP model and quality outputs, independently reloads translated/holed/concave
models through the mesh adapter, and checks error/reuse/ping behavior. Rust tests cover
strict codecs, typed input validation, native construction and adapter transport behavior.
The existing drawing and sheet checks exercise the actual product path. Current outcomes
are recorded in [DEVELOPMENT-STATUS.md](DEVELOPMENT-STATUS.md).
