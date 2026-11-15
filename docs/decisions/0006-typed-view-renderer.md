# 0006 — Typed view requests over pinned B-rep sources

2026-11-14. Continues [#26](https://github.com/krlybnd/krlyxd/issues/26) after [B-rep snapshot ownership](0005-brep-snapshot-boundary.md).

## Package boundary

`products/krly3d/view/api` (`krly3d-view-api`) owns typed requests, camera inputs, image results, source provenance, quality and timing. It depends on raw primitives and B-rep identity metadata, not kernels, algorithms or adapters. The package suffix avoids colliding with the still-active legacy `krly3d-view` facade. API 1.0 here is a local Rust contract; it advertises no JSON protocol, CLI/MCP tools or events.

`products/krly3d/view/renderer` (`krly3d-view-renderer`) owns preparation and execution over a B-rep owner. A prepared render fixes one node/version and originating services. It cannot be executed by another owner, including after replacing the engine's owner. Edits and rollback do not change the geometry already prepared.

The renderer selects the existing exact ray implementation or mesh backend. Camera projection, named camera conventions and PNG encoding remain single shared implementations in the active legacy view package. Mesh tessellation/cache remain in the mesh domain. No raster algorithm, projection algorithm or codec is copied into the new package.

## Supported behavior

This slice supports one node per image, seven named fitted views, explicit fitted orthographic direction/up, and pinhole eye/direction/up/focal/centre. Dimensions are bounded to 1–2048 on each side. World coordinates use model units, right-handed Z-up; focal length and optical centre use pixels. Mesh tolerance is requested model-space approximation tolerance, not measured error.

Results contain PNG bytes, named face coverage, normalized source identity/input, sampled quality and separate rendering/encoding timings. Achieved uncertainty and confidence stay unknown. Exact surface ray intersection does not make a raster image exact geometry: both backends report sampled image quality. Raycast does not claim a requested mesh tolerance.

Public local controlled execution uses the existing injected cancellation/deadline control and final publication guard. No partially encoded image is returned. There is no new asynchronous operation runner, early artifact or transport timeout field in this slice.

## Migration and validation

Verify PNG/face parity with legacy image queries for both backends and camera modes, pinned versions through edits/rollback, ownership rejection, invalid inputs and controlled stop/retry. Keep dependency gates and full workspace/CLI/MCP compatibility checks. A runnable example calls the same typed API, without becoming an independently advertised adapter.

Multiple bodies, painting, picking/likeness, strict wire descriptors/codecs and standalone CLI/MCP bindings remain follow-up work under #26. Existing legacy tools retain those capabilities. The preview's Context/Body bridge also remains until its consumers migrate. No whole legacy source file is superseded by exposing shared helpers; retain active files and preserve the [.tmp archive](../../.tmp/README.md).
