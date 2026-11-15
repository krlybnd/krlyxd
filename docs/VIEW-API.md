# Typed view API

The separate [view v2 operation contract](VIEW-OPERATIONS.md) now supplies bounded
background rendering, deadlines, cancellation and CLI/MCP operation tools. This page
describes the preserved v1 synchronous path.

[ADR 0006](decisions/0006-typed-view-renderer.md) records the new boundary:
[API](../products/krly3d/view/api) and [renderer](../products/krly3d/view/renderer)
are separate packages under the view product. The active legacy view facade and
mesh raster backend remain their shared algorithm providers.

Rust API 1.0 now has a strict synchronous JSON wire 1.0 and separate
[CLI](../products/krly3d/view/cli) / [MCP](../products/krly3d/view/mcp) adapters.
See [wire and adapter usage](VIEW-ADAPTERS.md).
It renders one node per call with a named, explicit orthographic or pinhole camera.
It does not yet expose multi-node scenes, painting, picking, likeness or streaming.
Existing legacy tools remain available for those operations.

## Use

```rust,ignore
use krly3d_view_api::{Backend, RenderCall, RenderInput, RequestId};
use krly3d_view_renderer::ViewEngine;

let engine = ViewEngine::from_k3d(model_text)?;
let mut input = RenderInput::new("part");
input.backend = Backend::Mesh;
input.width = 256;
input.height = 192;
let prepared = engine.prepare(RenderCall::new(RequestId::new("image-1"), input))?;
let result = engine.execute(prepared)?;
// result.png contains PNG bytes; result.provenance.source pins node/version/build.
```

A runnable example writes a new PNG file at the explicitly supplied output path:

```sh
cargo run -p krly3d-view-renderer --example render_snapshot -- \
  models/plate-with-hole.k3d part /tmp/plate-preview.png mesh
```

The example refuses to overwrite an existing file. It reports source identity and
render/encoding timing. It exercises the public API directly; no transport is involved.

## Semantics

`ViewEngine::new` consumes an existing B-rep owner without rebinding source identity.
`prepare` validates the request and resolves its node exactly once. Editing or
rolling back the live B-rep owner afterwards does not replace the prepared geometry.
Execution rejects a prepared request from another owner or after owner replacement.

Camera defaults are the same fitted iso view as legacy queries; raycast remains the
default backend. Image dimensions are 1–2048 pixels per side. World coordinates are
right-handed Z-up in model units; pinhole focal/centre are in pixels. Mesh requested
tolerance defaults to 0.05 model units and must exceed engine accuracy.

Both image paths report sampled quality. Mesh requested tolerance is recorded but
achieved positional uncertainty and confidence are unknown. Raycast reports no mesh
tolerance. Face coverage counts visible pixels per source face name. Source identity
is session provenance, not a portable content hash.

`execute_controlled` accepts the shared `CallControl` for cooperative cancellation or
an injected deadline. Rendering, coverage and PNG processing use checks and guard
final publication; cancellation never returns a partial image. Execution does not
install cancellation globally into the live owner. Timings use its injected clock:
rendering includes source conversion/bounds and excludes model loading and PNG;
encoding has its own duration. Face coverage counting is outside both measured intervals. Neither timing is a latency guarantee.

Remaining migration and verification evidence: [development status](DEVELOPMENT-STATUS.md).
