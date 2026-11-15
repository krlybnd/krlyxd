# Mesh preview

The opt-in `krly3d-view-mesh` backend lives in [the view product directory](../products/krly3d/view/mesh). It consumes the public mesh preview data contract through the mesh engine's temporary B-rep bridge. [ADR 0004](decisions/0004-mesh-preview-boundary.md) records the package boundary.

## Calling it

The existing `krly3d` CLI registers the backend. For an existing model:

```sh
krly3d --open models/plate-with-hole.k3d call render nodes=part renderer=mesh width=256 height=192
```

Plain JSON-RPC returns the PNG and structured metadata:

```json
{"jsonrpc":"2.0","id":1,"method":"render","params":{"nodes":["part"],"renderer":"mesh","width":256,"height":192}}
```

The same arguments work through legacy MCP `tools/call`, with `name: "render"`. MCP returns the same PNG and readable quality/timing text. `view` and `silhouette` also accept `renderer: "mesh"`; `pick` stays on exact surface rays and does not accept a renderer selector. Omitting the selector preserves the raycast path and existing image result shape.

The current selector uses 0.05 model units as requested tessellation tolerance. This is not a proof of achieved geometric error. Structured `quality` reports `representation: "triangle-preview"`, tolerance/unit and `achieved_error: "unknown"`; sampled exactness remains in the usual reply field. The legacy value grammar has no null, so unknown is explicit text. `timing.mesh_and_raster_ms` includes bounds, mesh lookup/conversion and rasterization, excluding PNG/base64 encoding. It does not assert whether every source was a cache hit.

Rust clients can call `krly3d_view_mesh::render_mesh(cx, request, tolerance)` with an explicit tolerance. The first implementation retains the legacy Context/Body camera boundary; a standalone versioned view API/CLI/MCP package suite remains #26 work.

## Behavior and limits

Orthographic and pinhole projections use the existing camera conventions. The backend clips triangles against the near plane and viewport, uses a z-buffer and perspective-correct interpolation, and carries source-face identity through paint and visibility. Exact and preview rendering share shading and crease/depth-border rules. Normals are per corner so a geometric crease is not smoothed away.

The tessellator records triangle provenance as it creates triangles. The engine reuses the existing mesh cache and caches successful preview conversion by body/context/tolerance. Unsupported provenance declines explicitly. Cancellation/deadlines are cooperative and prevent partial result/cache publication; individual surface evaluations, sorting and memory operations are not hard-time-bounded.

The maximum image side is 2048 pixels, matching the query facade. No early partial artifact or progressive refinement event is produced. The exact ray renderer remains available as a reference and for workloads where triangle approximation is unsuitable.

## Reproducing timing and fidelity

```sh
cargo run --release -p krly3d-view-mesh --example preview_timing -- models/plate-with-hole.k3d part 0.05
```

The example reports exact, cold mesh and warm mesh times at 256×192, verifies identical cold/warm images, and computes silhouette intersection-over-union against the exact renderer. Use the same model/node, build, hardware, size and tolerance when comparing runs. This is a single-process warm-cache measurement, not a latency guarantee.

The user retired the named F-35/737/moai fixture requirements on 2026-11-14; #18 is closed as not planned. #1 is complete under the revised acceptance using the existing regression and timing evidence. Alternative validation will be selected later; no named-model measurement is claimed. Standalone view contract migration remains #26. Latest measured evidence belongs in [development status](DEVELOPMENT-STATUS.md).
