# krly3d – handoff notes

State of the work. Read this first, then SPEC.md for the design.

## Where things stand

The kernel is being rebuilt from scratch on one plugin pattern (SPEC.md). The code inherited from
OpenGeometry 3.0.0 has been removed from the working tree; nothing of it was copied into the new files.

| Phase (SPEC.md chapter 6) | State |
|---|---|
| 0 Snapshot and reference numbers | done |
| 1 Removal, workspace, `core`, plugin registry | done |
| 2 Equations: `curve`, `surface`, `sweep`, `meet` | done |
| 3 Process: `topo`, `model`, `stage`, `tool`, `content`, `measure`, `read`/`write` | done |
| 4 Perception and the agent's loop | in part: session API, probe, trim, judge with expectations; queries, CLI and rendering are in unmerged agent copies (see below) |
| 5 General boolean | done for crossings with closed-form sections; tangent contact, tori being split and non-closed-form sections are declined |
| 6 Chamfer, fillet, mirror | done (see the scope below); the column model was dropped by the owner |
| 7 AI layer | `expect` judged; notes, watchers, proposals, sweep, history, contour not started |

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

### Phase 0 of the foundation plan (2026-11-14)

The plan of SPEC.md chapter 8 (one foundation, many interpretations) was approved. Phase 0 is done:

* `krly3d-core` holds the one plugin mould (`plugin.rs`: `Family` now names its `Context`) and the
  protocol schema (`schema.rs`); the kernel re-exports both. Quadrature (Gauss–Legendre) has one table,
  `core/src/math/quadrature.rs`. `crates/core/tests/one_copy.rs` fails when any of them is copied.
* The camera left the kernel: `crates/view` (`krly3d-view`) holds the render port, the renderer and the
  `render`, `view`, `silhouette`, `pick` queries; the CLI registers it. `krly3d-std` exposes its test
  helpers under the `testing` feature for plugin crates.
* Merged from the stopped agents: the queries agent's `index`, `keys`, names/anchors, the box-tree
  probe and the queries (`Context::derived` caches per body), and the export agent's STEP writer, now
  `crates/exchange` (`krly3d-exchange`, query `step`). The export agent's mesh port was unfinished (it
  names a missing contract and has no rules); it waits for phase 1, where `interpret::mesh` comes.
* SPEC.md chapter 8 and `docs/architecture.drawio` (new page "One foundation, many interpretations").
* Not done: switching krlyvis to the core mould. Its agent was editing it at the time; the switch is to
  replace `package/krlyvis/crates/kernel/src/plugin.rs` with `pub use krly3d_core::plugin::*;`, add
  `type Context = crate::context::Context;` to its families, and let `one_copy.rs` scan `package/` too.
* Verified: 674 tests green (`cargo test --workspace`), clippy clean.

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
* Verified: 681 tests green (`cargo test --workspace`), clippy clean; live through the CLI, the
  tesseract strut meshes closed (0.05: 1084 triangles, volume 40.29 against the exact 41.79).

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

### Unmerged work of the background agents

Three agents stopped on the usage limit. Their copies are in the session scratchpad
(`wt-queries`, `wt-cli`, `wt-export`). They were taken before the boolean, the section rules, the judge
stage, the predicates and the placement work, so a merge copies their own files and reconciles:
`ToolRequest.sources`, the trim rules, and in `crates/cli/src/json.rs` the list of required parameters,
which must read `param.required` instead of `param.default.is_none()`.

### Chamfer and fillet scope

Built in place, without the boolean: convex straight edges between flat faces whose ends each meet one
more flat face along straight edges; a fillet also needs that end face square to the edge (its end is
then a circular arc). Concave edges, and fillets ending on a curved or slanted face, are declined.

### krlyvis: the image reader (added 2026-11-14)

`package/krlyvis/` holds a second kernel on the same mould, for reading one photograph: LSD segments,
vanishing points by vote, the camera (focal length, axes) and lengths in a chosen plane. Its crates
(`krlyvis-kernel`, `krlyvis-std`, `krlyvis-cli`, binary `krlyvis`) are workspace members and share
`krly3d-core`. `package/krlyvis/SPEC.md` has the design, what is verified and what is not. The
`Plugin`/`Registry` mould is a copy of `kernel/src/plugin.rs` because the kernel's `Context` carries
geometry pipelines; hoisting the mould into `core` would remove the copy (not done, it edits the kernel).
Only Netpbm files are read; JPEG/PNG need a decoder crate, which waits for the owner's approval.

krlyvis has krly3d's command line: the transports in `crates/cli` now serve any
`endpoint::Endpoint` (the krly3d `Session` is one, krlyvis's `VisionSession` another).
`krly3d --peer vis=krlyvis` starts krlyvis as a child process and answers its methods as
`vis.…` over plain JSON-RPC (`crates/cli/src/peer.rs`: `Peer`, `Joined`), in the plain and
the MCP dialect alike. The kernel's `methods` reply became the public `api::describe`.
A peer's schema texts are leaked once at start (`peer::lasting`), because `Schema` holds
`&'static str`; bounded by the size of its method list.

## Build and test

```bash
export PATH="$HOME/.cargo/bin:$PATH"     # rustup lives in ~/.cargo, the shell profile was not changed
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all -- --check
```

This machine has no system C toolchain. Linking goes through `.cargo/cc-shim.sh` (the linker that
ships with Rust plus the start files of libc6-dev), wired up in `.cargo/config.toml`; both are
git-ignored. After `sudo apt install build-essential` the `.cargo/` folder can be deleted.

## Verified

* Now: `cargo test --workspace` passes 262 tests, clippy with the strict lint set and the format
  check are clean (2026-11-14).
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
* branch `legacy-opengeometry` – the complete tree before the rebuild

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
  (a duplicate name is invalid), and `judge` waits for phase 4 with expectations.
* Not built yet from the phase 3 list: `polyhedron` and the `bbox` measure. `mirror` is built: every
  curve and surface states its reflection (`mirrored`, checked by the family contracts), and a kind
  that does not is declined. A copy of a node is a `transform` under a new name, so
  there is no separate `instance` tool.
* `Kernel::apply` returns a typed `Delta` with a `text()`; the `Reply { data, text, exact }` wrapper
  comes with the protocol in phase 4. `expect` lines are stored and written back but not judged yet.
* The code format is set in `rustfmt.toml` (width 110).

## Decisions waiting for the owner

* Licence of the new crates. The manifests state none, and LICENSE.md and NOTICE.md are unchanged;
  they still describe the removed code.
* Whether to commit each finished phase on `rebuild/core`. Only the snapshot is committed so far.
* krlyvis: a JPEG/PNG decoder dependency (e.g. the `image` crate), and whether to hoist the plugin
  mould into `core` so that krly3d and krlyvis share one copy.

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
