# 0004 — Mesh preview boundary

2026-11-14. Implements the first slice of [#1](https://github.com/krlybnd/krlyxd/issues/1), following the [product handoff](../PRODUCT-ARCHITECTURE-HANDOFF.md). Full domain rollout remains [#26](https://github.com/krlybnd/krlyxd/issues/26).

## Placement and ownership

The new backend is `krly3d-view-mesh` under [products/krly3d/view/mesh](../../products/krly3d/view/mesh). The active legacy `krly3d-view` package retains cameras, the Render port, exact rays/picking and query adapters. The CLI composes both packages by registering the new renderer; the legacy view package never imports the backend or mesh engine. This avoids a dependency cycle and puts new domain implementation in its intended product directory immediately.

The backend consumes a separate public Rust `RenderMesh` contract in `krly3d-mesh::preview`. It carries indexed geometry, per-triangle source-face indices, per-corner outward normals, face names and requested tolerance. Per-corner normals retain sharp boundaries while allowing smooth shading within a face. The existing v1/v2 JSON mesh contracts are unchanged; this is not a new advertised wire capability.

The existing B-rep tessellator captures source-face identity while creating triangles. The mesh engine owns conversion and memoization over the existing body/context/tolerance cache; view does not implement tessellation or cache policy. A temporary `mesh-engine::preview` bridge accepts legacy Context/Body inputs. Removing that bridge requires the B-rep snapshot/conversion contract in #26, not a directory rename.

## Compatibility and evidence

Image queries accept `renderer=mesh` explicitly. The default stays `raycast`; exact picking retains its existing schema and algorithm. The initial legacy selector uses the shared default mesh tolerance, 0.05 model units. Rust callers can specify tolerance through the backend function. Mesh results report sampled quality, requested tolerance, unknown achieved error, and mesh/conversion/raster elapsed time excluding PNG encoding. Text also carries these facts for the existing MCP adapter.

Cancellation, source-face paint, camera projection, depth/crease lines, silhouette fidelity and cold/warm equality need regression evidence. Timings must state fixture, build, size, tolerance and hardware. Subsequent user decision on 2026-11-14 retired the F-35/737/moai fixture requirement (#18 closed as not planned). #1 is closed using existing regression/timing evidence under revised acceptance; no named-model measurement is claimed. Alternative validation will be selected later.

## Migration gate

Do not archive active legacy view/camera/kernel code. Move its remaining responsibilities only with a stable public view/B-rep contract, migrated consumers and passing dependency/parity/workspace checks. Superseded material goes to [.tmp](../../.tmp/README.md), with original path and replacement recorded, until final cleanup. This change creates a new backend; it does not yet retire an existing implementation.
