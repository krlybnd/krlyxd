# Drawing proposal pilot

A POSIX Python 3 CLI orchestrating existing domain executables. It implements the
process compatibility slice of #24. The local execution target is now the
[shared runtime](../../../../docs/SHARED-RUNTIME.md); this fuller process pilot remains
active for sheets and complete timeout/cancellation/backpressure behavior. The native
`krly3d-reconstruct --stream` now covers the seven-stage single-drawing route. Geometry calculations stay in krlyvis/krly3d/mesh/view.

```sh
cargo build -p krly3d-cli -p krlyvis-cli -p krly3d-mesh-cli -p krly3d-view-cli -p krly3d-brep-mcp -p krlyvis-drawing-mcp
python3 products/krly3d/reconstruction/cli/drawing_flow.py \
  --demo --pixel-unit 1 --height 3 --output /tmp/my-drawing-proposal
```

For a real image, replace `--demo` with `--image /absolute/path/drawing.png`.
`--pixel-unit` (model units per pixel) and uniform extrusion `--height` are required
user inputs. The view must be orthographic top. The output directory must not exist;
existing results are never overwritten. `--timeout` bounds computation (default 120s).
`--bin-dir` selects built executables. Linux/POSIX process groups and pipe selection
are required; this prototype is not a Windows launcher.

For several disconnected parts, use the [sheet runner](SHEET.md) with explicit per-part
seeds and thicknesses. A single run can also use `--seed X Y` to select a component
instead of the default largest one.

## What you get

The versioned `krly3d.drawing-flow/1` NDJSON stream flushes these stages:

1. `started`: scale, height, projection, hidden-surface assumptions and next questions.
2. `evidence`: pinned krlyvis contour, measurement metadata and input-image digest.
3. `coarse_preview`: a real mesh of the bounding prism, explicitly marked approximate.
4. `editable_proposal`: measured outer/hole contour profile plus uniform extrusion in `proposal.k3d`.
5. `preview`: PNG rendered from the pinned proposal node.
6. `validation`: per-station top-view outer-extent report plus a separate material-volume/mesh-closure check.
7. `completed`: artifacts and unresolved questions. Completion does not imply `matches=true`.

Every event has a flow ID, sequence, elapsed and stage milliseconds. The coarse event
is flushed **before** creating the contour profile/extrusion; it is not a retrospectively
reordered result. A stable logical proposal ID has revisions 1 and 2 and explicit
replacement. Provider mesh IDs and source revisions are retained separately; independent
processes have independent session identities. Domain Started/terminal streams are
saved verbatim. This is staged application output, not partial solver geometry.

The output contains the input image copy, assumptions/evidence, coarse model and mesh,
editable model, B-rep MCP request/result, PNG, likeness report, domain event streams, RPC replay journal and the
flow journal. Paths plus SHA-256 identify model/evidence/mesh artifacts. The coarse node
is rolled back before constructing the final document, so it cannot pollute later renders.
The final document contains editable profile coordinates and extrusion height. Its construction
uses the [B-rep planar contract](../../../../docs/BREP-PLANAR.md) through MCP; the returned
document is loaded unchanged for legacy likeness. `brep-request.json` and `brep-result.json`
retain the domain exchange; the source reference names the saved evidence SHA-256.
The editable event also includes B-rep construction provenance.

## Evidence limits

The pilot now selects the opt-in `trace-holes` plugin: it retains the selected component's
outer boundary and resolved interior pixel loops. These become **through-holes** in a
uniform extrusion. The old `trace` method still fills holes for compatibility. Other
disconnected subjects, blind cavities, hidden features, depth variation, scale and camera
correctness are not inferred. Ambiguous corner-touching boundaries are rejected.

The material area reported by krlyvis subtracts the hole areas. After saving, the pilot
reloads the editable model through the standalone mesh engine and checks its volume
against material area × supplied height and requires zero open mesh edges. Results are
saved in `solid-check.json` and `proposal-mesh.json`; disagreement fails the flow.
This validates volume and closure, not arbitrary topology or hidden geometry. Likeness
still measures sampled outer extents only; its `interior_features_validated=false` remains
explicit. Confidence and measurement uncertainty remain unknown.

Contours are exact pixel-edge corners with only collinear points removed; the new plugin
retains this zero-displacement result even when a positive simplification tolerance is
requested. The pilot rejects more than 256 total vertices across all loops. One-pixel
station tolerance uses the supplied model scale. Very large images can trigger domain limits.

## Cancellation and transport

Stages run serially; flushing stdout applies backpressure before the next stage, with
no accumulating preview-event queue. Domain streams use files instead of unbounded
subprocess capture; results above 24 MiB are rejected. Original images are capped at
16 MiB. Output is bounded by the fixed stage count and domain resource limits, not a
universal disk quota. A consumer that does not read can block output; deadlines are
checked when computation resumes, not as an independent blocked-write watchdog.

SIGINT/SIGTERM cancels the flow; timeouts fail it. Active process groups are terminated
and, if necessary, killed so peers are not orphaned. This is process cleanup, not the
unfinished remote cooperative cancellation contract (#16). Earlier artifacts remain
for inspection and the flow journal records failure/cancellation when possible; no
completion is invented. Mesh/view use their standalone v2 CLI contracts. krlyvis evidence uses its public drawing MCP operation; coarse-box construction and
likeness still use the legacy JSON-RPC peer facade. Likeness replays and verifies the
exact evidence before comparison. Editable B-rep
construction uses the synchronous `brep_planar` MCP tool; the orchestrator owns its deadline
and process cancellation. Shared-host migration of these remaining stages, consumer acknowledgment, general
artifact refinement and richer reconstruction remain open. Internal MCP-only routing
is no longer an acceptance requirement.

## Verification

```sh
python3 tests/drawing_flow_smoke.py
```

Real processes verify concave L geometry (not merely a box), early-stage ordering,
editable-model reload and watertight volume, PNG/report provenance, scale/height input,
input preservation, retained through-hole volume, an independent ray-render showing background through the hole,
timeout, SIGINT and overwrite rejection.
The existing geometry algorithms are shared with the B-rep domain operation. Current
verification results are recorded in the [development log](../../../../docs/DEVELOPMENT-STATUS.md).
