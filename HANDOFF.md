# krly3d – handoff notes

State of the work in the current tree. Read this first, then SPEC.md for the kernel design and
the product architecture handoff for the accepted package boundaries. Historical verification
figures below are snapshots from earlier checkpoints, not a report of today's baseline.

## Current local execution direction

The accepted local path now composes typed domain engines in one process with immutable
shared artifacts and revision-checked publication. See [ADR 0011](docs/decisions/0011-shared-runtime.md)
and [migration plan / usage](docs/SHARED-RUNTIME.md). The first host covers drawing evidence,
retained planar B-rep and mesh, with optional native coarse preview, render and station
likeness; `krly3d-reconstruct --stream` flushes seven ordered product events. Height edits reuse evidence,
tolerance edits also reuse B-rep, and stale/foreign results cannot become current.
Controlled edits/proposals now share the same cancellation/deadline boundary as
initial drawing jobs. The CLI has separate options, shared output metadata and stream
orchestration; output failure stops recovery writes and retracts the success manifest
best effort. Full CLI lifecycle and sheet migration remain pending.
CLI/MCP remain independently usable external adapters. CTM remains assembly/constraints.

The Python process/sheet flow remains active until sheets and full CLI lifecycle
(timeout/cancellation/backpressure) migrate; it is not obsolete. No files moved to `.tmp/` in this slice.
Vision and planar wire adapters delegate to the same typed native implementations.
Current integration evidence and uncommitted/unpushed status are in
[DEVELOPMENT-STATUS](docs/DEVELOPMENT-STATUS.md); earlier test counts below are historical.

## Where things stand

The kernel is being rebuilt from scratch on one plugin pattern (SPEC.md). The code inherited from
OpenGeometry 3.0.0 has been removed from the working tree; nothing of it was copied into the new files.

| Phase (SPEC.md chapter 6) | State |
|---|---|
| 0 Snapshot and reference numbers | done |
| 1 Removal, workspace, `core`, plugin registry | done |
| 2 Equations: `curve`, `surface`, `sweep`, `meet` | done |
| 3 Process: `topo`, `model`, `stage`, `tool`, `content`, `measure`, `read`/`write` | done |
| 4 Perception and the agent's loop | implemented in the current tree: session API, probe, trim, judge/expectations, queries, CLI and rendering |
| 5 General boolean | done for crossings with closed-form sections; tangent contact, tori being split and non-closed-form sections are declined |
| 6 Chamfer, fillet, mirror | done (see the scope below); the column model was dropped by the owner |
| 7 AI layer | `expect` claims are judged, including placement, face-name and likeness claims; notes, watchers, proposals, sweep, history and contour remain unimplemented |

Solids can be modelled from text (`models/*.k3d`) and through the session API: profiles, extrusions,
revolutions, the five primitives, transforms, patterns, mirror, union/subtract/intersect, chamfer and
fillet. Every model file is replayed by the tests with its expectations.

### Placement by intent (added after the tesseract drawn through the CLI went wrong)

Drawing a tesseract by hand, four struts were turned about axes of the wrong sign: the turn had to be
worked out by the caller, any axis was valid input, and no claim said where a strut was meant to end.
The picture showed it a round later. The cause is closed in three places:

* `cylinder`, `cone` and `torus` take `axis`, and `cylinder` and `cone` also `to` (the end of the axis,
  in place of `axis` and `h`). One reader in `std/src/tool/mod.rs` (`standing`, `Standing`) serves all of
  them; the turn is `core::math::frame::turning`, Rodrigues in closed form, exact for agreeing and
  opposite directions. Giving the same thing two ways (`h` and `to`, `axis` and `to`) is an input error.
* `transform` takes `align` and `onto`: a direction of the body and where it is to point.
* Claims `contains=x,y,z` (exact, by the probe) and `meets=node` (a common point found is proof; none
  found is reported with `?`, because only vertices, straight edges and loopless faces are searched).
* `Param::alternative` marks a parameter that is absent unless given; `Param.required` says what must be
  given. Regression tests: `tests/kernel.rs` (the wrong strut is refused, the drawn one is kept) and
  `models/tesseract.k3d`.

### Foundation work in the current tree

The plan of SPEC.md chapter 8 (one foundation, many interpretations) was approved. Phase 0 is done:

* `krly3d-core` holds the one plugin mould (`plugin.rs`: `Family` now names its `Context`) and the
  protocol schema (`schema.rs`); the kernel re-exports both. Quadrature (Gauss–Legendre) has one table,
  `core/src/math/quadrature.rs`. `crates/core/tests/one_copy.rs` fails when any of them is copied.
* The camera left the kernel: `crates/view` (`krly3d-view`) holds the render port, the renderer and the
  `render`, `view`, `silhouette`, `pick` queries; the CLI registers it. `krly3d-std` exposes its test
  helpers under the `testing` feature for plugin crates.
* Merged from the stopped agents: the queries agent's `index`, `keys`, names/anchors, the box-tree
  probe and the queries (`Context::derived` caches per body), and the export agent's STEP writer, now
  `crates/exchange` (`krly3d-exchange`, query `step`). Phase 1 has since added the `interpret::mesh`
  port and B-rep tessellator described below; STL/OBJ/glTF exchange remains target work.
* SPEC.md chapter 8 and `docs/architecture.drawio` (new page "One foundation, many interpretations").
* krlyvis now re-exports the shared plugin mould from `krly3d_core` (`package/krlyvis/crates/kernel/src/plugin.rs`).
  Its families use their own `Context`, as required by the shared `Family` contract.
* Historical snapshot: 674 tests passed at an earlier phase-0 checkpoint. This is not a current test count.

### Phase 1 of the foundation plan (2026-11-14)

* `kernel/ports/shape`: the `Shape` object family (signed `distance`, `bounds`, `children`) with a
  contract (Lipschitz, inside within bounds). `Leaf` wraps a `Body`; every tool still makes bodies, and
  `Model` stores `Built { shape, body, … }` (`Model::shape(node)`), so behaviour is unchanged.
* `kernel/ports/interpret`: the families `ToBody` and `ToMesh` (`body_of`, `mesh_of`) with a mesh
  contract (no open side, volume within area·tolerance). `kernel/mesh`: the `Mesh` data.
* `std/interpret`: `BrepBody` and `BrepMesh`, the one tessellator for every surface through the
  `Surface` trait. Edges are sampled once and shared by both faces; each face is laid out in its
  parameters (plain, seam between two loops going round, pole, whole surface); ear clipping with hole
  bridges, Lawson flips, refinement by 3D deviation. Points made on a seam are shared by both copies of
  the cut side; a face's triangles are turned outward by one area-weighted vote (a sliver's normal
  alone is noise). `Surface::poles()` (sphere, ellipsoid, cone) tells the layout where a face closes.
* Query `mesh` (`node`, `tolerance`): triangle and point counts, volume, area, open sides, marked `?`.
* Not done: performance (a 0.005 mesh of a sphere takes about a minute in a debug build, 3 s in
  release); slivers next to poles are allowed, not improved; mesh data is not exported yet (STL/glTF
  are phase 5 in `exchange`).
* Historical snapshot: 681 tests passed at an earlier phase-1 checkpoint; this is not a current test count.
  The implementation was exercised through the CLI, and the tesseract strut meshes closed
  (0.05: 1084 triangles, volume 40.29 against the exact 41.79).

### Toolset for free-form models (2026-11-14)

* `taper`: extrusion with a draft angle; straight sides become planes, arcs cones, the top
  is the exact inward offset. Arc corners that are not tangent are declined (they would
  run up a conic).
* `pipe`: a round bar along a polyline whose corners are bent with a radius; cylinders and
  torus pieces, exact.
* `ellipse` curve and `ellipsoid` surface and tool. Closed form: points, tangents, the
  ellipse foot point (a degree-two trigonometric equation), line hits, extremes, volume.
  Numeric and marked `~`: ellipse length (Gauss–Legendre), ellipsoid area (Carlson's R_G,
  `core::math::elliptic`), the foot point on an ellipsoid (Eberly's bisection). Only whole
  ellipsoids have a content rule; booleans with ellipsoids have no section rules yet.
* `chamfer`/`fillet` also take a circular rim between a flat face and a square cylinder
  (boss or hole): a cone or a quarter torus. The torus content rule gained bands between
  two circles around the axis.
* `group` keeps bodies together without joining them; `render` takes `paint`.
* Fixed on the way: the torus quartic is set up from the point of the ray nearest the
  centre (far rays lost close roots), and the probe ignores roots where a ray runs along a
  surface (a ray parallel to a turned cylinder found a spurious hit).
* NURBS (owner's choice for the loft): `core::math::nurbs` (Piegl–Tiller: basis and
  derivatives, rational curves and surfaces, exact arcs, knot insertion, degree elevation,
  global interpolation), the `nurbs` curve and surface plugins, a trim rule and a content
  rule for faces that are bands between two iso-curves of constant v, and the `loft` tool
  through two or more line/arc profiles. Evaluation, iso-curves and motions are exact; foot
  points, line hits, extremes, lengths and contents are numeric and marked `~`. Booleans
  with NURBS faces have no section rules; trimmed NURBS faces other than bands are declined.
* Rendering a NURBS face: patch boxes reject rays before any Newton step, and each face's
  band is worked out once per surface; a 600×900 picture of the sketch took 1.5 s.
* `meets=` also searches points along edges, so bodies without vertices (a bent bar) meet.
* For working from photographs: `view` takes a pinhole camera `{eye, toward|target, up, focal,
  centre}` so a picture lies on a photograph of the same camera pixel for pixel; `silhouette`
  gives the covered spans row by row and, `against` a photograph's spans, the IoU and the rows
  whose edges differ most; `pick` turns pixels into node#face and the point in space.
* `profile` draws whole ellipses (`E cx,cy a,b`); `loft` takes them exactly (an affine image of a
  rational arc). `pipe` takes `radii`, one per point: runs become frustums, bends keep the
  point's radius, all exact. The renderer draws lines only at creases (over ~20°) and depth steps.
* Not built: corner blends where three edges meet.

### Chamfer and fillet scope

Built in place, without the boolean: convex straight edges between flat faces whose ends each meet one
more flat face along straight edges; a fillet also needs that end face square to the edge (its end is
then a circular arc). Concave edges, and fillets ending on a curved or slanted face, are declined.

### Claims, document cache, peer transport, and profile syntax

* The shared `krly3d_core::document` mould is used by both products. krly3d stores geometry
  statements in `geo`; krlyvis stores `picture`, `outline` and `frame` statements in `vis`.
  Nodes have versions, can be opened/saved as text, and successful builds are reused by a
  content-derived key. krlyvis's session also exposes `polygons` and `sections` queries.
* krly3d claims include numeric and face-name expectations, `contains=x,y,z`, `meets=node`,
  and `iou:peer:drawing>=value`. `contains` counts the boundary as inside; `meets` is proved
  by a common point found among the searched vertices, edges and faces, so failure to find one
  is reported as unknown rather than proof of disjointness. `iou:` asks the peer for placed
  polygons and compares sampled silhouettes through the raster likeness plugin.
* The document cache memoizes successful node builds; a node build failure is retried. Separately,
  B-rep mesh successes are cached by body `Arc`, context and tolerance. A deterministic
  triangulation-budget decline is now memoized in that same bounded scope and exposed through
  `BrepMesh::decline_stats`; untyped declines, layout declines and transient failures are retried.
  This is not a content-hash or formally versioned cache. The user-facing mesh behavior is
  recorded in [interpret](crates/kernel/src/ports/interpret/mod.rs) and
  [BrepMesh](crates/std/src/interpret/mesh/mod.rs).
* `profile` accepts `R cx,cy w,h r` for a rounded rectangle, in addition to path, circle and
  ellipse forms. The radius must be positive and less than half the shorter side.
* NURBS projection tries several nearby grid seeds and uses Gauss–Newton with step
  halving to reduce distance. The thin, steeply swept band regression checks that surface
  points project back onto themselves; projection remains numeric, not an exact solver
  ([implementation and regression](crates/std/src/surface/nurbs.rs)).
* `--listen <socket>` serves one Unix-socket client at a time while keeping the session and
  peers alive across clients; `krly3d send <socket>` forwards stdin and copies the replies.
  Full socket hangup now cancels local read-only queries through a per-connection
  `CallControl`; normal write half-close still permits replies. The watcher is joined
  when the connection ends. A fresh connection gets a fresh control. Render/probe,
  image encoding and mesh work cooperate with cancellation. Mutating tools already
  running are not preempted; external plugin side effects are not rolled back.
  Peer calls are synchronous and one-at-a-time. Requests a peer sends while krly3d is waiting
  are refused as busy (notifications are ignored); callbacks, peer calls inside `batch`,
  asynchronous forwarding and cancellation propagation are not implemented.

### krlyvis: stateful vision product

`package/krlyvis/` is a separate current product made of workspace crates (`krlyvis-kernel`,
`krlyvis-std`, `krlyvis-cli`, binary `krlyvis`). It uses the shared `krly3d_core` plugin mould,
document, cache and claim grammar; its vision ports, data types and `Context` remain its own.
The `VisionSession` is stateful: `picture`, `outline` and `frame` calls add versioned nodes to
the `vis` document layer; `open`/`save` replay that document, `polygons` returns an outline,
and `sections` derives rounded-rectangle profile suggestions from two placed views. `analyse`
can read a path directly or use a stored picture node. PNM, PNG and JPEG readers are present.
The outline workflow is deterministic and does not use a learned model. It is not a complete
photo-to-CAD reconstruction system; real-photo depth, lens distortion and perspective-free
camera ambiguity remain limitations described in `package/krlyvis/SPEC.md`.

krlyvis has krly3d's command line: the transports in `crates/cli` now serve any
`endpoint::Endpoint` (the krly3d `Session` is one, krlyvis's `VisionSession` another).
`krly3d --peer vis=krlyvis` starts krlyvis as a child process and answers its methods as
`vis.…` over plain JSON-RPC (`crates/cli/src/peer.rs`: `Peer`, `Joined`), in the plain and
the MCP dialect alike. The kernel's `methods` reply became the public `api::describe`.
A peer's schema texts are leaked once at start (`peer::lasting`), because `Schema` holds
`&'static str`; bounded by the size of its method list.

### Raw seed and typed mesh pilot

`crates/krlyxd` now owns the independent IDs, clocks, exactness, math aliases and quadrature;
core preserves compatible re-exports. Error-coupled math, Accuracy and document/plugin/schema
support remain in core. [ADR 0008](docs/decisions/0008-raw-foundation-contract.md)
settles them as domain geometry support rather than pending raw exports. Shared coordinate,
confidence, unknown/measured uncertainty and producer metadata now live in raw; mesh/view
consume the validators without wire changes. One central Cargo/descriptor metadata gate
and the existing production dependency gate enforce the foundation contract (#20 closed).
See [raw kernel reference](docs/RAW-KERNEL.md).

`products/krly3d/mesh/api` and `engine` provide a typed synchronous mesh contract and snapshot
bridge. The legacy query and new bridge share one calculation path; the tessellator remains
in its temporary geometry location. The descriptor advertises no events/cancel and rejects
timeouts. See [mesh API](docs/MESH-API.md) for this unchanged v1 contract.

Local Rust callers can use `execute_controlled` with an injected deadline or cancellation
flag. Per-call contexts share services/caches; the tessellator and statistics stop
cooperatively, and cache/result publication is serialized against cancellation. The
[v2 operation runner](docs/MESH-OPERATIONS.md) now adds bounded Started/terminal events
and detached background work. Separate [mesh CLI/MCP adapters](docs/MESH-ADAPTERS.md)
now expose v2 through canonical CLI events and nonblocking MCP operation tools. Both load
native models through the same engine helper. The subsequent legacy socket fix carries
control to local read-only queries without changing their JSON-RPC/MCP reply shapes.
`Session::call_with_control` also accepts a Rust-side deadline; no new legacy wire timeout
field or progress stream is introduced. Peer forwarding cancellation remains #16;
other domain contracts and events remain #26.

### Current verification status

Final integrated verification on rustc 1.99.0 (b940084d7 2026-09-28) passed: `cargo test --workspace`
reported 1011 tests plus one compile-fail doctest (1012 total), 0 failed or ignored; `cargo clippy --workspace --all-targets -- -D warnings`
passed unsuppressed; and `cargo fmt --all -- --check` passed. The earlier 721-test run and clippy
failure were pre-fix baseline results; the first integrated wave passed 728 tests, the second 765,
the third 783, the fourth 821, and the fifth 856. The latest standalone
workspace run also completed its doctest phase successfully. All eight
recorded legacy CLI JSON-RPC responses match the new build. The official Python MCP SDK
smoke passes four CLI/MCP geometry-parity cases plus schema/error/deadline/cancellation
checks against the new standalone adapters. Wave six passed 883 tests; wave seven adds
32 tests for the [opt-in mesh preview](docs/MESH-PREVIEW.md), including source-face
metadata, rasterization, interruption and legacy CLI/MCP image parity. Full tests,
strict Clippy, fmt and fresh-binary compatibility passed after this integration.
The new backend lives under `products/krly3d/view/mesh`; the active legacy facade and
exact picking remain. #2 is closed; its remaining operation/event migration is in #26.
#1 is closed under revised acceptance: the user retired named F-35/737/moai benchmarks and closed #18 as not planned. Standalone view contract migration remains #26; alternative validation will be selected later.

The eighth wave extracts native loading and pinned source ownership into
[the B-rep API/engine](docs/BREP-SNAPSHOTS.md), consumed by mesh through an explicit
legacy geometry bridge. Ten runtime tests and one privacy doctest were added;
full workspace gates and fresh-binary CLI/MCP compatibility passed again. This
is an in-process ownership boundary; standalone B-rep/view contracts remain #26.

Wave nine adds the [typed view API/renderer](docs/VIEW-API.md) under
`products/krly3d/view/{api,renderer}`. Thirteen tests cover validation, legacy
PNG/face parity, pinned ownership, controlled execution and timing. Full workspace
gates and fresh-binary legacy/MCP compatibility passed again. A real model also
rendered successfully through the public API example. The single-node local Rust
contract is available; broader scene operations and a wire operation lifecycle remain #26.

Wave ten adds [synchronous view wire 1.0 and standalone CLI/MCP](docs/VIEW-ADAPTERS.md)
under `products/krly3d/view/{api,cli,mcp}`. A strict shared codec validates inputs,
encodes PNG/provenance/quality and supplies the descriptor-derived tool schema.
Six official SDK CLI/MCP camera/backend cases verify identical PNGs and metadata
except process-local identity/timings. No wire cancellation, timeout or event stream
is advertised: the synchronous MCP loop waits for rendering and writes.

## Build and test

```bash
export PATH="$HOME/.cargo/bin:$PATH"     # rustup lives in ~/.cargo, the shell profile was not changed
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all -- --check
```

The current environment has `/usr/bin/cc` (Ubuntu GCC 15.2.0) and no repository-local
`.cargo/` directory. Earlier environments used a local linker shim; it is not required
by the verified build above.

## Historical verification snapshots

* An earlier snapshot recorded 262 passing tests, clean strict clippy and formatting (2026-11-14).
  These are not today's results; see “Current verification status” above.
* Phases 1–3 at their end: `cargo test --workspace` passed 168 tests (46 in `core`, 23 in `kernel`, 86 unit and
  13 integration tests in `std`); clippy and the format check are clean.
  * registry: chain order, stopping at a failure, deterministic listing, lookup by family type,
    most-specific-first lookup of pair rules;
  * roots: quadratic, cubic, quartic and degree-two trigonometric equations against known roots
    and residuals;
  * every curve (line, circle) and surface (plane, sphere, cylinder, cone, torus) passes the contract
    of its family: projection round trip, tangent and normal against central differences, signed
    distance, line hits, quadric form, vector area against a fan of triangles, behaviour under a
    similarity;
  * every sweep rule passes the sweep contract and every meet rule the meet contract;
  * every tool result passes the validity check (names, closed loops, each edge used once in each
    direction, edges on their surfaces, Euler characteristic per shell);
  * volume and area of box, cylinder, cone, frustum, sphere, torus, tube, partial revolutions and
    extrusions with arcs and holes agree with the closed forms to 1e-12 relative; every content rule
    gives the opposite flux for the face turned inside out, and every measure follows the body under
    a similarity that moves it away from the origin;
  * against `tests/reference/legacy-kernel.json`: the six primitives match the recorded volume and
    area to 1e-12, and the fluted column shaft (24 flutes as one profile) matches the closed form
    and the recorded 6.001370331, with the same 50 faces;
  * the kernel end to end: a statement becomes a body and a delta naming its faces; redefining a
    name makes a new version; a dry run keeps nothing; rollback; a node is built once (same `Arc`)
    however often its document is built; a failing statement names its node; `k3d` text reads,
    builds, writes back identically, and a saved and reloaded model is the same model.
* Phase 0, on the old kernel before its removal (debug build, rustc 1.100.0):
  the unit suite passed 444 tests, 0 failed, 1 skipped (`two_million_entity`), in 76 s;
  the column example built 49 bodies and 17 shapes in 11.6 s and the shaft volume matched the closed
  form to 6.4e-14. The numbers are in `tests/reference/legacy-kernel.json`.

Not verified: the old integration tests were never run (`tests/step_oracle` needed a baseline file
that was not present). They were removed with the old code.

## Layout

* `crates/core`    – ids, `Attempt`, `Error`, `Accuracy`, `Exactness`, `Clock`; math: frames, closed-form
  roots, quadric algebra
* `crates/kernel`  – `Family`, `Plugin<F>`, `Registry<F>`, `KernelBuilder`, `Context`, `Kernel`; `schema`
  (parameters); `topo` (body, builder, validity check); `model` (operations, document, memoised build,
  delta); ports `curve`, `surface`, `sweep`, `meet`, `tool`, `stage`, `content`, `measure`, `read`,
  `write` with their contract tests
* `crates/std`     – the plugins: 2 curves, 5 surfaces, 4 sweep rules, 3 meet rules, 12 tools, 6 stages,
  5 content rules, 5 measures, the `k3d` reader and writer
* `models/`        – model files in `k3d` text; each one is replayed by the tests
* `tests/reference/` – numbers recorded from the old kernel; the only external oracle besides closed forms
* `porting/`       – the modules written for krly3d before the rebuild (kernel, boolean, section, naming,
  perception, protocol, the column example). They do not compile; each is deleted once its
  replacement passes the same assertions.
* `legacy-opengeometry` is mentioned in older notes as the pre-rebuild branch, but no local or
  remote-tracking ref with that name exists in this checkout. Keep the remaining `porting/`
  sources until their replacements and assertions are verified; do not rely on that branch as a
  recovery path.

## Conventions

* One file is one plugin; a new capability is a new file plus one registration line. Existing
  implementations are not edited to add a case.
* Every plugin runs the contract test of its family; every closed form is tested against an
  independent formula or a residual.
* Tolerances come from `Accuracy`; no numeric literals for tolerances anywhere else.
* Tests sit at the bottom of the file they test, are named after the behaviour, and use an injected
  `FakeClock` instead of the real clock.
* Measure before optimising, and report the measurement.

## Deviations from the plan, and why

* Roots are closed form (Cardano, Ferrari) instead of companion-matrix eigenvalues: the eigenvalue
  iteration of nalgebra does not converge on the companion matrix of x⁴ + 1 (a cyclic permutation).
* `ellipse` and `trace` curves and the `trace|quadric` rule wait for phase 5, where sections use them.
* Sweep rules are keyed `line|shift` rather than `line×shift`, so that one pair lookup serves all
  pair families.
* A seventeenth family, `content` (area and volume share of a face per kind of surface), carries
  what the plan put inside the `volume` measure.
* Stages: there is no `resolve` (every body is in world coordinates), `name` runs before `check`
  (a duplicate name is invalid), and the edit pipeline judges expectations before commit.
* Not built yet from the phase 3 list: `polyhedron` and the `bbox` measure. `mirror` is built: every
  curve and surface states its reflection (`mirrored`, checked by the family contracts), and a kind
  that does not is declined. A copy of a node is a `transform` under a new name, so
  there is no separate `instance` tool.
* `Kernel::apply` returns a typed `Delta` with a `text()`; the `Reply { data, text, exact }` wrapper
  is used by the session protocol. `expect` claims are judged; a failed expectation prevents commit.
* The code format is set in `rustfmt.toml` (width 110).

## Decisions waiting for the owner

* Licence of the new crates. The manifests state none, and LICENSE.md and NOTICE.md are unchanged;
  they still describe the removed code.
* Whether to commit each finished phase on `rebuild/core`. Only the snapshot is committed so far.
* The accepted product architecture and current workspace layout are documented separately in
  `docs/PRODUCT-ARCHITECTURE-HANDOFF.md`; the raw seed and synchronous mesh pilot are migrated; other target packages remain pending.

## Pitfalls already paid for (still true for the new code)

* A parameter that makes the caller compute a turn invites a sign error that every check passes. Give
  tools the intent (`to`, `align`/`onto`) and state where things must be (`contains`, `meets`).

* "Which way does the face lie from this edge" is only meaningful at a stated point: on a full
  circle the direction at the opposite point is mirrored. `topo::Bound` therefore carries the point.
* nalgebra's eigenvalue iteration does not converge on a cyclic companion matrix; a test hung on it.
  Run tests with a time limit when touching iterative numerics.
* A closed profile must end exactly at its start point and must not retrace a segment.
* A ray through the parametric seam of a cylinder lands on a trim boundary; such hits are kept and
  de-duplicated by parameter, not dropped.
* Full circles have coincident end points, so the direction of traversal cannot be inferred from them.
* A bounding-volume tree gains nothing if the bounds of the body are recomputed for every ray.


Wave eleven completes #20 raw foundation, #3 content-aware bounded decline memoization
and #7 rotated station matches. See [cache contract](docs/MESH-DECLINE-CACHE.md) and
[station reports](docs/LIKENESS-STATIONS.md). Real krlyvis peer replay passes 13 calls;
raster-derived contours retain a pixel uncertainty floor. Full workspace has 77 passing
suites; strict Clippy/format and existing SDK/legacy compatibility pass. There are 17 open
and 9 closed issues. #26 retains standalone likeness and controlled view lifecycle migration.
Workers stopped at quota; the integrator completed verification. All changes remain local,
uncommitted and unpushed. Active providers and the root `.tmp/` archive remain in place.


Wave twelve adds the [standalone view operation lifecycle](docs/VIEW-OPERATIONS.md)
under the existing `products/krly3d/view/{api,renderer,cli,mcp}` boundary (ADR 0009).
Protocol/API 2.0 retains Started plus exactly one terminal event with a v1 completed
render payload. Detached snapshots preserve source identity after owner drop. Bounded
admission, byte retention, deadlines, cancellation and explicit release are implemented;
worker capacity survives cancellation until actual exit. CLI `--operations` streams
canonical events; MCP adds start/next/cancel/release while preserving `view_render`.
EOF and I/O failures drop/cancel the runner. Backpressure can still block transport
writes, and legacy synchronous calls do not process cancellation notifications.
No progress or partial image capability is claimed. Broader domain and scene migration
stays in #26. Active legacy providers and `.tmp/` contents remain in place.

Wave twelve verification: 1012 tests across 79 suites, zero failed/ignored; strict
workspace Clippy, fmt and diff checks pass. Official SDK verifies six v1 and two v2
CLI/MCP image parity cases, schema/error handling, deadlines/cancel/release and EOF.
#26 updated; 17 issues open, nine closed. Changes remain local, uncommitted and unpushed.


Wave thirteen prioritizes a usable drawing-to-proposal flow over further infrastructure.
The [planar drawing pilot](products/krly3d/reconstruction/cli/README.md) orchestrates real
krlyvis evidence, a bounding-prism mesh before editable work, contour extrusion, PNG and
station validation. It requires user scale/height and reports hidden-feature assumptions.
The tracer fills holes; outer-extent matches do not validate interiors. Legacy peer
bridges remain, so #24's full MCP/refinement scope is open. Four real process tests pass;
small debug demo: coarse mesh 76.3 ms, editable model 82.8 ms, total 588.1 ms. Rust code
was unchanged; 1012 is the previous Rust baseline, not a new run. No commit/push.


Wave fourteen adds opt-in krlyvis `trace-holes`, retaining pixel voids while preserving
default `trace` compatibility. The pilot now extrudes all boundary loops, reloads the
editable model and checks net material area × height against closed mesh volume.
Corner-touch ambiguity fails explicitly; holes are assumed through-holes, not inferred
hidden cavities. Tests: 80 targeted krlyvis Rust tests, four process tests including an
independent exact-ray view through the hole, 13-call legacy peer replay; targeted Clippy,
fmt and diff checks pass. The full workspace was not rerun. #24 remains open; no commit/push.


## Wave 15: multiple selected parts from one drawing

The [sheet runner](products/krly3d/reconstruction/cli/SHEET.md) accepts 1–16 explicitly named/seeded components with individual extrusion heights and one shared pixel scale. Each yields a separately editable proposal, early mesh, PNG, hole-preserving profile and material-volume/closure check, in common drawing coordinates. The atomically updated manifest records per-part status and provenance. A shared deadline and process cancellation preserve completed parts; duplicate component selections fail. This does not infer assembly constraints or automatically recognize parts. The existing single flow also accepts an explicit seed. The implementation stays in the reconstruction orchestration layer, reusing active domain providers.

Verification: 14 real process tests pass together (4 single-flow + 10 sheet, 58.036 s), including holes/position/height, invalid inputs, overwrite rejection, duplicate selections, partial preservation, shared deadline and SIGINT/SIGTERM descendant cleanup. Python compilation/editor diagnostics and diff-check pass. No Rust change or new full-workspace run this wave.


## Wave 16: planar B-rep contract used through MCP

The [planar construction contract](docs/BREP-PLANAR.md) adds strict stateless construction
from bounded outer/hole polygon loops and height to the B-rep API/engine, with separate
CLI/MCP adapters. Existing geometry providers remain the only implementations. The drawing
and sheet consumers now use the actual `brep_planar` tool for editable proposals and preserve
the canonical request/result plus source evidence digest. Mesh/view reload the returned native
document; legacy likeness loads the same document. Coarse construction, krlyvis and likeness
still use the legacy facade. This synchronous operation does not supply a background lifecycle
or remote cancellation; product deadline/process cleanup remains explicit. See ADR 0010 and
the development log for final integration evidence. No source provider is retired.

Wave-16 verification: 1055 workspace tests across 87 suites, strict all-target Clippy/fmt;
14 real drawing/sheet tests and three official SDK CLI/MCP parity cases with independent
volume/closure reload pass. #24/#26 updated; broader scope stays open (17 open / 9 closed).
Implementation is local, uncommitted and unpushed.
