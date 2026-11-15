# 0015 — Reproducible construction history and visible layers

Date: 2026-11-14. Status: active; B-rep feature-tree vertical slice implemented.

## Goal

Show the actual, reproducible geometry operations in the lower-left of the
Studio viewport. A person should be able to see and replay a sketch, extrusion,
boolean, field feature, or mesh layer as it is constructed. The stream is a
public operation trace, not private model reasoning or a retrospective story.

The viewport can replay received mesh snapshots and now exposes the native
profile/extrusion in an Inventor-style B-rep tree. Drawing reconstruction still
builds its profile and extrusion together in one native call; the fixed
Pikachu route emits sampling and meshing status, then only its final mesh. The
native adapter's `details` are dropped as events pass through the workflow and
Protobuf. See [the reconstruction stream](../../products/krly3d/reconstruction/host-cli/src/stream.rs),
[the Studio event schema](../../products/krly3d/studio/protocol/krly/studio/v1/studio.proto),
[the workflow adapter](../../products/krly3d/studio/service/workflow.ts),
[the viewport model](../../products/krly3d/studio/web/src/model.ts), and
[the field recipe](../../models/pikachu.field.json).

## Proposed design

Make a versioned construction plan the source of truth for modeling intent.
The plan is an ordered DAG of typed, parameterized operations with stable step
IDs, dependencies, named display layers, source references, units, and an
engine/schema revision. A step result records its input and output revision
hashes, duration, geometry artifact reference, and validation evidence. Events
carry that record intact from native execution through Deno/gRPC to React.

Keep two replay modes distinct:

- **Playback** displays the exact snapshots and operation records emitted by
  the original run. It remains faithful even if a later engine changes.
- **Recompute** executes the saved, accepted operation plan using a named
  engine revision and compares the resulting geometry/report hashes. A changed
  engine produces a new run and an explicit comparison; it does not rewrite old
  evidence.

An exported replay bundle contains the plan, normalized settings, source image
hash and (when selected for export) source bytes, operation/event journal,
engine identity, intermediate checkpoints, final geometry, and per-revision
validation report. This is separate from the current one-hour in-memory Studio
artifact store. Display text should name the operation and its parameters;
private chain-of-thought is never part of the bundle.

### B-rep path and coordinate convention

The visible sketch comes from the exact native B-rep profile loops, including
holes, and is drawn on the actual XY plane. The feature tree records the native
profile and extrusion parameters. Selecting the sketch overlays its real
contours; selecting XY/XZ/YZ displays that origin plane. Later boolean, fillet,
chamfer, or other operations each create a separate immutable feature revision
and validation result. The current builder still computes profile and extrusion
inside one call, so the tree is truthful history but does not yet expose a
separately timed sketch-computation event or intermediate solid.

For source images with no functional datum, center the exact contour's XY
bounding box at `(0,0,0)`, retain the source orientation and scale, place the
sketch on `XY, z=0`, and extrude toward `+Z` by the requested depth. This gives
a deterministic neutral datum while preserving image evidence; the serialized
construction record retains the source-space bounds and center. The global
frame is right-handed with Z up. Show the three origin planes in the tree, but
create an offset work plane only when a real feature references that offset.
The simple image extrusion has no offset-plane dependency.

### Field and mesh path

The Pikachu is already a fixed 53-node volumetric recipe, not an image-to-mesh
guess. Add authored semantic checkpoints (body, head, appendages, face,
assembly, final mesh) to the same plan. Preview the cumulative geometry at a
coarse bounded resolution at each checkpoint, then make one full-resolution
mesh and run the existing final print checks. Keep semantic layer IDs so the
viewport can isolate body parts and show how they join. Do not independently
run a full-resolution 300k-triangle extraction for every low-level field node.

The lower-left construction rail selects a recorded step and its snapshot.
A compact layer list toggles named feature/component layers. The rail reports
execution progress, elapsed time, and validation separately from the Three.js
triangle-reveal animation. The chat side continues to show concise explanations
and questions; its text is not the evidence of what geometry ran.

### Image interpretation and pixel art

Keep the exact uploaded raster, dimensions, color mode, orientation, and SHA-256
as source evidence. Add an explicit input interpretation (`pixel_grid`,
`silhouette`, `line_drawing`, later others) plus a confidence/evidence record.
When the user identifies pixel art, preserve the discrete grid and palette:
nearest-neighbor display, pixel-cell occupancy and connectivity, no contour
smoothing or silent upscaling. Intrinsic grid recovery can propose a scale from
repeated runs/edge alignment, but low confidence or ambiguous diagonal
connectivity must become a visible A–D question. The 2023 pixel-art
depixelization work specifically treats connectivity and aesthetic intent as
ambiguous enough to require user guidance; its smoothing objective is therefore
an optional explicit operation, not the default pixel-art path.

One front image does not specify hidden depth. The plan must state the chosen
depth model (for example, constant-thickness extrusion or user-authored voxel
depths) and expose it as an assumption. For volumetric voxel reconstruction,
request orthographic side views or annotations when those regions matter; do
not present inferred unseen geometry as observed fact.

## Verification contract

Each accepted step is validated against the exact input revision it consumed.
At minimum, record: source/parameter/schema hashes; dependency IDs; units and
coordinate frame; output artifact hash; engine revision; operation-local
checks; status `passed | failed | unknown`; limitations; and measured duration.
Sketch checks include closure, self-intersections, loop nesting, and unit
scale. B-rep checks record kernel validity after each feature. Field/mesh checks
record sample bounds/spacing, topology, orientation, numerical contacts on the
mesh and export precision, plus unsupported checks such as wall thickness.
Unknown must never be promoted to passed. A run is replayable only when its
source, plan, engine identity, and validation references resolve.

## Critical path and OR-Tools

For a single model, use the plan DAG and a deterministic topological executor;
compute the longest dependency path directly to report the critical path.
This has no scheduling search problem when every operation is serially
dependent. Keep the first useful preview on the fast path, then refine and
validate; measure time-to-first-preview and end-to-end time before setting hard
latency promises. Independent branches can execute concurrently only when the
geometry engine and memory budget allow it; output order and hashes remain
canonical by stable step ID.

OR-Tools CP-SAT becomes useful when several independent models or branches
compete for bounded native worker slots, AI quota, or memory, and task durations
and no-overlap resource constraints are known. It can minimize makespan subject
to precedence/resource constraints. It should not be added to a single shape's
geometry kernel: exact geometric validity is continuous and remains the
geometry engine's responsibility. If later used for discrete pixel/layer
assignment, validate the solver's candidate geometrically and keep an explicit
quantization/error budget.

## Critical path to implementation

1. **Complete for the B-rep slice:** a versioned native construction payload
   now carries frame data, exact sketch loops, XY plane and extrusion vector/
   distance through workflow, protobuf and the Studio model. Keep extending it
   into the stable operation-plan contract without breaking the wire fields.
2. Adapt the existing complex demo as the first end-to-end timeline: it already
   emits four actual sequential meshes and stable K3D node names. Add stable
   step IDs/layer metadata and connect selection to the existing viewport
   playback.
3. Split the planar reconstruction builder into observable sketch and
   extrusion revisions; send the actual profile and coarse solid before the
   final B-rep proposal. Attach per-step validation.
4. Add semantic checkpoints and bounded coarse previews to the Pikachu plan,
   then add mesh layer isolation and an exported replay bundle.
5. Add pixel-grid interpretation with confidence evidence and an explicit
   override/question path. Preserve current contour extrusion as a separate
   mode.
6. Only after collecting native stage timings, prototype CP-SAT for batch/worker
   scheduling if makespan or resource contention is measurable.

Acceptance uses round-trip replay from the exported bundle, byte-identical
canonical operation journal, matching geometry hashes under the same pinned
engine, precise changed-step diagnostics under a changed input/engine, and a
UI test proving each displayed step is backed by its actual operation result.
Initial latency is benchmarked on the current 101 mm Pikachu; preview work must
not multiply the existing full-resolution extraction/print-check cost by the
number of low-level recipe nodes.

## Research

- Wu, Xiao, Zheng, [DeepCAD: A Deep Generative Network for Computer-Aided
  Design Models](https://arxiv.org/abs/2105.09492), SIGGRAPH 2021: represents
  editable CAD intent as sketch/extrusion/boolean command sequences, and notes
  generated sequences do not guarantee valid topology.
- Xu et al., [Inferring CAD Modeling Sequences Using Zone
  Graphs](https://openaccess.thecvf.com/content/CVPR2021/html/Xu_Inferring_CAD_Modeling_Sequences_Using_Zone_Graphs_CVPR_2021_paper.html),
  CVPR 2021: infers sketch/extrude/boolean histories with a search over valid
  geometric zones; it returns approximate best candidates when search limits
  are reached. This supports recording operation-level history and geometry
  checks, not treating a plausible explanation as a verified edit sequence.
- Matusovic, Parakkat, Eisemann, [Interactive Depixelization of Pixel Art
  through Spring Simulation](https://diglib.eg.org/items/802f28a6-c1a5-4301-9e69-78813bee8499),
  Computer Graphics Forum 2023: pixel connectivity and aesthetic intent are
  ambiguous; user interaction resolves them.
- Kopf and Lischinski, [Depixelizing Pixel
  Art](https://johanneskopf.de/publications/pixelart/), SIGGRAPH 2011: diagonal
  adjacency and feature connectivity on the square pixel lattice require
  explicit treatment; smoothing is a modeling choice, not neutral cleanup.
- Krajtmajer, [How Can We Convert 2D Pixel Art into a 3D Voxel
  Representation](https://repository.tudelft.nl/record/uuid%3Ac617a480-53fe-4e27-bfda-008cc718cee1),
  TU Delft bachelor thesis, 2025: compares silhouette intersection, carving,
  and depth estimates on multiple orthographic views and reports their
  different concavity/completion failures. Treat as early pilot evidence, not a
  mature benchmark.
- Google, [OR-Tools job-shop scheduling](https://developers.google.com/optimization/scheduling/job_shop):
  CP-SAT models task precedence, resource exclusivity, and makespan. This fits
  cross-job resource scheduling, not the geometry semantics of one object.

This implementation begins the architecture, but the recorded construction
payload is not yet a portable replay bundle. The remaining sequence and
acceptance criteria above stay open; this ADR does not mark the broader work
complete.
