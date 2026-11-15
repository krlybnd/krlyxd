# krly3d – technical specification

krly3d is an exact B-rep geometry kernel that an AI agent can model with quickly and
reliably. This document specifies the architecture of the rebuilt kernel. HANDOFF.md says
which parts exist today.

Revision of 2026-11-14: chapter 8 records the accepted target architecture: a raw shared kernel,
separate `krly3d-*` domain packages, a CTM assembly/extension host, and the separate `krlyvis`
product. It is a target, not a claim that those package boundaries have all been implemented.
Chapters 1–7 describe the rebuilt B-rep kernel and include older design sketches; the layout in
chapter 5 is historical and is not the current Cargo workspace. HANDOFF.md records current
implementation and verification evidence.

## 1. Principles

Each of these was learned on the first version of the kernel and is now a rule.

| Lesson | Rule |
|---|---|
| An AI reads text, not vectors | Every answer is `Reply { data, text }`; the sparse vectors (K, R) stay an internal index |
| Store percepts, not just geometry | Relations (adjacent, opposite, tangent, thin, reachable) come from `keys` plugins, deterministically |
| A ray is a sentence | `Sentence` is the only one-dimensional answer type: ray, contour, parameter, history |
| One formula for search | `find` is `s = K·q + α·R·K·q_near + α²·R²·K·q_near`; there is no graph database |
| Names come from faces | Names are computed, never stored; notes, watchers, PMI and expectations anchor to names |
| Symmetry and equations beat brute force | Recipes and `pattern`; in a chain the closed form stands before the general method |
| Measure before optimising | Every stage and plugin is timed through an injected `Clock`; a `stats` query reports it |
| Drawing is a better test than a test | A model file is a test: `models/*.k3d` is replayed with its expectations and rendered |
| The bottleneck is the agent's loop | Every tool is on the protocol; MCP dialect; an image in the answer; dry runs |
| Case enumeration was 40% of the old code | Pairwise knowledge lives only in rule registries (`section`, `meet`); the algorithm is one |

## 2. Architecture

### 2.1 Layers and patterns

Dependencies point downward only, enforced by the compiler through crate dependencies.

```
┌────────────────────────────────────────────────────────────────────────────┐
│ CLIENTS   AI agent (MCP) · UI · tests · model files (models/*.k3d)         │
└─────────────────────────────────────┬──────────────────────────────────────┘
                                      │ JSON-RPC 2.0 over stdio
┌─────────────────────────────────────▼──────────────────────────────────────┐
│ cli       Session · rpc · dialect/{plain, mcp} [Adapter]                   │
└─────────────────────────────────────┬──────────────────────────────────────┘
                                      │ standard() -> KernelBuilder
┌─────────────────────────────────────▼──────────────────────────────────────┐
│ std       every shipped capability; one file = one plugin    [Strategy]    │
│                                                                            │
│  equations    curve · surface · sweep · section · meet   [Double dispatch] │
│  process      tool · stage                               [Chain of Resp.]  │
│  perception   probe · index · keys · measure · content · query  [Visitor]  │
│  document     attr · read · write · render               [Interpreter]     │
└─────────────────────────────────────┬──────────────────────────────────────┘
                                      │ implements ports
┌─────────────────────────────────────▼──────────────────────────────────────┐
│ kernel    contracts and orchestration, zero implementations                │
│                                                                            │
│  Kernel          apply(op) · ask(query)        [Facade] [CQRS]             │
│  KernelBuilder   one Registry per family       [Builder]                   │
│  Context         accuracy, registries, clock   [Dependency Injection]      │
│  plugin          Plugin<F> · Registry<F>       [Registry]                  │
│  pipeline        two ordered Stage lists       [Pipes & Filters]           │
│  ports/<family>  trait + Request + Response    [Ports & Adapters]          │
│  answers         Reply · Sentence · Predicate  [Specification]             │
│  model/          Document = layer stack        [Composite] [Overlay]       │
│                  build: Document -> Model      [Memoised pure fn]          │
│                  op graph                      [Command] [Event Sourcing]  │
│                  checkpoints                   [Memento]                   │
│                  shared shapes                 [Flyweight]                 │
│  topo/           Body Face Edge Vertex · walk  [Visitor]                   │
└─────────────────────────────────────┬──────────────────────────────────────┘
                                      │ uses
┌─────────────────────────────────────▼──────────────────────────────────────┐
│ core      Id · Attempt<T> · Error · Accuracy · Exactness · Clock           │
│           math: nalgebra aliases, roots, quadric algebra                   │
└────────────────────────────────────────────────────────────────────────────┘
```

An editable version of these diagrams is in `docs/architecture.drawio`.

### 2.2 How a request runs

```
apply(op, dry?, expect?)

  edit pipeline      ┌───────┐   ┌───────┐   ┌───────┐   ┌────────┐
  (Stage list)       │ build │──▶│ delta │──▶│ judge │──▶│ commit │──▶ Reply
                     └───┬───┘   └───────┘   └───┬───┘   └────────┘
                         │                       │        skipped when dry or
                         │                       │        an expectation fails
                         │                       └─ predicates over measure plugins
                         │                          (expect of this op + ai.watch)
                         ▼ for each changed node, memoised by content hash
  node pipeline      ┌─────┐   ┌──────┐   ┌───────┐
  (Stage list)       │ run │──▶│ name │──▶│ check │──▶ Body
                     └──┬──┘   └──────┘   └───────┘
                        │ tool chain: Done | Declined -> next plugin
             ┌──────────┼──────────┬──────────┐
             ▼          ▼          ▼          ▼
           sweep     section      meet      probe   rule registries

ask(query)  ──▶  query plugin  ──▶  Model · index (K, R) · probe  ──▶  Reply

Reply = { data, text, exact }     every answer: structured data + short text
```

### 2.3 Layers of a document

```
strongest ▲  proposal   an agent's change as a layer: accept = merge down, discard = drop
          │  ai         notes · watchers · expectations         written by the agent
          │  pmi.gfx    leaders, text placement                 for people
          │  pmi.sem    tolerances, datums, finish              for CAM / CMM
weakest   │  geo        op graph + parameters                   the stored truth

derived, never stored as truth:   Model (B-rep, names)  ·  idx (keys K, relations R)
```

### 2.4 Design patterns

| Pattern | Where | What it solves |
|---|---|---|
| Ports & Adapters | `kernel/ports` ↔ `std` | the kernel knows no implementation |
| Strategy | every plugin | interchangeable algorithms |
| Registry + Builder + Dependency Injection | `Registry<F>`, `KernelBuilder`, `Context` | assembly from outside, no global state |
| Chain of Responsibility | the chain under one `Id`, `Attempt::Declined` | closed form before the general method; a new case never edits old code |
| Pipes & Filters | the two `Stage` lists | one path of execution; new behaviour is a new stage |
| Command + Event Sourcing | `Op`, the op graph | the process is the stored truth and can be replayed |
| Memoised pure function | `build(Document) -> Model` | incremental rebuild, undo, dry run and sweeps from one mechanism |
| Memento | a checkpoint is an older document pointer | rollback |
| Composite | `Recipe` tools, the node tree | a primitive is other operations |
| Visitor | `topo::walk` + `index` plugins | one traversal, many derived layers |
| Observer | `ai.watch` watchers | notification after every edit |
| Specification + Interpreter | predicates, search queries, `k3d` text | statements and models as data |
| Double dispatch through a registry | `section`, `meet` keyed `a\|b` | pairwise knowledge without case branches |
| Overlay | the layers of a `Document` | a proposal can be accepted or discarded |
| Flyweight | instances share one shape | 24 flutes, one definition |
| Facade + CQRS | `Kernel::apply` / `Kernel::ask` | edits and questions travel separately |
| Adapter | `dialect` (plain, MCP), `read`/`write` | the same kernel in several languages |
| Decorator | `Timed<P>`, `Cached<P>` | timing and caching without touching the plugin |

### 2.5 SOLID

- **S** – one file is one plugin is one responsibility.
- **O** – a new capability is a new file and one registration line; no existing implementation is edited.
- **L** – the contract test of a family runs against every plugin, so any of them can stand in for another.
- **I** – every port asks a single question (`call`); there is no collecting interface.
- **D** – `kernel` knows only ports, `std` implements them, `cli` sees only the `Kernel` facade.

## 3. Specification

### 3.1 The mould

Every family consists of the same four elements in the same places.

| Element | What | Where |
|---|---|---|
| Port | the `Family` marker with its `Request` and `Response` types | `kernel/src/ports/<family>/mod.rs` |
| Contract test | a generic test every plugin runs against itself | `kernel/src/ports/<family>/contract.rs` |
| Plugin | one file = one struct = one implementation with its tests | `std/src/<family>/<plugin>.rs` |
| Registration | `pub fn register(b: &mut KernelBuilder)` | `std/src/<family>/mod.rs` |

```rust
pub trait Family: 'static {                     // one marker type per family
    const NAME: &'static str;
    type Request;
    type Response;
}
pub trait Plugin<F: Family>: Send + Sync + 'static {
    fn id(&self) -> Id;                         // stable text id: "extrude", "plane|sphere"
    fn schema(&self) -> Option<Schema> { None } // only for plugins callable from outside
    fn call(&self, cx: &Context, req: &F::Request) -> Attempt<F::Response>;
}
pub struct Registry<F: Family> { /* BTreeMap<Id, chain of plugins in registration order> */ }
impl<F: Family> Registry<F> {
    pub fn add(&mut self, plugin: impl Plugin<F>) -> &mut Self;
    pub fn call(&self, cx: &Context, id: &Id, req: &F::Request) -> Attempt<F::Response>; // runs the chain
}
pub enum Attempt<T> { Done(T), Declined(String), Failed(Error) }

builder.plugin::<Sweep>(LineShift);              // registration
cx.call::<Sweep>(&id, &request);                 // a plugin asking another family
pair::ask::<Meet>(cx, curve.classes(), surface.classes(), &request);   // most specific pair first
```

- Every service family implements this one trait; the family only supplies `Request` and `Response`.
- Four object families (`curve`, `surface`, `probe`, `index`) have their own multi-method trait,
  because they are objects carrying data. Their folder and contract test are the same; curves and
  surfaces are created by the plugins that need them and are not registered.
- The registries are found by the marker type of the family (`Registries`), so a new family adds
  no field anywhere.
- Plugins sharing an `Id` form a chain. The first plugin that does not decline settles the request:
  `Done` answers it, `Failed` is reported as it is (a failure is never hidden behind a fallback),
  `Declined` passes it on. When all decline, the reasons are returned together.
- Keys are text `Id`s, not enums, so a crate outside the kernel can add a curve, a surface or a tool.
- Registries are deterministic. There is one `Error`, one `Accuracy`, one `Attempt`, one `Reply`;
  there is no global state, everything comes from `Context`.

### 3.2 Families

| Group | Family | Key | Request → Response | First plugins |
|---|---|---|---|---|
| equations | `curve`* | kind | `t` → point, tangent; projection | `line`, `circle`; with sections: `ellipse`, `trace` |
| | `surface`* | kind | `uv` → point, normal; signed distance; line hits; `quadric()` | `plane`, `sphere`, `cylinder`, `cone`, `torus` |
| | `sweep` | `curve\|motion` | profile edge + motion → surface | `line\|shift`, `circle\|shift`, `line\|turn`, `circle\|turn` |
| | `section` | `a\|b` | two surfaces → section curves | the six closed forms, `plane\|quadric`, `quadric\|quadric` |
| | `meet` | `curve\|surface` | curve + surface → parameters, or "lies within" | `line\|surface`, `circle\|plane`, `circle\|quadric`; with sections: `trace\|quadric` |
| process | `tool` | tool name | parameters + input bodies → `Body` | see 3.4 |
| | `stage` | name | running state → state | node stages `run`, `name`, `check`; edit stages `build`, `delta`, `commit`; with expectations: `judge` |
| perception | `probe`* | name | ray → hits; point → inside/outside and distance; box | `exhaustive` (reference), `box-tree` |
| | `index`* | name | traversal → derived index | `keys` (K and R) |
| | `keys` | prefix | element → sparse keys | `name`, `kind`, `made`, `n`, `r`, `axis`, `cell`, `coaxial`; later `ray`, `shape`, `param`, `hist`, `note` |
| | `measure` | name | body → number + exactness | `volume`, `area`, `faces`, `edges`, `vertices`; later `bbox`, `wall` |
| | `content` | surface class | face → area + volume share | `plane`, `cylinder`, `cone`, `sphere`, `torus` |
| | `query` | name | parameters → `Reply` | `faces`, `edges`, `here`, `ray`, `find`, `keys`, `measure`, `overview`, `brief`, `view`, `stats`; later `sweep`, `history`, `contour` |
| document | `attr` | namespace | anchor + value → checked attribute + keys | `ai.note`, `ai.watch`; later `pmi.sem.*`, `pmi.gfx.*` |
| | `read`, `write` | format | bytes ↔ `Document` | `k3d`; later `step` (write only) |
| | `render` | name | view → image | `raycast` |

\* object family. Seventeen families in all.

Every curve and surface states its classes from the most specific (`sphere`, `quadric`, `surface`);
pair rules are looked up in that order, so the fast closed form is asked before the general rule.

A line against a surface is the one pairing asked directly (`Surface::line`), without a registry
lookup or an allocation, because every ray of every probe goes through it; the rule `line|surface`
wraps it so that the other consumers see lines like any other curve.

### 3.3 The model is a pure function of the document

- A `Document` is an immutable value: a list of layers holding operations and attributes.
- `build(Document) -> Model` is a pure function, memoised per node by content hash
  (tool + parameters + hashes of the inputs).
- That one mechanism gives incremental rebuild (only the changed branch is recomputed), undo
  (an older document pointer), dry run (the pointer is not swapped), proposals (the document plus one
  layer) and parameter sweeps (many documents in parallel sharing the cache).
- A node is a label; a modifying tool rebinds it (`part@1`, `part@2`), so the log reads both as a
  sequence and as a dependency graph.
- There are two ordered `Stage` lists from the same family: building a node
  (`run → name → check`) and editing (`build → delta → commit`, with `judge` before `commit`
  once expectations exist). New behaviour is a new `Stage`. There is no `resolve` stage:
  every body is in world coordinates, so an input needs only to be looked up.

### 3.4 Tools, recipes, names

```rust
pub struct Op { pub tool: Id, pub params: Params, pub inputs: Vec<NodeRef> }
```

- **Recipes:** a primitive expands into other operations (`box` is a rectangle profile plus `extrude`;
  `sphere` is a half disc plus `revolve`), about ten lines each.
- **Tools:** `profile`, `rectangle`, `circle`, `extrude`, `revolve`, `box`, `cylinder`, `cone`, `sphere`,
  `torus`, `transform`, `pattern`; to come: `polyhedron`, `mirror`, `union`, `subtract`, `intersect`,
  `chamfer`, `fillet`. A copy of a node is a `transform` under another name, so there is no
  separate `instance` tool.
- **Conventions:** z is up. A profile is drawn in a named plane (`xy` the ground; `xz`, `yz` upright,
  with the world z as their second axis, so an upright profile reads as distance from the axis
  and height). `revolve` turns about that second axis by default. Angles are given in degrees.
  A `box` stands on the ground with its base centred on `at`; its faces are `bottom`, `top`,
  `front` (towards −y), `back`, `left` (towards −x), `right`.
- **Building a body:** a tool states each face as "on this surface, with the material away from
  that side, bounded by these edges, the face lying that way from each edge as seen at this
  point"; the builder works out the direction of every edge use and chains the loops. Tools
  contain no case analysis of orientation.
- **One `Schema`, three uses:** parameter checking on the protocol, the `k3d` codec, the MCP
  `inputSchema`. A new tool is therefore callable, storable and listed without further work.
- **Names:** an edge is named by its two faces (`front|top`), a new face by the operation
  (`chamfer(front|top)`, `hole:side`), a split face as `front#1`. The `name` stage computes them;
  nothing is stored.

### 3.5 Answers: data, text, exactness

```rust
pub struct Reply { pub data: Value, pub text: String, pub exact: Exactness }
pub enum Exactness { Exact, Numeric { tolerance: f64 }, Sampled { samples: u32 } }
```

In text, `=` marks a closed form, `~` a numerical value of stated tolerance and `?` a sampled bound,
so the reader sees how far a number can be trusted. The answer to an edit looks like

`ok part@1 subtract. +hole:side ~top ~bottom. faces 6>7. volume =23497.345175`

### 3.6 Mathematics

| Area | Solution |
|---|---|
| Vectors, matrices, transforms | nalgebra `Vector3<f64>`, `Matrix4<f64>`, `Isometry3<f64>` |
| Surfaces | Each surface is its own plugin. Plane, sphere, cylinder and cone share one quadric algebra: `xᵀQx = 0`, transformed as `MᵀQM`. The torus is an implicit quartic. |
| Ray and edge intersection (`meet`) | Substituting the curve into the implicit equation of the surface gives a univariate equation: line–quadric is quadratic, line–torus quartic, circle–quadric a trigonometric equation of degree two (a quartic in the tangent of the half angle, with the chart chosen away from the roots). Roots in closed form (Cardano, Ferrari), reported with multiplicity so that grazing never changes the parity of crossings, then polished and verified by residual. |
| Sweeping | profile edge × motion → surface, from a rule registry |
| Sections (`section`) | Closed forms come first in the chain. General quadric pair: the eigenvalues of the pencil `Q₁ − λQ₂` decide the type of the section; the curve is parameterised by substitution, `v(u) = (−b ± √D)/2a` (the `trace` curve). |
| Classification | winding number |
| Volume, area (`content`) | Divergence theorem per face. With `V` the integral of the outward normal over a face — a boundary integral, closed form for line and circle edges on any surface — `p·n` is on a plane the height of the plane, on a sphere or cylinder `anchor·n + r`, on a cone `apex·n`, so the volume share of a face is `anchor·V` plus a multiple of its area. The area comes from Green's theorem (cylinder, cone, torus) or Gauss–Bonnet (sphere, any circular boundary). |
| Fillet, chamfer | rolling ball: the path of the centre is the section of the offset surfaces |
| Tolerance | adjustable `Accuracy` scaled to the model size; no literals elsewhere |
| Bulk computation | batched calls (rays, replays, pixels) on rayon; measure first |

### 3.7 Testing contract

- Every plugin runs the `contract.rs` test of its family.
- Where a reference plugin exists (`exhaustive`), the others are tested for identical answers.
- Every closed form is tested against an independent formula or a residual.

### 3.8 Adding a plugin: always the same three steps

1. A new file `std/src/<family>/<name>.rs`: a struct, `impl Plugin<F>`, tests that call the contract test.
2. One line in `register` of `std/src/<family>/mod.rs`.
3. Done: it appears on the protocol, in the MCP tool list and in the `k3d` format.

A new family is `ports/<family>/{mod.rs, contract.rs}`; nothing else in the kernel changes.

## 4. The AI-facing half

### 4.1 Layered document, native text, a proposal as a layer

- One line is one statement; the codec is driven by schemas and knows no tool by name.
- Anchors: `node`, `node@2` (version), `node#face`, `node#a|b` (edge), `node.param`.
- Layers compose from weakest to strongest: a later layer may override a parameter or an attribute.
- A proposal: `propose` puts a new layer on top; `diff` is the Delta between the model built with and
  without it; `accept` merges it into its base layer; `discard` drops it. Until accepted, the other
  layers are untouched.

```
k3d 1
layer geo
plate = box w=60 d=40 h=10
hole  = cylinder r=4 h=10 at=20,15,0
part  = subtract plate hole
        expect faces=+1 adds=hole:side volume=-502.654825±1e-6
part  = fillet part edges=front|top r=2
layer pmi.sem
part#top        flatness=0.05
layer ai
part#hole:side  note="press fit, keep H7" keys=fit
part            watch="wall>=3"
layer proposal name="bigger hole" base=geo
hole.r = 5
```

The extension `.k3d` is a working name.

### 4.2 Notes: the agent's long-term memory

- An `ai.note` attribute is anchored to a name. Its keys enter the K matrix as `note:<key>`, so `find`
  retrieves it.
- A note travels with name inheritance: when a face splits, every piece carries it. When the anchor
  disappears the note becomes an orphan; the Delta reports it with its last known point and the agent
  can anchor it again.
- `here` shows the notes of a face. `brief` returns, in one answer on opening a model, the overview,
  the notes, the watchers and the last operations; this carries knowledge from one conversation to
  the next.
- A note can also sit on an operation or a parameter ("why is this 40?"), which keeps the intent.

### 4.3 Watchers and dry runs with expectations

- Both use one language: predicates over `measure` plugins. Forms: `wall>=2`, `volume=1200±0.5`,
  `faces=+2`, `adds=hole:side`, `keeps=front|top`.
- **Watcher:** an `ai.watch` attribute. The `judge` stage evaluates it after every edit on the bodies
  that changed and reports only transitions (holds → violated and back), which keeps answers short.
- **Dry run:** with `dry: true` the `commit` stage is skipped; the Delta and the verdicts come back.
- **Expectation:** with `expect: [...]` the edit is committed only if every expectation holds;
  otherwise the answer says which one failed and what the value was. The agent learns in one round
  trip that it was wrong.
- The first `wall` is ray based (marked `?`); the exact value comes with the `gap` family (4.8).

### 4.4 The parameter axis and the sentence as the general answer

- `Sentence = Vec<Span { from, to, label }>` generalises the ray sentence to every one-dimensional
  question.
- **`sweep`:** runs one parameter through a range. The document is replayed in parallel for several
  values; the topology signature (face names + adjacency) is constant on intervals and a break point
  is located by bisection down to `Accuracy`. The answer has the form (illustrative values)
  `r 0 > [ok faces=8] 4.2 > [fillet(front|top) gone, faces=7] 6 > [fail check: open shell]`
  and is marked `~` because a break point is numerical.
- **`history`:** what a point was after each operation: `box: solid > subtract: air`.
- **`contour`:** a face walked around, edge by edge: neighbour, convex/concave/tangent, angle, length.

### 4.5 General boolean

One algorithm in five steps; pairwise knowledge lives only in `section` and `meet` rules.

1. **Candidates:** a box tree over faces gives the overlapping face pairs.
2. **Section:** `section` on the two carrier surfaces gives curves.
3. **Split:** the points where a section curve crosses the boundary edges of a face come from `meet`
   (edge curve × the other surface); the parts of the curve inside the face split it.
4. **Classify:** one interior point per piece, `probe` against the other body; neighbouring pieces not
   separated by a section curve inherit the result.
5. **Select and sew:** a table per operation (union: A-out + B-out; subtract: A-out + B-in reversed;
   intersect: A-in + B-in). Pieces are sewn by the identity of their shared section curve, without a
   tolerance search.

- Coincident faces: the boundary edges of the other face are cutting curves, pieces get the class
  "on, same or opposite orientation", and the table gains two columns.
- Tangent contact is first answered with `Declined("tangent contact")`, never with a wrong result.
- Another implementation (for meshes, say) can be chained under the same `Id`.

### 4.6 The agent's loop

- **MCP dialect:** `initialize`, `tools/list` (from `Schema`), `tools/call`; the answer is text and an
  image. The agent models through tool calls without compiling Rust.
- **`render` (`raycast`):** analytic ray casting per pixel on rayon, no tessellation. PNG through an
  own uncompressed encoder, no new dependency; over MCP it returns as image content.
- **`view`:** the same view as text: visible faces by name, their share of the screen, what hides what.
- **Batch call:** several operations in one request; it stops at the first unmet expectation and the
  document stays unchanged.
- **`save` / `open` / `brief`:** the model is saved as `k3d` text with its notes and watchers.

### 4.7 A model file is a test

- `models/*.k3d` holds operations and `expect` lines. The test run replays each of them, checks the
  expectations and writes an image to look at.
- The Corinthian column becomes `models/corinthian.k3d` instead of a Rust example, which also shows
  that modelling needs no compilation.
- Every bug found becomes a new model file or a new `expect` line.

### 4.8 Later, on the same mould

- A `gap` family (`a|b` → smallest distance in closed form): exact `wall` and `clearance`.
- Empty space as an element: a `void` index in which a group of faces connected along concave edges
  is a pocket, a hole or a channel, with its own `here` view (mouths, depth, narrowest width).
- PMI attributes (`pmi.sem.*`, `pmi.gfx.*`); a `step` writer with shared definitions.
- An `offset` axis, winding number on curved faces, free-form surfaces, ornament stored as its rule.

## 5. Historical kernel layout sketch

The tree below is the original kernel design sketch, not the current workspace and not the
accepted product target. The implemented code includes `crates/krlyxd`, `crates/core`,
`crates/kernel`, `crates/std`, `crates/cli`, `crates/view`, `crates/exchange`,
`package/krlyvis/crates/*`, `products/krly3d/brep/{api,engine}`,
`products/krly3d/mesh/{api,engine,cli,mcp}` and
`products/krly3d/view/{api,renderer,mesh,cli,mcp}`. Raw, mesh and view boundaries
are implemented migration slices; the full domain split and CTM
extension host remain future work. See chapter 8 and `docs/PRODUCT-ARCHITECTURE-HANDOFF.md`.

```
krly3d/
├── Cargo.toml                      # workspace
├── LICENSE.md  NOTICE.md  HANDOFF.md  SPEC.md
├── docs/architecture.drawio        # the diagrams of chapter 2, editable
├── models/                         # *.k3d reference models that are also tests
├── tests/reference/*.json          # numbers recorded from the old kernel before its removal
└── crates/
    ├── core/src/                   # shared by every kernel (krly3d, krlyvis)
    │   ├── id.rs  attempt.rs  error.rs  accuracy.rs  exact.rs  clock.rs
    │   ├── plugin.rs               # Family { Request, Response, Context }, Plugin<F>, Registry<F>
    │   ├── schema.rs               # Value, Params, Schema: the protocol
    │   └── math/                   # frame roots quadric elliptic nurbs quadrature mirror bounds
    ├── kernel/src/                 # contracts and orchestration, zero implementations
    │   ├── plugin.rs  schema.rs    # re-exports of core
    │   ├── context.rs  builder.rs  kernel.rs  api.rs  predicate.rs
    │   ├── ports/pair.rs           # most-specific-first lookup of pair rules
    │   ├── ports/<family>/         # the same two files in each of the 17 families
    │   │   ├── mod.rs              #   Family (or object trait), Request, Response
    │   │   └── contract.rs         #   the test every plugin of the family must pass
    │   ├── topo/                   # body.rs builder.rs check.rs (walk.rs: phase 4)
    │   └── model/                  # op.rs document.rs build.rs delta.rs
    ├── std/src/                    # every shipped capability, one file per plugin
    │   ├── lib.rs                  # pub fn standard() -> KernelBuilder
    │   └── <family>/               # the same layout in each of the 17 families
    │       ├── mod.rs              #   pub fn register(b: &mut KernelBuilder)
    │       └── <plugin>.rs         #   one struct, impl Plugin<F>, its tests
    ├── view/src/                   # package: camera, render port, raycast, picture, view,
    │                               #   silhouette (outline.rs), pick
    ├── exchange/src/               # package: STEP (step.rs, part21.rs); STL, OBJ, glTF to come
    ├── game/src/                   # package (phase 5): LOD, UV, normals, materials, baking
    ├── fluid/src/                  # package (phase 6): grid, advection, pressure, level set
    └── cli/src/                    # assembles std + view + exchange (+ game, fluid); peers
        ├── main.rs  session.rs  rpc.rs  peer.rs
        └── dialect/                # mod.rs plain.rs mcp.rs
```

Plugin files of `std` by family:

```
curve/    line circle (ellipse trace)
surface/  plane sphere cylinder cone torus
sweep/    line_shift circle_shift line_turn circle_turn
section/  plane_plane plane_sphere plane_cylinder sphere_sphere sphere_cylinder_coaxial
          cylinder_cylinder_parallel plane_quadric quadric_quadric
meet/     line_surface circle_plane circle_quadric (trace_quadric)
tool/     profile rectangle circle extrude revolve cuboid cylinder cone sphere torus
          transform pattern (polyhedron mirror boolean/ chamfer fillet)
stage/    run name check build delta commit (judge)
probe/    exhaustive box_tree
index/    keys
keys/     name kind made normal radius axis cell coaxial
measure/  volume area count (bbox wall)
content/  plane cylinder cone sphere torus
query/    faces edges here ray find keys measure overview brief view stats
attr/     ai_note ai_watch
read/     k3d
write/    k3d
```

The renderer and its queries moved to the `view` package (`raycast`, `picture`, `view`,
`outline` for `silhouette`, `pick`).

A large plugin may be a folder (`tool/boolean/` with `candidates.rs`, `split.rs`, `classify.rs`,
`sew.rs`). The file of the `box` tool is `cuboid.rs` because `box` is a reserved word in Rust.

## 6. Phases and acceptance

| Phase | Content | Accepted when |
|---|---|---|
| 0 | Snapshot and reference numbers | done, see HANDOFF.md |
| 1 | Removal of the inherited code, workspace, `core`, plugin registry | registry tests pass (chain, order, determinism) |
| 2 | Equations: `curve`, `surface`, `sweep`, `meet` | evaluation, projection and implicit residuals within tolerance; roots agree with an independent formula; quadric transform round-trips |
| 3 | Process: `topo`, `model`, `stage`, `tool` (profile, sweeps, primitives, transform, pattern), `content`, `measure`, `read`/`write` | volume and area of box, cylinder, cone, sphere, torus, extrusion and revolution agree with the closed form to 1e-12 relative; Euler check passes; the fluted shaft matches the reference; save and reload build the same model |
| 4 | Perception and the agent's loop: `probe`, `index`, `keys`, `query`, `judge` with dry run and expectations, `cli` (plain + MCP), `render`, `view` | `exhaustive` and `box-tree` give identical hits; the ported perception assertions hold; one MCP session draws a box, an extrusion and a revolution without compiling Rust and gets the image back; ray and image times are measured and reported |
| 5 | General boolean | box − cylinder (also with coincident caps), sphere ∩ sphere, parallel cylinder − cylinder agree with the closed form; rounded flute ends succeed; names are inherited |
| 6 | Chamfer, fillet, the column | the filleted block agrees with the closed form; `models/corinthian.k3d` builds and is compared with the reference |
| 7 | The AI layer completed: `attr`, layers and proposals, `sweep`, `history`, `contour`, remaining key families | a note survives a chamfer and a face split; a lost anchor is reported as an orphan; a watcher fires when a wall goes under 2 mm; accept and discard leave the expected document; `brief` returns the notes after save and open; a sweep finds a break point known in closed form |
| 8 | The items of 4.8, tessellation | – |

## 7. Known risks

- The general boolean is the hardest part. It follows a method known from the literature and has not
  been tried on this code. Tangent contact is declined at first.
- A point common to three curved surfaces (`trace|quadric`) is a root of a polynomial of degree up
  to eight: numerical (`~`), verified by residual.
- The first `wall` and the break points of a sweep are not closed forms; the answer says so.
- After the removal, the only external oracle is `tests/reference/` and the closed forms.

## 8. One foundation, many interpretations (target architecture, revised 2026-11-14)

This chapter describes the accepted product direction. Items explicitly identified as current
are implemented in the existing workspace; the remaining package and feature descriptions are
target architecture and must not be read as shipped behavior.

### 8.1 Why

A B-rep kernel builds parts; it cannot express every implicit or subdivided shape or simulate
fluid. The accepted product direction keeps a raw, domain-independent `krlyxd` foundation,
separate `krly3d-*` domains, a `krly3d-ctm` assembly/extension host, and `krlyvis` as a separate
vision product. Mesh is a geometry domain; SubD is separate from mesh and owns subdivision and
related surface tools. Hydraulic network design is separate from 3D CFD. Shared algorithms
must have one definition, rather than being copied into product-specific packages.

### 8.2 Principles

1. One operation, one definition. A tool makes its geometry once: a curve, a surface, a field
   or a body. Every interpretation reads that geometry generically. A loft is a NURBS surface
   once; the B-rep reading wraps it in topology, the mesh reading tessellates the same
   surface, the field reading measures the distance to it.
2. An interpretation is a family, not a tool: `brep`, `mesh`, `field`, `levelset`, one generic
   plugin each, working through the traits (`Surface::point/normal/project/line`,
   `Shape::distance`).
3. Shared algorithms live in one module: skinning (loft and sweep), offset surfaces (shell,
   thicken, the centre line of a blend), section rules (boolean, blend, split), tessellation
   (export, LOD, render), quadrature, the plugin mould and the protocol schema. A test
   (`crates/core/tests/one_copy.rs`) fails when one of them is written twice.
4. Exactness marks stay: `=` closed form, `~` numeric, `?` sampled. An interpretation never
   claims an exactness it does not have.
5. The raw kernel knows no domain implementation. View, exchange, B-rep, mesh, field, SubD,
   hydraulic network, CFD, CTM and krlyvis have explicit product boundaries. These boundaries
   are the target architecture; the current crates have not all been migrated into them.

### 8.3 The foundation: `Shape`

The result of a model node is an `Arc<dyn Shape>`. Kinds: a `brep` leaf (today's `Body`,
which every existing tool keeps making), a `field` leaf (a sparse voxel grid for scans,
sculpting and fluids), a `subd` leaf (a Catmull–Clark cage whose limit surface is exact NURBS
on regular patches), and operators (union, subtract, intersect as min/max; blend as a smooth
minimum; offset; shell; transform).

Current: `Shape` exists, and current tools still produce B-rep `Body` values exposed as shape
leaves. The field, SubD and composed operator examples above describe intended future
representations; they are not implemented by the current workspace.

`Shape` is a geometry-domain abstraction in the krly3d layer, not a primitive of the raw
`krlyxd` kernel and not a mandatory universal representation for every domain package.
Domain packages may keep their native data types; explicit, quality-marked conversions are
used when domains interoperate. Current B-rep tools wrap their bodies in `Leaf` for the shape
and interpretation ports.
[The current Shape port](crates/kernel/src/ports/shape/mod.rs) defines this geometry-layer
abstraction.

```rust
pub trait Shape: Debug + Send + Sync + 'static {
    fn classes(&self) -> &'static [&'static str];
    fn distance(&self, cx: &Context, p: &Point3) -> Attempt<Signed>;
    fn bounds(&self, cx: &Context) -> Attempt<Bounds>;
    fn children(&self) -> &[Arc<dyn Shape>];
    fn as_any(&self) -> &dyn Any;
}
```

### 8.4 Interpretations

| Reading | Input → output | How, once |
|---|---|---|
| `brep` | Shape → Body | a leaf is itself; an operator goes through the chain of B-rep rules (boolean, blends, offset, shell); without an exact rule: field → mesh → faceted B-rep or fitted NURBS, marked `~` |
| `mesh` | Shape → Mesh | face by face within a chord tolerance for B-rep, with shared edge points so neighbouring faces meet; subdivision for SubD; dual contouring with Hermite data for fields |
| `field` | Shape → sparse grid | sampled from `distance` |
| `levelset` | field + velocity → field over time | the shared engine of fluids and sculpting |

The target model memoises readings by explicit, versioned package contracts. In the current
tree, successful document node builds use the document key cache; node build failures are
retried. B-rep mesh successes are separately cached by body `Arc`, context and tolerance.
Deterministic triangulation-budget declines are now memoized within that scope and reported
through `BrepMesh::decline_stats`; untyped declines, layout declines and transient failures
are retried. This cache has no content hash or formal API/solver version key yet. A tool that
needs a B-rep input should ask the B-rep reading; that adapter is the target conversion point
([mesh_of](crates/kernel/src/ports/interpret/mod.rs),
[BrepMesh](crates/std/src/interpret/mesh/mod.rs)).

### 8.5 Target package boundaries

The current workspace has `crates/view` (render, view, silhouette, pick and likeness) and
`crates/exchange` (STEP writing), alongside the combined `crates/kernel` and `crates/std`.
The [raw contract](docs/RAW-KERNEL.md) lives in `crates/krlyxd`;
[ADR 0008](docs/decisions/0008-raw-foundation-contract.md) settles neutral metadata/quality
primitives versus domain-owned Accuracy/frame/NURBS and aggregate provenance. The
[typed mesh API and synchronous bridge](docs/MESH-API.md) live in
`products/krly3d/mesh/{api,engine}`. The bridge temporarily consumes the legacy geometry
implementation. Local controlled execution and the separate
[v2 operation runner](docs/MESH-OPERATIONS.md) now provide cooperative cancellation,
deadlines and bounded Started/terminal events. Separate [mesh CLI/MCP adapters](docs/MESH-ADAPTERS.md)
now expose that pilot contract; other domain adapters remain pending.
The accepted target splits functionality into `krly3d-brep`, `krly3d-mesh`, `krly3d-field`,
`krly3d-subd`, `krly3d-view`, `krly3d-exchange`, `krly3d-cfd` and
`krly3d-hydraulic-network`; `krly3d-ctm` hosts assemblies, constraints and extensions.
SubD owns subdivision, LOD and any adopted UV/baking work; it is not folded into mesh. CFD
owns continuum and free-surface simulation; the hydraulic package owns pipe-network design
and flow. Each domain's versioned API is shared by its CLI and MCP adapters. These target
packages and adapters do not all exist yet. `krlyvis` is already a separate product, but its
current API/package structure is still documented in `package/krlyvis/SPEC.md` and is not
yet migrated to the full target package pattern.

### 8.6 Target capabilities

The target product architecture assigns these capabilities to separate domains. The current
subset and its limits are in HANDOFF.md; the list below is target scope and does not mark an
operation shipped.

- **B-rep/CAD:** rolling-ball blends across general edges, corner blends, offset/shell/thicken,
  sweeps with a rotation-minimising frame, helix/thread, split, pattern along a curve, face
  draft, polyhedra, a 2D sketch constraint solver, general surface–surface intersections for
  booleans, mass properties, `gap`/`wall`, and STEP AP242 with PMI.
- **Mesh:** reconstruction and mesh operations, import/export for STL, OBJ and glTF, and
  conversion from other geometry domains under explicit quality and tolerance contracts.
- **Field:** implicit leaves and operators, sphere tracing, dual contouring and field-to-B-rep
  conversion; it may support field-based blend and sculpting workflows.
- **SubD:** control-cage modelling and subdivision, limit-surface conversion, and any adopted
  LOD, UV/LSCM, normals, materials and baking features. These stay outside the mesh package.
- **3D CFD:** MAC-grid flow, advection, pressure projection and level sets/free surfaces.
  **Hydraulic network:** a separate domain for pipes, nodes, valves, pumps and network
  pressure/flow; it is not part of 3D CFD.
- **System-level AI/PMI and image workflow:** anchored notes, proposals, history, watchers,
  provenance and PMI; a vision-to-mesh-to-editable-B-rep flow coordinated across packages.

Every domain exposes its versioned public API to its CLI and MCP adapters. `krly3d-ctm`
hosts assembly, constraints and cross-domain extensions; it does not replace the domains'
standalone APIs.

### 8.7 Historical implementation phases and remaining target work

The table records an earlier roadmap, not current package status. Current implementation
evidence is summarized in HANDOFF.md. In particular, the old phase-0 note below that said the
krlyvis switch to the core mould was pending is obsolete: krlyvis now re-exports the shared
`krly3d_core` plugin mould. Its domain-specific `Context` remains separate.

| # | Phase | Acceptance |
|---|---|---|
| 0 | Tidy-up: this chapter and the diagrams; the mould, the schema and quadrature in `core`; the camera into `view`; the agents' queries and STEP export merged | all tests green; one copy of each shared module; the kernel does not depend on `view` |
| 1 | `Shape`, `interpret::brep` and `mesh`; the model stores shapes | behaviour unchanged; mesh volume within 1e-3 of the B-rep volume |
| 2 | CAD I: ellipse rules, offsets, rolling-ball blends, shell, sweep, helix, split, draft | closed-form volumes; loft and sweep share one skinning module |
| 3 | CAD II: surface–surface marching, NURBS booleans, mass properties, gap, wall, polyhedron, sketch solver, STEP AP242 | Steinmetz solid 16r³/3 to `~`; STEP read back gives the same volume |
| 4 | Fields: leaves, blend and offset operators, sphere tracing, dual contouring, B-rep fallback, corner blends, sculpting | the two renders of one model agree (IoU ≥ 0.99); a smooth union reads back as a valid B-rep |
| 5 | Game: SubD, LOD, UV, normals, materials, baking, glTF/OBJ/STL | a SubD cube's limit surface equals its NURBS reading; glTF passes a validator |
| 6 | Fluid package | dam break: volume loss measured per step; still water stays still |
| 7 | AI layer and the photograph chain | the acceptance of chapter 6, phase 7 |

As of the second development wave, the current tree also includes the separate raw seed
and versioned synchronous mesh API/engine. The plugin/document support remains in core;
the shape abstraction, tessellation and B-rep implementation remain in the legacy geometry
crates behind the bridge. Controlled execution and the v2 runner were added in waves three
and four; wave five adds the standalone mesh CLI/MCP adapters. Completing the domain
split, other domain adapters, CTM and field/SubD/CFD/hydraulic packages remains future work.
Wave six adds full-disconnect cancellation to legacy Unix-socket local queries while
preserving normal write half-close. Renderer/probe and PNG/base64 work use the shared
cooperative control; the legacy reply shapes remain unchanged. Already-running mutating
tools and remote peer work are not preempted; legacy progress events and wire timeout
parameters are not added. HANDOFF.md and
[development status](docs/DEVELOPMENT-STATUS.md) record current verification.


The opt-in [mesh preview backend](docs/MESH-PREVIEW.md) now lives in
`products/krly3d/view/mesh`. It consumes source-face/corner-normal metadata through
the mesh engine bridge and shares shading rules with exact rendering. The legacy
image facade accepts `renderer: "mesh"`, reports requested tolerance and timing,
and retains raycast as its default and exact picking path. Broader standalone
view migration remains work under #26; the user retired named-model benchmarks under #18. The implemented #1 preview is
closed under revised acceptance; alternative validation will be selected later.

B-rep source ownership now lives in `products/krly3d/brep/{api,engine}`. Its
[immutable snapshot boundary](docs/BREP-SNAPSHOTS.md) owns native loading,
source identity and version resolution; mesh composes this owner and converts the
pinned geometry through the explicit legacy bridge. This is an in-process migration
slice, not a new B-rep wire protocol or completed removal of kernel/std dependencies.

The local [typed view API and renderer](docs/VIEW-API.md) now live under
`products/krly3d/view/{api,renderer}`. One pinned node is rendered with a named,
orthographic or pinhole camera through the existing ray/mesh implementations.
PNG bytes, face coverage, source provenance, sampled quality and timings are returned
through Rust API 1.0, with cooperative controlled execution. The
[synchronous wire 1.0 and standalone CLI/MCP](docs/VIEW-ADAPTERS.md) now expose this
call using one strict codec and descriptor. Multi-node scenes, paint, pick, likeness,
wire cancellation/deadlines and a progress stream remain pending.
